//! SBS packages: the XML node-graph package format of the professional node-based material
//! workflow, and MaterialCraft's main project format. This is a clean-room reader and writer,
//! written from the format's public documentation and sample files; it shares no code with any
//! other implementation.
//!
//! **Reading** maps what MaterialCraft has and keeps going past what it doesn't: atomic filters
//! become the matching atomic nodes, library nodes become the MaterialCraft node of the same name,
//! instances of graphs in the same file become graph instances, and anything else becomes a
//! [`MISSING_NODE`] placeholder that keeps its wires and the data needed to write it back. Every
//! gap is reported in [`SbsRead::warnings`]; only a file that is not an SBS package at all is an
//! error.
//!
//! **Writing** produces an ordinary SBS package (graphs, nodes, wires, parameters, inputs and
//! outputs, frames, comments, and bitmaps embedded in the file) followed by an XML comment
//! holding the MaterialCraft package exactly, with a hash of the `<content>` it was written with.
//! When the hash still matches, reading uses the exact copy, so nothing MaterialCraft-specific
//! (function graphs, curves, gradients, presets, pins…) is lost; when the file was changed by
//! another program the graphs are read from the SBS data instead.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::sync::Arc;

use base64::Engine as _;
use materialcraft_core::{
    BaseParams, Comment, ExposedParam, Frame, Graph, MAX_SIZE_LOG2, MIN_SIZE_LOG2, MISSING_NODE, Node, NodeId, OutputFormat, Package, Resource, SizeMode,
    Value, linear_to_srgb, missing_ports, srgb_to_linear,
};
use materialcraft_nodes::{PortKind, find};
use roxmltree::Node as X;

use crate::{FormatError, Result};

/// The SBS package extension.
pub const SBS_EXT: &str = "sbs";

/// Largest SBS file read, largest embedded resource, and caps on what one file may hold.
const MAX_XML: usize = 256 << 20;
const MAX_EMBEDDED: usize = 256 << 20;
const MAX_XML_NODES: u32 = 8_000_000;
const MAX_GRAPHS: usize = 4096;
const MAX_NODES: usize = 100_000;
const MAX_RESOURCES: usize = 4096;
/// Warning lines kept per read (the rest are counted).
const MAX_WARNINGS: usize = 600;

/// The marker of the exact MaterialCraft copy at the end of a written package.
const EXACT_MARK: &str = "materialcraft-package";
/// Version of the exact copy's encoding.
const EXACT_VERSION: u32 = 1;
/// Dependency file name of MaterialCraft library nodes written as instances.
const LIBRARY_DEP: &str = "materialcraft://library.sbs";
/// The dependency file name a package uses for itself.
const SELF_DEP: &str = "?himself";
/// Format version written in the package header.
const FORMAT_VERSION: &str = "1.1.0.202401";

/// Half the size of a node, between an SBS node position (its center) and MaterialCraft's (the
/// top-left corner).
const NODE_HALF: [f32; 2] = [50.0, 40.0];

/// A read SBS package.
#[derive(Debug)]
pub struct SbsRead {
    pub pkg: Package,
    /// Everything that could not be read exactly: missing nodes, unsupported parameters,
    /// unavailable resources. Empty for a package MaterialCraft wrote and nobody changed.
    pub warnings: Vec<String>,
    /// Whether the package came from MaterialCraft's exact copy (nothing was lost).
    pub exact: bool,
}

/// Whether `bytes` look like an SBS package (XML whose root is `<package>`).
pub fn is_sbs(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(4096)];
    let text = String::from_utf8_lossy(head);
    let t = text.trim_start_matches('\u{feff}').trim_start();
    if !t.starts_with('<') {
        return false;
    }
    // Skip the XML declaration and comments before the root element.
    let mut rest = t;
    loop {
        rest = rest.trim_start();
        if let Some(r) = rest.strip_prefix("<?") {
            match r.find("?>") {
                Some(i) => rest = &r[i + 2..],
                None => return false,
            }
        } else if let Some(r) = rest.strip_prefix("<!--") {
            match r.find("-->") {
                Some(i) => rest = &r[i + 3..],
                None => return false,
            }
        } else {
            return rest.starts_with("<package") && rest[8..].starts_with(|c: char| c == '>' || c == '/' || c.is_whitespace());
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Warnings
// ---------------------------------------------------------------------------------------------

/// Warning lines, deduplicated with counts, in first-seen order.
#[derive(Default)]
struct Warn {
    lines: Vec<(String, usize)>,
    index: HashMap<String, usize>,
}

impl Warn {
    fn add(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        if let Some(&i) = self.index.get(&msg) {
            if let Some(l) = self.lines.get_mut(i) {
                l.1 += 1;
            }
            return;
        }
        self.index.insert(msg.clone(), self.lines.len());
        self.lines.push((msg, 1));
    }

    fn finish(self) -> Vec<String> {
        let total = self.lines.len();
        let mut out: Vec<String> = self.lines.into_iter().take(MAX_WARNINGS).map(|(m, n)| if n > 1 { format!("{m} (×{n})") } else { m }).collect();
        if total > MAX_WARNINGS {
            out.push(format!("…and {} more warnings", total - MAX_WARNINGS));
        }
        out
    }
}

// ---------------------------------------------------------------------------------------------
// Atomic filters
// ---------------------------------------------------------------------------------------------

/// How an SBS parameter becomes a MaterialCraft one.
#[derive(Clone, Copy, PartialEq)]
enum Conv {
    /// By the MaterialCraft parameter's type.
    Plain,
    /// A boolean that is `true` for option 0 of a two-option choice.
    BoolFirst,
}

/// An SBS atomic filter and the MaterialCraft atomic node it maps to.
struct FilterMap {
    sbs: &'static str,
    def: &'static str,
    /// (SBS input identifier, MaterialCraft input port).
    inputs: &'static [(&'static str, &'static str)],
    /// (SBS parameter, MaterialCraft parameter, conversion).
    params: &'static [(&'static str, &'static str, Conv)],
}

use Conv::{BoolFirst, Plain};

const FILTERS: &[FilterMap] = &[
    FilterMap { sbs: "uniform", def: "uniform", inputs: &[], params: &[("outputcolor", "color", Plain), ("colorswitch", "mode", BoolFirst)] },
    FilterMap {
        sbs: "blend",
        def: "blend",
        inputs: &[("source", "foreground"), ("destination", "background"), ("opacity", "mask")],
        params: &[("blendingmode", "mode", Plain), ("opacitymult", "opacity", Plain)],
    },
    FilterMap { sbs: "blur", def: "blur", inputs: &[("input1", "input")], params: &[("intensity", "intensity", Plain)] },
    FilterMap {
        sbs: "shuffle",
        def: "channels_shuffle",
        inputs: &[("input1", "input1"), ("input2", "input2")],
        params: &[("channelred", "red", Plain), ("channelgreen", "green", Plain), ("channelblue", "blue", Plain), ("channelalpha", "alpha", Plain)],
    },
    FilterMap { sbs: "curve", def: "curve", inputs: &[("input1", "input")], params: &[] },
    FilterMap {
        sbs: "dirmotionblur",
        def: "directional_blur",
        inputs: &[("input1", "input")],
        params: &[("intensity", "intensity", Plain), ("mblurangle", "angle", Plain)],
    },
    FilterMap {
        sbs: "directionalwarp",
        def: "directional_warp",
        inputs: &[("input1", "input"), ("inputintensity", "intensity_input")],
        params: &[("intensity", "intensity", Plain), ("warpangle", "angle", Plain)],
    },
    FilterMap {
        sbs: "distance",
        def: "distance",
        inputs: &[("mask", "mask"), ("source", "source")],
        params: &[("distance", "distance", Plain), ("combinedistance", "combine", Plain)],
    },
    FilterMap {
        sbs: "emboss",
        def: "emboss",
        inputs: &[("input1", "input"), ("inputgradient", "height")],
        params: &[
            ("intensity", "intensity", Plain),
            ("lightangle", "light_angle", Plain),
            ("highlightcolor", "highlight", Plain),
            ("shadowcolor", "shadow", Plain),
        ],
    },
    FilterMap {
        sbs: "dyngradient",
        def: "gradient_dynamic",
        inputs: &[("input1", "input"), ("input2", "gradient"), ("inputgradient", "gradient")],
        params: &[("position", "position", Plain), ("orientation", "orientation", Plain)],
    },
    FilterMap { sbs: "gradient", def: "gradient_map", inputs: &[("input1", "input")], params: &[("colorswitch", "mode", BoolFirst)] },
    FilterMap { sbs: "grayscaleconversion", def: "grayscale_conversion", inputs: &[("input1", "input")], params: &[] },
    FilterMap {
        sbs: "hsl",
        def: "hsl",
        inputs: &[("input1", "input")],
        params: &[("hue", "hue", Plain), ("saturation", "saturation", Plain), ("luminance", "lightness", Plain)],
    },
    FilterMap { sbs: "bitmap", def: "bitmap", inputs: &[], params: &[] },
    FilterMap {
        sbs: "levels",
        def: "levels",
        inputs: &[("input1", "input")],
        params: &[
            ("levelinlow", "in_low", Plain),
            ("levelinmid", "in_mid", Plain),
            ("levelinhigh", "in_high", Plain),
            ("leveloutlow", "out_low", Plain),
            ("levelouthigh", "out_high", Plain),
        ],
    },
    FilterMap {
        sbs: "normal",
        def: "normal",
        inputs: &[("input1", "input")],
        params: &[("intensity", "intensity", Plain), ("inversedy", "format", BoolFirst), ("input2alpha", "height_alpha", Plain)],
    },
    FilterMap {
        sbs: "pixelprocessor",
        def: "pixel_processor",
        inputs: &[("input", "input0"), ("input:1", "input1"), ("input:2", "input2"), ("input:3", "input3")],
        params: &[],
    },
    FilterMap { sbs: "valueprocessor", def: "value_processor", inputs: &[("input", "input0")], params: &[] },
    FilterMap { sbs: "sharpen", def: "sharpen", inputs: &[("input1", "input")], params: &[("intensity", "intensity", Plain)] },
    FilterMap {
        sbs: "transformation",
        def: "transformation",
        inputs: &[("input1", "input")],
        params: &[("offset", "offset", Plain), ("mattecolor", "matte", Plain)],
    },
    FilterMap { sbs: "warp", def: "warp", inputs: &[("input1", "input"), ("inputgradient", "gradient")], params: &[("intensity", "intensity", Plain)] },
    FilterMap { sbs: "passthrough", def: "dot", inputs: &[("input", "input"), ("input1", "input")], params: &[] },
    FilterMap { sbs: "text", def: "text", inputs: &[], params: &[("text", "text", Plain)] },
    FilterMap { sbs: "svg", def: "svg", inputs: &[], params: &[] },
    FilterMap { sbs: "fxmaps", def: "fx_map", inputs: &[], params: &[] },
];

/// SBS parameters every node has, handled as base parameters.
const BASE_PARAMS: &[&str] = &["outputsize", "format", "randomseed", "tiling", "pixelsize", "pixelratio", "quality"];

/// Parameters of filters that are read but have no direct entry in [`FILTERS`] (handled in code
/// or knowingly ignored).
const HANDLED_PARAMS: &[(&str, &str)] = &[
    ("bitmap", "bitmapresourcepath"),
    ("bitmap", "colorspace"),
    ("grayscaleconversion", "channelsweights"),
    ("transformation", "matrix22"),
    ("curve", "exposecurve"),
];

fn filter_by_sbs(name: &str) -> Option<&'static FilterMap> {
    FILTERS.iter().find(|f| f.sbs == name)
}

fn filter_by_def(def: &str) -> Option<&'static FilterMap> {
    FILTERS.iter().find(|f| f.def == def)
}

