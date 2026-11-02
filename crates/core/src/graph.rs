//! Graphs: nodes, connections, frames, comments, navigation pins and exposed inputs.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{CoreError, Result, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u64);

/// How a node's output size follows its surroundings ("Output size" base parameter).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SizeMode {
    /// The size of the main (first connected) input; the graph size when none is connected.
    #[default]
    RelativeToInput,
    /// The graph ("parent") size.
    RelativeToParent,
    /// `offset` is the absolute log2 size (e.g. 10 = 1024 px).
    Absolute,
}

/// The output bit depth ("Output format").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OutputFormat {
    /// The main input's format; 16 bits for generators.
    #[default]
    Inherit,
    U8,
    U16,
    F16,
    F32,
}

impl OutputFormat {
    pub const ALL: [OutputFormat; 5] = [OutputFormat::Inherit, OutputFormat::U8, OutputFormat::U16, OutputFormat::F16, OutputFormat::F32];

    pub fn label(self) -> &'static str {
        match self {
            OutputFormat::Inherit => "Relative to input",
            OutputFormat::U8 => "8 bits per channel",
            OutputFormat::U16 => "16 bits per channel",
            OutputFormat::F16 => "16 bits per channel float (HDR)",
            OutputFormat::F32 => "32 bits per channel float (HDR)",
        }
    }

    /// The suffix of a node's size label: `8`, `16`, `16F`, `32F`.
    pub fn bits(self) -> &'static str {
        match self {
            OutputFormat::Inherit | OutputFormat::U16 => "16",
            OutputFormat::U8 => "8",
            OutputFormat::F16 => "16F",
            OutputFormat::F32 => "32F",
        }
    }
}

/// Parameters every node has ("Base parameters").
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BaseParams {
    pub size_mode: SizeMode,
    /// log2 offset per axis (relative modes) or log2 size (absolute).
    pub size: [i32; 2],
    pub format: OutputFormat,
    /// Added to the graph's random seed for this node.
    pub seed: i32,
    /// Whether sampling wraps at the edges (horizontal, vertical).
    pub tiling: [bool; 2],
}

impl Default for BaseParams {
    fn default() -> Self {
        BaseParams { size_mode: SizeMode::RelativeToInput, size: [0, 0], format: OutputFormat::Inherit, seed: 0, tiling: [true, true] }
    }
}

/// A node instance: a catalog definition (`def`) with parameter overrides.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: NodeId,
    /// Catalog id, e.g. `blend`, `perlin_noise`, `output`, `input_color`, `graph_instance`.
    pub def: String,
    /// User label; empty = the definition's label.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    /// Graph-space position of the node's top-left corner.
    pub pos: [f32; 2],
    /// Values that differ from the catalog defaults.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, Value>,
    /// Parameters driven by a graph input (param id → exposed input id).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bindings: BTreeMap<String, String>,
    /// Dynamic parameters: param id → a function graph computing its value from the graph's
    /// inputs at cook time.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub functions: BTreeMap<String, crate::function::FnGraph>,
    #[serde(default)]
    pub base: BaseParams,
    /// A disabled node passes its first input through.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
}

impl Node {
    pub fn new(id: NodeId, def: &str, pos: [f32; 2]) -> Node {
        Node {
            id,
            def: def.into(),
            label: String::new(),
            pos,
            params: BTreeMap::new(),
            bindings: BTreeMap::new(),
            functions: BTreeMap::new(),
            base: BaseParams::default(),
            disabled: false,
        }
    }
}

/// A link from an output port to an input port.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub from: NodeId,
    pub from_port: String,
    pub to: NodeId,
    pub to_port: String,
}

/// A titled, colored rectangle behind a group of nodes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub id: u64,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// `[x, y, width, height]` in graph space.
    pub rect: [f32; 4],
    /// Linear RGBA (alpha is the fill opacity).
    pub color: [f32; 4],
}

/// A sticky note on the canvas.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Comment {
    pub id: u64,
    pub text: String,
    pub pos: [f32; 2],
    #[serde(default = "comment_size")]
    pub size: [f32; 2],
}

fn comment_size() -> [f32; 2] {
    [180.0, 70.0]
}

