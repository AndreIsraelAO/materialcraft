# Parity with the reference workflow

> **Last reviewed:** 2026-11-02 · **Last updated:** 2026-11-02 · **Change:** major (first assessment) · **Target:** the professional node-based material workflow (desktop app; the web app is assessed later)

**Overall: ≈ 51%** of a material artist's daily workflow works end to end (engine + UI +
automation + tests), weighted by how much of the day each area carries. **Remaining: ≈ 120 Opus
5.5 agent hours** (see [roadmap.md](roadmap.md)).

| Area | Weight | Done | What works | Main gaps |
|---|---:|---:|---|---|
| Graph editing | 15 | 75% | Pan/zoom canvas with grid; node cards with headers by category, live thumbnails and size/format labels; typed ports; bezier wires; drag to wire, re-route and unplug; drop a wire on empty canvas to add a wired node; box/Ctrl/Shift selection; move; frames (move with contents, resize), comments (inline edit), navigation pins; node search (Space, double-click); library drag and drop; align and distribute; copy/cut/paste/duplicate (with or without links); delete and relink; disable; select upstream/downstream; create graph from selection; snapshot undo with merged slider drags | Dot-node chains editing, wire tension preferences, compact node display mode, multi-graph tabs, minimap |
| Atomic nodes | 12 | 80% | Bitmap, Blend (16 modes, masks, alpha), Blur, Channels Shuffle, Curve, Directional Blur, Directional Warp, Distance (with color propagation), Emboss, Gradient (Dynamic), Gradient Map, Grayscale Conversion, HSL, Levels, Normal, Pixel Processor, Value Processor, Sharpen, Transformation 2D, Uniform Color, Warp, inputs, outputs, dots, graph instances | Text, SVG, FX-Map |
| Node library | 20 | 25% | 29 noises, 16 patterns (tile generator, tile sampler, bricks, hexagons, weave, splatter, scratches, shapes, polygons, stars, stripes, gradients), 50 filters (adjustments, blending, blurs, channels, effects, normal map tools, tiling, transforms), 4 PBR utilities | Most of the reference library's ~400 graphs: 3D noises, more grunge maps, flood fill family, shape splatter, material blending/height blending of whole materials, scan processing, mesh-based generators, spline tools, SDF functions |
| Function graphs | 10 | 35% | A compiled, 4-wide evaluator with ~55 ops (constants, $pos/$size, exposed variables, samplers, vectors, swizzles, math, logic, comparison, noise, random, if/else), pixel and value processors | Visual function editor (JSON editor for now), dynamic parameters (functions on any parameter), loops, function graphs as package items |
| Graph parameters and instances | 8 | 50% | Exposed inputs (float, int, bool, enum, color) bound to node parameters, set from the graph's properties or an instance's; sub-graph instances with their own inputs and outputs; graph attributes | Presets, parameter groups and visibility conditions, per-instance output sizes, embedded vs. linked packages |
| 2D view | 5 | 70% | Pan/zoom, fit and 1:1, RGBA/R/G/B/A isolation, 3×3 tiling check, pixel readout, size/format line, save image | Histogram, color picker to parameters, UV overlay |
| 3D view | 10 | 45% | Rounded cube, cube, sphere, cylinder, plane, torus and material ball; base color, normal, roughness, metallic, height (parallax occlusion), AO, emissive and opacity; 4 procedural environments with rotation and exposure; orbit/zoom; tiling; supersampling at rest | GPU renderer with tessellation, lights, custom meshes, material/shader switching, path-traced renderer |
| Files | 8 | 50% | `.mcraft` packages with embedded bitmaps; export of every output (PNG, JPEG, TGA, TIFF, BMP, WEBP, EXR, HDR; 8/16/32 bits; name pattern; sRGB vs. linear by usage); bitmap import (PNG, JPEG, TGA, TIFF, BMP, WEBP, EXR, HDR); atomic writes; autosave; recent files | Published (cooked) material archives, mesh import for the 3D view, package dependencies, SVG/font resources, browser file I/O |
| UI and UX | 8 | 55% | Dockable/floatable panels (explorer, library, graph, 3D, 2D, properties, console, history), menus, main toolbar, graph toolbar with atomic quick-insert, library with live previews, properties with curve/gradient/color editors, New graph dialog with rendered templates, export dialog, preferences (brightness, accent, scale, preview size, autosave), shortcuts list, home dialog, status bar | Translations, keymap editor, accessible names on custom widgets, home screen at startup |
| Platforms | 4 | 40% | Windows, macOS, Linux, FreeBSD and web builds compile; packaging scripts and release workflows copied from TextureCraft | First release run; browser file I/O |
| **Total** | **100** | **≈ 51%** | | |

Beyond the reference app: every edit is a command available through MCP, the CLI and the app's
control channel (including synthetic mouse and keyboard input), the engine runs headless with the
same results, and everything compiles to WebAssembly.

## Method

Scores are self-assessed from our code against public documentation of the professional
node-based material workflow and observation of the reference desktop app's menus, panels and
node library, area by area. A feature counts only when it works end to end (engine, UI, automation)
and is tested.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-02 | major | First assessment after the first day of work |
