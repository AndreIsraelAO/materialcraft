//! The graph editor: a pannable, zoomable canvas of node cards (colored header, live thumbnail,
//! size label), bezier wires between typed ports, frames, comments and navigation pins.
//!
//! Gestures: left-drag a node to move it (the selection moves together), drag from a port to wire
//! it (dropping on empty canvas opens the node search, wired), drag an input to unplug and re-route
//! it, drag the empty canvas to box-select (Ctrl adds), middle-drag or Alt+drag to pan, wheel to
//! zoom, double-click a node to show it in the 2D view (a graph instance opens its graph), Space, Tab or
//! double-click the canvas for the node search, right-click for menus, drop library items to add
//! them. Every edit is an engine command; the gestures only draw previews until they commit.

use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};
use egui_phosphor::regular as icon;
use materialcraft_engine::core::{Node, NodeId, Value};
use materialcraft_engine::nodes::PortKind;
use serde_json::json;

use crate::MaterialCraftApp;
use crate::i18n::tr;
use crate::theme::Tokens;

/// Node card metrics in graph units.
pub const NODE_W: f32 = 100.0;
const HEADER_H: f32 = 18.0;
const FOOTER_H: f32 = 14.0;
const PORT_GAP: f32 = 18.0;
const PORT_R: f32 = 4.5;
const DOT_R: f32 = 7.0;

/// A library item being dragged onto the canvas.
#[derive(Clone, Debug)]
pub struct LibraryDrag(pub String);

#[derive(Clone, Debug)]
enum Drag {
    Nodes { delta: Vec2 },
    Box { start: Pos2 },
    Wire { from: (NodeId, String), detached: Option<(NodeId, String)> },
    Pan,
    Frame { id: u64, delta: Vec2, resize: bool, nodes: Vec<NodeId> },
    Comment { id: u64, delta: Vec2 },
}

/// The node search popup (Space or Tab).
#[derive(Clone, Debug)]
struct Search {
    at: Pos2,
    text: String,
    sel: usize,
    connect: Option<(NodeId, String)>,
    focus: bool,
}

/// One port as drawn this frame.
#[derive(Clone, Debug)]
pub struct PortHit {
    pub node: NodeId,
    pub output: bool,
    pub port: String,
    pub label: String,
    pub pos: Pos2,
    pub kind: PortKind,
}

pub struct GraphView {
    pub(crate) keyboard_was_focused: bool,
    drag: Option<Drag>,
    search: Option<Search>,
    /// Comment being edited: id and text.
    editing: Option<(u64, String)>,
    /// Node rectangles on screen last frame (for `ui.inspect` and scripted input).
    pub rects: Vec<(NodeId, Rect)>,
    pub ports: Vec<PortHit>,
    /// The canvas rectangle last frame.
    pub view: Rect,
    /// Graph point of the last right-click (context menu actions add things there).
    menu_at: Pos2,
    /// The graph the camera last framed.
    framed_graph: Option<usize>,
}

impl Default for GraphView {
    fn default() -> Self {
        GraphView {
            keyboard_was_focused: false,
            drag: None,
            search: None,
            editing: None,
            rects: Vec::new(),
            ports: Vec::new(),
            view: Rect::NOTHING,
            menu_at: Pos2::ZERO,
            framed_graph: None,
        }
    }
}

struct Cam {
    origin: Pos2,
    pan: Vec2,
    zoom: f32,
}

impl Cam {
    fn to_screen(&self, p: Pos2) -> Pos2 {
        self.origin + (p.to_vec2() - self.pan) * self.zoom
    }
    fn to_graph(&self, s: Pos2) -> Pos2 {
        ((s - self.origin) / self.zoom + self.pan).to_pos2()
    }
}

/// Header strip color of a node by its library category.
pub fn header_color(def: &str) -> Color32 {
    let cat = materialcraft_engine::nodes::find(def).map_or("", |d| d.category);
    match def {
        "output" => Color32::from_rgb(0x2c, 0x8f, 0x5e),
        "input_color" | "input_grayscale" => Color32::from_rgb(0x6d, 0x52, 0xb5),
        "graph_instance" => Color32::from_rgb(0x1e, 0x8a, 0xa0),
        _ if cat.starts_with("Atomic") => Color32::from_rgb(0x5c, 0x62, 0x6e),
        _ if cat.contains("Noises") => Color32::from_rgb(0xa8, 0x3c, 0x34),
        _ if cat.contains("Patterns") => Color32::from_rgb(0xb3, 0x5a, 0x2c),
        _ if cat.starts_with("Material") => Color32::from_rgb(0x3a, 0x66, 0xb0),
        _ if cat.starts_with("Filters") => Color32::from_rgb(0x3b, 0x7f, 0x4c),
        _ => Color32::from_rgb(0x5c, 0x62, 0x6e),
    }
}

pub fn port_color(kind: PortKind) -> Color32 {
    match kind {
        PortKind::Gray => Color32::from_rgb(0xc8, 0xc8, 0xc8),
        PortKind::Color => Color32::from_rgb(0xe8, 0xa9, 0x3c),
        PortKind::Any => Color32::from_rgb(0x8c, 0xb4, 0xe0),
    }
}

/// A node's display label.
pub fn node_label(n: &Node) -> String {
    if !n.label.is_empty() {
        return n.label.clone();
    }
    match n.def.as_str() {
        "graph_instance" => match n.params.get("graph") {
            Some(Value::Text(g)) => g.clone(),
            _ => tr("Graph instance").into(),
        },
        "output" | "input_color" | "input_grayscale" => materialcraft_engine::eval::identifier(n),
        d => materialcraft_engine::nodes::find(d).map_or(d, |d| d.label).to_string(),
    }
}

/// Graph-space rectangle of a node card.
pub fn node_rect(n: &Node, n_in: usize, n_out: usize, thumbs: bool) -> Rect {
    if n.def == "dot" {
        return Rect::from_center_size(pos2(n.pos[0], n.pos[1]), vec2(DOT_R * 2.0, DOT_R * 2.0));
    }
    let ports = n_in.max(n_out) as f32 * PORT_GAP + 8.0;
    let body = if thumbs { NODE_W.max(ports) } else { ports.max(26.0) };
    Rect::from_min_size(pos2(n.pos[0], n.pos[1]), vec2(NODE_W, HEADER_H + body + FOOTER_H))
}

/// Graph-space port positions along a card side.
fn port_pos(r: Rect, i: usize, n: usize, output: bool, dot: bool) -> Pos2 {
    if dot {
        return pos2(if output { r.right() } else { r.left() }, r.center().y);
    }
    let top = r.top() + HEADER_H;
    let body = r.height() - HEADER_H - FOOTER_H;
    let start = top + (body - n as f32 * PORT_GAP) * 0.5 + PORT_GAP * 0.5;
    pos2(if output { r.right() } else { r.left() }, start + i as f32 * PORT_GAP)
}

