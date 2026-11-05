//! Offscreen renders of the real UI (no window): `MC_SNAPSHOT_DIR=<dir> cargo test -p
//! materialcraft-ui-egui --release --test snapshot -- --ignored --nocapture` writes PNGs of a few
//! scenes. Used to check UI changes visually and to make the README screenshots.

use std::time::Duration;

use egui::vec2;
use egui_kittest::{Harness, kittest::Queryable};
use materialcraft_ui_egui::{MaterialCraftApp, UiState};
use serde_json::json;

fn harness(app: MaterialCraftApp, w: f32, h: f32) -> Harness<'static, MaterialCraftApp> {
    Harness::builder().with_size(vec2(w, h)).with_pixels_per_point(1.0).wgpu().build_ui_state(
        |ui, app: &mut MaterialCraftApp| {
            app.step(&ui.ctx().clone());
            app.draw(ui);
        },
        app,
    )
}

/// Run frames until background work (cooks, previews) settles.
fn settle(h: &mut Harness<'static, MaterialCraftApp>, frames: usize) {
    for _ in 0..frames {
        h.step();
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn save(h: &mut Harness<'static, MaterialCraftApp>, dir: &std::path::Path, name: &str) {
    let img = h.render().expect("render");
    img.save(dir.join(format!("{name}.png"))).expect("save");
    println!("wrote {}", dir.join(format!("{name}.png")).display());
}

/// The startup graph stays empty through real UI frames; templates are still available on demand.
#[test]
#[ignore = "requires an offscreen GPU; run explicitly with --ignored"]
fn empty_startup_can_create_a_template() {
    let app = MaterialCraftApp::headless(materialcraft_engine::new_session(), UiState::default());
    let mut h = harness(app, 1600.0, 960.0);
    settle(&mut h, 40);
    assert_eq!(h.state().session.pkg.graphs.len(), 1);
    assert!(h.state().session.graph().unwrap().nodes.is_empty());
    assert!(h.state().graph.rects.is_empty());
    if let Some(dir) = std::env::var_os("MC_SNAPSHOT_DIR").map(std::path::PathBuf::from) {
        std::fs::create_dir_all(&dir).unwrap();
        save(&mut h, &dir, "empty-startup");
    }
    h.get_by_label("New graph...").click();
    settle(&mut h, 4);
    h.get_by_label("Brick wall").click();
    settle(&mut h, 4);
    h.get_by_label("Create").click();
    settle(&mut h, 40);
    assert_eq!(h.state().session.pkg.graphs.len(), 2);
    assert!(!h.state().session.graph().unwrap().nodes.is_empty());
    assert!(!h.state().graph.rects.is_empty());
    if let Some(dir) = std::env::var_os("MC_SNAPSHOT_DIR").map(std::path::PathBuf::from) {
        save(&mut h, &dir, "chosen-template");
    }
}

#[test]
#[ignore]
fn snapshots() {
    let Some(dir) = std::env::var_os("MC_SNAPSHOT_DIR").map(std::path::PathBuf::from) else {
        eprintln!("set MC_SNAPSHOT_DIR");
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let app = MaterialCraftApp::headless(materialcraft_engine::sample_session(), UiState::default());
    let mut h = harness(app, 1600.0, 960.0);
    settle(&mut h, 40);
    // Select a node so the properties panel shows parameters.
    let id = h.state().session.graph().unwrap().nodes[0].id;
    h.state_mut().run("graph.select", json!({"ids": [id.0]}));
    h.state_mut().run("view.2d", json!({"node": 16, "histogram": true}));
    settle(&mut h, 30);
    save(&mut h, &dir, "workspace");
    for (tpl, name) in
        [("wood", "wood"), ("rusty_metal", "rusty-metal"), ("stone_tiles", "stone-tiles"), ("fabric", "fabric"), ("ground", "ground"), ("lava", "lava")]
    {
        h.state_mut().run("file.new", json!({"template": tpl, "size": 1024}));
        h.state_mut().ui.frame_all = true;
        settle(&mut h, 40);
        save(&mut h, &dir, name);
    }
    h.state_mut().ui.windows.new_graph = true;
    settle(&mut h, 40);
    save(&mut h, &dir, "new-graph");
    h.state_mut().ui.windows.new_graph = false;

    // A cut-out material in the 3D view (an opacity output), a reroute dot pulled out of a wire
    // and another wire selected; then the alpha channel view.
    h.state_mut().run("file.new", json!({"template": "wood", "size": 512}));
    let holes = h.state_mut().run("graph.node.add", json!({"def": "grid_lines", "at": [0, -260]})).unwrap()["id"].as_u64().unwrap();
    let out = h.state_mut().run("graph.node.add", json!({"def": "output", "at": [420, -260], "connectFrom": {"node": holes}, "params": {"usage": 7}})).unwrap()
        ["id"]
        .as_u64()
        .unwrap();
    h.state_mut().run("graph.wire.insertDot", json!({"to": out, "toPort": "input", "at": [210, -330]}));
    let wire = h.state().session.graph().unwrap().connections.iter().find(|c| c.to.0 != out).map(|c| (c.to, c.to_port.clone()));
    h.state_mut().graph.selected_wires = wire.into_iter().collect();
    h.state_mut().run("view.3d", json!({"scene": "sphere"}));
    h.state_mut().ui.frame_all = true;
    settle(&mut h, 60);
    save(&mut h, &dir, "alpha-transparency");
    h.state_mut().run("view.3d", json!({"alphaView": true}));
    settle(&mut h, 40);
    save(&mut h, &dir, "alpha-channel");
    h.state_mut().run("view.3d", json!({"alphaView": false, "scene": "rounded_cube"}));

    // A pixel processor drawing rings, with its function graph open.
    h.state_mut().run("file.new", json!({"template": "empty", "name": "Rings"}));
    let f = json!({"t": "function", "v": {"nodes": [
        {"id": 1, "op": "get_pos", "pos": [0, 0]},
        {"id": 2, "op": "const_float2", "value": {"t": "float2", "v": [0.5, 0.5]}, "pos": [0, 110]},
        {"id": 3, "op": "sub", "inputs": [1, 2], "pos": [170, 40]},
        {"id": 4, "op": "length", "inputs": [3], "pos": [340, 40]},
        {"id": 5, "op": "const_float", "value": {"t": "float", "v": 60.0}, "pos": [340, 130]},
        {"id": 6, "op": "mul", "inputs": [4, 5], "pos": [510, 60]},
        {"id": 7, "op": "sin", "inputs": [6], "pos": [680, 60]}
    ], "output": 7}});
    let id = h.state_mut().run("graph.node.add", json!({"def": "pixel_processor", "at": [0, 0], "params": {"output": 1}})).unwrap()["id"].as_u64().unwrap();
    h.state_mut().run("graph.node.param", json!({"id": id, "param": "function", "value": f}));
    let out =
        h.state_mut().run("graph.node.add", json!({"def": "output", "connectFrom": {"node": id}, "params": {"usage": 4}})).unwrap()["id"].as_u64().unwrap();
    let _ = out;
    h.state_mut().run("graph.select", json!({"ids": [id]}));
    h.state_mut().run("view.2d", json!({"node": id}));
    h.state_mut().fn_editor = Some(materialcraft_ui_egui::function_editor::FnEditor::new(materialcraft_engine::core::NodeId(id), "function"));
    h.state_mut().ui.frame_all = true;
    settle(&mut h, 40);
    save(&mut h, &dir, "function-editor");
    h.state_mut().fn_editor = None;

    // The Levels editor: the input histogram under the transfer curve, with its handles.
    h.state_mut().run("file.new", json!({"template": "stone_tiles", "size": 512}));
    let levels = {
        let g = h.state().session.graph().unwrap();
        g.nodes.iter().find(|n| n.def == "levels" && g.connections.iter().any(|c| c.to == n.id)).map(|n| n.id.0)
    };
    if let Some(id) = levels {
        h.state_mut().run("graph.select", json!({"ids": [id]}));
        h.state_mut().run("graph.node.param", json!({"id": id, "param": "in_low", "value": 0.3}));
        h.state_mut().run("graph.node.param", json!({"id": id, "param": "in_mid", "value": 0.35}));
        h.state_mut().run("graph.node.param", json!({"id": id, "param": "in_high", "value": 0.85}));
        h.state_mut().run("view.2d", json!({"node": id}));
    }
    h.state_mut().ui.frame_all = true;
    settle(&mut h, 40);
    save(&mut h, &dir, "levels-editor");

    // An SBS file with nodes MaterialCraft doesn't have: placeholders on the canvas, the warnings
    // in the Console, which comes forward by itself.
    let sample = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/sbs/all_nodes.sbs");
    materialcraft_ui_egui::windows::open_path(h.state_mut(), sample.to_str().unwrap());
    settle(&mut h, 60);
    save(&mut h, &dir, "sbs-warnings");
}

/// Steady-state UI build time per frame with each panel closed in turn (release):
/// `cargo test -p materialcraft-ui-egui --release --test snapshot perf -- --ignored --nocapture`.
#[test]
#[ignore]
fn perf() {
    use materialcraft_ui_egui::docking::Panel;
    let app = MaterialCraftApp::headless(materialcraft_engine::sample_session(), UiState::default());
    let mut h = harness(app, 1600.0, 960.0);
    settle(&mut h, 60);
    let measure = |h: &mut Harness<'static, MaterialCraftApp>| {
        let t = std::time::Instant::now();
        for _ in 0..30 {
            h.step();
        }
        t.elapsed().as_secs_f64() * 1000.0 / 30.0
    };
    println!("all panels: {:.2} ms/frame", measure(&mut h));
    for p in [Panel::Library, Panel::View3d, Panel::View2d, Panel::Properties, Panel::Explorer] {
        h.state_mut().ui.dock.close(p);
        settle(&mut h, 3);
        println!("without {p:?}: {:.2} ms/frame", measure(&mut h));
        h.state_mut().ui.dock.focus(p);
        settle(&mut h, 3);
    }
}
