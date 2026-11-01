# Gaps

> **Last reviewed:** 2026-11-02 · **Last updated:** 2026-11-02 · **Change:** major (first list) · **Target:** the professional node-based material workflow

Every known shortfall, most important first. Pick from the top.

| # | Gap | Evidence | Impact | Estimate | Parity doc |
|---:|---|---|---|---:|---|
| 1 | Visual function-graph editor | Pixel/value processor functions are edited as JSON in Properties | Function graphs are a core power-user feature | 10 h | target-app-parity (Function graphs) |
| 2 | Node library breadth | ~120 nodes vs. ~400 in the reference library | Many recipes need nodes we lack (flood fill, shape splatter, 3D noises, material blends) | 40 h | target-app-parity (Node library) |
| 3 | FX-Map, Text and SVG atomic nodes | Not in the catalog | Hand-placed patterns and lettering | 12 h | target-app-parity (Atomic nodes) |
| 4 | Dynamic parameters | Only exposed-input bindings, no per-parameter functions | Procedural parameter logic | 6 h | target-app-parity (Function graphs) |
| 5 | GPU 3D view with tessellation and lights | CPU renderer with parallax only | Displacement previews, light setups | 12 h | target-app-parity (3D view) |
| 6 | Mesh-based generators and bakers | None | Workflows that start from a mesh | 15 h | target-app-parity (Node library) |
| 7 | Published material archives | Only `.mcraft` sources | Sharing cooked materials with other tools | 6 h | target-app-parity (Files) |
| 8 | Browser file I/O | The web build shows a notice for open/save/import | Web users cannot keep their work | 3 h | target-app-parity (Platforms) |
| 9 | `ui.screenshot` times out on this test machine | The window-capture callback never fires here; `view.screenshot` and the offscreen snapshot test work | Agents bridged to the app see views but not the whole window | 1 h | — |
| 10 | Accessible names for custom widgets | egui_kittest's accessibility check finds 12 unnamed widgets (canvas, tiles) | Screen readers; the snapshot test disables the check | 2 h | target-app-parity (UI and UX) |
| 11 | Presets and parameter groups | Not implemented | Organizing large graphs' inputs | 4 h | target-app-parity (Graph parameters) |
| 12 | Translations and keymap editor | English only, fixed shortcuts | Non-English users; custom shortcuts | 8 h | localization-parity |
| 13 | Owner's mascot app icon | A placeholder drawn in code | Brand consistency with the other Crafting Apps | owner | — |
| 14 | First signed release | Workflows copied, not run | Downloads | 3 h | target-app-parity (Platforms) |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-02 | major | First list |