fn bezier(p: &egui::Painter, a: Pos2, b: Pos2, stroke: Stroke) {
    let dx = ((b.x - a.x).abs() * 0.5).max(30.0);
    let shape = egui::epaint::CubicBezierShape::from_points_stroke([a, a + vec2(dx, 0.0), b - vec2(dx, 0.0), b], false, Color32::TRANSPARENT, stroke);
    p.add(shape);
}

/// The graph panel: toolbar row and canvas.
pub fn show(app: &mut MaterialCraftApp, ui: &mut Ui) {
    toolbar(app, ui);
    canvas(app, ui);
}

fn toolbar(app: &mut MaterialCraftApp, ui: &mut Ui) {
    let tk = Tokens::get(ui.ctx());
    egui::Frame::NONE.fill(tk.panel).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let b = |ui: &mut Ui, ic: &str, tip: &str| crate::widgets::icon_button(ui, ic, tip, false, 22.0).clicked();
            if b(ui, icon::CORNERS_OUT, tr("Frame all (F)")) {
                app.ui.frame_all = true;
            }
            for (ic, mode, tip) in [
                (icon::ALIGN_LEFT, "left", "Align left"),
                (icon::ALIGN_TOP, "top", "Align top"),
                (icon::ALIGN_CENTER_HORIZONTAL, "hcenter", "Align centers horizontally"),
                (icon::ALIGN_CENTER_VERTICAL, "vcenter", "Align centers vertically"),
                (icon::COLUMNS, "hdistribute", "Distribute horizontally"),
                (icon::ROWS, "vdistribute", "Distribute vertically"),
            ] {
                if b(ui, ic, tr(tip)) {
                    app.run("graph.align", json!({"mode": mode}));
                }
            }
            ui.separator();
            if b(ui, icon::SELECTION, tr("Add frame around the selection")) {
                app.run("graph.frame.add", json!({}));
            }
            if b(ui, icon::NOTE, tr("Add comment")) {
                app.run("graph.comment.add", json!({}));
            }
            if b(ui, icon::MAP_PIN, tr("Add navigation pin")) {
                let c = view_center(app);
                app.run("graph.pin.add", json!({"at": [c.x, c.y]}));
            }
            ui.separator();
            // Quick insert of the atomic nodes.
            for (ic, def) in [
                (icon::IMAGE, "bitmap"),
                (icon::PALETTE, "uniform"),
                (icon::STACK, "blend"),
                (icon::DROP, "blur"),
                (icon::SHUFFLE, "channels_shuffle"),
                (icon::CHART_LINE, "curve"),
                (icon::WIND, "directional_blur"),
                (icon::ARROWS_OUT_LINE_HORIZONTAL, "directional_warp"),
                (icon::RULER, "distance"),
                (icon::MOUNTAINS, "emboss"),
                (icon::GRADIENT, "gradient_map"),
                (icon::CIRCLE_HALF, "grayscale_conversion"),
                (icon::SWATCHES, "hsl"),
                (icon::SLIDERS_HORIZONTAL, "levels"),
                (icon::CUBE, "normal"),
                (icon::CODE, "pixel_processor"),
                (icon::DIAMOND, "sharpen"),
                (icon::ARROWS_CLOCKWISE, "transformation"),
                (icon::WAVES, "warp"),
                (icon::SIGN_IN, "input_grayscale"),
                (icon::SIGN_OUT, "output"),
            ] {
                let label = materialcraft_engine::nodes::find(def).map_or(def, |d| d.label);
                if b(ui, ic, label) {
                    add_node_near_selection(app, def);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let thumbs = app.ui.prefs.thumbnails;
                if crate::widgets::icon_button(ui, icon::IMAGE, tr("Show node thumbnails"), thumbs, 22.0).clicked() {
                    app.ui.prefs.thumbnails = !thumbs;
                }
                parent_size(app, ui);
            });
        });
    });
}

/// "Parent size: [1024 ▾] × [1024 ▾]".
fn parent_size(app: &mut MaterialCraftApp, ui: &mut Ui) {
    let Ok(g) = app.session.graph() else { return };
    let [w, h] = g.size;
    let mut nw = w;
    let mut nh = h;
    let combo = |ui: &mut Ui, id: &str, v: &mut u32| {
        let r = egui::ComboBox::from_id_salt(id).width(64.0).selected_text(format!("{}", 1u32 << *v)).show_ui(ui, |ui| {
            for l in 4..=13u32 {
                ui.selectable_value(v, l, format!("{}", 1u32 << l));
            }
        });
        crate::widgets::name(&r.response, egui::Role::ComboBox, if id == "parent-w" { "Parent width" } else { "Parent height" });
    };
    combo(ui, "parent-h", &mut nh);
    ui.label("x");
    combo(ui, "parent-w", &mut nw);
    ui.label(egui::RichText::new(tr("Parent size:")).color(Tokens::get(ui.ctx()).text_dim));
    if (nw, nh) != (w, h) {
        app.run("graph.size", json!({"log2": [nw, nh]}));
    }
}

fn view_center(app: &MaterialCraftApp) -> Pos2 {
    let c = &app.ui.graph_camera;
    let v = app.graph.view;
    pos2(c.pan[0] + v.width() * 0.5 / c.zoom, c.pan[1] + v.height() * 0.5 / c.zoom)
}

/// Add `def` right of the last selected node (wired from it when it fits), else mid-view.
pub fn add_node_near_selection(app: &mut MaterialCraftApp, def: &str) {
    let sel = app.session.selection.last().copied();
    let has_inputs = materialcraft_engine::nodes::find(def).is_some_and(|d| !d.inputs.is_empty());
    let params = match sel {
        Some(id) if has_inputs && !app.session.output_ports(id).is_empty() => json!({"def": def, "connectFrom": {"node": id.0}}),
        _ => {
            let c = view_center(app);
            json!({"def": def, "at": [c.x - NODE_W * 0.5, c.y - 60.0]})
        }
    };
    app.run("graph.node.add", params);
}

/// Frame the graph (or the selection) in the view.
fn frame(app: &mut MaterialCraftApp, view: Rect, only_selection: bool) {
    let thumbs = app.ui.prefs.thumbnails;
    let Ok(g) = app.session.graph() else { return };
    let mut r = Rect::NOTHING;
    for n in &g.nodes {
        if only_selection && !app.session.selection.contains(&n.id) {
            continue;
        }
        let (i, o) = materialcraft_engine::eval::ports(&app.session.pkg, n);
        r = r.union(node_rect(n, i.len(), o.len(), thumbs));
    }
    for f in &g.frames {
        if !only_selection {
            r = r.union(Rect::from_min_size(pos2(f.rect[0], f.rect[1]), vec2(f.rect[2], f.rect[3])));
        }
    }
    if !r.is_finite() || r.width() <= 0.0 {
        app.ui.graph_camera = crate::state::GraphCamera::default();
        return;
    }
    let r = r.expand(40.0);
    let zoom = (view.width() / r.width()).min(view.height() / r.height()).clamp(0.1, 1.5);
    let pan = r.center().to_vec2() - vec2(view.width(), view.height()) * 0.5 / zoom;
    app.ui.graph_camera = crate::state::GraphCamera { pan: [pan.x, pan.y], zoom };
}

