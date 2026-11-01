# MaterialCraft roadmap

> **Last reviewed:** 2026-11-02 · **Last updated:** 2026-11-02 · **Change:** major (first version) · **Target:** the professional node-based material workflow

**Stage:** alpha (in active development). MaterialCraft aims for full parity with the professional
node-based procedural material workflow, with better performance, every feature drivable by agents
(MCP, CLI, control channel), and builds for Windows, macOS, Linux, FreeBSD and the web.

## Headline numbers

- **Parity: ≈ 51%** of a material artist's daily workflow works end to end
  ([docs/target-app-parity.md](docs/target-app-parity.md)).
- **Remaining: ≈ 120 Opus 5.5 agent hours** ([docs/roadmap.md](docs/roadmap.md)).
- **Nodes:** ~120 (all tileable), cooked on every CPU core through a content-addressed cache; the
  brick wall sample cooks in about 40 ms at preview size.

## By area

| Area | Weight | Done |
|---|---:|---:|
| Graph editing | 15 | 75% |
| Atomic nodes | 12 | 80% |
| Node library | 20 | 25% |
| Function graphs | 10 | 35% |
| Graph parameters and instances | 8 | 50% |
| 2D view | 5 | 70% |
| 3D view | 10 | 45% |
| Files | 8 | 50% |
| UI and UX | 8 | 55% |
| Platforms | 4 | 40% |
| **Total** | **100** | **≈ 51%** |

## What's next

1. The visual function-graph editor and dynamic parameters.
2. More of the node library: flood fill, shape splatter, 3D noises, grunge maps, whole-material
   blending.
3. FX-Map lite, Text and SVG nodes.
4. A GPU 3D view with tessellation and lights; mesh import.
5. Browser file I/O and the first release on every platform.

The ranked work list is [docs/gaps.md](docs/gaps.md); file formats are in
[docs/file-format-parity.md](docs/file-format-parity.md) and languages in
[docs/localization-parity.md](docs/localization-parity.md).

## Progress log

| Date | What landed |
|---|---|
| 2026-11-02 | First day: engine, ~120 nodes, cooking cache, packages, export, the full editor UI, 3D preview, CLI, control channel with synthetic input, MCP server, wasm builds |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-02 | major | First version |
