//! Headless MaterialCraft command line: inspect, script, cook, export and render packages, and
//! serve agents over MCP or JSON lines. Never opens a window.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::sync::{Arc, Mutex};

use materialcraft_automation::security::{client_token, server_token};
use materialcraft_automation::{AuthorizedWorkspace, BridgeClient, Filesystem, Headless, MaterialcraftMcp, RenderOptions, rpc};
use serde_json::{Value, json};

const USAGE: &str = "\
materialcraft-cli: headless MaterialCraft

USAGE:
  materialcraft-cli --version
  materialcraft-cli info <package.mcraft | sample>
      Graphs, nodes, outputs and cook results, as JSON.
  materialcraft-cli commands [--filter <text>]
      List the engine commands (id, label, params).
  materialcraft-cli nodes [--filter <text>] [--category <text>]
      List the node library (ids, ports, parameters).
  materialcraft-cli run <input> <command-id> [params-json] [-o <out.mcraft>]
      Run one command and print its JSON result; -o saves the package.
  materialcraft-cli batch <script.json>
      Run [{\"id\", \"params\"}, ...] or {\"open\": <input>, \"steps\": [...], \"save\": <out.mcraft>}.
  materialcraft-cli new <template> <out.mcraft> [--size <px>] [--name <name>]
      A new package with one graph from a template (empty, pbr, bricks, stone_tiles, wood,
      metal_plates, rusty_metal, lava, marble, wicker, fabric, ground).
  materialcraft-cli cook <input> [--graph <index>] [--set <input>=<value>]... [--full]
      Cook a graph and print per-node timings and errors.
  materialcraft-cli export <input> <dir> [--graph <index>] [--set <input>=<value>]... [--format <ext>]
                   [--depth 8|16|32] [--size <px>] [--pattern <p>]
      Cook at full size and write every output (png, jpg, tga, tif, bmp, webp, exr, hdr).
  materialcraft-cli render <input> <out.png> [--size <W>x<H>] [--view 3d|2d] [--scene <id>] [--node <id>]
                   [--yaw <deg>] [--pitch <deg>] [--graph <index>] [--set <input>=<value>]... [--full]
                   [--tiling <n>] [--hq [--displacement <radii>]]
      Render the material on a 3D scene (rounded_cube, cube, sphere, cylinder, plane, torus,
      material_ball), or one node's output (--view 2d), to PNG. No GPU needed. --hq is a beauty
      render: real displacement from the height output, ambient occlusion and a key light.
  materialcraft-cli mcp [--bridge <port>] [--token <token>] [--read-root <dir>] [--write-root <dir>]
      MCP server on stdio: headless (file access only below the roots), or bridged to
      `materialcraft --control <port>`.
  materialcraft-cli serve [--port <port>] [--read-root <dir>] [--write-root <dir>]
      JSON lines ({\"id\",\"method\",\"params\"}) on stdio, or on 127.0.0.1:<port> with a token.

<input> is a .mcraft package or `sample` (the built-in brick wall). --set gives a graph input a value
(JSON, e.g. `--set roughness=0.4 --set tint=\"#aa7744\"`) before cooking. See docs/control-protocol.md.
";

/// (name, flags taking a value, bare flags)
const SUBCOMMANDS: &[(&str, &[&str], &[&str])] = &[
    ("info", &[], &[]),
    ("commands", &["--filter"], &[]),
    ("nodes", &["--filter", "--category"], &[]),
    ("run", &["-o"], &[]),
    ("batch", &[], &[]),
    ("new", &["--size", "--name"], &[]),
    ("cook", &["--graph", "--set"], &["--full"]),
    ("export", &["--graph", "--set", "--format", "--depth", "--size", "--pattern"], &[]),
    ("render", &["--size", "--view", "--scene", "--node", "--yaw", "--pitch", "--graph", "--set", "--tiling", "--displacement"], &["--full", "--hq"]),
    ("mcp", &["--bridge", "--token", "--read-root", "--write-root"], &[]),
    ("serve", &["--port", "--read-root", "--write-root"], &[]),
];

struct Args {
    pos: Vec<String>,
    flags: Vec<(String, Option<String>)>,
}

impl Args {
    fn get(&self, k: &str) -> Option<&str> {
        self.flags.iter().rev().find(|(f, _)| f == k).and_then(|(_, v)| v.as_deref())
    }
    fn has(&self, k: &str) -> bool {
        self.flags.iter().any(|(f, _)| f == k)
    }
    fn num<T: std::str::FromStr>(&self, k: &str) -> Result<Option<T>, String> {
        self.get(k).map(|v| v.parse().map_err(|_| format!("{k}: `{v}` is not a valid number"))).transpose()
    }
    fn pos(&self, n: usize, what: &str) -> Result<&str, String> {
        self.pos.get(n).map(String::as_str).ok_or_else(|| format!("missing {what}"))
    }
}

fn parse(cmd: &str, raw: &[String]) -> Result<Args, String> {
    let (_, valued, bare) = SUBCOMMANDS.iter().find(|(n, ..)| *n == cmd).ok_or_else(|| format!("unknown command `{cmd}`\n\n{USAGE}"))?;
    let mut a = Args { pos: Vec::new(), flags: Vec::new() };
    let mut it = raw.iter();
    while let Some(x) = it.next() {
        if valued.contains(&x.as_str()) {
            let v = it.next().ok_or_else(|| format!("{x} needs a value"))?;
            a.flags.push((x.clone(), Some(v.clone())));
        } else if bare.contains(&x.as_str()) {
            a.flags.push((x.clone(), None));
        } else if x.starts_with('-') && x.len() > 1 && x.parse::<f64>().is_err() {
            return Err(format!("unknown option `{x}` for `{cmd}`"));
        } else {
            a.pos.push(x.clone());
        }
    }
    Ok(a)
}

fn version() -> String {
    let sha = env!("MATERIALCRAFT_BUILD_SHA");
    let short = sha.get(..7).unwrap_or(sha);
    let date = env!("MATERIALCRAFT_BUILD_DATE");
    if short.is_empty() {
        format!("materialcraft-cli {}", materialcraft_engine::VERSION)
    } else {
        format!("materialcraft-cli {} ({short} {date})", materialcraft_engine::VERSION)
    }
}

/// A headless session on `input` (a .mcraft package or `sample`), with local file access.
fn open(input: &str) -> Result<Headless, String> {
    let mut h = Headless::new(Headless::default_session(), Filesystem::TrustedLocal);
    if input != "sample" {
        h.open(input).map_err(|e| e.to_string())?;
    }
    Ok(h)
}

fn print(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}")));
}

/// Open `--graph` and apply every `--set <input>=<value>` (the value is JSON, else a string).
fn select_graph(h: &mut Headless, a: &Args) -> Result<(), String> {
    if let Some(g) = a.num::<u64>("--graph")? {
        h.command_run("graph.open", json!({"graph": g})).map_err(|e| e.to_string())?;
    }
    for (_, v) in a.flags.iter().filter(|(f, _)| f == "--set") {
        let (id, value) = v.as_deref().and_then(|v| v.split_once('=')).ok_or("--set is <input>=<value>")?;
        let value = serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.into()));
        h.command_run("graph.exposed.set", json!({"id": id, "value": value})).map_err(|e| format!("--set {id}: {e}"))?;
    }
    Ok(())
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let code = match raw.first().map(String::as_str) {
        None | Some("-h" | "--help" | "help") => {
            print!("{USAGE}");
            0
        }
        Some("-V" | "--version") => {
            println!("{}", version());
            0
        }
        Some(cmd) => match parse(cmd, raw.get(1..).unwrap_or(&[])).and_then(|a| run(cmd, &a)) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("materialcraft-cli: {e}");
                1
            }
        },
    };
    std::process::exit(code);
}

