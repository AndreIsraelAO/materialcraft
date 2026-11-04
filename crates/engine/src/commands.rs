//! The command registry. Every user-visible action is a command with a stable id; the menus,
//! shortcuts, CLI, control channel and MCP server all dispatch them by id.

use serde_json::{Value as J, json};

use materialcraft_core::NodeId;

use crate::{EngineError, Result, Session};

pub type Run = fn(&mut Session, &J) -> Result<J>;
pub type Enabled = fn(&Session) -> std::result::Result<(), String>;

pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// Menu placement, e.g. `["Edit"]`. Empty = not in menus.
    pub menu: &'static [&'static str],
    /// Default shortcut in egui notation (`Ctrl+Shift+E`).
    pub shortcut: Option<&'static str>,
    /// Parameters, for humans and agents.
    pub params: &'static str,
    pub enabled: Enabled,
    pub run: Run,
}

pub(crate) fn always(_: &Session) -> std::result::Result<(), String> {
    Ok(())
}

pub(crate) fn has_graph(s: &Session) -> std::result::Result<(), String> {
    s.graph().map(|_| ()).map_err(|_| "no graph is open".into())
}

pub(crate) fn has_selection(s: &Session) -> std::result::Result<(), String> {
    has_graph(s)?;
    if s.selection.is_empty() { Err("no node is selected".into()) } else { Ok(()) }
}

#[macro_export]
#[doc(hidden)]
macro_rules! cmd {
    ($id:literal, $label:literal, [$($m:literal),*], $sc:expr, $params:literal, $en:expr, $run:expr) => {
        $crate::commands::CommandSpec { id: $id, label: $label, menu: &[$($m),*], shortcut: $sc, params: $params, enabled: $en, run: $run }
    };
}

pub fn command_specs() -> &'static [CommandSpec] {
    static SPECS: std::sync::OnceLock<Vec<CommandSpec>> = std::sync::OnceLock::new();
    SPECS.get_or_init(|| {
        let mut v = Vec::new();
        v.extend(crate::file_cmds::specs());
        v.extend(crate::material_cmds::specs());
        v.extend(crate::edit_cmds::specs());
        v.extend(crate::graph_cmds::specs());
        v.extend(crate::node_cmds::specs());
        v.extend(crate::view_cmds::specs());
        v.extend(crate::wire_cmds::specs());
        v
    })
}

pub fn find(id: &str) -> Option<&'static CommandSpec> {
    command_specs().iter().find(|c| c.id == id)
}

/// Run a command by id with JSON params (empty params = defaults, never a dialog).
pub fn execute(s: &mut Session, id: &str, params: &J) -> Result<J> {
    let spec = find(id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
    (spec.enabled)(s).map_err(|why| EngineError::Disabled { cmd: id.to_string(), why })?;
    (spec.run)(s, params)
}

/// Every command, as JSON (for `commands` listings and MCP).
pub fn list(s: &Session, filter: Option<&str>) -> J {
    let f = filter.map(str::to_ascii_lowercase);
    J::Array(
        command_specs()
            .iter()
            .filter(|c| f.as_ref().is_none_or(|f| c.id.to_ascii_lowercase().contains(f) || c.label.to_ascii_lowercase().contains(f)))
            .map(|c| json!({"id": c.id, "label": c.label, "menu": c.menu, "shortcut": c.shortcut, "params": c.params, "enabled": (c.enabled)(s).is_ok()}))
            .collect(),
    )
}

// ---------- param helpers ----------

pub(crate) fn bad(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.into(), msg: msg.into() }
}

pub(crate) fn f32p(p: &J, key: &str) -> Option<f32> {
    p.get(key).and_then(J::as_f64).map(|v| v as f32).filter(|v| v.is_finite())
}

pub(crate) fn u64p(p: &J, key: &str) -> Option<u64> {
    p.get(key).and_then(|v| v.as_u64().or_else(|| v.as_f64().filter(|f| f.is_finite() && *f >= 0.0).map(|f| f.round() as u64)))
}

pub(crate) fn i64p(p: &J, key: &str) -> Option<i64> {
    p.get(key).and_then(|v| v.as_i64().or_else(|| v.as_f64().filter(|f| f.is_finite()).map(|f| f.round() as i64)))
}

pub(crate) fn boolp(p: &J, key: &str) -> Option<bool> {
    p.get(key).and_then(J::as_bool)
}

pub(crate) fn strp<'a>(p: &'a J, key: &str) -> Option<&'a str> {
    p.get(key).and_then(J::as_str)
}

/// A node id param (`key`), defaulting to the last selected node.
pub(crate) fn node_param(s: &Session, cmd: &str, p: &J, key: &str) -> Result<NodeId> {
    let id = match u64p(p, key) {
        Some(id) => NodeId(id),
        None => *s.selection.last().ok_or_else(|| bad(cmd, format!("no node selected and no `{key}` given")))?,
    };
    s.graph()?.node(id).map_err(|_| bad(cmd, format!("no node {}", id.0)))?;
    Ok(id)
}

/// A list of node ids (`ids`), defaulting to the selection.
pub(crate) fn ids_param(s: &Session, cmd: &str, p: &J) -> Result<Vec<NodeId>> {
    let ids: Vec<NodeId> = match p.get("ids").and_then(J::as_array) {
        Some(a) => a.iter().filter_map(J::as_u64).map(NodeId).collect(),
        None => match u64p(p, "id") {
            Some(id) => vec![NodeId(id)],
            None => s.selection.clone(),
        },
    };
    let g = s.graph()?;
    for id in &ids {
        g.node(*id).map_err(|_| bad(cmd, format!("no node {}", id.0)))?;
    }
    Ok(ids)
}

/// A 2D point param (`[x, y]`).
pub(crate) fn point(p: &J, key: &str) -> Option<[f32; 2]> {
    let a = p.get(key)?.as_array()?;
    let x = a.first()?.as_f64()? as f32;
    let y = a.get(1)?.as_f64()? as f32;
    (x.is_finite() && y.is_finite()).then_some([x, y])
}
