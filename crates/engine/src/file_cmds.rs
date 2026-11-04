//! File menu: new/open/save packages, new graphs from templates, bitmap resources and texture
//! export. Host-path commands are native only; the browser and agents use the byte-based
//! functions (`open_bytes`, `save_bytes`, `export_outputs`).
//!
//! SBS (`.sbs`) is the main project format: packages save as SBS unless their file is a `.mcraft`
//! package (the older native format, still read and written). Opening an SBS file never fails on
//! what MaterialCraft lacks: missing nodes become placeholders and every gap is a warning in the
//! session log (the Console) and in `session.inspect` (`package.warnings`).

use serde_json::{Value as J, json};

use materialcraft_core::{Package, Resource};
use materialcraft_formats::{Depth, encode, output_name};

use crate::commands::{CommandSpec, always, bad, has_graph, strp, u64p};
use crate::samples::{TEMPLATES, template};
use crate::{Result, Session, cmd};

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "file.new",
            "Graph...",
            ["File", "New"],
            Some("Ctrl+N"),
            "{name?, template? (see graph.templates), size? (px, power of two)} adds a new graph to the package and opens it",
            always,
            new_graph
        ),
        cmd!("file.newPackage", "Empty package", ["File", "New"], None, "{name?} replaces the session with an empty package", always, new_package),
        cmd!(
            "file.open",
            "Open...",
            ["File"],
            Some("Ctrl+O"),
            "{path} opens an .sbs or .mcraft package (or a .mcmat material's source); returns `warnings` for anything an SBS file holds that MaterialCraft can't read yet",
            always,
            open
        ),
        cmd!(
            "file.save",
            "Save",
            ["File"],
            Some("Ctrl+S"),
            "{path?} saves the package to its own file or `path`: SBS (.sbs, the default; appended when `path` has no package extension) or .mcraft",
            always,
            save
        ),
        cmd!("file.saveAs", "Save as...", ["File"], Some("Ctrl+Shift+S"), "{path} saves the package under a new name", always, save),
        cmd!(
            "file.export",
            "Export outputs...",
            ["File"],
            Some("Ctrl+Shift+E"),
            "{dir, format? (png|jpg|tga|tif|bmp|webp|exr|hdr), pattern? ($(graph)_$(identifier)), depth? (8|16|32), graph?} cooks the graph at full size and writes every output",
            has_graph,
            export
        ),
        cmd!(
            "file.importBitmap",
            "Import bitmap...",
            ["File"],
            None,
            "{path, name?} embeds an image file as a resource; returns its name",
            always,
            import_bitmap
        ),
        cmd!("graph.templates", "Graph templates", [], None, "{} lists the templates for file.new", always, |_, _| Ok(json!(
            TEMPLATES.iter().map(|(id, l)| json!({"id": id, "label": l})).collect::<Vec<_>>()
        ))),
    ]
}

fn new_graph(s: &mut Session, p: &J) -> Result<J> {
    let tpl = strp(p, "template").unwrap_or("pbr");
    let label = TEMPLATES.iter().find(|t| t.0 == tpl).map_or("Graph", |t| t.1);
    let size = u64p(p, "size").unwrap_or(1024).clamp(16, 8192);
    let log2 = 63 - size.leading_zeros();
    let base = strp(p, "name").map(str::to_string).unwrap_or_else(|| label.replace(' ', "_"));
    s.edit("New graph", |s| {
        let name = s.pkg.unique_graph_name(&base);
        s.pkg.graphs.push(template(tpl, &name, log2));
        s.graph = s.pkg.graphs.len() - 1;
        s.selection.clear();
        s.view2d.node = None;
        Ok(json!({"graph": s.graph, "name": name}))
    })
}

fn new_package(s: &mut Session, p: &J) -> Result<J> {
    let mut pkg = Package::new(strp(p, "name").unwrap_or("Untitled package"));
    pkg.graphs.push(template("pbr", "New_graph", 10));
    replace_package(s, pkg, None);
    Ok(json!({"name": s.pkg.name}))
}

