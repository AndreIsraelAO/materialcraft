# Roadmap detail

> **Last reviewed:** 2026-11-02 · **Last updated:** 2026-11-02 · **Change:** major (first version) · **Target:** the professional node-based material workflow

## Milestones

| Milestone | Status | Scope |
|---|---|---|
| M0 Skeleton | Done | Workspace, licences, AGENTS.md, xtask (layers, wasm, ci, version, ico, icns), CI and packaging |
| M1 Core + engine | Done | Document model, images, node library, cooking with cache, session, commands, inspect, samples |
| M2 UI shell | Done | Docked layout, graph editor, library, properties, 2D and 3D views, dialogs, menus, status bar |
| M3 Node breadth | In progress | ~120 of ~400 library nodes |
| M4 Function graphs | In progress | Evaluator done; visual editor and dynamic parameters next |
| M5 Graph features | In progress | Exposed inputs and instances done; presets, groups, visibility next |
| M6 Files | In progress | Packages, bitmaps, export done; published archives, mesh import, browser I/O next |
| M7 Automation | Done | CLI, control channel with synthetic input, MCP server |
| M8 3D view depth | Planned | GPU renderer, tessellation, lights, meshes |
| M9 Web + packaging | In progress | WASM builds; browser file I/O and the first release run next |
| M10 Polish | Planned | Home screen at startup, keymap editor, translations, accessibility |

## Current focus

1. The visual function-graph editor (gap 1) and dynamic parameters (gap 4).
2. Node library breadth (gap 2): flood fill family, shape splatter, 3D noises, more grunge maps,
   whole-material blending and height blending.
3. FX-Map lite, Text and SVG nodes (gap 3).

## Remaining effort (Opus 5.5 agent hours)

| Work | Estimate |
|---|---:|
| Node library to ~300 nodes | 40 h |
| Function editor + dynamic parameters | 16 h |
| FX-Map, Text, SVG | 12 h |
| 3D view on the GPU, tessellation, lights, mesh import | 15 h |
| Mesh-based generators | 15 h |
| Files: published archives, browser I/O | 9 h |
| UX: presets, groups, keymap, translations, accessibility, home screen | 10 h |
| Release | 3 h |
| **Total** | **≈ 120 h** |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-02 | major | First version |