fn canvas(app: &mut MaterialCraftApp, ui: &mut Ui) {
    let tk = Tokens::get(ui.ctx());
    let ctx = ui.ctx().clone();
    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    crate::widgets::name(&resp, egui::Role::Canvas, "Node graph");
    app.graph.view = rect;
    if app.ui.frame_all || app.graph.framed_graph != Some(app.session.graph) {
        app.ui.frame_all = false;
        app.graph.framed_graph = Some(app.session.graph);
        frame(app, rect, false);
    }
    let painter = ui.painter_at(rect);
    // Background and grid.
    painter.rect_filled(rect, 0.0, Color32::from_rgb(0x2b, 0x2d, 0x31));
    let cam = Cam { origin: rect.min, pan: Vec2::from(app.ui.graph_camera.pan), zoom: app.ui.graph_camera.zoom.clamp(0.08, 4.0) };
    grid(&painter, &cam, rect);

    let pointer = ctx.input(|i| i.pointer.hover_pos());
    let hovered_canvas = pointer.is_some_and(|p| rect.contains(p)) && resp.hovered();

    // Zoom around the pointer.
    if hovered_canvas || ui.rect_contains_pointer(rect) {
        let scroll = ctx.input(|i| i.smooth_scroll_delta.y + i.zoom_delta().ln() * 200.0);
        if scroll.abs() > 0.0
            && let Some(p) = pointer
            && rect.contains(p)
        {
            let before = cam.to_graph(p);
            let zoom = (cam.zoom * (scroll * 0.0025).exp()).clamp(0.08, 4.0);
            let pan = before.to_vec2() - (p - rect.min) / zoom;
            app.ui.graph_camera = crate::state::GraphCamera { pan: [pan.x, pan.y], zoom };
        }
    }
    let cam = Cam { origin: rect.min, pan: Vec2::from(app.ui.graph_camera.pan), zoom: app.ui.graph_camera.zoom.clamp(0.08, 4.0) };

    let thumbs_on = app.ui.prefs.thumbnails;
    let Ok(g) = app.session.graph() else { return };
    let g = g.clone();
    let selection = app.session.selection.clone();
    let node_delta = match &app.graph.drag {
        Some(Drag::Nodes { delta }) => *delta,
        _ => Vec2::ZERO,
    };
    let frame_move = match &app.graph.drag {
        Some(Drag::Frame { id, delta, resize: false, nodes }) => Some((*id, *delta, nodes.clone())),
        _ => None,
    };

    // Layout every node and its ports.
    struct Laid {
        id: NodeId,
        rect: Rect,
        ins: Vec<(String, String, PortKind, Pos2)>,
        outs: Vec<(String, String, PortKind, Pos2)>,
    }
    let mut laid: Vec<Laid> = Vec::with_capacity(g.nodes.len());
    for n in &g.nodes {
        let (ins, outs) = materialcraft_engine::eval::ports(&app.session.pkg, n);
        let mut r = node_rect(n, ins.len(), outs.len(), thumbs_on);
        if selection.contains(&n.id) {
            r = r.translate(node_delta);
        }
        if let Some((_, d, nodes)) = &frame_move
            && nodes.contains(&n.id)
        {
            r = r.translate(*d);
        }
        let dot = n.def == "dot";
        let pi = |i: usize, output: bool, total: usize| port_pos(r, i, total, output, dot);
        laid.push(Laid {
            id: n.id,
            rect: r,
            ins: ins.iter().enumerate().map(|(i, p)| (p.id.clone(), p.label.clone(), p.kind, pi(i, false, ins.len()))).collect(),
            outs: outs.iter().enumerate().map(|(i, p)| (p.id.clone(), p.label.clone(), p.kind, pi(i, true, outs.len()))).collect(),
        });
    }
    app.graph.rects = laid.iter().map(|l| (l.id, Rect::from_min_max(cam.to_screen(l.rect.min), cam.to_screen(l.rect.max)))).collect();
    app.graph.ports = laid
        .iter()
        .flat_map(|l| {
            let ins = l.ins.iter().map(move |(id, label, kind, p)| PortHit {
                node: l.id,
                output: false,
                port: id.clone(),
                label: label.clone(),
                pos: *p,
                kind: *kind,
            });
            let outs = l.outs.iter().map(move |(id, label, kind, p)| PortHit {
                node: l.id,
                output: true,
                port: id.clone(),
                label: label.clone(),
                pos: *p,
                kind: *kind,
            });
            ins.chain(outs)
        })
        .map(|mut h| {
            h.pos = cam.to_screen(h.pos);
            h
        })
        .collect();

    // Frames (behind everything).
    let mut frame_hits: Vec<(u64, Rect, Rect)> = Vec::new();
    for f in &g.frames {
        let mut r = Rect::from_min_size(pos2(f.rect[0], f.rect[1]), vec2(f.rect[2], f.rect[3]));
        match &app.graph.drag {
            Some(Drag::Frame { id, delta, resize: false, .. }) if *id == f.id => r = r.translate(*delta),
            Some(Drag::Frame { id, delta, resize: true, .. }) if *id == f.id => r.max += *delta,
            _ => {}
        }
        let sr = Rect::from_min_max(cam.to_screen(r.min), cam.to_screen(r.max));
        let c = f.color;
        let fill = Color32::from_rgba_unmultiplied((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8, (c[3].clamp(0.05, 0.9) * 255.0) as u8);
        painter.rect_filled(sr, 4.0, fill);
        let title = Rect::from_min_size(sr.min, vec2(sr.width(), (20.0 * cam.zoom).max(12.0)));
        painter.rect_filled(title, CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, fill.gamma_multiply(1.6));
        painter.text(
            title.left_center() + vec2(6.0, 0.0),
            Align2::LEFT_CENTER,
            &f.title,
            FontId::proportional((12.0 * cam.zoom).clamp(8.0, 18.0)),
            tk.text_strong,
        );
        let grip = Rect::from_min_size(sr.max - vec2(12.0, 12.0), vec2(12.0, 12.0));
        painter.line_segment([grip.left_bottom(), grip.right_top()], Stroke::new(1.0, tk.text_dim));
        frame_hits.push((f.id, title, grip));
    }

    // Wires.
    let cooked = &app.session.cooked;
    let detached = match &app.graph.drag {
        Some(Drag::Wire { detached: Some(d), .. }) => Some(d.clone()),
        _ => None,
    };
    for c in &g.connections {
        if detached.as_ref().is_some_and(|(n, p)| *n == c.to && *p == c.to_port) {
            continue;
        }
        let a = laid.iter().find(|l| l.id == c.from).and_then(|l| l.outs.iter().find(|p| p.0 == c.from_port));
        let b = laid.iter().find(|l| l.id == c.to).and_then(|l| l.ins.iter().find(|p| p.0 == c.to_port));
        if let (Some(a), Some(b)) = (a, b) {
            let hot = selection.contains(&c.from) || selection.contains(&c.to);
            let col = if hot { tk.accent } else { Color32::from_gray(0xb4) };
            bezier(&painter, cam.to_screen(a.3), cam.to_screen(b.3), Stroke::new((2.0 * cam.zoom).clamp(1.0, 3.0), col));
        }
    }

    // Nodes.
    let font = FontId::proportional((10.5 * cam.zoom).clamp(6.0, 16.0));
    let small = FontId::proportional((9.0 * cam.zoom).clamp(5.5, 13.0));
    let mut node_resps: Vec<(NodeId, egui::Response)> = Vec::new();
    for l in &laid {
        let Ok(n) = g.node(l.id) else { continue };
        let sr = Rect::from_min_max(cam.to_screen(l.rect.min), cam.to_screen(l.rect.max));
        if !sr.intersects(rect) {
            continue;
        }
        let selected = selection.contains(&n.id);
        let result = cooked.nodes.get(&n.id);
        let error = result.and_then(|r| r.error.clone());
        let resp = ui.interact(sr, ui.scope_id().with(("node", n.id.0)), Sense::click_and_drag());
        crate::widgets::name(&resp, egui::Role::Button, &node_label(n));
        if n.def == "dot" {
            painter.circle(
                sr.center(),
                DOT_R * cam.zoom,
                Color32::from_gray(0x50),
                Stroke::new(1.5, if selected { tk.accent } else { Color32::from_gray(0xb4) }),
            );
        } else {
            let header = Rect::from_min_size(sr.min, vec2(sr.width(), HEADER_H * cam.zoom));
            let r = (5.0 * cam.zoom).clamp(1.0, 6.0) as u8;
            painter.rect_filled(sr, r, Color32::from_rgb(0x1e, 0x20, 0x24));
            painter.rect_filled(header, CornerRadius { nw: r, ne: r, sw: 0, se: 0 }, header_color(&n.def));
            let title = crate::widgets::elided(&painter, node_label(n), font.size, Color32::WHITE, header.width() - 8.0);
            painter.galley(header.left_center() + vec2(4.0, -title.size().y * 0.5), title, Color32::WHITE);
            // Thumbnail or compact body.
            let body = Rect::from_min_max(pos2(sr.left(), header.bottom()), pos2(sr.right(), sr.bottom() - FOOTER_H * cam.zoom));
            if let Some(r0) = result
                && let Some(img) = r0.outputs.first()
                && thumbs_on
            {
                let side = body.width().min(body.height()) - 6.0 * cam.zoom;
                let tr_ = Rect::from_center_size(body.center(), vec2(side, side));
                let tex = app.thumbs.node(&ctx, r0.key, 0, img);
                painter.image(tex, tr_, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), if n.disabled { Color32::from_gray(90) } else { Color32::WHITE });
            } else if result.is_none() {
                painter.text(body.center(), Align2::CENTER_CENTER, icon::HOURGLASS_MEDIUM, small.clone(), tk.text_dim);
            }
            // Footer: size and format.
            let footer = Rect::from_min_max(pos2(sr.left(), body.bottom()), sr.max);
            if let Some(img) = result.and_then(|r| r.outputs.first()) {
                let bits = if n.base.format == materialcraft_engine::core::OutputFormat::Inherit { g.format.bits() } else { n.base.format.bits() };
                let txt = format!("{}x{} - {}{}", img.w, img.h, if img.is_gray() { "L" } else { "C" }, bits);
                painter.text(footer.center(), Align2::CENTER_CENTER, txt, small.clone(), tk.text_dim);
            }
            if n.def == "output" {
                painter.text(header.right_center() - vec2(4.0, 0.0), Align2::RIGHT_CENTER, icon::SIGN_OUT, small.clone(), Color32::WHITE);
            }
            let outline = if error.is_some() {
                Stroke::new(2.0, Color32::from_rgb(0xff, 0x30, 0xd0))
            } else if selected {
                Stroke::new(2.0, tk.accent)
            } else if resp.hovered() {
                Stroke::new(1.0, Color32::from_gray(0x90))
            } else {
                Stroke::new(1.0, Color32::from_gray(0x10))
            };
            painter.rect_stroke(sr, r, outline, StrokeKind::Outside);
            if n.disabled {
                painter.line_segment([sr.left_top(), sr.right_bottom()], Stroke::new(1.5, tk.danger));
            }
            if let Some(e) = &error {
                resp.clone().on_hover_text(e);
            }
        }
        node_resps.push((n.id, resp));
    }

    // Comments.
    let mut comment_hits: Vec<(u64, Rect)> = Vec::new();
    for c in &g.comments {
        let mut p = pos2(c.pos[0], c.pos[1]);
        if let Some(Drag::Comment { id, delta }) = &app.graph.drag
            && *id == c.id
        {
            p += *delta;
        }
        let r = Rect::from_min_size(cam.to_screen(p), vec2(c.size[0], c.size[1]) * cam.zoom);
        painter.rect_filled(r, 4.0, Color32::from_rgba_unmultiplied(0x5a, 0x54, 0x2e, 230));
        painter.rect_stroke(r, 4.0, Stroke::new(1.0, Color32::from_rgb(0x9a, 0x8c, 0x40)), StrokeKind::Inside);
        if app.graph.editing.as_ref().is_some_and(|(id, _)| *id == c.id) {
            comment_hits.push((c.id, r));
            continue;
        }
        let galley =
            painter.layout(c.text.clone(), FontId::proportional((11.0 * cam.zoom).clamp(6.0, 18.0)), Color32::from_rgb(0xf3, 0xec, 0xc8), r.width() - 10.0);
        painter.galley(r.min + vec2(5.0, 4.0), galley, Color32::WHITE);
        comment_hits.push((c.id, r));
    }

    // Pins.
    for pin in &g.pins {
        let p = cam.to_screen(pos2(pin.pos[0], pin.pos[1]));
        painter.text(p, Align2::CENTER_BOTTOM, icon::MAP_PIN, FontId::proportional(18.0), tk.accent);
        painter.text(p + vec2(10.0, -10.0), Align2::LEFT_CENTER, &pin.name, FontId::proportional(11.0), tk.text);
    }

    // Ports (drawn last, interactable over the node cards).
    let port_hover = pointer.and_then(|p| nearest_port(&app.graph.ports, p, 9.0)).cloned();
    for h in &app.graph.ports {
        if !rect.contains(h.pos) {
            continue;
        }
        let hot = port_hover.as_ref().is_some_and(|x| x.node == h.node && x.port == h.port && x.output == h.output);
        let r = (PORT_R * cam.zoom).clamp(2.5, 7.0) * if hot { 1.4 } else { 1.0 };
        painter.circle(h.pos, r, port_color(h.kind), Stroke::new(1.0, Color32::from_gray(0x18)));
    }
    if let Some(h) = &port_hover {
        let anchor = if h.output { Align2::LEFT_CENTER } else { Align2::RIGHT_CENTER };
        let off = if h.output { vec2(10.0, 0.0) } else { vec2(-10.0, 0.0) };
        let g_ = painter.layout_no_wrap(h.label.clone(), FontId::proportional(11.0), tk.text_strong);
        let bg = anchor.anchor_size(h.pos + off, g_.size()).expand(3.0);
        painter.rect_filled(bg, 3.0, Color32::from_black_alpha(200));
        painter.galley(bg.min + vec2(3.0, 3.0), g_, tk.text_strong);
    }

    handle_input(app, ui, &resp, rect, &cam, &node_resps, &frame_hits, &comment_hits, port_hover, &laid.iter().map(|l| (l.id, l.rect)).collect::<Vec<_>>());

    // Drag previews.
    match &app.graph.drag {
        Some(Drag::Box { start }) => {
            if let Some(p) = pointer {
                let r = Rect::from_two_pos(*start, p);
                painter.rect(r, 0.0, tk.accent.gamma_multiply(0.12), Stroke::new(1.0, tk.accent), StrokeKind::Inside);
            }
        }
        Some(Drag::Wire { from, .. }) => {
            if let (Some(p), Some(src)) = (pointer, app.graph.ports.iter().find(|h| h.output && h.node == from.0 && h.port == from.1)) {
                bezier(&painter, src.pos, p, Stroke::new(2.0, tk.accent));
            }
        }
        _ => {}
    }

    search_popup(app, ui, &cam);
    comment_editor(app, ui);
    let (failed, missing) = cooked_error_count(app);
    let mut at = rect.right_bottom() - vec2(8.0, 8.0);
    if failed > 0 {
        let r = painter.text(at, Align2::RIGHT_BOTTOM, format!("{} {failed} node(s) failed", icon::WARNING), FontId::proportional(11.0), tk.danger);
        at.y = r.top() - 2.0;
    }
    if missing > 0 {
        // Placeholders for nodes an SBS file uses that MaterialCraft doesn't have: a soft warning.
        painter.text(at, Align2::RIGHT_BOTTOM, format!("{} {missing} missing node(s), see the Console", icon::WARNING), FontId::proportional(11.0), tk.warning);
    }
}