fn run(cmd: &str, a: &Args) -> Result<(), String> {
    match cmd {
        "info" => {
            let mut h = open(a.pos(0, "<input>")?)?;
            print(&h.inspect());
        }
        "commands" => {
            let h = open("sample")?;
            print(&h.command_list(a.get("--filter")));
        }
        "nodes" => {
            let mut h = open("sample")?;
            print(&h.command_run("library.list", json!({"filter": a.get("--filter"), "category": a.get("--category")})).map_err(|e| e.to_string())?);
        }
        "run" => {
            let mut h = open(a.pos(0, "<input>")?)?;
            let id = a.pos(1, "<command-id>")?;
            let params: Value = match a.pos.get(2) {
                Some(p) => serde_json::from_str(p).map_err(|e| format!("params: {e}"))?,
                None => Value::Null,
            };
            print(&h.command_run(id, params).map_err(|e| e.to_string())?);
            if let Some(out) = a.get("-o") {
                h.save(Some(out)).map_err(|e| e.to_string())?;
            }
        }
        "batch" => {
            let text = std::fs::read_to_string(a.pos(0, "<script.json>")?).map_err(|e| e.to_string())?;
            let script: Value = serde_json::from_str(&text).map_err(|e| format!("script: {e}"))?;
            let (input, steps, save) = match &script {
                Value::Array(s) => ("sample", s.clone(), None),
                obj => (
                    obj.get("open").and_then(Value::as_str).unwrap_or("sample"),
                    obj.get("steps").and_then(Value::as_array).cloned().unwrap_or_default(),
                    obj.get("save").and_then(Value::as_str),
                ),
            };
            let mut h = open(input)?;
            let r = h.batch(&steps);
            print(&r);
            if !r["failed"].is_null() {
                return Err("the batch stopped at an error".into());
            }
            if let Some(out) = save {
                h.save(Some(out)).map_err(|e| e.to_string())?;
            }
        }
        "new" => {
            let tpl = a.pos(0, "<template>")?;
            let out = a.pos(1, "<out.mcraft>")?;
            let mut h = Headless::new(Headless::default_session(), Filesystem::TrustedLocal);
            h.command_run("file.newPackage", json!({"name": a.get("--name").unwrap_or("Untitled package")})).map_err(|e| e.to_string())?;
            h.command_run("file.new", json!({"template": tpl, "size": a.num::<u64>("--size")?.unwrap_or(1024), "name": a.get("--name")}))
                .map_err(|e| e.to_string())?;
            h.command_run("graph.delete", json!({"graph": 0})).map_err(|e| e.to_string())?;
            print(&h.save(Some(out)).map_err(|e| e.to_string())?);
        }
        "cook" => {
            let mut h = open(a.pos(0, "<input>")?)?;
            select_graph(&mut h, a)?;
            print(&h.command_run("graph.cook", json!({"full": a.has("--full")})).map_err(|e| e.to_string())?);
        }
        "export" => {
            let mut h = open(a.pos(0, "<input>")?)?;
            select_graph(&mut h, a)?;
            let dir = a.pos(1, "<dir>")?;
            let p = json!({"dir": dir, "graph": a.num::<u64>("--graph")?, "format": a.get("--format"), "depth": a.num::<u64>("--depth")?, "size": a.num::<u64>("--size")?, "pattern": a.get("--pattern")});
            print(&h.export(&p).map_err(|e| e.to_string())?);
        }
        "render" => {
            let mut h = open(a.pos(0, "<input>")?)?;
            select_graph(&mut h, a)?;
            let out = a.pos(1, "<out.png>")?;
            let (w, hh) = match a.get("--size") {
                Some(s) => {
                    let (w, h) = s.split_once('x').ok_or("--size is <W>x<H>")?;
                    (Some(w.parse::<u32>().map_err(|_| "bad --size width")?), Some(h.parse::<u32>().map_err(|_| "bad --size height")?))
                }
                None => (None, None),
            };
            let o = RenderOptions {
                view: a.get("--view").map(str::to_string),
                width: w,
                height: hh,
                yaw: a.num("--yaw")?,
                pitch: a.num("--pitch")?,
                scene: a.get("--scene").map(str::to_string),
                node: a.num("--node")?,
                port: None,
                full: a.has("--full"),
                tiling: a.num("--tiling")?,
                hq: a.has("--hq"),
                displacement: a.num("--displacement")?,
            };
            let r = h.render_png(&o).map_err(|e| e.to_string())?;
            materialcraft_engine::formats::write_atomic(std::path::Path::new(out), &r.png).map_err(|e| e.to_string())?;
            print(&json!({"path": out, "width": r.width, "height": r.height}));
        }
        "mcp" => {
            let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
            let server = match a.num::<u16>("--bridge")? {
                Some(port) => {
                    let token = client_token(port, a.get("--token").map(str::to_string)).map_err(|e| e.to_string())?;
                    MaterialcraftMcp::bridge(BridgeClient::new(port, token))
                }
                None => MaterialcraftMcp::headless(Headless::new(Headless::default_session(), workspace(a)?)),
            };
            rt.block_on(server.serve_stdio()).map_err(|e| e.to_string())?;
        }
        "serve" => {
            let h = Arc::new(Mutex::new(Headless::new(Headless::default_session(), workspace(a)?)));
            match a.num::<u16>("--port")? {
                Some(port) => {
                    let (token, file) = server_token(port).map_err(|e| e.to_string())?;
                    rpc::serve_tcp(port, h, token, |addr| {
                        eprintln!("materialcraft-cli: serving JSON lines on {addr}");
                        if let Some(f) = &file {
                            eprintln!("materialcraft-cli: token in {}", f.display());
                        }
                    })
                    .map_err(|e| e.to_string())?;
                }
                None => rpc::serve_lines(&h, std::io::stdin().lock(), std::io::stdout().lock()).map_err(|e| e.to_string())?,
            }
        }
        other => return Err(format!("unknown command `{other}`")),
    }
    Ok(())
}

/// File access for servers: none without roots, else only below them.
fn workspace(a: &Args) -> Result<Filesystem, String> {
    let (r, w) = (a.get("--read-root"), a.get("--write-root"));
    if r.is_none() && w.is_none() {
        return Ok(Filesystem::Denied);
    }
    let ws = AuthorizedWorkspace::new(r.map(std::path::Path::new), w.map(std::path::Path::new)).map_err(|e| e.to_string())?;
    Ok(Filesystem::Workspace(ws))
}