/// SBS usage names of MaterialCraft's output usages (same order as `USAGE_IDS`).
fn usage_index(name: &str) -> u32 {
    let ids = materialcraft_nodes::atomic::USAGE_IDS;
    let n = name.trim();
    ids.iter().position(|u| u.eq_ignore_ascii_case(n)).or_else(|| (n.eq_ignore_ascii_case("diffuse")).then_some(0)).unwrap_or(ids.len().saturating_sub(1))
        as u32
}

fn usage_name(i: u32) -> &'static str {
    materialcraft_nodes::atomic::USAGE_IDS.get(i as usize).copied().unwrap_or("any")
}

// ---------------------------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------------------------

/// A constant value as SBS stores it.
#[derive(Clone, Debug, PartialEq)]
enum SVal {
    F(Vec<f32>),
    I(Vec<i32>),
    B(bool),
    S(String),
}

fn nums_f(s: &str) -> Vec<f32> {
    s.split_whitespace().take(16).map(|t| t.parse::<f32>().ok().filter(|v| v.is_finite()).unwrap_or(0.0)).collect()
}

fn nums_i(s: &str) -> Vec<i32> {
    s.split_whitespace()
        .take(16)
        .map(|t| t.parse::<i64>().map(|v| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32).or_else(|_| t.parse::<f64>().map(|v| v as i32)).unwrap_or(0))
        .collect()
}

/// The constant under `holder` (a `paramValue`, `defaultValue` or `constValue`).
fn constant(holder: X) -> Option<SVal> {
    let c = holder.children().find(|c| c.is_element())?;
    let v = c.attribute("v").unwrap_or("");
    let tag = c.tag_name().name();
    if tag == "constantValueBool" {
        Some(SVal::B(nums_i(v).first().is_some_and(|b| *b != 0) || v.trim().eq_ignore_ascii_case("true")))
    } else if tag == "constantValueString" {
        Some(SVal::S(v.to_string()))
    } else if tag.starts_with("constantValueFloat") {
        Some(SVal::F(nums_f(v)))
    } else if tag.starts_with("constantValueInt") {
        Some(SVal::I(nums_i(v)))
    } else {
        None
    }
}

impl SVal {
    fn f(&self, i: usize) -> f32 {
        match self {
            SVal::F(v) => v.get(i).or(v.first()).copied().unwrap_or(0.0),
            SVal::I(v) => v.get(i).or(v.first()).copied().unwrap_or(0) as f32,
            SVal::B(b) => f32::from(u8::from(*b)),
            SVal::S(_) => 0.0,
        }
    }

    fn i(&self, i: usize) -> i32 {
        match self {
            SVal::I(v) => v.get(i).or(v.first()).copied().unwrap_or(0),
            SVal::F(v) => v.get(i).or(v.first()).copied().unwrap_or(0.0).round() as i32,
            SVal::B(b) => i32::from(*b),
            SVal::S(_) => 0,
        }
    }

    fn b(&self) -> bool {
        match self {
            SVal::B(b) => *b,
            SVal::S(s) => s == "1" || s.eq_ignore_ascii_case("true"),
            _ => self.f(0) != 0.0,
        }
    }

    fn len(&self) -> usize {
        match self {
            SVal::F(v) => v.len(),
            SVal::I(v) => v.len(),
            _ => 1,
        }
    }
}

/// Convert an SBS constant to a value of the same type as `like` (a MaterialCraft parameter's
/// default). `options` is the option count of an enum. `None` for types SBS constants can't hold.
fn coerce(like: &Value, v: &SVal, options: usize) -> Option<Value> {
    Some(match like {
        Value::Float(_) => Value::Float(v.f(0)),
        Value::Float2(_) => Value::Float2([v.f(0), v.f(1)]),
        Value::Float3(_) => Value::Float3([v.f(0), v.f(1), v.f(2)]),
        Value::Float4(_) => Value::Float4([v.f(0), v.f(1), v.f(2), v.f(3)]),
        Value::Color(_) => {
            // SBS colors are stored as displayed (sRGB); MaterialCraft's are linear.
            if v.len() >= 3 {
                Value::Color([srgb_to_linear(v.f(0)), srgb_to_linear(v.f(1)), srgb_to_linear(v.f(2)), if v.len() >= 4 { v.f(3) } else { 1.0 }])
            } else {
                let g = srgb_to_linear(v.f(0));
                Value::Color([g, g, g, 1.0])
            }
        }
        Value::Int(_) => Value::Int(v.i(0)),
        Value::Int2(_) => Value::Int2([v.i(0), v.i(1)]),
        Value::Bool(_) => Value::Bool(v.b()),
        Value::Enum(_) => Value::Enum(v.i(0).clamp(0, options.saturating_sub(1).min(i32::MAX as usize) as i32) as u32),
        Value::Text(_) => match v {
            SVal::S(s) => Value::Text(s.clone()),
            _ => return None,
        },
        Value::Curve(_) | Value::Gradient(_) | Value::Function(_) => return None,
    })
}

/// An SBS constant without a MaterialCraft parameter to match (exposed graph inputs).
fn natural(v: &SVal) -> Value {
    match v {
        SVal::B(b) => Value::Bool(*b),
        SVal::S(s) => Value::Text(s.clone()),
        SVal::I(n) => match n.len() {
            0 | 1 => Value::Int(n.first().copied().unwrap_or(0)),
            _ => Value::Int2([v.i(0), v.i(1)]),
        },
        SVal::F(n) => match n.len() {
            0 | 1 => Value::Float(v.f(0)),
            2 => Value::Float2([v.f(0), v.f(1)]),
            3 => Value::Float3([v.f(0), v.f(1), v.f(2)]),
            _ => Value::Float4([v.f(0), v.f(1), v.f(2), v.f(3)]),
        },
    }
}

/// An exposed input's value type from its SBS type code.
fn exposed_default(code: i64, v: Option<&SVal>) -> Value {
    let v = v.cloned().unwrap_or(SVal::F(vec![0.0]));
    match code {
        4 => Value::Bool(v.b()),
        16 => Value::Int(v.i(0)),
        32 | 64 | 128 => Value::Int2([v.i(0), v.i(1)]),
        256 => Value::Float(v.f(0)),
        512 => Value::Float2([v.f(0), v.f(1)]),
        1024 => Value::Float3([v.f(0), v.f(1), v.f(2)]),
        2048 => Value::Float4([v.f(0), v.f(1), v.f(2), v.f(3)]),
        16384 => match v {
            SVal::S(s) => Value::Text(s),
            _ => Value::Text(String::new()),
        },
        _ => natural(&v),
    }
}

fn exposed_code(v: &Value) -> u32 {
    match v {
        Value::Bool(_) => 4,
        Value::Int(_) | Value::Enum(_) => 16,
        Value::Int2(_) => 32,
        Value::Float(_) => 256,
        Value::Float2(_) => 512,
        Value::Float3(_) => 1024,
        Value::Float4(_) | Value::Color(_) => 2048,
        Value::Text(_) => 16384,
        Value::Curve(_) | Value::Gradient(_) | Value::Function(_) => 256,
    }
}

fn fmt_f(v: f32) -> String {
    if v.is_finite() { format!("{v}") } else { "0".into() }
}

/// The SBS constant element for `v` (`None` for curves, gradients and functions).
fn constant_xml(v: &Value) -> Option<String> {
    let join = |xs: &[f32]| xs.iter().map(|x| fmt_f(*x)).collect::<Vec<_>>().join(" ");
    let (tag, text) = match v {
        Value::Float(x) => ("constantValueFloat1", fmt_f(*x)),
        Value::Float2(x) => ("constantValueFloat2", join(x)),
        Value::Float3(x) => ("constantValueFloat3", join(x)),
        Value::Float4(x) => ("constantValueFloat4", join(x)),
        Value::Color(c) => ("constantValueFloat4", join(&[linear_to_srgb(c[0]), linear_to_srgb(c[1]), linear_to_srgb(c[2]), c[3]])),
        Value::Int(i) => ("constantValueInt32", i.to_string()),
        Value::Int2(i) => ("constantValueInt2", format!("{} {}", i[0], i[1])),
        Value::Enum(e) => ("constantValueInt32", e.to_string()),
        Value::Bool(b) => ("constantValueBool", u8::from(*b).to_string()),
        Value::Text(s) => ("constantValueString", s.clone()),
        Value::Curve(_) | Value::Gradient(_) | Value::Function(_) => return None,
    };
    Some(format!("<{tag} v=\"{}\"/>", esc(&text)))
}

// ---------------------------------------------------------------------------------------------
// XML helpers
// ---------------------------------------------------------------------------------------------

fn child<'a, 'i>(n: X<'a, 'i>, tag: &str) -> Option<X<'a, 'i>> {
    n.children().find(|c| c.is_element() && c.tag_name().name() == tag)
}

fn kids<'a, 'i: 'a>(n: X<'a, 'i>, tag: &'a str) -> impl Iterator<Item = X<'a, 'i>> + 'a {
    n.children().filter(move |c| c.is_element() && c.tag_name().name() == tag)
}

/// The `v` attribute of child `tag`.
fn val<'a>(n: X<'a, '_>, tag: &str) -> Option<&'a str> {
    child(n, tag).and_then(|c| c.attribute("v"))
}

/// XML-escape `s` for attribute values and text, dropping characters XML 1.0 can't hold.
fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            '\t' => o.push_str("&#9;"),
            '\n' => o.push_str("&#10;"),
            '\r' => o.push_str("&#13;"),
            c if (c as u32) < 0x20 || c == '\u{fffe}' || c == '\u{ffff}' => {}
            c => o.push(c),
        }
    }
    o
}

/// FNV-1a, 64 bits: the hash of the `<content>` an exact copy belongs to.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3))
}

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// Embedded data: a big-endian u32 length followed by the zlib-compressed bytes, in base64.
fn decode_embedded(strdata: &str) -> std::result::Result<Vec<u8>, String> {
    let raw = b64().decode(strdata.split_whitespace().collect::<String>()).map_err(|e| format!("bad base64: {e}"))?;
    let (len, z) = raw.split_at_checked(4).ok_or("too short")?;
    let n = u32::from_be_bytes([len[0], len[1], len[2], len[3]]) as usize;
    if n > MAX_EMBEDDED {
        return Err(format!("{n} bytes is over the {MAX_EMBEDDED}-byte limit"));
    }
    let mut out = Vec::with_capacity(n.min(1 << 24));
    flate2::read::ZlibDecoder::new(z).take(MAX_EMBEDDED as u64 + 1).read_to_end(&mut out).map_err(|e| format!("bad compressed data: {e}"))?;
    if out.len() > MAX_EMBEDDED {
        return Err("over the size limit".into());
    }
    Ok(out)
}

fn encode_embedded(bytes: &[u8]) -> Result<String> {
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(bytes).map_err(|e| FormatError::Io(e.to_string()))?;
    let packed = z.finish().map_err(|e| FormatError::Io(e.to_string()))?;
    let len = u32::try_from(bytes.len()).map_err(|_| FormatError::Io("resource too large".into()))?;
    let mut raw = len.to_be_bytes().to_vec();
    raw.extend_from_slice(&packed);
    Ok(b64().encode(raw))
}

// ---------------------------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------------------------