/// A named bookmark on the canvas (Previous/Next pin).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pin {
    pub id: u64,
    pub name: String,
    pub pos: [f32; 2],
}

/// The kind of value an exposed graph input carries.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExposedParam {
    /// Identifier, unique in the graph.
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    pub default: Value,
    #[serde(default)]
    pub min: f32,
    #[serde(default = "one")]
    pub max: f32,
    /// Labels when the value is an enum.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
}

fn one() -> f32 {
    1.0
}

/// A named set of input values (graph presets).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub values: BTreeMap<String, Value>,
}

/// Descriptive metadata ("Attributes").
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Attributes {
    pub category: String,
    pub description: String,
    pub author: String,
    pub tags: String,
}

/// Whether a graph makes images or is a function.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GraphKind {
    #[default]
    Material,
    Function,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Graph {
    pub name: String,
    #[serde(default)]
    pub kind: GraphKind,
    pub nodes: Vec<Node>,
    pub connections: Vec<Connection>,
    #[serde(default)]
    pub frames: Vec<Frame>,
    #[serde(default)]
    pub comments: Vec<Comment>,
    #[serde(default)]
    pub pins: Vec<Pin>,
    /// log2 of the graph ("parent") size per axis.
    pub size: [u32; 2],
    #[serde(default)]
    pub format: OutputFormat,
    #[serde(default)]
    pub seed: i32,
    #[serde(default)]
    pub exposed: Vec<ExposedParam>,
    /// Current values of the exposed inputs (missing = default).
    #[serde(default)]
    pub inputs: BTreeMap<String, Value>,
    /// Named sets of input values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presets: Vec<Preset>,
    /// Preview images of the graph's input nodes: identifier → bitmap resource (a mesh map
    /// baked in TextureCraft, say). Instances feed their inputs from their ports instead.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub input_images: BTreeMap<String, String>,
    #[serde(default)]
    pub attributes: Attributes,
    /// The id the next node, frame, comment or pin gets.
    pub next_id: u64,
}

/// Smallest and largest graph size, as log2 (16 px .. 8192 px).
pub const MIN_SIZE_LOG2: u32 = 4;
pub const MAX_SIZE_LOG2: u32 = 13;

impl Graph {
    pub fn new(name: &str) -> Graph {
        Graph {
            name: name.into(),
            kind: GraphKind::Material,
            nodes: Vec::new(),
            connections: Vec::new(),
            frames: Vec::new(),
            comments: Vec::new(),
            pins: Vec::new(),
            size: [10, 10],
            format: OutputFormat::U16,
            seed: 0,
            exposed: Vec::new(),
            inputs: BTreeMap::new(),
            presets: Vec::new(),
            input_images: BTreeMap::new(),
            attributes: Attributes::default(),
            next_id: 1,
        }
    }

    pub fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    pub fn node(&self, id: NodeId) -> Result<&Node> {
        self.nodes.iter().find(|n| n.id == id).ok_or(CoreError::NoNode(id.0))
    }

