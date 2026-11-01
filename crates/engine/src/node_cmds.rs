//! Node commands: add, move, rename, disable, parameters, base parameters, bindings, wires.

use serde_json::{Value as J, json};

use materialcraft_core::{NodeId, OutputFormat, SizeMode, Value};

use crate::commands::{CommandSpec, bad, boolp, has_graph, has_selection, i64p, ids_param, node_param, point, strp, u64p};
use crate::{Result, Session, cmd};

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "graph.node.add",
            "Add node",
            [],
            None,
            "{def (see library.list), at? [x, y], params? {id: value}, connectFrom? {node, port?} wires that output into the new node's first input, label?}",
            has_graph,
            add
        ),
        cmd!("graph.node.move", "Move nodes", [], None, "{ids?, by? [dx, dy], to? [x, y] (single node)}", has_selection, move_nodes),
        cmd!("graph.node.rename", "Rename node", [], None, "{id?, label}", has_selection, rename),
        cmd!("graph.node.disable", "Disable selection", [], Some("Shift+D"), "{ids?, disabled? (default: toggle)}", has_selection, disable),
        cmd!(
            "graph.node.param",
            "Set parameter",
            [],
            None,
            "{id?, param, value} (numbers, [x, y], \"#rrggbb\", enum index, typed {t, v})",
            has_selection,
            set_param
        ),
        cmd!("graph.node.reset", "Reset parameters", [], None, "{id?, param? (default: all)}", has_selection, reset),
        cmd!(
            "graph.node.base",
            "Set base parameters",
            [],
            None,
            "{id?, sizeMode? (relativeToInput|relativeToParent|absolute), size? [w, h] (log2 offset, or log2 size when absolute), format? (inherit|u8|u16|f16|f32), seed?, tiling? bool}",
            has_selection,
            base
        ),
        cmd!("graph.node.bind", "Bind parameter to input", [], None, "{id?, param, input (exposed id); input null unbinds}", has_selection, bind),
        cmd!(
            "graph.node.function",
            "Set parameter function",
            [],
            None,
            "{id?, param, function?: {nodes, output} (default: a constant of the current value), remove?: bool} drives a parameter with a function graph of the graph's inputs",
            has_selection,
            set_function
        ),
        cmd!("graph.connect", "Connect", [], None, "{from, fromPort? (output), to, toPort? (first input)}", has_graph, connect),
        cmd!("graph.disconnect", "Disconnect", [], None, "{to, toPort}", has_graph, disconnect),
    ]
}

fn add(s: &mut Session, p: &J) -> Result<J> {
    let def = strp(p, "def").ok_or_else(|| bad("graph.node.add", "`def` is required (see library.list)"))?.to_string();
    let catalog = materialcraft_nodes::find(&def);
    if catalog.is_none() {
        return Err(bad("graph.node.add", format!("unknown node `{def}`")));
    }
    // Typed parameter values, checked against the catalog.
    let mut params = Vec::new();
    if let Some(obj) = p.get("params").and_then(J::as_object) {
        for (k, v) in obj {
            let v = if def == "graph_instance" && k == "graph" {
                Value::Text(v.as_str().ok_or_else(|| bad("graph.node.add", "`graph` is a graph name"))?.into())
            } else {
                let pd = catalog.and_then(|d| d.param(k)).ok_or_else(|| bad("graph.node.add", format!("`{def}` has no parameter `{k}`")))?;
                pd.default.coerce_json(v).ok_or_else(|| bad("graph.node.add", format!("`{k}` takes a {}", pd.default.type_name())))?
            };
            params.push((k.clone(), v));
        }
    }
    let from = p.get("connectFrom").and_then(|c| Some((NodeId(c.get("node")?.as_u64()?), c.get("port").and_then(J::as_str).unwrap_or("output").to_string())));
    let at = point(p, "at").unwrap_or_else(|| {
        let g = s.graph().ok();
        let src = from.as_ref().and_then(|(n, _)| g.and_then(|g| g.node(*n).ok()));
        src.map_or([0.0, 0.0], |n| [n.pos[0] + 150.0, n.pos[1]])
    });
    let label = strp(p, "label").unwrap_or("").to_string();
    s.edit("Add node", |s| {
        let g = s.graph_mut()?;
        let id = g.add_node(&def, at);
        let n = g.node_mut(id)?;
        n.label = label;
        n.params.extend(params);
        if let Some((src, port)) = from {
            let first = s.input_ports(id).into_iter().next().ok_or_else(|| bad("graph.node.add", format!("`{def}` has no inputs")))?;
            s.graph_mut()?.connect(src, &port, id, &first)?;
        }
        s.selection = vec![id];
        Ok(json!({"id": id.0}))
    })
}