/// Read an SBS package. `name` is the file name (its stem names the package); `linked` loads a
/// file the package links to by a path relative to the package (bitmaps not embedded in it), or
/// returns `None` (the browser, a sandbox, or a missing file).
pub fn read_sbs(bytes: &[u8], name: &str, linked: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<SbsRead> {
    if bytes.len() > MAX_XML {
        return Err(FormatError::BadPackage(format!("the SBS file is over the {} MB limit", MAX_XML >> 20)));
    }
    let text = std::str::from_utf8(bytes).map_err(|e| FormatError::BadPackage(format!("the SBS file is not UTF-8 text: {e}")))?;
    let text = text.trim_start_matches('\u{feff}');
    let opts = roxmltree::ParsingOptions { allow_dtd: false, nodes_limit: MAX_XML_NODES };
    let doc = roxmltree::Document::parse_with_options(text, opts).map_err(|e| FormatError::BadPackage(format!("the SBS file is not valid XML: {e}")))?;
    let root = doc.root_element();
    if root.tag_name().name() != "package" {
        return Err(FormatError::BadPackage("not an SBS package (the root element is not <package>)".into()));
    }
    let mut w = Warn::default();
    let stem = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let stem = stem.rsplit_once('.').map_or(stem, |(s, _)| s).trim();
    let pkg_name = match val(root, "identifier").map(str::trim) {
        Some(id) if !id.is_empty() && id != "Unsaved Package" => id.to_string(),
        _ if !stem.is_empty() => stem.to_string(),
        _ => "Untitled package".to_string(),
    };
    let content = child(root, "content");
    let mut r = Reader { text, w: &mut w, deps: HashMap::new(), graphs: HashMap::new(), resources: HashMap::new(), nodes: 0 };
    if let Some(deps) = child(root, "dependencies") {
        for d in kids(deps, "dependency") {
            if let (Some(uid), Some(file)) = (val(d, "uid"), val(d, "filename")) {
                r.deps.insert(uid.to_string(), file.to_string());
            }
        }
    }

    // Resources first: both the exact copy and the SBS graphs use them.
    let mut pkg = Package::new(&pkg_name);
    let mut graph_nodes: Vec<(String, X)> = Vec::new();
    if let Some(c) = content {
        r.collect(c, "", &mut pkg, &mut graph_nodes, linked);
    }

    // The exact copy, when the content is unchanged.
    if let Some(c) = content
        && let Some(exact) = read_exact(root, &text[c.range()], r.w)
    {
        let mut e = exact;
        for res in &mut e.resources {
            if let Some(b) = pkg.resources.iter().find(|x| x.name == res.name) {
                res.bytes = b.bytes.clone();
            } else if res.bytes.is_empty() {
                r.w.add(format!("resource `{}` is missing from the file", res.name));
            }
        }
        missing_report(&e, r.w);
        return Ok(SbsRead { pkg: e, warnings: w.finish(), exact: true });
    }

    // Name every graph first so instances can refer to graphs defined later in the file.
    let mut names = Vec::new();
    for (path, g) in graph_nodes.iter().take(MAX_GRAPHS) {
        let base = val(*g, "identifier").unwrap_or("Graph").trim();
        let unique = unique_name(&names, if base.is_empty() { "Graph" } else { base });
        r.graphs.insert(path.clone(), unique.clone());
        names.push(unique);
    }
    if graph_nodes.len() > MAX_GRAPHS {
        r.w.add(format!("only the first {MAX_GRAPHS} graphs were read"));
    }
    for ((_, g), name) in graph_nodes.iter().zip(names) {
        let graph = r.graph(*g, name);
        pkg.graphs.push(graph);
    }
    if pkg.graphs.is_empty() {
        r.w.add("the package has no graphs MaterialCraft can open; an empty graph was added");
        pkg.graphs.push(Graph::new("Graph"));
    }
    missing_report(&pkg, r.w);
    Ok(SbsRead { pkg, warnings: w.finish(), exact: false })
}

/// One summary line per missing node kind and graph.
fn missing_report(pkg: &Package, w: &mut Warn) {
    for g in &pkg.graphs {
        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
        for n in g.nodes.iter().filter(|n| n.def == MISSING_NODE) {
            let name = match n.params.get("sbs_name") {
                Some(Value::Text(s)) if !s.is_empty() => s.clone(),
                _ => n.label.clone(),
            };
            *kinds.entry(name).or_default() += 1;
        }
        for (k, n) in kinds {
            let times = if n > 1 { format!(" ×{n}") } else { String::new() };
            w.add(format!("graph `{}`: missing node `{k}`{times} is not in MaterialCraft's library yet; kept as a placeholder", g.name));
        }
    }
}

/// The exact MaterialCraft copy, if present and written for exactly this `content`.
fn read_exact(root: X, content: &str, w: &mut Warn) -> Option<Package> {
    let comment = root.children().filter(|c| c.is_comment()).filter_map(|c| c.text()).find(|t| t.trim_start().starts_with(EXACT_MARK))?;
    let mut version = 0;
    let mut hash = String::new();
    let mut data = "";
    for part in comment.split_whitespace() {
        if let Some(v) = part.strip_prefix("version=") {
            version = v.parse().unwrap_or(0);
        } else if let Some(v) = part.strip_prefix("hash=") {
            hash = v.to_string();
        } else if let Some(v) = part.strip_prefix("data=") {
            data = v;
        }
    }
    if version != EXACT_VERSION {
        w.add("the MaterialCraft data in this file is from a newer version; the graphs were read from the SBS data");
        return None;
    }
    if hash != format!("{:016x}", fnv(content.as_bytes())) {
        w.add("this file was changed outside MaterialCraft since it was saved; the graphs were read from the SBS data");
        return None;
    }
    match decode_embedded(data).and_then(|json| serde_json::from_slice::<Package>(&json).map_err(|e| e.to_string())) {
        Ok(p) => Some(p),
        Err(e) => {
            w.add(format!("the MaterialCraft data in this file is damaged ({e}); the graphs were read from the SBS data"));
            None
        }
    }
}

fn unique_name(taken: &[String], base: &str) -> String {
    if !taken.iter().any(|t| t == base) {
        return base.to_string();
    }
    (1..).map(|i| format!("{base}_{i}")).find(|n| !taken.iter().any(|t| t == n)).unwrap_or_else(|| base.to_string())
}

struct Reader<'t, 'w> {
    text: &'t str,
    w: &'w mut Warn,
    /// Dependency uid → file name.
    deps: HashMap<String, String>,
    /// Package path (`Folder/Graph`) → graph name.
    graphs: HashMap<String, String>,
    /// Package path (`Resources/Bitmap`) → resource name.
    resources: HashMap<String, String>,
    nodes: usize,
}

/// A node being read: its id, SBS input identifier → port, and SBS output uid → port.
struct ReadNode {
    id: NodeId,
    inputs: HashMap<String, String>,
    /// (SBS output uid, port), in output order.
    outputs: Vec<(String, String)>,
    /// For placeholders: inputs are whatever wires arrive.
    open_inputs: bool,
}

impl<'t> Reader<'t, '_> {
    /// Walk `content` (and nested groups): queue graphs, load resources, note what is skipped.
    fn collect<'a>(
        &mut self,
        content: X<'a, 't>,
        folder: &str,
        pkg: &mut Package,
        graphs: &mut Vec<(String, X<'a, 't>)>,
        linked: &dyn Fn(&str) -> Option<Vec<u8>>,
    ) {
        for item in content.children().filter(|c| c.is_element()) {
            let id = val(item, "identifier").unwrap_or("").trim().to_string();
            let path = if folder.is_empty() { id.clone() } else { format!("{folder}/{id}") };
            match item.tag_name().name() {
                "graph" => graphs.push((path, item)),
                "group" => {
                    if let Some(c) = child(item, "content") {
                        self.collect(c, &path, pkg, graphs, linked);
                    }
                }
                "resource" => {
                    if pkg.resources.len() >= MAX_RESOURCES {
                        self.w.add(format!("only the first {MAX_RESOURCES} resources were read"));
                        continue;
                    }
                    self.resource(item, &id, &path, pkg, linked);
                }
                "function" => self.w.add(format!("function graph `{path}` is not imported yet (SBS function graphs)")),
                "mdl_graph" | "mdlGraph" => self.w.add(format!("MDL graph `{path}` is not supported")),
                other => self.w.add(format!("package item <{other}> `{path}` is not supported")),
            }
        }
    }

    fn resource(&mut self, item: X, id: &str, path: &str, pkg: &mut Package, linked: &dyn Fn(&str) -> Option<Vec<u8>>) {
        let kind = val(item, "type").unwrap_or("bitmap");
        let format = val(item, "format").unwrap_or("png").trim().to_ascii_lowercase();
        let source = child(item, "source");
        let mut bytes: Option<Vec<u8>> = None;
        let mut why = String::new();
        if let Some(e) = source.and_then(|s| child(s, "binembedded")) {
            match decode_embedded(val(e, "strdata").unwrap_or("")) {
                Ok(b) => bytes = Some(b),
                Err(e) => why = format!("its embedded data is damaged ({e})"),
            }
        }
        if bytes.is_none() {
            let copy = source.and_then(|s| child(s, "externalcopy")).and_then(|e| val(e, "filename"));
            let file = val(item, "filepath");
            for p in [copy, file].into_iter().flatten().filter(|p| !p.trim().is_empty()) {
                if p.contains("://") {
                    why = format!("it links to `{p}`, which is not available");
                    continue;
                }
                match linked(p) {
                    Some(b) => {
                        bytes = Some(b);
                        break;
                    }
                    None => why = format!("its file `{p}` was not found next to the package"),
                }
            }
        }
        let name = {
            let taken: Vec<String> = pkg.resources.iter().map(|r| r.name.clone()).collect();
            unique_name(&taken, if id.is_empty() { "resource" } else { id })
        };
        self.resources.insert(path.to_string(), name.clone());
        match bytes {
            Some(b) => {
                let ext = if kind == "svg" { "svg".to_string() } else { format };
                pkg.resources.push(Resource { name: name.clone(), file: crate::safe_file_name(&format!("{name}.{ext}")), bytes: Arc::new(b) });
            }
            None => {
                let why = if why.is_empty() { "it has no data".to_string() } else { why };
                self.w.add(format!("resource `{path}` could not be loaded: {why}"));
            }
        }
    }

    /// A package path in a `pkg:///path?dependency=uid` reference: (path, dependency file).
    fn reference(&self, url: &str) -> (String, Option<String>) {
        let rest = url.trim().strip_prefix("pkg:///").or_else(|| url.trim().strip_prefix("pkg://")).unwrap_or(url.trim());
        let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
        let dep = query.split('&').find_map(|kv| kv.strip_prefix("dependency=")).and_then(|uid| self.deps.get(uid).cloned());
        (path.trim_matches('/').to_string(), dep)
    }

    fn graph(&mut self, g: X<'_, 't>, name: String) -> Graph {
        let mut graph = Graph::new(&name);
        // Graph size: the default parent size option ("11x11"), else 1024 px.
        if let Some(opts) = child(g, "options") {
            for o in kids(opts, "option") {
                if val(o, "name") == Some("defaultParentSize")
                    && let Some((a, b)) = val(o, "value").and_then(|v| v.split_once('x'))
                {
                    let p = |s: &str| s.trim().parse::<i64>().unwrap_or(10).clamp(i64::from(MIN_SIZE_LOG2), i64::from(MAX_SIZE_LOG2)) as u32;
                    graph.size = [p(a), p(b)];
                }
            }
        }
        if let Some(a) = child(g, "attributes") {
            graph.attributes.category = val(a, "category").unwrap_or("").to_string();
            graph.attributes.description = val(a, "description").unwrap_or("").to_string();
            graph.attributes.author = val(a, "author").unwrap_or("").to_string();
            graph.attributes.tags = val(a, "tags").unwrap_or("").to_string();
        }
        // Graph inputs: images (fed by input bridge nodes) and exposed values.
        let mut image_inputs: HashMap<String, (String, bool)> = HashMap::new();
        for p in child(g, "paraminputs").into_iter().flat_map(|p| kids(p, "paraminput")) {
            let id = val(p, "identifier").unwrap_or("input").to_string();
            let uid = val(p, "uid").unwrap_or("").to_string();
            let code = val(p, "type").and_then(|t| t.trim().parse::<i64>().ok()).unwrap_or(256);
            if code == 1 || code == 2 {
                image_inputs.insert(uid, (id, code == 2));
                continue;
            }
            if graph.exposed.iter().any(|e| e.id == id) {
                continue;
            }
            let default = exposed_default(code, child(p, "defaultValue").and_then(constant).as_ref());
            let label = child(p, "attributes").and_then(|a| val(a, "label")).filter(|l| !l.is_empty()).unwrap_or(&id).to_string();
            let (mut min, mut max) = (0.0, 1.0);
            for o in child(p, "defaultWidget").and_then(|d| child(d, "options")).into_iter().flat_map(|o| kids(o, "option")) {
                let v = val(o, "value").and_then(|v| v.split_whitespace().next()).and_then(|v| v.parse::<f32>().ok()).filter(|v| v.is_finite());
                match (val(o, "name"), v) {
                    (Some("min"), Some(v)) => min = v,
                    (Some("max"), Some(v)) => max = v,
                    _ => {}
                }
            }
            graph.exposed.push(ExposedParam { id, label, group: val(p, "group").unwrap_or("").to_string(), default, min, max, options: Vec::new() });
        }
        let mut outputs: HashMap<String, (String, u32)> = HashMap::new();
        for o in child(g, "graphOutputs").into_iter().flat_map(|o| kids(o, "graphoutput")) {
            let id = val(o, "identifier").unwrap_or("output").to_string();
            let usage = child(o, "usages").and_then(|u| child(u, "usage")).and_then(|u| val(u, "name")).map_or(usage_index("any"), usage_index);
            outputs.insert(val(o, "uid").unwrap_or("").to_string(), (id, usage));
        }

        // Nodes, then wires.
        let mut read: HashMap<String, ReadNode> = HashMap::new();
        let comp: Vec<X> = child(g, "compNodes").into_iter().flat_map(|c| kids(c, "compNode")).collect();
        for n in &comp {
            if self.nodes >= MAX_NODES {
                self.w.add(format!("only the first {MAX_NODES} nodes were read"));
                break;
            }
            let Some(uid) = val(*n, "uid").map(str::to_string) else { continue };
            if read.contains_key(&uid) {
                self.w.add(format!("graph `{name}`: duplicate node uid {uid} skipped"));
                continue;
            }
            self.nodes += 1;
            let node = self.node(&mut graph, *n, &image_inputs, &outputs);
            read.insert(uid, node);
        }
        for n in &comp {
            let Some(to) = val(*n, "uid").and_then(|u| read.get(u)) else { continue };
            let (to_id, open) = (to.id, to.open_inputs);
            for c in child(*n, "connections").into_iter().flat_map(|c| kids(c, "connection")) {
                let sbs_in = val(c, "identifier").unwrap_or("").to_string();
                let Some(from) = val(c, "connRef").and_then(|u| read.get(u)) else {
                    self.w.add(format!("graph `{name}`: a wire from a node that is not in the file was dropped"));
                    continue;
                };
                let out_uid = val(c, "connRefOutput").unwrap_or("");
                let Some(from_port) = from.outputs.iter().find(|(u, _)| u == out_uid).or(from.outputs.first()).map(|(_, p)| p.clone()) else {
                    self.w.add(format!("graph `{name}`: a wire from a node with no outputs was dropped"));
                    continue;
                };
                let from_id = from.id;
                let to_port = match to.inputs.get(&sbs_in).cloned() {
                    Some(p) => p,
                    None if open => {
                        add_missing_input(&mut graph, to_id, &sbs_in);
                        sbs_in.clone()
                    }
                    None => {
                        let def = graph.node(to_id).map(|n| n.def.clone()).unwrap_or_default();
                        self.w.add(format!("graph `{name}`: `{def}` has no input `{sbs_in}`; its wire was dropped"));
                        continue;
                    }
                };
                if graph.connect(from_id, &from_port, to_id, &to_port).is_err() {
                    self.w.add(format!("graph `{name}`: a wire that would make a cycle was dropped"));
                }
            }
        }

        // Frames and comments.
        for o in child(g, "GUIObjects").into_iter().flat_map(|o| kids(o, "GUIObject")) {
            let layout = child(o, "GUILayout");
            let pos = layout.and_then(|l| val(l, "gpos")).map(nums_f).unwrap_or_default();
            let size = layout.and_then(|l| val(l, "size")).map(nums_f).unwrap_or_default();
            let (x, y) = (pos.first().copied().unwrap_or(0.0), pos.get(1).copied().unwrap_or(0.0));
            let (sw, sh) = (size.first().copied().unwrap_or(180.0).max(8.0), size.get(1).copied().unwrap_or(70.0).max(8.0));
            let text = val(o, "GUIName").unwrap_or("").to_string();
            match val(o, "type") {
                Some("COMMENT") if val(o, "isFrameVisible") == Some("1") => {
                    let c = val(o, "frameColor").map(nums_f).unwrap_or_default();
                    let color = if c.len() >= 4 { [srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2]), c[3]] } else { [0.2, 0.4, 0.8, 0.25] };
                    let id = graph.alloc_id();
                    graph.frames.push(Frame { id, title: val(o, "title").unwrap_or("").to_string(), description: text, rect: [x, y, sw, sh], color });
                }
                Some("COMMENT") => {
                    let id = graph.alloc_id();
                    graph.comments.push(Comment { id, text, pos: [x, y], size: [sw, sh] });
                }
                Some(other) => self.w.add(format!("graph `{name}`: canvas item `{other}` is not supported")),
                None => {}
            }
        }
        graph
    }

    /// Read one `compNode` into `graph`.
    fn node(&mut self, graph: &mut Graph, n: X<'_, 't>, image_inputs: &HashMap<String, (String, bool)>, outputs: &HashMap<String, (String, u32)>) -> ReadNode {
        let gpos = child(n, "GUILayout").and_then(|l| val(l, "gpos")).map(nums_f).unwrap_or_default();
        let pos = [gpos.first().copied().unwrap_or(0.0) - NODE_HALF[0], gpos.get(1).copied().unwrap_or(0.0) - NODE_HALF[1]];
        let out_uids: Vec<(String, bool)> = child(n, "compOutputs")
            .into_iter()
            .flat_map(|o| kids(o, "compOutput"))
            .map(|o| (val(o, "uid").unwrap_or("").to_string(), val(o, "comptype") == Some("2")))
            .collect();
        let gname = graph.name.clone();
        let Some(imp) = child(n, "compImplementation").and_then(|i| i.children().find(|c| c.is_element())) else {
            let id = graph.add_node(MISSING_NODE, pos);
            self.missing(graph, id, "other", "empty node", "", None, &out_uids, None);
            return ReadNode { id, inputs: HashMap::new(), outputs: Vec::new(), open_inputs: true };
        };
        let params = child(imp, "parameters");
        let all_to = |port: &str| out_uids.iter().map(|(u, _)| (u.clone(), port.to_string())).collect::<Vec<_>>();
        match imp.tag_name().name() {
            "compInputBridge" => {
                let entry = val(imp, "entry").unwrap_or("");
                let (ident, gray) = image_inputs.get(entry).cloned().unwrap_or_else(|| ("input".into(), out_uids.first().is_some_and(|o| o.1)));
                let id = graph.add_node(if gray { "input_grayscale" } else { "input_color" }, pos);
                if let Ok(node) = graph.node_mut(id) {
                    node.params.insert("identifier".into(), Value::Text(ident));
                    self.base(node, params, &gname);
                }
                ReadNode { id, inputs: HashMap::new(), outputs: all_to("output"), open_inputs: false }
            }
            "compOutputBridge" => {
                let out = val(imp, "output").unwrap_or("");
                let (ident, usage) = outputs.get(out).cloned().unwrap_or_else(|| ("output".into(), usage_index("any")));
                let id = graph.add_node("output", pos);
                if let Ok(node) = graph.node_mut(id) {
                    node.params.insert("identifier".into(), Value::Text(ident));
                    node.params.insert("usage".into(), Value::Enum(usage));
                }
                ReadNode { id, inputs: [("inputNodeOutput".to_string(), "input".to_string())].into_iter().collect(), outputs: Vec::new(), open_inputs: false }
            }
            "compFilter" => {
                let filter = val(imp, "filter").unwrap_or("").to_string();
                match filter_by_sbs(&filter).filter(|f| find(f.def).is_some()) {
                    Some(f) => {
                        let id = graph.add_node(f.def, pos);
                        if let Ok(node) = graph.node_mut(id) {
                            self.filter_params(node, f, params, &gname);
                            self.base(node, params, &gname);
                        }
                        let inputs = f.inputs.iter().map(|(s, m)| (s.to_string(), m.to_string())).collect();
                        ReadNode { id, inputs, outputs: all_to("output"), open_inputs: false }
                    }
                    None => {
                        let id = graph.add_node(MISSING_NODE, pos);
                        let text = self.text;
                        let raw = params.map(|p| &text[p.range()]);
                        self.missing(graph, id, "filter", &filter, "", raw, &out_uids, None);
                        let outs = missing_outputs(&out_uids, None);
                        ReadNode { id, inputs: HashMap::new(), outputs: outs, open_inputs: true }
                    }
                }
            }
            "compInstance" => self.instance(graph, imp, pos, &out_uids),
            other => {
                let id = graph.add_node(MISSING_NODE, pos);
                self.missing(graph, id, "other", other, "", None, &out_uids, None);
                ReadNode { id, inputs: HashMap::new(), outputs: missing_outputs(&out_uids, None), open_inputs: true }
            }
        }
    }

    fn instance(&mut self, graph: &mut Graph, imp: X<'_, 't>, pos: [f32; 2], out_uids: &[(String, bool)]) -> ReadNode {
        let gname = graph.name.clone();
        let (path, dep) = self.reference(val(imp, "path").unwrap_or(""));
        let params = child(imp, "parameters");
        let bridgings: Vec<(String, String)> = child(imp, "outputBridgings")
            .into_iter()
            .flat_map(|b| kids(b, "outputBridging"))
            .map(|b| (val(b, "uid").unwrap_or("").to_string(), val(b, "identifier").unwrap_or("output").to_string()))
            .collect();
        let own = dep.as_deref().is_none_or(|d| d == SELF_DEP) && self.graphs.contains_key(&path);
        if own {
            let sub = self.graphs.get(&path).cloned().unwrap_or_default();
            let id = graph.add_node("graph_instance", pos);
            if let Ok(node) = graph.node_mut(id) {
                node.params.insert("graph".into(), Value::Text(sub));
                for p in params.into_iter().flat_map(|p| kids(p, "parameter")) {
                    let pname = val(p, "name").unwrap_or("");
                    if BASE_PARAMS.contains(&pname) {
                        continue;
                    }
                    if let Some(v) = child(p, "paramValue").and_then(constant) {
                        node.params.insert(pname.to_string(), natural(&v));
                    }
                }
                self.base(node, params, &gname);
            }
            return ReadNode { id, inputs: HashMap::new(), outputs: bridgings, open_inputs: true };
        }
        let short = path.rsplit('/').next().unwrap_or(&path).to_string();
        let def = find(&short).filter(|d| d.id != "graph_instance" && !d.id.starts_with("input_") && d.id != "output");
        let Some(def) = def else {
            let id = graph.add_node(MISSING_NODE, pos);
            let text = self.text;
            let raw = params.map(|p| &text[p.range()]);
            self.missing(graph, id, "instance", &short, dep.as_deref().unwrap_or(""), raw, out_uids, Some(&bridgings));
            // Keep the full path for writing back.
            if let Ok(node) = graph.node_mut(id) {
                node.params.insert("sbs_path".into(), Value::Text(path));
            }
            return ReadNode { id, inputs: HashMap::new(), outputs: missing_outputs(out_uids, Some(&bridgings)), open_inputs: true };
        };
        let id = graph.add_node(def.id, pos);
        if let Ok(node) = graph.node_mut(id) {
            for p in params.into_iter().flat_map(|p| kids(p, "parameter")) {
                let pname = val(p, "name").unwrap_or("");
                if BASE_PARAMS.contains(&pname) {
                    continue;
                }
                match def.param(pname) {
                    Some(pd) => self.set(node, pname, &pd.default, pd.options.len(), p, &gname),
                    None => self.w.add(format!("`{}`: parameter `{pname}` has no MaterialCraft equivalent and was ignored", def.id)),
                }
            }
            self.base(node, params, &gname);
        }
        let inputs = def.inputs.iter().map(|p| (p.id.to_string(), p.id.to_string())).collect();
        let outputs = bridgings
            .iter()
            .map(|(uid, ident)| {
                let port = if def.output_index(ident).is_some() { ident.clone() } else { def.outputs.first().map_or("output", |o| o.id).to_string() };
                (uid.clone(), port)
            })
            .chain(
                out_uids
                    .iter()
                    .filter(|(u, _)| !bridgings.iter().any(|b| &b.0 == u))
                    .map(|(u, _)| (u.clone(), def.outputs.first().map_or("output", |o| o.id).to_string())),
            )
            .collect::<Vec<_>>();
        ReadNode { id, inputs, outputs, open_inputs: false }
    }

    /// Fill in a [`MISSING_NODE`] placeholder.
    #[allow(clippy::too_many_arguments)]
    fn missing(
        &mut self,
        graph: &mut Graph,
        id: NodeId,
        kind: &str,
        name: &str,
        package: &str,
        raw: Option<&str>,
        outs: &[(String, bool)],
        bridgings: Option<&[(String, String)]>,
    ) {
        let Ok(node) = graph.node_mut(id) else { return };
        node.label = name.to_string();
        node.params.insert("sbs_kind".into(), Value::Text(kind.into()));
        node.params.insert("sbs_name".into(), Value::Text(name.into()));
        if !package.is_empty() {
            node.params.insert("sbs_package".into(), Value::Text(package.into()));
        }
        if let Some(raw) = raw {
            node.params.insert("sbs_parameters".into(), Value::Text(raw.into()));
        }
        let mut seen = BTreeSet::new();
        let list: Vec<String> = missing_outputs(outs, bridgings)
            .into_iter()
            .map(|(uid, port)| {
                let gray = outs.iter().any(|(u, g)| *u == uid && *g);
                (port, gray)
            })
            .filter(|(p, _)| seen.insert(p.clone()))
            .map(|(p, gray)| if gray { format!("{p}:gray") } else { p })
            .collect();
        node.params.insert("outputs".into(), Value::Text(list.join(",")));
        node.params.insert("inputs".into(), Value::Text(String::new()));
    }

    /// Set MaterialCraft parameter `id` (default `like`) from SBS `<parameter>` `p`.
    fn set(&mut self, node: &mut Node, id: &str, like: &Value, options: usize, p: X, graph: &str) {
        let Some(pv) = child(p, "paramValue") else { return };
        if let Some(v) = constant(pv) {
            match coerce(like, &v, options) {
                Some(v) => {
                    node.params.insert(id.to_string(), v);
                }
                None => self.w.add(format!("`{}`: parameter `{id}` has a value type MaterialCraft can't read yet; kept the default", node.def)),
            }
            return;
        }
        if let Some(d) = child(pv, "dynamicValue") {
            // A parameter linked to a graph input: one `get_*` node naming it.
            let fn_nodes: Vec<X> = child(d, "paramNodes").into_iter().flat_map(|p| kids(p, "paramNode")).collect();
            if let [only] = fn_nodes.as_slice()
                && val(*only, "function").is_some_and(|f| f.starts_with("get_"))
                && let Some(var) = only.descendants().find(|c| c.tag_name().name() == "constantValueString").and_then(|c| c.attribute("v"))
            {
                node.bindings.insert(id.to_string(), var.to_string());
                return;
            }
            self.w.add(format!("graph `{graph}`: `{}` parameter `{id}` is driven by a function; imported its default", node.def));
        }
    }

    fn filter_params(&mut self, node: &mut Node, f: &FilterMap, params: Option<X>, graph: &str) {
        let Some(def) = find(f.def) else { return };
        for p in params.into_iter().flat_map(|p| kids(p, "parameter")) {
            let pname = val(p, "name").unwrap_or("");
            if BASE_PARAMS.contains(&pname) {
                continue;
            }
            let value = child(p, "paramValue").and_then(constant);
            if let Some(&(_, mc, conv)) = f.params.iter().find(|(s, _, _)| *s == pname) {
                let Some(pd) = def.param(mc) else { continue };
                if conv == BoolFirst {
                    if let Some(v) = value {
                        node.params.insert(mc.into(), Value::Enum(if v.b() { 0 } else { 1 }));
                    }
                } else {
                    self.set(node, mc, &pd.default, pd.options.len(), p, graph);
                }
                continue;
            }
            match (f.sbs, pname, value) {
                ("bitmap", "bitmapresourcepath", Some(SVal::S(url))) => {
                    let (path, _) = self.reference(&url);
                    match self.resources.get(&path) {
                        Some(name) => {
                            node.params.insert("resource".into(), Value::Text(name.clone()));
                        }
                        None => self.w.add(format!("graph `{graph}`: Bitmap uses resource `{path}`, which is not in the package")),
                    }
                }
                ("grayscaleconversion", "channelsweights", Some(v)) => {
                    for (i, c) in ["red", "green", "blue", "alpha"].into_iter().enumerate() {
                        node.params.insert(c.into(), Value::Float(v.f(i)));
                    }
                }
                ("transformation", "matrix22", Some(v)) => {
                    let (a, b, c, d) = (v.f(0), v.f(1), v.f(2), v.f(3));
                    let sx = (a * a + c * c).sqrt();
                    let sy = (b * b + d * d).sqrt();
                    if sx > 1e-6 && sy > 1e-6 {
                        node.params.insert("scale".into(), Value::Float2([sx, sy]));
                        node.params.insert("rotation".into(), Value::Float(c.atan2(a) / std::f32::consts::TAU));
                    }
                }
                _ if HANDLED_PARAMS.contains(&(f.sbs, pname)) => {}
                _ => self.w.add(format!("`{}`: SBS parameter `{pname}` is not supported yet and was ignored", f.def)),
            }
        }
        if matches!(f.sbs, "curve" | "gradient" | "pixelprocessor" | "valueprocessor" | "svg" | "fxmaps")
            && params.is_some_and(|p| kids(p, "parameter").any(|x| !BASE_PARAMS.contains(&val(x, "name").unwrap_or(""))))
        {
            let what = match f.sbs {
                "curve" => "curve points",
                "gradient" => "gradient keys",
                "pixelprocessor" | "valueprocessor" => "function graph",
                "svg" => "vector shapes",
                _ => "pattern settings",
            };
            self.w.add(format!("graph `{graph}`: the {what} of `{}` nodes are not imported yet; they use MaterialCraft's defaults", f.def));
        }
    }

    /// Base parameters: output size, format and random seed.
    fn base(&mut self, node: &mut Node, params: Option<X>, _graph: &str) {
        let mut base = BaseParams::default();
        for p in params.into_iter().flat_map(|p| kids(p, "parameter")) {
            let rel = val(p, "relativeTo").and_then(|r| r.trim().parse::<i32>().ok()).unwrap_or(1);
            let Some(v) = child(p, "paramValue").and_then(constant) else { continue };
            match val(p, "name") {
                Some("outputsize") => {
                    base.size_mode = match rel {
                        0 => SizeMode::Absolute,
                        2 => SizeMode::RelativeToParent,
                        _ => SizeMode::RelativeToInput,
                    };
                    let lim = |x: i32| x.clamp(-(MAX_SIZE_LOG2 as i32), MAX_SIZE_LOG2 as i32);
                    base.size = [lim(v.i(0)), lim(v.i(1))];
                }
                Some("format") if rel == 0 => {
                    base.format = match v.i(0) {
                        0 => OutputFormat::U8,
                        1 => OutputFormat::U16,
                        2 => OutputFormat::F16,
                        3 => OutputFormat::F32,
                        _ => OutputFormat::Inherit,
                    }
                }
                Some("randomseed") if rel == 0 => base.seed = v.i(0),
                _ => {}
            }
        }
        node.base = base;
    }
}

