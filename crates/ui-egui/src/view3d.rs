//! The 3D view: the open graph's material on a built-in scene, rendered by
//! `materialcraft-render` (one ray per pixel at half resolution while the camera moves, 2×2
//! supersampled when it rests). Left-drag orbits, Alt+drag or middle-drag pans, the wheel zooms,
//! Shift+drag turns the light and a double-click resets the camera.

use egui::{Color32, ColorImage, Rect, Sense, TextureHandle, TextureOptions, Ui, pos2, vec2};
use egui_phosphor::regular as icon;
use materialcraft_engine::view_cmds::{ENVIRONMENTS, SCENES};
use materialcraft_render::{Lighting, Maps, Scene, Settings};
use serde_json::json;

use crate::MaterialCraftApp;
use crate::i18n::tr;
use crate::theme::Tokens;

#[derive(Default)]
pub struct View3dState {
    scene: Option<(String, Scene)>,
    light: Option<Lighting>,
    tex: Option<TextureHandle>,
    /// What the texture shows: (content key, full quality).
    key: Option<(u64, bool)>,
    pub render_ms: f32,
    /// The last frame shown (RGBA8, for `view.screenshot`).
    pub shown: Option<(u32, u32, Vec<u8>)>,
    show_env: bool,
}

pub fn scene_label(app: &MaterialCraftApp) -> String {
    let id = &app.session.view3d.scene;
    if id == "custom"
        && let Some((name, _)) = &app.session.custom_mesh
    {
        return name.clone();
    }
    SCENES.iter().find(|(s, _)| s == id).map_or(id.as_str(), |(_, l)| l).to_string()
}

fn menus(app: &mut MaterialCraftApp, ui: &mut Ui) {
    let tk = Tokens::get(ui.ctx());
    egui::Frame::NONE.fill(tk.panel).inner_margin(egui::Margin::symmetric(4, 1)).show(ui, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(tr("Scene"), |ui| {
                for (id, label) in SCENES {
                    if ui.selectable_label(app.session.view3d.scene == *id, tr(label)).clicked() {
                        app.run("view.3d", json!({"scene": id}));
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button(tr("Load mesh...")).clicked() {
                    crate::web_files::load_mesh(app, ui.ctx());
                    ui.close();
                }
                if app.session.custom_mesh.is_some() && ui.selectable_label(app.session.view3d.scene == "custom", scene_label_custom(app)).clicked() {
                    app.run("view.3d", json!({"scene": "custom"}));
                    ui.close();
                }
                ui.separator();
                if ui.button(tr("Reset scene")).clicked() {
                    app.run("view.3d.reset", json!({}));
                    ui.close();
                }
            });
            ui.menu_button(tr("Camera"), |ui| {
                for (label, yaw, pitch) in [("Default", 35.0, 25.0), ("Front", 0.0, 0.0), ("Top", 0.0, 89.0), ("Side", 90.0, 0.0)] {
                    if ui.button(tr(label)).clicked() {
                        app.run("view.3d", json!({"yaw": yaw, "pitch": pitch, "distance": 3.2, "target": [0.0, 0.0, 0.0]}));
                        ui.close();
                    }
                }
            });
            ui.menu_button(tr("Environment"), |ui| {
                for (id, label) in ENVIRONMENTS {
                    if ui.selectable_label(app.session.view3d.environment == *id, tr(label)).clicked() {
                        app.run("view.3d", json!({"environment": id}));
                        ui.close();
                    }
                }
                ui.separator();
                let mut rot = app.session.view3d.env_rotation;
                if crate::widgets::slider(ui, tr("Rotation"), &mut rot, 0.0..=360.0, 0).changed() {
                    app.run("view.3d", json!({"envRotation": rot}));
                }
                let mut exp = app.session.view3d.exposure;
                if crate::widgets::slider(ui, tr("Exposure"), &mut exp, -4.0..=4.0, 2).changed() {
                    app.run("view.3d", json!({"exposure": exp}));
                }
            });
            ui.menu_button(tr("Display"), |ui| {
                ui.checkbox(&mut app.view3d.show_env, tr("Environment background"));
            });
            ui.menu_button(tr("Renderer"), |ui| {
                let mut h = app.session.view3d.height_scale * 100.0;
                if crate::widgets::slider(ui, tr("Height (parallax) depth"), &mut h, 0.0..=10.0, 2).changed() {
                    app.run("view.3d", json!({"heightScale": h / 100.0}));
                }
                let mut t = app.session.view3d.tiling;
                if crate::widgets::slider(ui, tr("Tiling"), &mut t, 0.25..=8.0, 2).changed() {
                    app.run("view.3d", json!({"tiling": t}));
                }
            });
        });
    });
}