/// (Nodes whose cook failed, placeholders for missing SBS nodes) in the last cook.
fn cooked_error_count(app: &MaterialCraftApp) -> (usize, usize) {
    let missing = |id: &NodeId| app.session.graph().ok().and_then(|g| g.node(*id).ok()).is_some_and(|n| n.def == materialcraft_engine::core::MISSING_NODE);
    let errors: Vec<&NodeId> = app.session.cooked.nodes.iter().filter(|(_, r)| r.error.is_some()).map(|(id, _)| id).collect();
    let m = errors.iter().filter(|id| missing(id)).count();
    (errors.len() - m, m)
}

fn grid(p: &egui::Painter, cam: &Cam, rect: Rect) {
    for (step, alpha) in [(32.0f32, 10u8), (256.0, 22)] {
        let s = step * cam.zoom;
        if s < 8.0 {
            continue;
        }
        let col = Color32::from_white_alpha(alpha);
        let x0 = cam.pan.x - (cam.pan.x.rem_euclid(step));
        let mut x = x0;
        while cam.to_screen(pos2(x, 0.0)).x < rect.right() {
            let sx = cam.to_screen(pos2(x, 0.0)).x;
            p.line_segment([pos2(sx, rect.top()), pos2(sx, rect.bottom())], Stroke::new(1.0, col));
            x += step;
        }
        let y0 = cam.pan.y - (cam.pan.y.rem_euclid(step));
        let mut y = y0;
        while cam.to_screen(pos2(0.0, y)).y < rect.bottom() {
            let sy = cam.to_screen(pos2(0.0, y)).y;
            p.line_segment([pos2(rect.left(), sy), pos2(rect.right(), sy)], Stroke::new(1.0, col));
            y += step;
        }
    }
}