/// Make `pkg` the session's document (after open/new): history and views reset.
pub fn replace_package(s: &mut Session, pkg: Package, path: Option<String>) {
    s.pkg = pkg;
    s.path = path;
    s.graph = 0;
    s.selection.clear();
    s.view2d.node = None;
    s.reset_history();
    s.modified = false;
    s.dirty = true;
    s.load_warnings.clear();
}

/// A package file format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageFormat {
    /// SBS XML, the main project format.
    Sbs,
    /// The older native zip format.
    Mcraft,
}

impl PackageFormat {
    /// The format a package file at `path` is saved in: `.mcraft` stays `.mcraft`, anything
    /// else is SBS.
    pub fn of_path(path: &str) -> PackageFormat {
        if path.to_ascii_lowercase().ends_with(&format!(".{}", materialcraft_core::PACKAGE_EXT)) { PackageFormat::Mcraft } else { PackageFormat::Sbs }
    }

    pub fn ext(self) -> &'static str {
        match self {
            PackageFormat::Sbs => materialcraft_formats::sbs::SBS_EXT,
            PackageFormat::Mcraft => materialcraft_core::PACKAGE_EXT,
        }
    }
}

/// `path` with the package extension appended unless it already ends in `.sbs` or `.mcraft`.
pub fn package_path(path: &str) -> String {
    let lower = path.to_ascii_lowercase();
    if [PackageFormat::Sbs, PackageFormat::Mcraft].iter().any(|f| lower.ends_with(&format!(".{}", f.ext()))) {
        path.to_string()
    } else {
        format!("{path}.{}", PackageFormat::Sbs.ext())
    }
}

/// Open package bytes (any platform): SBS (by content), `.mcraft`, or a portable material
/// (`.mcmat`), which opens the package it was cooked from, on its graph, with no file name
/// (saving must not overwrite the material). Files an SBS package links to (bitmaps kept next
/// to it) are not available this way: see [`open_bytes_linked`].
pub fn open_bytes(s: &mut Session, bytes: &[u8], name: Option<String>) -> Result<()> {
    open_bytes_linked(s, bytes, name, &|_| None)
}

/// [`open_bytes`] with `linked` loading the files an SBS package refers to by a path relative to
/// it. Warnings (missing nodes, unreadable parameters or resources) go to the session log and
/// [`Session::load_warnings`].
pub fn open_bytes_linked(s: &mut Session, bytes: &[u8], name: Option<String>, linked: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<()> {
    if materialcraft_formats::material::is_material(bytes) {
        let (manifest, source) = materialcraft_formats::material::read_material(bytes)?;
        replace_package(s, materialcraft_formats::load_package(&source)?, None);
        s.graph = s.pkg.graph_by_name(&manifest.graph).map_or(0, |(i, _)| i);
        return Ok(());
    }
    if materialcraft_formats::sbs::is_sbs(bytes) {
        let read = materialcraft_formats::sbs::read_sbs(bytes, name.as_deref().unwrap_or(""), linked)?;
        replace_package(s, read.pkg, name);
        if !read.warnings.is_empty() {
            s.warn(format!("{} opened with {} warning(s): what MaterialCraft can't read yet is listed below", s.pkg.name, read.warnings.len()));
            for w in &read.warnings {
                s.warn(w.clone());
            }
        }
        s.load_warnings = read.warnings;
        return Ok(());
    }
    let pkg = materialcraft_formats::load_package(bytes)?;
    replace_package(s, pkg, name);
    Ok(())
}

/// The package as SBS bytes (the main project format).
pub fn save_bytes(s: &Session) -> Result<Vec<u8>> {
    save_bytes_as(s, PackageFormat::Sbs)
}

/// The package as bytes of `format`.
pub fn save_bytes_as(s: &Session, format: PackageFormat) -> Result<Vec<u8>> {
    Ok(match format {
        PackageFormat::Sbs => materialcraft_formats::sbs::write_sbs(&s.pkg)?,
        PackageFormat::Mcraft => materialcraft_formats::save_package(&s.pkg)?,
    })
}

/// A reader for the files an SBS package at `path` links to: plain relative paths inside the
/// package's folder only (no absolute paths, `..`, drive letters or URLs; symlinks may not leave
/// the folder), up to 256 MB each.
#[cfg(not(target_arch = "wasm32"))]
pub fn linked_files(path: &str) -> impl Fn(&str) -> Option<Vec<u8>> + use<> {
    let dir = std::path::Path::new(path).parent().map(std::path::Path::to_path_buf).unwrap_or_default();
    move |rel: &str| {
        let rel = rel.trim().replace('\\', "/");
        let p = std::path::Path::new(&rel);
        if rel.is_empty() || rel.contains(':') || rel.starts_with('/') || p.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
            return None;
        }
        let base = std::fs::canonicalize(if dir.as_os_str().is_empty() { std::path::Path::new(".") } else { dir.as_path() }).ok()?;
        let full = std::fs::canonicalize(base.join(p)).ok()?;
        let meta = std::fs::metadata(&full).ok()?;
        if !full.starts_with(&base) || !meta.is_file() || meta.len() > 256 << 20 {
            return None;
        }
        std::fs::read(full).ok()
    }
}