pub fn show(app: &mut MaterialCraftApp, ui: &mut Ui) {
    let tk = Tokens::get(ui.ctx());
    menus(app, ui);
    let avail = ui.available_size();
    let (rect, resp) = ui.allocate_exact_size(vec2(avail.x, (avail.y - 22.0).max(40.0)), Sense::click_and_drag());
    crate::widgets::name(&resp, egui::Role::Image, "3D view");
    // Camera gestures.
    let mut moving = false;
    if resp.dragged() {
        let d = resp.drag_delta();
        let v = &app.session.view3d;
        if ui.input(|i| i.modifiers.alt) || resp.dragged_by(egui::PointerButton::Middle) {
            if let Some((_, scene)) = &app.view3d.scene {
                let s = Settings { yaw: v.yaw, pitch: v.pitch, distance: v.distance, target: v.target, ..Settings::default() };
                let cam = app.session.camera;
                let k = cam.pan_speed / rect.height();
                app.session.view3d.target = cam.clamp_target(materialcraft_render::pan(scene, &s, d.x * k, d.y * k));
            }
        } else if ui.input(|i| i.modifiers.shift) {
            let rot = (v.env_rotation + d.x * 0.5).rem_euclid(360.0);
            app.session.view3d.env_rotation = rot;
        } else {
            let (yaw, pitch) = (v.yaw - d.x * 0.4, (v.pitch + d.y * 0.4).clamp(-89.0, 89.0));
            app.session.view3d.yaw = yaw.rem_euclid(360.0);
            app.session.view3d.pitch = pitch;
        }
        moving = true;
    }
    if resp.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.0 {
            app.session.view3d.distance = (app.session.view3d.distance * (-scroll * 0.002).exp()).clamp(1.2, 20.0);
            moving = true;
        }
    }
    if resp.double_clicked() {
        app.run("view.3d", json!({"yaw": 35.0, "pitch": 25.0, "distance": 3.2, "target": [0.0, 0.0, 0.0]}));
    }
    render(app, ui.ctx(), rect, moving || resp.is_pointer_button_down_on());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, Color32::from_rgb(0x1b, 0x1c, 0x1e));
    if let Some(t) = &app.view3d.tex {
        p.image(t.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
    if app.session.material_maps().is_empty() {
        p.text(
            rect.center_top() + vec2(0.0, 18.0),
            egui::Align2::CENTER_TOP,
            tr("Add Output nodes with usages (base color, normal, ...) to see the material"),
            egui::FontId::proportional(11.0),
            tk.viewport_text,
        );
    }
    p.text(
        rect.right_bottom() - vec2(8.0, 6.0),
        egui::Align2::RIGHT_BOTTOM,
        format!("{}: {:.0} ms", tr("Render time"), app.view3d.render_ms),
        egui::FontId::proportional(10.5),
        tk.viewport_text,
    );
    // Bottom bar.
    let bar = Rect::from_min_size(pos2(rect.left(), rect.bottom()), vec2(rect.width(), 22.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(bar), |ui| {
        ui.painter().rect_filled(bar, 0.0, tk.panel);
        ui.horizontal_centered(|ui| {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(icon::SPHERE).color(tk.text_dim));
            let r = egui::ComboBox::from_id_salt("scene3d").width(110.0).selected_text(scene_label(app)).show_ui(ui, |ui| {
                for (id, label) in SCENES {
                    if ui.selectable_label(app.session.view3d.scene == *id, tr(label)).clicked() {
                        app.run("view.3d", json!({"scene": id}));
                    }
                }
            });
            crate::widgets::name(&r.response, egui::Role::ComboBox, "Scene");
            ui.label(egui::RichText::new(format!("{}  sRGB / ACES", icon::SUN)).size(11.0).color(tk.text_dim));
        });
    });
}

fn scene_label_custom(app: &MaterialCraftApp) -> String {
    app.session.custom_mesh.as_ref().map_or_else(String::new, |(n, _)| n.clone())
}

fn content_key(app: &MaterialCraftApp, w: u32, h: u32, show_env: bool) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hs = DefaultHasher::new();
    let v = &app.session.view3d;
    (w, h, show_env, &v.scene, &v.environment, app.session.custom_mesh.as_ref().map(|(n, _)| n)).hash(&mut hs);
    for f in [v.yaw, v.pitch, v.distance, v.target[0], v.target[1], v.target[2], v.tiling, v.exposure, v.env_rotation, v.height_scale] {
        f.to_bits().hash(&mut hs);
    }
    if let Ok(g) = app.session.graph() {
        for (id, usage, _, _) in materialcraft_engine::eval::outputs(g, &app.session.cooked) {
            (usage, app.session.cooked.nodes.get(&id).map(|r| r.key)).hash(&mut hs);
        }
    }
    hs.finish()
}