fn move_nodes(s: &mut Session, p: &J) -> Result<J> {
    let ids = ids_param(s, "graph.node.move", p)?;
    let by = point(p, "by");
    let to = point(p, "to");
    if by.is_none() && to.is_none() {
        return Err(bad("graph.node.move", "`by` or `to` is required"));
    }
    s.edit("Move nodes", |s| {
        let g = s.graph_mut()?;
        for n in g.nodes.iter_mut().filter(|n| ids.contains(&n.id)) {
            if let Some(t) = to {
                n.pos = t;
            } else if let Some(d) = by {
                n.pos = [n.pos[0] + d[0], n.pos[1] + d[1]];
            }
        }
        Ok(J::Null)
    })
}

fn rename(s: &mut Session, p: &J) -> Result<J> {
    let id = node_param(s, "graph.node.rename", p, "id")?;
    let label = strp(p, "label").unwrap_or("").trim().to_string();
    s.edit("Rename node", |s| {
        s.graph_mut()?.node_mut(id)?.label = label;
        Ok(J::Null)
    })
}

fn disable(s: &mut Session, p: &J) -> Result<J> {
    let ids = ids_param(s, "graph.node.disable", p)?;
    let set = boolp(p, "disabled");
    s.edit("Disable nodes", |s| {
        let g = s.graph_mut()?;
        for n in g.nodes.iter_mut().filter(|n| ids.contains(&n.id)) {
            n.disabled = set.unwrap_or(!n.disabled);
        }
        Ok(J::Null)
    })
}

fn set_param(s: &mut Session, p: &J) -> Result<J> {
    let id = node_param(s, "graph.node.param", p, "id")?;
    let param = strp(p, "param").ok_or_else(|| bad("graph.node.param", "`param` is required"))?.to_string();
    let v = p.get("value").ok_or_else(|| bad("graph.node.param", "`value` is required"))?;
    let node = s.graph()?.node(id)?.clone();
    let value = if node.def == "graph_instance" {
        // Instance parameters: the sub-graph's exposed inputs (and `graph`).
        if param == "graph" {
            Value::Text(v.as_str().ok_or_else(|| bad("graph.node.param", "`graph` is a graph name"))?.into())
        } else {
            let sub =
                materialcraft_eval::instance_graph(&s.pkg, &node).map(|(_, g)| g).ok_or_else(|| bad("graph.node.param", "the instanced graph is missing"))?;
            let e = sub.exposed.iter().find(|e| e.id == param).ok_or_else(|| bad("graph.node.param", format!("the graph has no input `{param}`")))?;
            e.default.coerce_json(v).ok_or_else(|| bad("graph.node.param", format!("`{param}` takes a {}", e.default.type_name())))?
        }
    } else {
        let def = materialcraft_nodes::find(&node.def).ok_or_else(|| bad("graph.node.param", format!("unknown node `{}`", node.def)))?;
        let pd = def.param(&param).ok_or_else(|| bad("graph.node.param", format!("`{}` has no parameter `{param}`", node.def)))?;
        let mut v = pd.default.coerce_json(v).ok_or_else(|| bad("graph.node.param", format!("`{param}` takes a {}", pd.default.type_name())))?;
        if let Value::Enum(i) = &mut v
            && !pd.options.is_empty()
        {
            *i = (*i).min(pd.options.len() as u32 - 1);
        }
        v
    };
    s.edit_merged(&format!("param:{}:{param}", id.0), "Set parameter", |s| {
        s.graph_mut()?.node_mut(id)?.params.insert(param.clone(), value);
        Ok(J::Null)
    })
}