/// Open the package file at `path` (native): SBS, `.mcraft` or `.mcmat`, with the bitmaps an SBS
/// package keeps next to it.
#[cfg(not(target_arch = "wasm32"))]
pub fn open_file(s: &mut Session, path: &str) -> Result<()> {
    let bytes = std::fs::read(path).map_err(|e| bad("file.open", format!("{path}: {e}")))?;
    open_bytes_linked(s, &bytes, Some(path.to_string()), &linked_files(path))
}

#[cfg(not(target_arch = "wasm32"))]
fn open(s: &mut Session, p: &J) -> Result<J> {
    let path = strp(p, "path").ok_or_else(|| bad("file.open", "`path` is required"))?;
    open_file(s, path)?;
    s.log(format!("Opened {path}"));
    Ok(json!({"name": s.pkg.name, "graphs": s.pkg.graphs.len(), "warnings": s.load_warnings}))
}

#[cfg(target_arch = "wasm32")]
fn open(_: &mut Session, _: &J) -> Result<J> {
    Err(bad("file.open", "the browser opens files with the file picker"))
}

#[cfg(not(target_arch = "wasm32"))]
fn save(s: &mut Session, p: &J) -> Result<J> {
    let path = match strp(p, "path") {
        Some(path) => package_path(path),
        None => s.path.clone().ok_or_else(|| bad("file.save", "the package has no file yet: give a `path`"))?,
    };
    let bytes = save_bytes_as(s, PackageFormat::of_path(&path))?;
    materialcraft_formats::write_atomic(std::path::Path::new(&path), &bytes)?;
    s.path = Some(path.clone());
    s.modified = false;
    s.log(format!("Saved {path}"));
    Ok(json!({"path": path, "bytes": bytes.len()}))
}

#[cfg(target_arch = "wasm32")]
fn save(_: &mut Session, _: &J) -> Result<J> {
    Err(bad("file.save", "the browser saves with a download"))
}

/// One exported file.
pub struct Exported {
    pub file: String,
    pub bytes: Vec<u8>,
}

/// Cook graph `gi` at full size and encode every output (`pattern` names files).
pub fn export_outputs(s: &Session, gi: usize, ext: &str, pattern: &str, depth: Option<Depth>) -> Result<Vec<Exported>> {
    let g = s.pkg.graph(gi)?;
    let cooked = s.cook_full(gi);
    let mut out = Vec::new();
    for (id, usage, ident, img) in materialcraft_eval::outputs(g, &cooked) {
        let Some(img) = img else { continue };
        let node = g.node(id)?;
        let depth = depth
            .unwrap_or_else(|| Depth::from_format(if node.base.format == materialcraft_core::OutputFormat::Inherit { g.format } else { node.base.format }));
        let bytes = encode(img, ext, depth, materialcraft_formats::material::srgb_usage(usage))?;
        out.push(Exported { file: format!("{}.{}", output_name(pattern, &g.name, &ident, usage), ext.trim_start_matches('.')), bytes });
    }
    if out.is_empty() {
        return Err(bad("file.export", "the graph has no connected output nodes"));
    }
    Ok(out)
}