fn nearest_port(ports: &[PortHit], p: Pos2, radius: f32) -> Option<&PortHit> {
    ports.iter().filter(|h| h.pos.distance(p) <= radius).min_by(|a, b| a.pos.distance(p).total_cmp(&b.pos.distance(p)))
}

#[allow(clippy::too_many_arguments)]
fn handle_input(
    app: &mut MaterialCraftApp,
    ui: &mut Ui,
    canvas: &egui::Response,
    rect: Rect,
    cam: &Cam,
    nodes: &[(NodeId, egui::Response)],
    frames: &[(u64, Rect, Rect)],
    comments: &[(u64, Rect)],
    port_hover: Option<PortHit>,
    graph_rects: &[(NodeId, Rect)],
) {
    let ctx = ui.ctx().clone();
    let (pointer, mods, primary_pressed, primary_down, middle_down, released) = ctx.input(|i| {
        (i.pointer.interact_pos(), i.modifiers, i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.middle_down(), i.pointer.any_released())
    });
    let zoom = cam.zoom;
    let motion = ctx.input(|i| i.pointer.delta()) / zoom;

    // Starting gestures (only on a press inside the canvas, with no gesture running).
    if app.graph.drag.is_none()
        && let Some(p) = pointer
        && rect.contains(p)
        && app.graph.search.is_none()
    {
        if middle_down || (primary_pressed && mods.alt) {
            app.graph.drag = Some(Drag::Pan);
        } else if primary_pressed {
            if let Some(h) = &port_hover {
                if h.output {
                    app.graph.drag = Some(Drag::Wire { from: (h.node, h.port.clone()), detached: None });
                } else if let Some(c) = app.session.graph().ok().and_then(|g| g.input_of(h.node, &h.port).cloned()) {
                    app.graph.drag = Some(Drag::Wire { from: (c.from, c.from_port.clone()), detached: Some((h.node, h.port.clone())) });
                }
            } else if let Some((id, title, grip)) = frames.iter().find(|(_, t, g)| t.contains(p) || g.contains(p)) {
                let resize = grip.contains(p) && !title.contains(p);
                // Moving a frame carries the nodes inside it.
                let inside: Vec<NodeId> = app
                    .session
                    .graph()
                    .ok()
                    .and_then(|g| g.frames.iter().find(|f| f.id == *id))
                    .map(|f| {
                        let fr = Rect::from_min_size(pos2(f.rect[0], f.rect[1]), vec2(f.rect[2], f.rect[3]));
                        graph_rects.iter().filter(|(_, r)| fr.contains_rect(*r)).map(|(n, _)| *n).collect()
                    })
                    .unwrap_or_default();
                app.graph.drag = Some(Drag::Frame { id: *id, delta: Vec2::ZERO, resize, nodes: if resize { Vec::new() } else { inside } });
            } else if let Some((id, _)) = comments.iter().rev().find(|(_, r)| r.contains(p)) {
                app.graph.drag = Some(Drag::Comment { id: *id, delta: Vec2::ZERO });
            } else if let Some((id, _)) = nodes.iter().rev().find(|(_, r)| r.rect.contains(p)) {
                if !app.session.selection.contains(id) {
                    if mods.ctrl || mods.shift {
                        app.session.selection.push(*id);
                    } else {
                        app.session.selection = vec![*id];
                    }
                } else if mods.ctrl {
                    app.session.selection.retain(|s| s != id);
                }
                app.graph.drag = Some(Drag::Nodes { delta: Vec2::ZERO });
            } else {
                app.graph.drag = Some(Drag::Box { start: p });
            }
        }
    }

    // Continuing gestures.
    match &mut app.graph.drag {
        Some(Drag::Pan) => {
            let d = ctx.input(|i| i.pointer.delta()) / zoom;
            app.ui.graph_camera.pan = [app.ui.graph_camera.pan[0] - d.x, app.ui.graph_camera.pan[1] - d.y];
        }
        Some(Drag::Nodes { delta }) | Some(Drag::Frame { delta, .. }) | Some(Drag::Comment { delta, .. }) => *delta += motion,
        _ => {}
    }

    // Ending gestures.
    let ended = released && !primary_down && !middle_down;
    if ended && let Some(drag) = app.graph.drag.take() {
        match drag {
            Drag::Pan => {}
            Drag::Nodes { delta } => {
                if delta.length() > 0.5 {
                    app.run("graph.node.move", json!({"by": [delta.x, delta.y]}));
                }
            }
            Drag::Box { start } => {
                if let Some(p) = pointer {
                    let r = Rect::from_two_pos(start, p);
                    if r.width() > 3.0 || r.height() > 3.0 {
                        let hit: Vec<NodeId> = app.graph.rects.iter().filter(|(_, nr)| r.intersects(*nr)).map(|(id, _)| *id).collect();
                        if mods.ctrl || mods.shift {
                            for id in hit {
                                if !app.session.selection.contains(&id) {
                                    app.session.selection.push(id);
                                }
                            }
                        } else {
                            app.session.selection = hit;
                        }
                    } else if !mods.ctrl && !mods.shift {
                        app.session.selection.clear();
                    }
                }
            }
            Drag::Wire { from, detached } => {
                let target = pointer.and_then(|p| nearest_port(&app.graph.ports, p, 12.0)).filter(|h| !h.output).cloned();
                match target {
                    Some(t) => {
                        if let Some((n, p)) = &detached
                            && (*n != t.node || *p != t.port)
                        {
                            app.run("graph.disconnect", json!({"to": n.0, "toPort": p}));
                        }
                        app.run("graph.connect", json!({"from": from.0.0, "fromPort": from.1, "to": t.node.0, "toPort": t.port}));
                    }
                    None => {
                        if let Some((n, p)) = &detached {
                            app.run("graph.disconnect", json!({"to": n.0, "toPort": p}));
                        } else if let Some(p) = pointer
                            && rect.contains(p)
                        {
                            open_search(app, cam.to_graph(p), Some(from));
                        }
                    }
                }
            }
            Drag::Frame { id, delta, resize, nodes } => {
                if delta.length() > 0.5
                    && let Some(f) = app.session.graph().ok().and_then(|g| g.frames.iter().find(|f| f.id == id)).cloned()
                {
                    let r = if resize {
                        [f.rect[0], f.rect[1], f.rect[2] + delta.x, f.rect[3] + delta.y]
                    } else {
                        [f.rect[0] + delta.x, f.rect[1] + delta.y, f.rect[2], f.rect[3]]
                    };
                    app.run("graph.frame.edit", json!({"id": id, "rect": r}));
                    if !nodes.is_empty() {
                        app.run("graph.node.move", json!({"ids": nodes.iter().map(|n| n.0).collect::<Vec<_>>(), "by": [delta.x, delta.y]}));
                    }
                }
            }
            Drag::Comment { id, delta } => {
                if delta.length() > 0.5
                    && let Some(c) = app.session.graph().ok().and_then(|g| g.comments.iter().find(|c| c.id == id)).cloned()
                {
                    app.run("graph.comment.edit", json!({"id": id, "at": [c.pos[0] + delta.x, c.pos[1] + delta.y]}));
                }
            }
        }
    }

    // Clicks.
    for (id, r) in nodes {
        if r.double_clicked() {
            let def = app.session.graph().ok().and_then(|g| g.node(*id).ok()).map(|n| (n.def.clone(), n.params.get("graph").cloned()));
            match def {
                Some((d, Some(Value::Text(name)))) if d == "graph_instance" => {
                    app.run("graph.open", json!({"graph": name}));
                }
                Some((d, _)) if d == "pixel_processor" || d == "value_processor" => {
                    app.run("view.2d", json!({"node": id.0, "port": 0}));
                    app.fn_editor = Some(crate::function_editor::FnEditor::new(*id, "function"));
                }
                _ => {
                    app.run("view.2d", json!({"node": id.0, "port": 0}));
                }
            }
        }
        r.context_menu(|ui| node_menu(app, ui, *id));
    }
    if let Some(h) = &port_hover
        && h.output
        && ctx.input(|i| i.pointer.primary_clicked())
    {
        let idx = app.session.output_ports(h.node).iter().position(|p| *p == h.port).unwrap_or(0);
        app.run("view.2d", json!({"node": h.node.0, "port": idx}));
    }
    for (id, r) in comments {
        if canvas.double_clicked() && pointer.is_some_and(|p| r.contains(p)) {
            let text = app.session.graph().ok().and_then(|g| g.comments.iter().find(|c| c.id == *id)).map(|c| c.text.clone()).unwrap_or_default();
            app.graph.editing = Some((*id, text));
        }
    }
    let on_node = pointer.is_some_and(|p| nodes.iter().any(|(_, r)| r.rect.contains(p)) || comments.iter().any(|(_, r)| r.contains(p)));
    if canvas.double_clicked()
        && !on_node
        && let Some(p) = pointer
    {
        open_search(app, cam.to_graph(p), None);
    }
    if canvas.secondary_clicked()
        && let Some(p) = pointer
    {
        app.graph.menu_at = cam.to_graph(p);
    }
    let at = app.graph.menu_at;
    canvas.context_menu(|ui| canvas_menu(app, ui, at));

    // Library drops.
    if let Some(payload) = canvas.dnd_release_payload::<LibraryDrag>()
        && let Some(p) = pointer
    {
        let gp = cam.to_graph(p);
        let at = [gp.x - NODE_W * 0.5, gp.y - 20.0];
        match payload.0.strip_prefix('@') {
            Some(graph) => app.run("graph.node.add", json!({"def": "graph_instance", "at": at, "params": {"graph": graph}})),
            None => app.run("graph.node.add", json!({"def": payload.0, "at": at})),
        };
    }

    // Keys over the canvas.
    let hovered = pointer.is_some_and(|p| rect.contains(p));
    if hovered && app.graph.search.is_none() {
        let space = !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::Space));
        let tab = !app.graph.keyboard_was_focused && !ctx.text_edit_focused() && ctx.input(|i| i.key_pressed(egui::Key::Tab));
        if (space || tab)
            && let Some(p) = pointer
        {
            open_search(app, cam.to_graph(p), None);
        }
        if !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::F) && i.modifiers.is_none()) {
            let sel = !app.session.selection.is_empty();
            frame(app, rect, sel);
        }
    }
}