/// (Output uid, port) of a placeholder, in output order: the instance's bridgings, else
/// `output`, `output_2`…
fn missing_outputs(outs: &[(String, bool)], bridgings: Option<&[(String, String)]>) -> Vec<(String, String)> {
    let mut m: Vec<(String, String)> = Vec::new();
    for (i, (uid, _)) in outs.iter().enumerate() {
        let port = bridgings
            .and_then(|b| b.iter().find(|(u, _)| u == uid))
            .map(|(_, id)| id.clone())
            .unwrap_or_else(|| if i == 0 { "output".to_string() } else { format!("output_{}", i + 1) });
        m.push((uid.clone(), port));
    }
    for (uid, id) in bridgings.unwrap_or_default() {
        if !m.iter().any(|(u, _)| u == uid) {
            m.push((uid.clone(), id.clone()));
        }
    }
    m
}

/// Record a wire arriving at placeholder input `port`.
fn add_missing_input(graph: &mut Graph, id: NodeId, port: &str) {
    if port.is_empty() || port.contains(',') {
        return;
    }
    let Ok(node) = graph.node_mut(id) else { return };
    let (ins, _) = missing_ports(node);
    if ins.iter().any(|p| p == port) {
        return;
    }
    let mut ins = ins;
    ins.push(port.to_string());
    node.params.insert("inputs".into(), Value::Text(ins.join(",")));
}