fn render(app: &mut MaterialCraftApp, ctx: &egui::Context, rect: Rect, moving: bool) {
    let ppp = ctx.pixels_per_point();
    let full = !moving;
    let scale = if full { ppp } else { ppp * 0.5 };
    let (w, h) = (((rect.width() * scale) as u32).clamp(16, 4096), ((rect.height() * scale) as u32).clamp(16, 4096));
    let key = content_key(app, rect.width() as u32, rect.height() as u32, app.view3d.show_env);
    if app.view3d.key == Some((key, true)) || (!full && app.view3d.key == Some((key, false))) {
        return;
    }
    let v = app.session.view3d.clone();
    let scene_key = match (&*v.scene, &app.session.custom_mesh) {
        ("custom", Some((name, _))) => format!("custom:{name}"),
        _ => v.scene.clone(),
    };
    if app.view3d.scene.as_ref().is_none_or(|(id, _)| *id != scene_key) {
        let scene = match (&*v.scene, &app.session.custom_mesh) {
            ("custom", Some((_, mesh))) => Scene::new((**mesh).clone()),
            _ => Scene::builtin(&v.scene),
        };
        app.view3d.scene = Some((scene_key, scene));
    }
    if app.view3d.light.as_ref().is_none_or(|l| l.name != v.environment) {
        app.view3d.light = Some(Lighting::new(&v.environment));
    }
    let settings = Settings {
        yaw: v.yaw,
        pitch: v.pitch,
        distance: v.distance,
        target: v.target,
        tiling: v.tiling,
        exposure: v.exposure,
        env_rotation: v.env_rotation,
        height_scale: v.height_scale,
        show_env: app.view3d.show_env,
        supersample: if full { 2 } else { 1 },
        ..Settings::default()
    };
    let maps = Maps::from_usages(app.session.material_maps());
    let (Some((_, scene)), Some(light)) = (&app.view3d.scene, &app.view3d.light) else { return };
    let t0 = crate::now_ms();
    let px = materialcraft_render::render(scene, &maps, light, &settings, w, h);
    app.view3d.render_ms = (crate::now_ms() - t0) as f32;
    let img = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px);
    match &mut app.view3d.tex {
        Some(t) => t.set(img, TextureOptions::LINEAR),
        None => app.view3d.tex = Some(ctx.load_texture("view3d", img, TextureOptions::LINEAR)),
    }
    app.view3d.shown = Some((w, h, px));
    app.view3d.key = Some((key, full));
    if !full {
        // Come back for the full-quality frame once the camera rests.
        ctx.request_repaint_after(std::time::Duration::from_millis(120));
    }
}
