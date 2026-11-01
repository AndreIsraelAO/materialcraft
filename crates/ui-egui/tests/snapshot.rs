//! Offscreen renders of the real UI (no window): `MC_SNAPSHOT_DIR=<dir> cargo test -p
//! materialcraft-ui-egui --release --test snapshot -- --ignored --nocapture` writes PNGs of a few
//! scenes. Used to check UI changes visually and to make the README screenshots.

use std::time::Duration;

use egui::vec2;
use egui_kittest::Harness;
use materialcraft_ui_egui::{MaterialCraftApp, UiState};
use serde_json::json;

fn harness(app: MaterialCraftApp, w: f32, h: f32) -> Harness<'static, MaterialCraftApp> {
    Harness::builder().with_size(vec2(w, h)).with_pixels_per_point(1.0).with_accessibility_check(false).wgpu().build_ui_state(
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
    h.state_mut().run("view.2d", json!({"node": 17}));
    settle(&mut h, 30);
    save(&mut h, &dir, "workspace");
    for (tpl, name) in [("wood", "wood"), ("rusty_metal", "rusty-metal"), ("stone_tiles", "stone-tiles"), ("fabric", "fabric"), ("ground", "ground")] {
        h.state_mut().run("file.new", json!({"template": tpl, "size": 1024}));
        h.state_mut().ui.frame_all = true;
        settle(&mut h, 40);
        save(&mut h, &dir, name);
    }
    h.state_mut().ui.windows.new_graph = true;
    settle(&mut h, 40);
    save(&mut h, &dir, "new-graph");
}