// ---------------------------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------------------------

/// Write `pkg` as an SBS package (UTF-8 XML), with bitmaps embedded and MaterialCraft's exact
/// copy at the end.
pub fn write_sbs(pkg: &Package) -> Result<Vec<u8>> {
    let mut wr = Writer { uid: 1_000_000_000, deps: BTreeMap::new() };
    let self_uid = wr.next();
    let lib_uid = wr.next();
    let mut content = String::from("<content>");
    // Graph name → uid, for instances.
    for g in &pkg.graphs {
        wr.graph(&mut content, pkg, g, self_uid, lib_uid)?;
    }
    if !pkg.resources.is_empty() {
        let _ = write!(content, "<group><uid v=\"{}\"/><identifier v=\"Resources\"/><content>", wr.next());
        for r in &pkg.resources {
            let svg = r.file.to_ascii_lowercase().ends_with(".svg");
            let ext = r.file.rsplit_once('.').map_or("png", |(_, e)| e);
            let _ = write!(
                content,
                "<resource><identifier v=\"{}\"/><uid v=\"{}\"/><type v=\"{}\"/><format v=\"{}\"/><filepath v=\"\"/><source><binembedded><datalength v=\"{}\"/><strdata v=\"{}\"/></binembedded></source></resource>",
                esc(&r.name),
                wr.next(),
                if svg { "svg" } else { "bitmap" },
                esc(ext),
                r.bytes.len(),
                encode_embedded(&r.bytes)?
            );
        }
        content.push_str("</content></group>");
    }
    content.push_str("</content>");

    let mut out = String::with_capacity(content.len() + 4096);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?><package>");
    let _ = write!(
        out,
        "<identifier v=\"{}\"/><formatVersion v=\"{FORMAT_VERSION}\"/><updaterVersion v=\"{FORMAT_VERSION}\"/><fileUID v=\"0\"/><versionUID v=\"0\"/><dependencies>",
        esc(&pkg.name)
    );
    let dep = |out: &mut String, file: &str, uid: u64| {
        let _ = write!(
            out,
            "<dependency><filename v=\"{}\"/><uid v=\"{uid}\"/><type v=\"package\"/><fileUID v=\"0\"/><versionUID v=\"0\"/></dependency>",
            esc(file)
        );
    };
    dep(&mut out, SELF_DEP, self_uid);
    dep(&mut out, LIBRARY_DEP, lib_uid);
    for (file, uid) in &wr.deps {
        dep(&mut out, file, *uid);
    }
    out.push_str("</dependencies>");
    out.push_str(&content);
    // The exact copy (resource bytes live in the resources above).
    let json = serde_json::to_vec(pkg).map_err(|e| FormatError::Io(e.to_string()))?;
    let _ = write!(out, "<!-- {EXACT_MARK} version={EXACT_VERSION} hash={:016x} data={} -->", fnv(content.as_bytes()), encode_embedded(&json)?);
    out.push_str("</package>\n");
    Ok(out.into_bytes())
}