fn open_search(app: &mut MaterialCraftApp, at: Pos2, connect: Option<(NodeId, String)>) {
    app.graph.search = Some(Search { at, text: String::new(), sel: 0, connect, focus: true });
}

/// Catalog entries matching `text` (label or id contains every word).
fn matches(text: &str) -> Vec<&'static materialcraft_engine::nodes::NodeDef> {
    let words: Vec<String> = text.split_whitespace().map(str::to_ascii_lowercase).collect();
    let mut v: Vec<_> = materialcraft_engine::nodes::catalog()
        .iter()
        .filter(|d| d.id != "graph_instance")
        .filter(|d| {
            let hay = format!("{} {} {}", d.label.to_ascii_lowercase(), d.id, d.category.to_ascii_lowercase());
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .collect();
    if let Some(w) = words.first() {
        v.sort_by_key(|d| !d.label.to_ascii_lowercase().starts_with(w.as_str()));
    }
    v
}

fn search_popup(app: &mut MaterialCraftApp, ui: &mut Ui, cam: &Cam) {
    let Some(s) = app.graph.search.as_mut() else { return };
    let tk = Tokens::get(ui.ctx());
    let screen = cam.to_screen(s.at);
    let mut close = false;
    let mut pick: Option<&'static str> = None;
    egui::Area::new(egui::Id::unique("node-search")).fixed_pos(screen).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_width(260.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(icon::MAGNIFYING_GLASS).color(tk.text_dim));
                let r = ui.add(egui::TextEdit::singleline(&mut s.text).hint_text(tr("Search nodes...")).desired_width(220.0));
                crate::widgets::name(&r, egui::Role::SearchInput, "Search nodes");
                if s.focus {
                    r.request_focus();
                    s.focus = false;
                }
                if r.changed() {
                    s.sel = 0;
                }
            });
            let list = matches(&s.text);
            let (up, down, enter, esc) = ui.input(|i| {
                (i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape))
            });
            if down {
                s.sel = (s.sel + 1).min(list.len().saturating_sub(1));
            }
            if up {
                s.sel = s.sel.saturating_sub(1);
            }
            if esc {
                close = true;
            }
            if enter && let Some(d) = list.get(s.sel) {
                pick = Some(d.id);
            }
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for (i, d) in list.iter().enumerate().take(60) {
                    let r = ui.add(
                        egui::Button::selectable(i == s.sel, format!("{}   ", d.label))
                            .right_text(egui::RichText::new(d.group()).size(10.0).color(tk.text_dim)),
                    );
                    if i == s.sel && (up || down) {
                        r.scroll_to_me(None);
                    }
                    if r.clicked() {
                        pick = Some(d.id);
                    }
                }
                if list.is_empty() {
                    ui.label(egui::RichText::new(tr("No matching node")).color(tk.text_dim));
                }
            });
        });
    });
    let clicked_out = ui.input(|i| i.pointer.any_pressed())
        && ui.ctx().pointer_interact_pos().is_some_and(|p| !ui.ctx().layer_id_at(p).is_some_and(|l| l.id == egui::Id::unique("node-search")));
    if let Some(def) = pick {
        let s = app.graph.search.take();
        if let Some(s) = s {
            let mut p = json!({"def": def, "at": [s.at.x, s.at.y - 20.0]});
            if let Some((n, port)) = s.connect {
                p["connectFrom"] = json!({"node": n.0, "port": port});
            }
            app.run("graph.node.add", p);
        }
    } else if close || clicked_out {
        app.graph.search = None;
    }
}