fn reset(s: &mut Session, p: &J) -> Result<J> {
    let id = node_param(s, "graph.node.reset", p, "id")?;
    let param = strp(p, "param").map(str::to_string);
    s.edit("Reset parameters", |s| {
        let n = s.graph_mut()?.node_mut(id)?;
        match &param {
            Some(k) => {
                n.params.remove(k);
            }
            None => n.params.retain(|k, _| k == "graph" || k == "identifier"),
        }
        Ok(J::Null)
    })
}

fn base(s: &mut Session, p: &J) -> Result<J> {
    let id = node_param(s, "graph.node.base", p, "id")?;
    let mode = match strp(p, "sizeMode") {
        None => None,
        Some("relativeToInput") => Some(SizeMode::RelativeToInput),
        Some("relativeToParent") => Some(SizeMode::RelativeToParent),
        Some("absolute") => Some(SizeMode::Absolute),
        Some(o) => return Err(bad("graph.node.base", format!("unknown sizeMode `{o}`"))),
    };
    let format = match strp(p, "format") {
        None => None,
        Some("inherit") => Some(OutputFormat::Inherit),
        Some("u8") => Some(OutputFormat::U8),
        Some("u16") => Some(OutputFormat::U16),
        Some("f16") => Some(OutputFormat::F16),
        Some("f32") => Some(OutputFormat::F32),
        Some(o) => return Err(bad("graph.node.base", format!("unknown format `{o}`"))),
    };
    let size = p
        .get("size")
        .and_then(J::as_array)
        .map(|a| [a.first().and_then(J::as_i64).unwrap_or(0).clamp(-12, 13) as i32, a.get(1).and_then(J::as_i64).unwrap_or(0).clamp(-12, 13) as i32]);
    let seed = i64p(p, "seed").map(|v| v.clamp(0, i32::MAX as i64) as i32);
    let tiling = boolp(p, "tiling");
    s.edit_merged(&format!("base:{}", id.0), "Set base parameters", |s| {
        let b = &mut s.graph_mut()?.node_mut(id)?.base;
        if let Some(m) = mode {
            b.size_mode = m;
        }
        if let Some(sz) = size {
            b.size = sz;
        }
        if let Some(f) = format {
            b.format = f;
        }
        if let Some(sd) = seed {
            b.seed = sd;
        }
        if let Some(t) = tiling {
            b.tiling = [t, t];
        }
        Ok(J::Null)
    })
}

fn bind(s: &mut Session, p: &J) -> Result<J> {
    let id = node_param(s, "graph.node.bind", p, "id")?;
    let param = strp(p, "param").ok_or_else(|| bad("graph.node.bind", "`param` is required"))?.to_string();
    let input = strp(p, "input").map(str::to_string);
    if let Some(i) = &input
        && !s.graph()?.exposed.iter().any(|e| &e.id == i)
    {
        return Err(bad("graph.node.bind", format!("no input `{i}`")));
    }
    s.edit("Bind parameter", |s| {
        let n = s.graph_mut()?.node_mut(id)?;
        match input {
            Some(i) => n.bindings.insert(param, i),
            None => n.bindings.remove(&param),
        };
        Ok(J::Null)
    })
}