struct Writer {
    uid: u64,
    /// Other packages placeholders come from: file name → dependency uid.
    deps: BTreeMap<String, u64>,
}

/// An output port's SBS type (1 color, 2 grayscale).
fn comptype(kind: PortKind) -> u8 {
    if kind == PortKind::Gray { 2 } else { 1 }
}

impl Writer {
    fn next(&mut self) -> u64 {
        self.uid = self.uid.saturating_add(1);
        self.uid
    }

    fn dep(&mut self, file: &str) -> u64 {
        if let Some(u) = self.deps.get(file) {
            return *u;
        }
        let u = self.next();
        self.deps.insert(file.to_string(), u);
        u
    }

    /// The output ports of `n` as (port id, SBS type).
    fn outputs(pkg: &Package, n: &Node) -> Vec<(String, u8)> {
        match n.def.as_str() {
            "output" => Vec::new(),
            "graph_instance" => {
                let sub = match n.params.get("graph") {
                    Some(Value::Text(name)) => pkg.graph_by_name(name).map(|(_, g)| g),
                    _ => None,
                };
                sub.map(|g| {
                    g.nodes
                        .iter()
                        .filter(|o| o.def == "output")
                        .map(|o| {
                            (
                                match o.params.get("identifier") {
                                    Some(Value::Text(s)) if !s.is_empty() => s.clone(),
                                    _ => "output".into(),
                                },
                                1,
                            )
                        })
                        .collect()
                })
                .unwrap_or_default()
            }
            d if d == MISSING_NODE => missing_ports(n).1.into_iter().map(|(id, gray)| (id, if gray { 2 } else { 1 })).collect(),
            d => find(d).map(|def| def.outputs.iter().map(|o| (o.id.to_string(), comptype(o.kind))).collect()).unwrap_or_else(|| vec![("output".into(), 1)]),
        }
    }

    fn graph(&mut self, out: &mut String, pkg: &Package, g: &Graph, self_uid: u64, lib_uid: u64) -> Result<()> {
        let _ = write!(out, "<graph><identifier v=\"{}\"/><uid v=\"{}\"/><graphtype v=\"material\"/>", esc(&g.name), self.next());
        let a = &g.attributes;
        if !(a.category.is_empty() && a.description.is_empty() && a.author.is_empty() && a.tags.is_empty()) {
            let _ = write!(
                out,
                "<attributes><category v=\"{}\"/><description v=\"{}\"/><author v=\"{}\"/><tags v=\"{}\"/></attributes>",
                esc(&a.category),
                esc(&a.description),
                esc(&a.author),
                esc(&a.tags)
            );
        }
        let text = |n: &Node, k: &str, d: &str| match n.params.get(k) {
            Some(Value::Text(s)) if !s.is_empty() => s.clone(),
            _ => d.to_string(),
        };
        // Node uids and output uids.
        let mut uids: HashMap<NodeId, u64> = HashMap::new();
        let mut outs: HashMap<NodeId, Vec<(String, u8, u64)>> = HashMap::new();
        for n in &g.nodes {
            uids.insert(n.id, self.next());
            let o = Self::outputs(pkg, n).into_iter().map(|(p, t)| (p, t, self.next())).collect();
            outs.insert(n.id, o);
        }
        // Graph inputs: image inputs (input nodes) and exposed values.
        let mut entries: HashMap<NodeId, u64> = HashMap::new();
        out.push_str("<paraminputs>");
        for n in g.nodes.iter().filter(|n| n.def == "input_color" || n.def == "input_grayscale") {
            let uid = self.next();
            entries.insert(n.id, uid);
            let gray = n.def == "input_grayscale";
            let d = match n.params.get("default") {
                Some(v @ Value::Color(_)) => constant_xml(v).unwrap_or_default(),
                _ => "<constantValueFloat4 v=\"0.5 0.5 0.5 1\"/>".into(),
            };
            let _ = write!(
                out,
                "<paraminput><identifier v=\"{}\"/><uid v=\"{uid}\"/><isConnectable v=\"1\"/><type v=\"{}\"/><defaultValue>{d}</defaultValue><defaultWidget><name v=\"\"/><options/></defaultWidget></paraminput>",
                esc(&text(n, "identifier", "input")),
                if gray { 2 } else { 1 }
            );
        }
        for e in &g.exposed {
            let d = constant_xml(&e.default).unwrap_or_else(|| "<constantValueFloat1 v=\"0\"/>".into());
            let _ = write!(
                out,
                "<paraminput><identifier v=\"{}\"/><uid v=\"{}\"/><attributes><label v=\"{}\"/></attributes><type v=\"{}\"/><defaultValue>{d}</defaultValue><defaultWidget><name v=\"slider\"/><options><option><name v=\"min\"/><value v=\"{}\"/></option><option><name v=\"max\"/><value v=\"{}\"/></option></options></defaultWidget>{}</paraminput>",
                esc(&e.id),
                self.next(),
                esc(&e.label),
                exposed_code(&e.default),
                fmt_f(e.min),
                fmt_f(e.max),
                if e.group.is_empty() { String::new() } else { format!("<group v=\"{}\"/>", esc(&e.group)) }
            );
        }
        out.push_str("</paraminputs>");
        // Graph outputs (output nodes).
        let mut bridges: HashMap<NodeId, u64> = HashMap::new();
        out.push_str("<graphOutputs>");
        for n in g.nodes.iter().filter(|n| n.def == "output") {
            let uid = self.next();
            bridges.insert(n.id, uid);
            let usage = n.params.get("usage").and_then(Value::as_i32).unwrap_or(0).max(0) as u32;
            let ident = text(n, "identifier", "output");
            let _ = write!(
                out,
                "<graphoutput><identifier v=\"{}\"/><uid v=\"{uid}\"/><attributes><label v=\"{}\"/></attributes><usages><usage><components v=\"RGBA\"/><name v=\"{}\"/></usage></usages><group v=\"Material\"/></graphoutput>",
                esc(&ident),
                esc(&if n.label.is_empty() { ident.clone() } else { n.label.clone() }),
                usage_name(usage)
            );
        }
        out.push_str("</graphOutputs><compNodes>");
        for n in &g.nodes {
            let uid = uids.get(&n.id).copied().unwrap_or(0);
            let _ = write!(
                out,
                "<compNode><uid v=\"{uid}\"/><GUILayout><gpos v=\"{} {} 0\"/></GUILayout>",
                fmt_f(n.pos[0] + NODE_HALF[0]),
                fmt_f(n.pos[1] + NODE_HALF[1])
            );
            let my_outs = outs.get(&n.id).cloned().unwrap_or_default();
            if !my_outs.is_empty() {
                out.push_str("<compOutputs>");
                for (_, t, u) in &my_outs {
                    let _ = write!(out, "<compOutput><uid v=\"{u}\"/><comptype v=\"{t}\"/></compOutput>");
                }
                out.push_str("</compOutputs>");
            }
            let filter = filter_by_def(&n.def);
            let wires: Vec<_> = g.connections.iter().filter(|c| c.to == n.id).collect();
            if !wires.is_empty() {
                out.push_str("<connections>");
                for c in wires {
                    let Some(from_uid) = uids.get(&c.from) else { continue };
                    let Some((_, _, out_uid)) = outs.get(&c.from).and_then(|o| o.iter().find(|(p, _, _)| *p == c.from_port)) else { continue };
                    let ident = if n.def == "output" {
                        "inputNodeOutput".to_string()
                    } else {
                        filter.and_then(|f| f.inputs.iter().find(|(_, m)| *m == c.to_port)).map_or(c.to_port.clone(), |(s, _)| s.to_string())
                    };
                    let _ =
                        write!(out, "<connection><identifier v=\"{}\"/><connRef v=\"{from_uid}\"/><connRefOutput v=\"{out_uid}\"/></connection>", esc(&ident));
                }
                out.push_str("</connections>");
            }
            out.push_str("<compImplementation>");
            let base = base_xml(&n.base);
            match n.def.as_str() {
                "output" => {
                    let _ = write!(out, "<compOutputBridge><output v=\"{}\"/></compOutputBridge>", bridges.get(&n.id).copied().unwrap_or(0));
                }
                "input_color" | "input_grayscale" => {
                    let _ = write!(
                        out,
                        "<compInputBridge><entry v=\"{}\"/><parameters>{base}</parameters></compInputBridge>",
                        entries.get(&n.id).copied().unwrap_or(0)
                    );
                }
                "graph_instance" => {
                    let sub = text(n, "graph", "");
                    let params: String = n.params.iter().filter(|(k, _)| *k != "graph").filter_map(|(k, v)| param_xml(k, v)).collect();
                    let _ = write!(out, "<compInstance><path v=\"pkg:///{}?dependency={self_uid}\"/><parameters>{base}{params}</parameters>", esc(&sub));
                    bridgings(out, &my_outs);
                    out.push_str("</compInstance>");
                }
                d if d == MISSING_NODE => {
                    let kind = text(n, "sbs_kind", "instance");
                    let name = text(n, "sbs_name", &n.label);
                    let raw = match n.params.get("sbs_parameters") {
                        Some(Value::Text(s)) if well_formed(s) => s.clone(),
                        _ => "<parameters/>".into(),
                    };
                    if kind == "filter" {
                        let _ = write!(out, "<compFilter><filter v=\"{}\"/>{raw}</compFilter>", esc(&name));
                    } else {
                        let package = text(n, "sbs_package", "");
                        let path = text(n, "sbs_path", &name);
                        let dep = if package.is_empty() { lib_uid } else { self.dep(&package) };
                        let _ = write!(out, "<compInstance><path v=\"pkg:///{}?dependency={dep}\"/>{raw}", esc(&path));
                        bridgings(out, &my_outs);
                        out.push_str("</compInstance>");
                    }
                }
                d => match (filter, find(d)) {
                    (Some(f), Some(def)) => {
                        let _ = write!(out, "<compFilter><filter v=\"{}\"/><parameters>{base}", f.sbs);
                        let get = |id: &str| n.params.get(id).or_else(|| def.param(id).map(|p| &p.default));
                        for (sbs, mc, conv) in f.params {
                            let Some(v) = get(mc) else { continue };
                            let v = if *conv == BoolFirst { Value::Bool(v.as_i32() == Some(0)) } else { v.clone() };
                            if let Some(x) = param_xml(sbs, &v) {
                                out.push_str(&x);
                            }
                        }
                        match f.sbs {
                            "bitmap" => {
                                if let Some(Value::Text(r)) = get("resource")
                                    && !r.is_empty()
                                {
                                    out.push_str(
                                        &param_xml("bitmapresourcepath", &Value::Text(format!("pkg:///Resources/{r}?dependency={self_uid}")))
                                            .unwrap_or_default(),
                                    );
                                }
                            }
                            "grayscaleconversion" => {
                                let c = |k: &str| get(k).and_then(Value::as_f32).unwrap_or(0.0);
                                let v = Value::Float4([c("red"), c("green"), c("blue"), c("alpha")]);
                                out.push_str(&param_xml("channelsweights", &v).unwrap_or_default());
                            }
                            "transformation" => {
                                let s = match get("scale") {
                                    Some(Value::Float2(s)) => *s,
                                    _ => [1.0, 1.0],
                                };
                                let r = get("rotation").and_then(Value::as_f32).unwrap_or(0.0) * std::f32::consts::TAU;
                                let (sin, cos) = r.sin_cos();
                                let m = Value::Float4([s[0] * cos, -s[1] * sin, s[0] * sin, s[1] * cos]);
                                out.push_str(&param_xml("matrix22", &m).unwrap_or_default());
                            }
                            _ => {}
                        }
                        out.push_str("</parameters></compFilter>");
                    }
                    _ => {
                        let params: String = n.params.iter().filter_map(|(k, v)| param_xml(k, v)).collect();
                        let _ = write!(out, "<compInstance><path v=\"pkg:///{}?dependency={lib_uid}\"/><parameters>{base}{params}</parameters>", esc(d));
                        bridgings(out, &my_outs);
                        out.push_str("</compInstance>");
                    }
                },
            }
            out.push_str("</compImplementation></compNode>");
        }
        out.push_str("</compNodes><baseParameters/><GUIObjects>");
        for f in &g.frames {
            let c = f.color;
            let _ = write!(
                out,
                "<GUIObject><type v=\"COMMENT\"/><GUILayout><gpos v=\"{} {} -100\"/><size v=\"{} {}\"/></GUILayout><GUIName v=\"{}\"/><uid v=\"{}\"/><title v=\"{}\"/><frameColor v=\"{} {} {} {}\"/><isTitleVisible v=\"1\"/><isFrameVisible v=\"1\"/></GUIObject>",
                fmt_f(f.rect[0]),
                fmt_f(f.rect[1]),
                fmt_f(f.rect[2]),
                fmt_f(f.rect[3]),
                esc(&f.description),
                self.next(),
                esc(&f.title),
                fmt_f(linear_to_srgb(c[0])),
                fmt_f(linear_to_srgb(c[1])),
                fmt_f(linear_to_srgb(c[2])),
                fmt_f(c[3])
            );
        }
        for c in &g.comments {
            let _ = write!(
                out,
                "<GUIObject><type v=\"COMMENT\"/><GUILayout><gpos v=\"{} {} 0\"/><size v=\"{} {}\"/></GUILayout><GUIName v=\"{}\"/><uid v=\"{}\"/><isFrameVisible v=\"0\"/></GUIObject>",
                fmt_f(c.pos[0]),
                fmt_f(c.pos[1]),
                fmt_f(c.size[0]),
                fmt_f(c.size[1]),
                esc(&c.text),
                self.next()
            );
        }
        let [sx, sy] = [g.size[0].clamp(MIN_SIZE_LOG2, MAX_SIZE_LOG2), g.size[1].clamp(MIN_SIZE_LOG2, MAX_SIZE_LOG2)];
        let _ = write!(out, "</GUIObjects><options><option><name v=\"defaultParentSize\"/><value v=\"{sx}x{sy}\"/></option></options><root><rootOutputs>");
        for uid in g.nodes.iter().filter_map(|n| bridges.get(&n.id)) {
            let _ = write!(out, "<rootOutput><output v=\"{uid}\"/><format v=\"0\"/><usertag v=\"\"/></rootOutput>");
        }
        out.push_str("</rootOutputs></root></graph>");
        Ok(())
    }
}