fn comment_editor(app: &mut MaterialCraftApp, ui: &mut Ui) {
    let Some((id, text)) = app.graph.editing.as_mut() else { return };
    let Some(r) = app.session.graph().ok().and_then(|g| g.comments.iter().find(|c| c.id == *id)).map(|c| (c.pos, c.size)) else {
        app.graph.editing = None;
        return;
    };
    let cam = &app.ui.graph_camera;
    let origin = app.graph.view.min;
    let at = origin + (Vec2::from(r.0) - Vec2::from(cam.pan)) * cam.zoom;
    let mut done = false;
    egui::Area::new(egui::Id::unique("comment-edit")).fixed_pos(at).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        let resp = ui.add(egui::TextEdit::multiline(text).desired_width(r.1[0] * cam.zoom).desired_rows(3));
        resp.request_focus();
        done = resp.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Escape));
    });
    if done && let Some((id, text)) = app.graph.editing.take() {
        app.run("graph.comment.edit", json!({"id": id, "text": text}));
    }
}

fn node_menu(app: &mut MaterialCraftApp, ui: &mut Ui, id: NodeId) {
    if !app.session.selection.contains(&id) {
        app.session.selection = vec![id];
    }
    let mut item = |ui: &mut Ui, label: &str, cmd: &str, params: serde_json::Value| {
        if ui.button(tr(label)).clicked() {
            app.run(cmd, params);
            ui.close();
        }
    };
    item(ui, "View in 2D view", "view.2d", json!({"node": id.0}));
    ui.separator();
    item(ui, "Copy selection", "edit.copy", json!({}));
    item(ui, "Duplicate selection", "edit.duplicate", json!({}));
    item(ui, "Duplicate selection without link", "edit.duplicate", json!({"withoutLinks": true}));
    item(ui, "Delete selection", "edit.delete", json!({}));
    item(ui, "Delete and relink", "edit.deleteRelink", json!({}));
    ui.separator();
    item(ui, "Select upstream nodes", "graph.selectUpstream", json!({}));
    item(ui, "Select downstream nodes", "graph.selectDownstream", json!({}));
    item(ui, "Disable selection", "graph.node.disable", json!({}));
    item(ui, "Reset parameters", "graph.node.reset", json!({"id": id.0}));
    ui.separator();
    item(ui, "Add frame around selection", "graph.frame.add", json!({}));
    item(ui, "Create graph from selection", "graph.createFromSelection", json!({}));
}

