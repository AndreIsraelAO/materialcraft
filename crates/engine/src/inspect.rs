//! `session.inspect`: everything an agent needs to verify its work without a screenshot.

use serde_json::{Value as J, json};

use crate::Session;

pub fn session(s: &Session, graph: Option<usize>) -> J {
    let gi = graph.unwrap_or(s.graph);
    let g = s.pkg.graphs.get(gi);
    let cooked = (gi == s.graph).then_some(&s.cooked);
    let graph_json = g.map(|g| {
        json!({
            "index": gi,
            "name": g.name,
            "size": g.size_px(),
            "format": g.format,
            "seed": g.seed,
            "attributes": g.attributes,
            "exposed": g.exposed,
            "inputs": g.inputs,
            "nodes": g.nodes.iter().map(|n| {
                let r = cooked.and_then(|c| c.nodes.get(&n.id));
                let (ins, outs) = materialcraft_eval::ports(&s.pkg, n);
                json!({
                    "id": n.id.0,
                    "def": n.def,
                    "label": if n.label.is_empty() { materialcraft_nodes::find(&n.def).map_or(n.def.as_str(), |d| d.label).to_string() } else { n.label.clone() },
                    "pos": n.pos,
                    "params": n.params,
                    "bindings": n.bindings,
                    "base": n.base,
                    "disabled": n.disabled,
                    "inputs": ins.iter().map(|p| &p.id).collect::<Vec<_>>(),
                    "outputs": outs.iter().map(|p| &p.id).collect::<Vec<_>>(),
                    "cooked": r.map(|r| json!({
                        "error": r.error, "ms": r.ms, "cached": r.cached,
                        "size": r.outputs.first().map(|i| [i.w, i.h]),
                        "kind": r.outputs.first().map(|i| if i.is_gray() { "gray" } else { "color" }),
                        "mean": r.outputs.first().map(|i| i.mean()),
                    })),
                })
            }).collect::<Vec<_>>(),
            "connections": g.connections,
            "frames": g.frames,
            "comments": g.comments,
            "pins": g.pins,
        })
    });
    json!({
        "package": {"name": s.pkg.name, "path": s.path, "modified": s.modified, "resources": s.pkg.resources.iter().map(|r| json!({"name": r.name, "bytes": r.bytes.len()})).collect::<Vec<_>>()},
        "graphs": s.pkg.graphs.iter().enumerate().map(|(i, g)| json!({"index": i, "name": g.name, "nodes": g.nodes.len(), "kind": g.kind})).collect::<Vec<_>>(),
        "openGraph": s.graph,
        "graph": graph_json,
        "selection": s.selection.iter().map(|i| i.0).collect::<Vec<_>>(),
        "view2d": s.view2d,
        "view3d": s.view3d,
        "previewSize": s.preview_size,
        "cookMs": s.cooked.ms,
        "dirty": s.dirty,
        "history": s.history(),
        "canUndo": s.can_undo(),
        "canRedo": s.can_redo(),
        "log": s.log.iter().rev().take(20).collect::<Vec<_>>(),
    })
}