fn bridgings(out: &mut String, outs: &[(String, u8, u64)]) {
    out.push_str("<outputBridgings>");
    for (p, _, u) in outs {
        let _ = write!(out, "<outputBridging><uid v=\"{u}\"/><identifier v=\"{}\"/></outputBridging>", esc(p));
    }
    out.push_str("</outputBridgings>");
}

/// One `<parameter>` (`None` for values SBS constants can't hold).
fn param_xml(name: &str, v: &Value) -> Option<String> {
    Some(format!("<parameter><name v=\"{}\"/><relativeTo v=\"0\"/><paramValue>{}</paramValue></parameter>", esc(name), constant_xml(v)?))
}

fn base_xml(b: &BaseParams) -> String {
    let mut s = String::new();
    if b.size_mode != SizeMode::RelativeToInput || b.size != [0, 0] {
        let rel = match b.size_mode {
            SizeMode::Absolute => 0,
            SizeMode::RelativeToInput => 1,
            SizeMode::RelativeToParent => 2,
        };
        let _ = write!(
            s,
            "<parameter><name v=\"outputsize\"/><relativeTo v=\"{rel}\"/><paramValue><constantValueInt2 v=\"{} {}\"/></paramValue></parameter>",
            b.size[0], b.size[1]
        );
    }
    let fmt = match b.format {
        OutputFormat::Inherit => None,
        OutputFormat::U8 => Some(0),
        OutputFormat::U16 => Some(1),
        OutputFormat::F16 => Some(2),
        OutputFormat::F32 => Some(3),
    };
    if let Some(f) = fmt {
        let _ = write!(s, "<parameter><name v=\"format\"/><relativeTo v=\"0\"/><paramValue><constantValueInt1 v=\"{f}\"/></paramValue></parameter>");
    }
    if b.seed != 0 {
        let _ = write!(s, "<parameter><name v=\"randomseed\"/><relativeTo v=\"0\"/><paramValue><constantValueInt1 v=\"{}\"/></paramValue></parameter>", b.seed);
    }
    s
}

/// Whether `s` is one well-formed `<parameters>` element (raw SBS kept by a placeholder).
fn well_formed(s: &str) -> bool {
    s.len() < (16 << 20)
        && roxmltree::Document::parse_with_options(s, roxmltree::ParsingOptions { allow_dtd: false, nodes_limit: 1_000_000 })
            .is_ok_and(|d| d.root_element().tag_name().name() == "parameters")
}

#[cfg(test)]
mod tests {
    use super::*;
    use materialcraft_core::Graph;

    fn none(_: &str) -> Option<Vec<u8>> {
        None
    }

    fn sample_pkg() -> Package {
        let mut p = Package::new("Bricks");
        let mut g = Graph::new("Wall");
        g.size = [9, 9];
        let a = g.add_node("brick_generator", [0.0, 0.0]);
        let b = g.add_node("blur", [150.0, 0.0]);
        let u = g.add_node("uniform", [0.0, 150.0]);
        let bl = g.add_node("blend", [300.0, 0.0]);
        let o = g.add_node("output", [450.0, 0.0]);
        g.connect(a, "output", b, "input").unwrap();
        g.connect(b, "output", bl, "foreground").unwrap();
        g.connect(u, "output", bl, "background").unwrap();
        g.connect(bl, "output", o, "input").unwrap();
        let n = g.node_mut(b).unwrap();
        n.params.insert("intensity".into(), Value::Float(4.5));
        n.base.size_mode = SizeMode::Absolute;
        n.base.size = [8, 8];
        g.node_mut(u).unwrap().params.insert("color".into(), Value::Color([0.25, 0.5, 0.75, 1.0]));
        g.node_mut(bl).unwrap().params.insert("mode".into(), Value::Enum(3));
        g.node_mut(o).unwrap().params.insert("usage".into(), Value::Enum(4));
        g.node_mut(o).unwrap().params.insert("identifier".into(), Value::Text("height".into()));
        g.node_mut(a).unwrap().params.insert("curvy".into(), Value::Curve(materialcraft_core::Curve::default()));
        g.frames.push(Frame {
            id: 90,
            title: "Frame & <title>".into(),
            description: "note".into(),
            rect: [-10.0, -10.0, 300.0, 200.0],
            color: [0.1, 0.2, 0.3, 0.4],
        });
        p.graphs.push(g);
        let png =
            crate::encode(&materialcraft_image::Image::filled(4, 4, materialcraft_image::Kind::Color, [1.0, 0.5, 0.0, 1.0]), "png", crate::Depth::U8, true)
                .unwrap();
        p.resources.push(Resource { name: "tex".into(), file: "tex.png".into(), bytes: Arc::new(png) });
        p
    }

    #[test]
    fn sniffs_sbs() {
        assert!(is_sbs(b"<?xml version=\"1.0\"?>\n<package><content/></package>"));
        assert!(is_sbs(b"\xef\xbb\xbf<package>"));
        assert!(is_sbs(b"<!-- hi --><package >"));
        assert!(!is_sbs(b"<packages>"));
        assert!(!is_sbs(b"PK\x03\x04zip"));
        assert!(!is_sbs(b"<?xml"));
        assert!(!is_sbs(b""));
    }

    #[test]
    fn exact_round_trip() {
        let p = sample_pkg();
        let bytes = write_sbs(&p).unwrap();
        assert!(is_sbs(&bytes));
        let r = read_sbs(&bytes, "Bricks.sbs", &none).unwrap();
        assert!(r.exact, "{:?}", r.warnings);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        assert_eq!(r.pkg.graphs, p.graphs);
        assert_eq!(r.pkg.resources[0].bytes, p.resources[0].bytes);
        assert_eq!(r.pkg.name, "Bricks");
    }