fn canvas_menu(app: &mut MaterialCraftApp, ui: &mut Ui, at: Pos2) {
    ui.menu_button(tr("Add node"), |ui| {
        for (cat, ids) in materialcraft_engine::nodes::categories() {
            ui.menu_button(cat, |ui| {
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    for id in ids {
                        let label = materialcraft_engine::nodes::find(id).map_or(id, |d| d.label);
                        if ui.button(label).clicked() {
                            app.run("graph.node.add", json!({"def": id, "at": [at.x, at.y]}));
                            ui.close();
                        }
                    }
                });
            });
        }
        let others: Vec<String> = app.session.pkg.graphs.iter().enumerate().filter(|(i, _)| *i != app.session.graph).map(|(_, g)| g.name.clone()).collect();
        if !others.is_empty() {
            ui.menu_button(tr("Graphs in this package"), |ui| {
                for name in others {
                    if ui.button(&name).clicked() {
                        app.run("graph.node.add", json!({"def": "graph_instance", "at": [at.x, at.y], "params": {"graph": name}}));
                        ui.close();
                    }
                }
            });
        }
    });
    let mut item = |ui: &mut Ui, label: &str, cmd: &str, params: serde_json::Value| {
        if ui.button(tr(label)).clicked() {
            app.run(cmd, params);
            ui.close();
        }
    };
    item(ui, "Add Comment", "graph.comment.add", json!({"at": [at.x, at.y]}));
    item(ui, "Add Frame", "graph.frame.add", json!({"rect": [at.x, at.y, 400.0, 260.0]}));
    item(ui, "Add Navigation pin", "graph.pin.add", json!({"at": [at.x, at.y]}));
    item(ui, "Add Dot node", "graph.node.add", json!({"def": "dot", "at": [at.x, at.y]}));
    ui.separator();
    item(ui, "Compute graph", "graph.cook", json!({}));
    item(ui, "Paste", "edit.paste", json!({"at": [at.x, at.y]}));
    item(ui, "Select all", "edit.selectAll", json!({}));
    ui.separator();
    let pins: Vec<(String, [f32; 2])> = app.session.graph().map(|g| g.pins.iter().map(|p| (p.name.clone(), p.pos)).collect()).unwrap_or_default();
    if !pins.is_empty() {
        ui.menu_button(tr("Go to pin"), |ui| {
            for (name, pos) in pins {
                if ui.button(&name).clicked() {
                    let v = app.graph.view;
                    let z = app.ui.graph_camera.zoom;
                    app.ui.graph_camera.pan = [pos[0] - v.width() * 0.5 / z, pos[1] - v.height() * 0.5 / z];
                    ui.close();
                }
            }
        });
    }
    if ui.button(tr("Frame all")).clicked() {
        app.ui.frame_all = true;
        ui.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_opens_search_in_the_full_workspace() {
        let mut app = MaterialCraftApp::headless(materialcraft_engine::sample_session(), crate::UiState::default());
        let ctx = egui::Context::default();
        for _ in 0..3 {
            ctx.run_ui(egui::RawInput::default(), |ui| {
                app.step(&ui.ctx().clone());
                app.draw(ui);
            })
            .textures_delta
            .clear();
        }
        let pointer = app.graph.view.center();
        let input = egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(pointer),
                egui::Event::Key { key: egui::Key::Tab, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE },
            ],
            ..Default::default()
        };
        ctx.run_ui(input, |ui| {
            app.step(&ui.ctx().clone());
            app.draw(ui);
        })
        .textures_delta
        .clear();
        assert!(app.graph.search.is_some());
    }

    #[test]
    fn node_search_shortcuts_respect_hover_and_keyboard_focus() {
        for key in [egui::Key::Space, egui::Key::Tab] {
            for (hovered, editing) in [(true, false), (false, false), (true, true)] {
                let ctx = egui::Context::default();
                let mut app = MaterialCraftApp::headless(materialcraft_engine::sample_session(), crate::UiState::default());
                let mut text = String::new();
                let mut draw = |ui: &mut Ui| {
                    if editing {
                        ui.text_edit_singleline(&mut text).request_focus();
                    }
                    app.graph.keyboard_was_focused = ui.ctx().egui_wants_keyboard_input();
                    show(&mut app, ui);
                };
                ctx.run_ui(egui::RawInput::default(), &mut draw).textures_delta.clear();
                let pointer = if hovered { pos2(200.0, 200.0) } else { pos2(-10.0, -10.0) };
                let input = egui::RawInput {
                    events: vec![
                        egui::Event::PointerMoved(pointer),
                        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE },
                    ],
                    ..Default::default()
                };
                ctx.run_ui(input, &mut draw).textures_delta.clear();
                assert_eq!(app.graph.search.is_some(), hovered && !editing, "{key:?}: hovered={hovered}, editing={editing}");
                if let Some(search) = &app.graph.search {
                    let cam = Cam { origin: app.graph.view.min, pan: Vec2::from(app.ui.graph_camera.pan), zoom: app.ui.graph_camera.zoom };
                    assert_eq!(search.at, cam.to_graph(pointer));
                    assert!(search.text.is_empty());
                }
            }
        }
    }

    #[test]
    fn search_finds_by_label_and_category() {
        assert!(matches("perlin").iter().any(|d| d.id == "perlin_noise"));
        assert!(matches("blur").first().is_some_and(|d| d.label.to_ascii_lowercase().starts_with("blur")));
        assert!(matches("noises cells").iter().all(|d| d.category.contains("Noises")));
        assert!(matches("zzzz").is_empty());
    }

    #[test]
    fn port_positions_stay_on_the_card() {
        let n = Node::new(NodeId(1), "blend", [0.0, 0.0]);
        let r = node_rect(&n, 3, 1, true);
        for i in 0..3 {
            let p = port_pos(r, i, 3, false, false);
            assert!(r.contains(p) || (p.x - r.left()).abs() < 1e-3);
            assert!(p.y > r.top() + HEADER_H && p.y < r.bottom() - FOOTER_H);
        }
    }
}
