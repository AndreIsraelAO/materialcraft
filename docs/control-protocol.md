# Control protocol and MCP

> **Last reviewed:** 2026-11-02 · **Last updated:** 2026-11-02 · **Change:** major (first version) · **Target:** the professional node-based material workflow

Agents drive MaterialCraft three ways, all dispatching the same engine commands:

1. **MCP** (`materialcraft-cli mcp`): headless (an in-process session, files only below
   `--read-root` / `--write-root`) or bridged to the running app (`--bridge <port>`).
2. **JSON lines** (`materialcraft-cli serve`): the same methods on stdio, or on `127.0.0.1:<port>`
   behind a token.
3. **The app's control channel** (`materialcraft --control <port>`): one JSON request per line,
   `{"id", "method", "params"}` → `{"id", "ok": true, "result"}` or `{"id", "ok": false, "error"}`.
   The first request must be `{"method":"auth","params":{"token":"…"}}` with the token from
   `MATERIALCRAFT_CONTROL_TOKEN` or the file `<temp>/materialcraft-control-<port>.token`.
   Set `MATERIALCRAFT_NO_PREFS=1` for scripted runs so the user's layout is left alone.

## Methods

| Method | Where | What |
|---|---|---|
| `ping` | all | `"pong"` |
| `command.list {filter?}` | all | every command: id, label, menu, shortcut, params doc, enabled |
| `command.run {id, params?}` | all | run one command (programmatic: never opens a dialog) |
| `command.batch {steps}` | headless | run `[{id, params}]` in order, stop at the first error |
| `session.inspect` | all | package, graphs, the open graph's nodes (def, params, bindings, base, ports, cook error/size/mean/ms), wires, frames, comments, pins, exposed inputs, views, history |
| `package.open {path}` / `package.save {path?}` | headless | workspace-scoped package I/O |
| `bitmap.import {path, name?}` | headless | embed an image as a resource |
| `outputs.export {dir, format?, pattern?, depth?, graph?}` | headless | cook at full size and write every output |
| `render {view?, width?, height?, yaw?, pitch?, scene?, node?, port?, full?}` | headless | PNG (base64) of the 3D view or of one node |
| `ui.inspect` | app | UI state, the graph view rectangle, every node's screen rectangle, every port's screen position, fps, toasts |
| `ui.menu.list` / `ui.menu.invoke {id}` | app | the menu catalog; behave like a click |
| `ui.set {...}` | app | merge fields into the UI state |
| `ui.input {events}` | app | synthetic input, below |
| `view.screenshot {view?: "3d"\|"2d"}` | app | PNG of the 3D or 2D view |
| `ui.screenshot` | app | PNG of the whole window |
| `ui.resize {width, height}` | app | window size in points |

### Synthetic input (`ui.input`)

Events are applied one frame at a time, so egui sees real presses, drags and releases. Positions are
window points (take them from `ui.inspect`'s `nodes` and `ports`).

```json
{"method": "ui.input", "params": {"events": [
  {"type": "drag", "from": [512, 140], "to": [600, 220], "steps": 12},
  {"type": "click", "pos": [700, 300], "button": "left", "modifiers": ["ctrl"]},
  {"type": "doubleClick", "pos": [640, 180]},
  {"type": "scroll", "pos": [640, 300], "delta": [0, -120]},
  {"type": "key", "key": "Space"},
  {"type": "text", "text": "perlin"},
  {"type": "key", "key": "Enter"}
]}}
```

Types: `move`, `down`, `up`, `click`, `doubleClick`, `drag`, `scroll`, `key` (egui key names:
`A`, `Enter`, `Space`, `Delete`, `F5`, …), `text`. Buttons: `left`, `right`, `middle`.

## Key commands

| Command | Params |
|---|---|
| `file.new` | `{template?, name?, size?}`; templates from `graph.templates` (`empty`, `pbr`, `bricks`, `stone_tiles`, `wood`, `rusty_metal`, `fabric`, `ground`) |
| `graph.node.add` | `{def, at?, params?, connectFrom?: {node, port?}, label?}` |
| `graph.connect` / `graph.disconnect` | `{from, fromPort?, to, toPort?}` / `{to, toPort}` |
| `graph.node.param` | `{id?, param, value}`: numbers, `[x, y]`, `"#rrggbb"`, enum index, curve `[[x, y], …]`, gradient `[[pos, color], …]`, or a typed value `{"t": "function", "v": {...}}` |
| `graph.node.base` | `{id?, sizeMode?, size?, format?, seed?, tiling?}` |
| `graph.exposed.add` / `graph.exposed.set` | `{id, label?, default?, bind?: {node, param}}` / `{id, value}` |
| `graph.node.function` | `{id?, param, function}` drives a parameter by a function graph of the graph's inputs (dynamic parameter); `{id?, param, remove: true}` removes it |
| `graph.preset.save` / `.apply` / `.delete` | `{name}`: store, restore or remove the current input values |
| `graph.createFromSelection` | `{name?, ids?}`: move nodes into a sub-graph and replace them with an instance |
| `view.2d` / `view.3d` | which node the 2D view shows, channel, tiling, histogram; scene, environment, camera, tiling, parallax depth |
| `view.3d.loadMesh` | `{path}` shows an OBJ, glTF or GLB mesh (scene `custom`) |
| `library.list` | `{filter?, category?}`: every node with ports and parameters |

## Function graphs

Pixel and value processors carry a `function` parameter: `{"nodes": [{"id", "op", "inputs": [ids],
"value"?}], "output": id}`. Ops: constants (`const_float`, `const_float2/3/4`, `const_int`,
`const_bool`), variables (`get_pos` = normalized pixel position, `get_size`, `get_var` with
`value` = an exposed input id), samplers (`sample_gray`, `sample_color` with `value` = input index
0–3, input = a float2 position), vectors (`vec2/3/4`, `swizzle` with `value` = `"xy"` etc.),
casts, arithmetic (`add sub mul div mod neg dot`), logic and comparison (`and or not gt ge lt le eq
ne`), functions (`abs floor ceil frac sqrt exp log sin cos tan atan2 pow min max clamp lerp length
normalize rand noise`) and `if_else`. Values are 4-wide; scalars broadcast.

## MCP tools

`session_inspect`, `command_list`, `command_run`, `command_batch`, `library_list`, `graph_new`,
`node_add`, `node_connect`, `node_param`, `package_open`, `package_save`, `bitmap_import`,
`outputs_export`, `view_render` (headless) and, bridged to the app, `view_screenshot`,
`ui_screenshot`, `ui_inspect`, `ui_menu_invoke`, `ui_input`.