    /// The SBS data alone (as another program would leave it) carries the graph.
    #[test]
    fn structural_round_trip() {
        let p = sample_pkg();
        let text = String::from_utf8(write_sbs(&p).unwrap()).unwrap();
        let cut = text.find("<!--").unwrap();
        let plain = format!("{}</package>", &text[..cut]);
        let r = read_sbs(plain.as_bytes(), "x.sbs", &none).unwrap();
        assert!(!r.exact);
        let (g, q) = (&p.graphs[0], &r.pkg.graphs[0]);
        assert_eq!(q.name, "Wall");
        assert_eq!(q.size, [9, 9]);
        assert_eq!(q.nodes.len(), g.nodes.len());
        assert_eq!(q.connections.len(), g.connections.len());
        for (a, b) in g.nodes.iter().zip(&q.nodes) {
            assert_eq!(a.def, b.def);
            assert!((a.pos[0] - b.pos[0]).abs() < 1e-3 && (a.pos[1] - b.pos[1]).abs() < 1e-3);
            assert_eq!(a.base, b.base);
        }
        let blur = &q.nodes[1];
        assert_eq!(blur.params.get("intensity"), Some(&Value::Float(4.5)));
        let Some(Value::Color(c)) = q.nodes[2].params.get("color") else { panic!("uniform color") };
        assert!((c[0] - 0.25).abs() < 1e-3 && (c[2] - 0.75).abs() < 1e-3);
        assert_eq!(q.nodes[3].params.get("mode"), Some(&Value::Enum(3)));
        assert_eq!(q.nodes[4].params.get("usage"), Some(&Value::Enum(4)));
        assert_eq!(q.nodes[4].params.get("identifier"), Some(&Value::Text("height".into())));
        assert_eq!(q.frames[0].title, "Frame & <title>");
        assert_eq!(r.pkg.resources[0].bytes, p.resources[0].bytes);
        // Wires keep their ports.
        let bl = q.nodes[3].id;
        assert_eq!(q.input_of(bl, "foreground").map(|c| c.from), Some(q.nodes[1].id));
        assert_eq!(q.input_of(bl, "background").map(|c| c.from), Some(q.nodes[2].id));
    }

    #[test]
    fn edited_file_falls_back_to_sbs_data() {
        let p = sample_pkg();
        let text = String::from_utf8(write_sbs(&p).unwrap()).unwrap().replace("<identifier v=\"Wall\"/>", "<identifier v=\"Wall2\"/>");
        let r = read_sbs(text.as_bytes(), "x.sbs", &none).unwrap();
        assert!(!r.exact);
        assert_eq!(r.pkg.graphs[0].name, "Wall2");
        assert!(r.warnings.iter().any(|w| w.contains("changed outside")), "{:?}", r.warnings);
    }

    #[test]
    fn missing_nodes_are_placeholders_that_survive_saving() {
        let xml = r#"<?xml version="1.0"?><package><identifier v="P"/><dependencies><dependency><filename v="sbs://noise_fancy.sbs"/><uid v="7"/></dependency></dependencies><content><graph><identifier v="G"/>
            <compNodes>
              <compNode><uid v="1"/><GUILayout><gpos v="0 0 0"/></GUILayout><compOutputs><compOutput><uid v="11"/><comptype v="2"/></compOutput></compOutputs>
                <compImplementation><compInstance><path v="pkg:///fancy_noise?dependency=7"/><parameters><parameter><name v="scale"/><paramValue><constantValueFloat1 v="3"/></paramValue></parameter></parameters>
                <outputBridgings><outputBridging><uid v="11"/><identifier v="noise"/></outputBridging></outputBridgings></compInstance></compImplementation></compNode>
              <compNode><uid v="2"/><GUILayout><gpos v="200 0 0"/></GUILayout><compOutputs><compOutput><uid v="21"/><comptype v="1"/></compOutput></compOutputs>
                <connections><connection><identifier v="input1"/><connRef v="1"/><connRefOutput v="11"/></connection></connections>
                <compImplementation><compFilter><filter v="blur"/><parameters><parameter><name v="intensity"/><paramValue><constantValueFloat1 v="6"/></paramValue></parameter><parameter><name v="bogus"/><paramValue><constantValueFloat1 v="6"/></paramValue></parameter></parameters></compFilter></compImplementation></compNode>
              <compNode><uid v="3"/><GUILayout><gpos v="400 0 0"/></GUILayout><compOutputs><compOutput><uid v="31"/><comptype v="1"/></compOutput></compOutputs>
                <connections><connection><identifier v="weird_in"/><connRef v="2"/><connRefOutput v="21"/></connection></connections>
                <compImplementation><compFilter><filter v="futurefilter"/><parameters/></compFilter></compImplementation></compNode>
            </compNodes></graph></content></package>"#;
        let r = read_sbs(xml.as_bytes(), "p.sbs", &none).unwrap();
        let g = &r.pkg.graphs[0];
        assert_eq!(g.nodes.len(), 3);
        let fancy = &g.nodes[0];
        assert_eq!(fancy.def, MISSING_NODE);
        assert_eq!(fancy.label, "fancy_noise");
        assert_eq!(missing_ports(fancy).1, vec![("noise".to_string(), true)]);
        assert_eq!(g.nodes[1].params.get("intensity"), Some(&Value::Float(6.0)));
        assert_eq!(g.input_of(g.nodes[1].id, "input").map(|c| c.from_port.as_str()), Some("noise"));
        let future = &g.nodes[2];
        assert_eq!(missing_ports(future).0, vec!["weird_in".to_string()]);
        assert_eq!(g.connections.len(), 2);
        assert!(r.warnings.iter().any(|w| w.contains("missing node `fancy_noise`")), "{:?}", r.warnings);
        assert!(r.warnings.iter().any(|w| w.contains("missing node `futurefilter`")), "{:?}", r.warnings);
        assert!(r.warnings.iter().any(|w| w.contains("bogus")), "{:?}", r.warnings);

        // Saved and read back without the exact copy, the placeholders are written as they came.
        let text = String::from_utf8(write_sbs(&r.pkg).unwrap()).unwrap();
        assert!(text.contains("sbs://noise_fancy.sbs"));
        assert!(text.contains("pkg:///fancy_noise?dependency="));
        assert!(text.contains("<filter v=\"futurefilter\"/>"));
        let plain = format!("{}</package>", &text[..text.find("<!--").unwrap()]);
        let back = read_sbs(plain.as_bytes(), "p.sbs", &none).unwrap();
        let bg = &back.pkg.graphs[0];
        assert_eq!(bg.nodes[0].def, MISSING_NODE);
        assert_eq!(bg.nodes[0].params.get("sbs_parameters"), fancy.params.get("sbs_parameters"));
        assert_eq!(bg.connections.len(), 2);
    }

    #[test]
    fn hostile_input_is_an_error_or_a_warning_never_a_panic() {
        let cases: &[&str] = &[
            "",
            "<package",
            "<notpackage/>",
            "<!DOCTYPE package [<!ENTITY x \"y\">]><package/>",
            "<package><content><graph/></content></package>",
            "<package><content><graph><compNodes><compNode/><compNode><uid v=\"1\"/><compImplementation><compInstance/></compImplementation></compNode><compNode><uid v=\"1\"/></compNode></compNodes></graph></content></package>",
            "<package><content><graph><options><option><name v=\"defaultParentSize\"/><value v=\"99999x-4\"/></option></options><compNodes><compNode><uid v=\"1\"/><connections><connection><identifier v=\"input1\"/><connRef v=\"1\"/><connRefOutput v=\"9\"/></connection></connections><compImplementation><compFilter><filter v=\"blur\"/><parameters><parameter><name v=\"outputsize\"/><relativeTo v=\"0\"/><paramValue><constantValueInt2 v=\"99999999999 nan\"/></paramValue></parameter><parameter><name v=\"intensity\"/><paramValue><constantValueFloat1 v=\"inf\"/></paramValue></parameter></parameters></compFilter></compImplementation></compNode></compNodes></graph></content></package>",
            "<package><content><resource><identifier v=\"r\"/><source><binembedded><strdata v=\"////\"/></binembedded></source></resource><resource><identifier v=\"s\"/><source><binembedded><strdata v=\"f////w==\"/></binembedded></source></resource><resource><identifier v=\"t\"/><filepath v=\"../../etc/passwd\"/></resource></content></package>",
            "<package><content/><!-- materialcraft-package version=1 hash=0 data=AAAA --></package>",
            "<package><content><group><content><group><content><graph><identifier v=\"Deep\"/></graph></content></group></content></group><function><identifier v=\"f\"/></function><weird/></content></package>",
        ];
        for c in cases {
            if let Ok(r) = read_sbs(c.as_bytes(), "x.sbs", &none) {
                assert!(!r.pkg.graphs.is_empty(), "{c}");
            }
        }
        assert!(read_sbs(b"\xff\xfe", "x.sbs", &none).is_err());
        let deep = read_sbs(cases[9].as_bytes(), "x.sbs", &none).unwrap();
        assert_eq!(deep.pkg.graphs[0].name, "Deep");
        let size = read_sbs(cases[6].as_bytes(), "x.sbs", &none).unwrap();
        assert_eq!(size.pkg.graphs[0].size, [MAX_SIZE_LOG2, MIN_SIZE_LOG2]);
        assert!(size.pkg.graphs[0].connections.is_empty(), "a self-wire is a cycle");
    }

    #[test]
    fn linked_resources_load_through_the_callback() {
        let xml = "<package><content><resource><identifier v=\"img\"/><type v=\"bitmap\"/><format v=\"png\"/><source><externalcopy><filename v=\"p.resources/img.png\"/></externalcopy></source></resource></content></package>";
        let r = read_sbs(xml.as_bytes(), "p.sbs", &|p| (p == "p.resources/img.png").then(|| vec![1, 2, 3])).unwrap();
        assert_eq!(r.pkg.resources[0].bytes.as_slice(), &[1, 2, 3]);
        let r = read_sbs(xml.as_bytes(), "p.sbs", &none).unwrap();
        assert!(r.pkg.resources.is_empty());
        assert!(r.warnings.iter().any(|w| w.contains("img")), "{:?}", r.warnings);
    }

    #[test]
    fn embedded_data_round_trips() {
        let data: Vec<u8> = (0..5000u32).map(|i| (i * 7 % 251) as u8).collect();
        assert_eq!(decode_embedded(&encode_embedded(&data).unwrap()).unwrap(), data);
    }

    /// The repository's sample with every node of the reference library loads softly: every
    /// node is kept (mapped or as a placeholder) and each gap is a warning.
    #[test]
    fn the_all_nodes_sample_loads_with_warnings() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/sbs");
        let bytes = std::fs::read(dir.join("all_nodes.sbs")).unwrap();
        assert!(is_sbs(&bytes));
        let r = read_sbs(&bytes, "all_nodes.sbs", &|p| std::fs::read(dir.join(p)).ok()).unwrap();
        let g = &r.pkg.graphs[0];
        assert_eq!(g.nodes.len(), 536);
        let missing = g.nodes.iter().filter(|n| n.def == MISSING_NODE).count();
        let mapped = g.nodes.len() - missing;
        assert!(mapped >= 100, "{mapped} mapped");
        assert!(missing > 300, "{missing} placeholders");
        assert!(r.warnings.iter().any(|w| w.contains("missing node `3D_perlin_noise`")), "{:?}", &r.warnings[..5]);
        assert!(r.pkg.resources.iter().any(|x| x.name == "Untitled_Bitmap" && !x.bytes.is_empty()));
        let svg = r.pkg.resources.iter().find(|x| x.name == "Untitled_Vector_Graphics").unwrap();
        assert!(String::from_utf8_lossy(&svg.bytes).contains("<svg"));
        assert_eq!(g.frames.len(), 26);
        assert_eq!(g.size, [11, 11]);
        // Saved as SBS and read back: same nodes, exactly.
        let back = read_sbs(&write_sbs(&r.pkg).unwrap(), "x.sbs", &none).unwrap();
        assert!(back.exact);
        assert_eq!(back.pkg.graphs, r.pkg.graphs);
        eprintln!("{} warnings; first: {:#?}", r.warnings.len(), &r.warnings[..r.warnings.len().min(12)]);
    }
}
