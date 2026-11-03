//! Wire commands: edit a single wire (connection) of the open graph, addressed by the input it
//! feeds (`to`, `toPort`). Deleting a wire is `graph.disconnect`; this module splits a wire with
//! a reroute dot so it can be repositioned.

use serde_json::{Value as J, json};

use materialcraft_core::NodeId;

use crate::commands::{CommandSpec, bad, has_graph, point, strp, u64p};
use crate::{Result, Session, cmd};

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "graph.wire.insertDot",
        "Insert reroute dot",
        [],
        None,
        "{to, toPort, at? [x, y] (graph point, default: midway)} splits the wire feeding to.toPort with a Dot node there, selected",
        has_graph,
        insert_dot
    )]
}

fn insert_dot(s: &mut Session, p: &J) -> Result<J> {
    const ID: &str = "graph.wire.insertDot";
    let to = NodeId(u64p(p, "to").ok_or_else(|| bad(ID, "`to` is required"))?);
    let to_port = strp(p, "toPort").ok_or_else(|| bad(ID, "`toPort` is required"))?.to_string();
    let g = s.graph()?;
    let c = g.input_of(to, &to_port).cloned().ok_or_else(|| bad(ID, format!("no wire feeds `{to_port}` of node {}", to.0)))?;
    let at = match point(p, "at") {
        Some(at) => at,
        None => {
            let (a, b) = (g.node(c.from)?.pos, g.node(to)?.pos);
            [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]
        }
    };
    // Hostile but finite coordinates stay within a sane canvas.
    let at = at.map(|v| v.clamp(-1e7, 1e7));
    s.edit("Insert reroute dot", |s| {
        let id = s.graph_mut()?.add_node("dot", at);
        let input = s.input_ports(id).into_iter().next().ok_or_else(|| bad(ID, "the dot has no input"))?;
        let output = s.output_ports(id).into_iter().next().ok_or_else(|| bad(ID, "the dot has no output"))?;
        let g = s.graph_mut()?;
        g.connect(c.from, &c.from_port, id, &input)?;
        g.connect(id, &output, to, &to_port)?;
        s.selection = vec![id];
        Ok(json!({"id": id.0}))
    })
}