fn set_function(s: &mut Session, p: &J) -> Result<J> {
    let id = node_param(s, "graph.node.function", p, "id")?;
    let param = strp(p, "param").ok_or_else(|| bad("graph.node.function", "`param` is required"))?.to_string();
    let node = s.graph()?.node(id)?.clone();
    let def = materialcraft_nodes::find(&node.def).ok_or_else(|| bad("graph.node.function", "unknown node type"))?;
    let pd = def.param(&param).ok_or_else(|| bad("graph.node.function", format!("`{}` has no parameter `{param}`", node.def)))?;
    if matches!(pd.default, Value::Text(_) | Value::Curve(_) | Value::Gradient(_) | Value::Function(_)) {
        return Err(bad("graph.node.function", format!("`{param}` is a {} and can't take a function", pd.default.type_name())));
    }
    if boolp(p, "remove") == Some(true) {
        return s.edit("Remove parameter function", |s| {
            s.graph_mut()?.node_mut(id)?.functions.remove(&param);
            Ok(J::Null)
        });
    }
    let f = match p.get("function") {
        Some(f) if !f.is_null() => {
            serde_json::from_value::<materialcraft_core::function::FnGraph>(f.clone()).map_err(|e| bad("graph.node.function", format!("bad function: {e}")))?
        }
        _ => match node.functions.get(&param) {
            Some(f) => f.clone(),
            None => materialcraft_core::function::FnGraph::constant(node.params.get(&param).cloned().unwrap_or_else(|| pd.default.clone())),
        },
    };
    if f.nodes.len() > 4096 {
        return Err(bad("graph.node.function", "the function is too large"));
    }
    s.edit_merged(&format!("fn:{}:{param}", id.0), "Set parameter function", |s| {
        s.graph_mut()?.node_mut(id)?.functions.insert(param.clone(), f);
        Ok(J::Null)
    })
}

fn connect(s: &mut Session, p: &J) -> Result<J> {
    let from = NodeId(u64p(p, "from").ok_or_else(|| bad("graph.connect", "`from` is required"))?);
    let to = NodeId(u64p(p, "to").ok_or_else(|| bad("graph.connect", "`to` is required"))?);
    let (ins, outs) = (s.input_ports(to), s.output_ports(from));
    let from_port =
        strp(p, "fromPort").map(str::to_string).or_else(|| outs.first().cloned()).ok_or_else(|| bad("graph.connect", "the source has no outputs"))?;
    let to_port = strp(p, "toPort").map(str::to_string).or_else(|| ins.first().cloned()).ok_or_else(|| bad("graph.connect", "the target has no inputs"))?;
    if !outs.contains(&from_port) {
        return Err(bad("graph.connect", format!("no output `{from_port}` on node {} (has {outs:?})", from.0)));
    }
    if !ins.contains(&to_port) {
        return Err(bad("graph.connect", format!("no input `{to_port}` on node {} (has {ins:?})", to.0)));
    }
    s.edit("Connect", |s| {
        s.graph_mut()?.connect(from, &from_port, to, &to_port)?;
        Ok(J::Null)
    })
}

fn disconnect(s: &mut Session, p: &J) -> Result<J> {
    let to = NodeId(u64p(p, "to").ok_or_else(|| bad("graph.disconnect", "`to` is required"))?);
    let port = strp(p, "toPort").ok_or_else(|| bad("graph.disconnect", "`toPort` is required"))?.to_string();
    s.edit("Disconnect", |s| {
        if !s.graph_mut()?.disconnect(to, &port) {
            return Err(bad("graph.disconnect", "nothing is connected there"));
        }
        Ok(J::Null)
    })
}

impl Session {
    /// Input port ids of node `id` (empty for a missing node).
    pub fn input_ports(&self, id: NodeId) -> Vec<String> {
        self.graph()
            .ok()
            .and_then(|g| g.node(id).ok())
            .map(|n| materialcraft_eval::ports(&self.pkg, n).0.into_iter().map(|p| p.id).collect())
            .unwrap_or_default()
    }

    /// Output port ids of node `id`.
    pub fn output_ports(&self, id: NodeId) -> Vec<String> {
        self.graph()
            .ok()
            .and_then(|g| g.node(id).ok())
            .map(|n| materialcraft_eval::ports(&self.pkg, n).1.into_iter().map(|p| p.id).collect())
            .unwrap_or_default()
    }
}