    pub fn node_mut(&mut self, id: NodeId) -> Result<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id).ok_or(CoreError::NoNode(id.0))
    }

    /// Add a node of definition `def` at `pos`; returns its id.
    pub fn add_node(&mut self, def: &str, pos: [f32; 2]) -> NodeId {
        let id = NodeId(self.alloc_id());
        self.nodes.push(Node::new(id, def, pos));
        id
    }

    /// Remove nodes and every connection touching them.
    pub fn remove_nodes(&mut self, ids: &[NodeId]) {
        self.nodes.retain(|n| !ids.contains(&n.id));
        self.connections.retain(|c| !ids.contains(&c.from) && !ids.contains(&c.to));
    }

    /// Connect `from.from_port` → `to.to_port`, replacing whatever fed that input. Refuses
    /// connections that would make a cycle.
    pub fn connect(&mut self, from: NodeId, from_port: &str, to: NodeId, to_port: &str) -> Result<()> {
        self.node(from)?;
        self.node(to)?;
        if from == to || self.reaches(to, from) {
            return Err(CoreError::Cycle);
        }
        self.connections.retain(|c| !(c.to == to && c.to_port == to_port));
        self.connections.push(Connection { from, from_port: from_port.into(), to, to_port: to_port.into() });
        Ok(())
    }

    pub fn disconnect(&mut self, to: NodeId, to_port: &str) -> bool {
        let before = self.connections.len();
        self.connections.retain(|c| !(c.to == to && c.to_port == to_port));
        before != self.connections.len()
    }

    /// The connection feeding `to.to_port`.
    pub fn input_of(&self, to: NodeId, to_port: &str) -> Option<&Connection> {
        self.connections.iter().find(|c| c.to == to && c.to_port == to_port)
    }

    /// Whether `b` is downstream of (or equal to) `a`.
    pub fn reaches(&self, a: NodeId, b: NodeId) -> bool {
        let mut stack = vec![a];
        let mut seen = std::collections::BTreeSet::new();
        while let Some(n) = stack.pop() {
            if n == b {
                return true;
            }
            if !seen.insert(n) {
                continue;
            }
            stack.extend(self.connections.iter().filter(|c| c.from == n).map(|c| c.to));
        }
        false
    }

    /// Nodes in dependency order (inputs before the nodes they feed). Nodes on a cycle (only
    /// possible in a hand-edited file) are left out.
    pub fn topo_order(&self) -> Vec<NodeId> {
        let mut indeg: BTreeMap<NodeId, usize> = self.nodes.iter().map(|n| (n.id, 0)).collect();
        for c in &self.connections {
            if let Some(d) = indeg.get_mut(&c.to)
                && self.nodes.iter().any(|n| n.id == c.from)
            {
                *d += 1;
            }
        }
        let mut ready: Vec<NodeId> = indeg.iter().filter(|(_, d)| **d == 0).map(|(id, _)| *id).collect();
        let mut out = Vec::with_capacity(self.nodes.len());
        while let Some(n) = ready.pop() {
            out.push(n);
            for c in self.connections.iter().filter(|c| c.from == n) {
                if let Some(d) = indeg.get_mut(&c.to) {
                    *d = d.saturating_sub(1);
                    if *d == 0 {
                        ready.push(c.to);
                    }
                }
            }
        }
        out
    }

    /// Every node upstream of `id` (not including it).
    pub fn upstream(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(n) = stack.pop() {
            for c in self.connections.iter().filter(|c| c.to == n) {
                if !out.contains(&c.from) {
                    out.push(c.from);
                    stack.push(c.from);
                }
            }
        }
        out
    }

    /// Every node downstream of `id` (not including it).
    pub fn downstream(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(n) = stack.pop() {
            for c in self.connections.iter().filter(|c| c.from == n) {
                if !out.contains(&c.to) {
                    out.push(c.to);
                    stack.push(c.to);
                }
            }
        }
        out
    }

    /// The value of exposed input `id` (its default when unset).
    pub fn input_value(&self, id: &str) -> Option<&Value> {
        self.inputs.get(id).or_else(|| self.exposed.iter().find(|e| e.id == id).map(|e| &e.default))
    }

    /// Graph size in pixels.
    pub fn size_px(&self) -> [u32; 2] {
        [1 << self.size[0].clamp(MIN_SIZE_LOG2, MAX_SIZE_LOG2), 1 << self.size[1].clamp(MIN_SIZE_LOG2, MAX_SIZE_LOG2)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_refuses_cycles_and_replaces_inputs() {
        let mut g = Graph::new("g");
        let a = g.add_node("uniform", [0.0, 0.0]);
        let b = g.add_node("blur", [100.0, 0.0]);
        let c = g.add_node("blur", [200.0, 0.0]);
        g.connect(a, "output", b, "input").unwrap();
        g.connect(b, "output", c, "input").unwrap();
        assert!(matches!(g.connect(c, "output", a, "input"), Err(CoreError::Cycle)));
        assert!(matches!(g.connect(c, "output", c, "input"), Err(CoreError::Cycle)));
        g.connect(a, "output", c, "input").unwrap();
        assert_eq!(g.connections.len(), 2);
        assert_eq!(g.topo_order().first(), Some(&a));
        assert_eq!(g.upstream(c), vec![a]);
        g.remove_nodes(&[a]);
        assert_eq!(g.connections.len(), 0);
    }
}
