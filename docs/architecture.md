# Architecture

> **Last reviewed:** 2026-11-02 · **Last updated:** 2026-11-02 · **Change:** minor (portable materials and TextureCraft) · **Target:** the professional node-based material workflow

MaterialCraft is a Cargo workspace of small crates in strict layers (`cargo xtask layers` enforces
them). Everything below the UI is plain Rust with no UI toolkit and builds for
`wasm32-unknown-unknown`.

```text
L0  core      document model: Package → Graph → Node / Connection / Frame / Comment / Pin / ExposedParam;
              Value (float, vectors, int, bool, color, enum, text, curve, gradient, function graph)
L0  image     Image: grayscale or RGBA f32 planes behind an Arc (copy-on-write, free to share);
              wrap/clamp sampling, bilinear, box/bilinear resampling, sRGB, 8-bit display
L0  geom      meshes, primitives (rounded cube, sphere, cylinder, plane, torus, material ball), BVH, camera
L1  nodes     the node library: NodeDef (ports, parameters, category, cook fn) for 227 nodes, the
              tileable noise primitives, shared image ops (blur, EDT, HSL, blend modes) and the
              function-graph compiler/evaluator for pixel and value processors
L2  eval      cooking: topological order, output sizes (relative to input / parent, absolute, preview
              cap), parameter resolution (defaults, overrides, bindings to exposed inputs), the
              content-addressed LRU cache, graph instances (sub-graphs, recursion-guarded)
L2  formats   .sbs packages (the main format, sbs.rs), .mcraft packages (zip: package.json +
              resources/), .mcmat portable materials, bitmap
              decode, texture encode (PNG/JPEG/TGA/TIFF/BMP/WEBP/EXR/HDR at 8/16/32 bits), atomic writes
L3  engine    Session (package, open graph, selection, 2D/3D view state, snapshot undo, cache),
              the command registry, inspect, sample materials and graph templates
L3  render    the 3D view: BVH ray casts, parallax occlusion from the height output, GGX PBR with
              prefiltered IBL + SH diffuse, alpha transparency (layered ray compositing) and an
              alpha channel view, ACES tone mapping; procedural environments
L4  automation MCP server (rmcp), JSON-lines RPC (stdio/TCP with token), bridge client, workspace sandbox
L5  ui-egui   the egui shell: docking (craft-ui), graph editor, library, properties, 2D/3D views,
              dialogs, menus, control channel with synthetic input
apps          materialcraft (desktop), materialcraft-cli, materialcraft-web
```

## Cooking

A node's cook is a pure function `fn(&Ctx) -> Result<Vec<Image>, String>`: `Ctx` carries the output
size, the seed, tiling, the inputs (by port order, `None` when unconnected), the resolved parameters,
a resource decoder and the graph's exposed values. `eval::cook` walks the graph in dependency order.
For each node it hashes the definition id, resolved parameters, size, seed, tiling, the keys of its
input images, resource bytes and (for instances) the sub-graph: equal keys hit the cache, so an edit
re-cooks only the nodes downstream of it, and undo finds earlier results again. A failing node shows
a magenta image and its error; nodes after it still cook.

Sizes: generators take the graph ("parent") size; filters take their main input's size; both can be
offset by powers of two or set absolutely. While editing, outputs are capped at the preview size
(512 px by default), with pixel-based parameters scaled by `w / 256` so previews and full-size
exports look the same. Exports cook at full size through the same cache.

The desktop UI cooks on a worker thread (`ui-egui/src/cook.rs`) from a snapshot of the package and
keeps drawing the last results; the browser cooks on the UI thread.

## Commands and automation

Every user-visible action is a `CommandSpec` (`engine/src/*_cmds.rs`): id, label, menu, shortcut,
params doc, `enabled`, `run`. The UI calls them through `MaterialCraftApp::run`; the CLI, the JSON
control channel and MCP dispatch the same ids. `Session::edit` snapshots the package before running
an edit and restores it on error; `edit_merged` folds repeated edits with the same key (a slider
drag) into one undo step. See [control-protocol.md](control-protocol.md).

## The UI

`MaterialCraftApp` owns the session and `UiState` (serde, persisted by eframe, readable and writable
through `ui.inspect` / `ui.set`). The dock (craft-ui) hosts eight panels around the protected graph
panel. The graph editor lays nodes out in graph space, draws cards with thumbnails from the cook
results (uploaded once per cache key), wires as cubic beziers between typed ports (hit-tested
against the curve, so wires can be selected, deleted, swapped or split with a reroute dot), and turns
gestures into commands only when they complete. See [wires-and-alpha.md](wires-and-alpha.md). The 3D view renders with `materialcraft-render`
on the CPU: half resolution while the camera moves, 2×2 supersampled when it rests.

`crates/ui-egui/tests/snapshot.rs` renders the real UI offscreen with egui_kittest + wgpu for visual
checks and README screenshots.

The properties panel builds every parameter editor from the node catalog; the Levels node adds a
histogram editor with range handles on top of its sliders ([levels-editor.md](levels-editor.md)).

## Files

SBS (`.sbs`, `formats/src/sbs.rs`) is the main project format: XML with the package's graphs, nodes,
wires, parameters, graph inputs and outputs, frames and comments, and bitmaps embedded as
zlib/base64 data. MaterialCraft writes the SBS data plus, in a trailing XML comment, its own exact
copy of the package with a hash of the `<content>` it belongs to; reading uses the exact copy while
the hash matches (so nothing MaterialCraft-specific is lost) and the SBS data otherwise. Reading never
fails on what MaterialCraft lacks: atomic filters map to atomic nodes, library nodes map by name,
instances of graphs in the same file become graph instances, and anything else becomes a
`missing_node` placeholder (`core::MISSING_NODE`) that keeps its ports, wires and raw SBS parameters,
cooks to a soft error, and is written back unchanged. Every gap is a warning in
`Session::load_warnings`, the Console (`Session::warn`) and `session.inspect` (`package.warnings`).
Bitmaps an SBS file keeps next to it load from its folder only (`file_cmds::linked_files`).

A `.mcraft` package (the older format, still read and written: packages opened from a `.mcraft`
save back to it) is a zip: `package.json` (the serde `Package`, format version 1) and
`resources/<name>` for embedded bitmaps. Node parameters are stored only where they differ from the
catalog defaults, so new defaults reach old files and files stay small.

## Other apps

A `.mcmat` portable material (`formats/src/material.rs`, `engine/src/material_cmds.rs`) holds a
graph's cooked maps, its portable exposed inputs, presets and input nodes, and the source package.
TextureCraft paints with it and re-cooks it by running `materialcraft-cli run <package>
file.exportMaterial {path, size, values, inputs}`: the apps share the file format and the command
line, never code. Graphs keep preview images for their input nodes (`Graph::input_images`, mesh
maps from TextureCraft); `eval::cook` feeds them to the top-level graph, and a caller's images win.