/// Export params: (graph index, extension, pattern, depth).
pub fn export_params(s: &Session, p: &J) -> Result<(usize, String, String, Option<Depth>)> {
    let gi = u64p(p, "graph").map_or(s.graph, |g| g as usize);
    s.pkg.graph(gi)?;
    let ext = strp(p, "format").unwrap_or("png").trim_start_matches('.').to_ascii_lowercase();
    if !materialcraft_formats::EXPORT_FORMATS.iter().any(|(e, _)| *e == ext) {
        return Err(bad("file.export", format!("unknown format `{ext}`")));
    }
    let pattern = strp(p, "pattern").unwrap_or("$(graph)_$(identifier)").to_string();
    let depth = match u64p(p, "depth") {
        None => None,
        Some(8) => Some(Depth::U8),
        Some(16) => Some(Depth::U16),
        Some(32) => Some(Depth::F32),
        Some(d) => return Err(bad("file.export", format!("depth must be 8, 16 or 32, not {d}"))),
    };
    Ok((gi, ext, pattern, depth))
}

#[cfg(not(target_arch = "wasm32"))]
fn export(s: &mut Session, p: &J) -> Result<J> {
    let dir = strp(p, "dir").ok_or_else(|| bad("file.export", "`dir` is required"))?;
    let (gi, ext, pattern, depth) = export_params(s, p)?;
    let files = export_outputs(s, gi, &ext, &pattern, depth)?;
    std::fs::create_dir_all(dir).map_err(|e| bad("file.export", format!("{dir}: {e}")))?;
    let mut written = Vec::new();
    for f in files {
        let path = std::path::Path::new(dir).join(&f.file);
        materialcraft_formats::write_atomic(&path, &f.bytes)?;
        written.push(path.display().to_string());
    }
    s.log(format!("Exported {} files to {dir}", written.len()));
    Ok(json!({"files": written}))
}

#[cfg(target_arch = "wasm32")]
fn export(_: &mut Session, _: &J) -> Result<J> {
    Err(bad("file.export", "the browser exports with downloads"))
}

/// Embed image bytes as a resource; returns the resource name.
pub fn add_resource(s: &mut Session, file: &str, bytes: Vec<u8>, name: Option<&str>) -> Result<String> {
    materialcraft_formats::decode(&bytes)?;
    s.edit("Import bitmap", |s| Ok(push_resource(&mut s.pkg, file, bytes, name)))
}

/// Add `bytes` to `pkg` as a resource named `name` (or after `file`), made unique; returns the name.
pub(crate) fn push_resource(pkg: &mut Package, file: &str, bytes: Vec<u8>, name: Option<&str>) -> String {
    let file = materialcraft_formats::safe_file_name(file);
    let stem = name.map(str::to_string).unwrap_or_else(|| file.rsplit_once('.').map_or(file.clone(), |(a, _)| a.to_string()));
    let mut name = stem.clone();
    let mut i = 1;
    while pkg.resource(&name).is_some() {
        name = format!("{stem}_{i}");
        i += 1;
    }
    pkg.resources.push(Resource { name: name.clone(), file: format!("{name}_{file}"), bytes: std::sync::Arc::new(bytes) });
    name
}

#[cfg(not(target_arch = "wasm32"))]
fn import_bitmap(s: &mut Session, p: &J) -> Result<J> {
    let path = strp(p, "path").ok_or_else(|| bad("file.importBitmap", "`path` is required"))?;
    let bytes = std::fs::read(path).map_err(|e| bad("file.importBitmap", format!("{path}: {e}")))?;
    let file = std::path::Path::new(path).file_name().and_then(|f| f.to_str()).unwrap_or("bitmap.png").to_string();
    let name = add_resource(s, &file, bytes, strp(p, "name"))?;
    Ok(json!({"name": name, "use": {"command": "graph.node.add", "params": {"def": "bitmap", "params": {"resource": name}}}}))
}

#[cfg(target_arch = "wasm32")]
fn import_bitmap(_: &mut Session, _: &J) -> Result<J> {
    Err(bad("file.importBitmap", "the browser imports with the file picker"))
}
