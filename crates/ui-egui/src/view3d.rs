//! The 3D view: the open graph's material on a built-in scene, rendered by
//! `materialcraft-render` (one ray per pixel at half resolution while the camera moves, 2×2
//! supersampled when it rests). Left-drag orbits, Alt+drag or middle-drag pans, the wheel zooms,
//! Shift+drag turns the light and a double-click resets the camera.
//!
//! The full-quality frame waits until the view's size and the camera have rested for [`SETTLE`]
//! (a panel, divider or window drag changes the size on every step), then renders on a worker
//! thread so the UI never waits for it; until it lands the last frame stays up, kept in proportion.
//! A frame that is no longer wanted is cancelled. The browser has no threads and renders here.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use egui::{Color32, ColorImage, Rect, Sense, TextureHandle, TextureOptions, Ui, pos2, vec2};
use egui_phosphor::regular as icon;
use materialcraft_engine::view_cmds::{ENVIRONMENTS, SCENES};
use materialcraft_render::{Lighting, Maps, Scene, Settings};
use serde_json::json;

use crate::MaterialCraftApp;
use crate::i18n::tr;
use crate::theme::Tokens;

/// Seconds the view's size and the camera rest before the full-quality frame renders.
pub const SETTLE: f64 = 0.15;

#[derive(Default)]
pub struct View3dState {
    scene: Option<(String, Arc<Scene>)>,
    light: Option<Arc<Lighting>>,
    tex: Option<TextureHandle>,
    /// What the texture shows: (content key, full quality).
    key: Option<(u64, bool)>,
    /// The view size (points) the texture was rendered for.
    tex_size: Option<(u32, u32)>,
    /// The view size on the last frame.
    size: Option<(u32, u32)>,
    /// When the view's size or the camera last changed (app seconds).
    changed_at: Option<f64>,
    /// The full-quality frame rendering on a worker thread.
    job: Option<Job>,
    pub render_ms: f32,
    /// Frames rendered so far (previews and full quality).
    pub renders: u64,
    /// The last frame shown (RGBA8, for `view.screenshot`).
    pub shown: Option<(u32, u32, Vec<u8>)>,
    show_env: bool,
}

impl View3dState {
    /// A full-quality frame is rendering in the background.
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
}

/// A rendered frame: RGBA8 pixels and how long they took.
struct Frame {
    w: u32,
    h: u32,
    px: Vec<u8>,
    ms: f32,
}

/// A full-quality frame rendering on a worker thread. Dropping it cancels the render.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
struct Job {
    key: u64,
    size: (u32, u32),
    cancel: Arc<AtomicBool>,
    rx: Receiver<Option<Frame>>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
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
                ui.separator();
                let mut t = app.session.view3d.transparency;
                if ui.checkbox(&mut t, tr("Transparency (alpha)")).changed() {
                    app.run("view.3d", json!({"transparency": t}));
                }
                let mut a = app.session.view3d.alpha_view;
                if ui.checkbox(&mut a, tr("Show alpha channel")).changed() {
                    app.run("view.3d", json!({"alphaView": a}));
                }
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
        p.image(t.id(), frame_rect(rect, app.view3d.tex_size), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
    if app.session.view3d.alpha_view {
        p.text(rect.left_top() + vec2(8.0, 6.0), egui::Align2::LEFT_TOP, tr("Alpha channel"), egui::FontId::proportional(11.0), tk.viewport_text);
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
            let a = app.session.view3d.alpha_view;
            if crate::widgets::icon_button(ui, icon::CHECKERBOARD, tr("Show alpha channel"), a, 20.0).clicked() {
                app.run("view.3d", json!({"alphaView": !a}));
            }
        });
    });
}

fn scene_label_custom(app: &MaterialCraftApp) -> String {
    app.session.custom_mesh.as_ref().map_or_else(String::new, |(n, _)| n.clone())
}

/// The view's size in whole points: part of the content key and of `tex_size`.
fn view_size(rect: Rect) -> (u32, u32) {
    (rect.width() as u32, rect.height() as u32)
}

/// Where the last frame goes in `rect`: all of it when it was rendered for this size, otherwise
/// at the view's height and centred. The camera's field of view is vertical, so that is what a
/// new render will show (cropped, or with bars at the sides) while a resize is in progress,
/// instead of a stretched picture.
fn frame_rect(rect: Rect, rendered: Option<(u32, u32)>) -> Rect {
    match rendered {
        Some((w, h)) if h > 0 && (w, h) != view_size(rect) => Rect::from_center_size(rect.center(), vec2(rect.height() * w as f32 / h as f32, rect.height())),
        _ => rect,
    }
}

fn content_key(app: &MaterialCraftApp, w: u32, h: u32, show_env: bool) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hs = DefaultHasher::new();
    let v = &app.session.view3d;
    (w, h, show_env, &v.scene, &v.environment, app.session.custom_mesh.as_ref().map(|(n, _)| n), v.transparency, v.alpha_view).hash(&mut hs);
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

/// Keep the 3D view's texture up to date: a quick frame right away while the camera moves, the
/// full-quality one once the view's size and the camera have rested for [`SETTLE`].
fn render(app: &mut MaterialCraftApp, ctx: &egui::Context, rect: Rect, moving: bool) {
    let now = ctx.input(|i| i.time);
    let size = view_size(rect);
    if moving || app.view3d.size.is_some_and(|s| s != size) {
        app.view3d.changed_at = Some(now);
    }
    app.view3d.size = Some(size);
    let key = content_key(app, size.0, size.1, app.view3d.show_env);
    collect(app, ctx, key);
    if app.view3d.key == Some((key, true)) || (moving && app.view3d.key == Some((key, false))) {
        return;
    }
    let ppp = ctx.pixels_per_point();
    if moving {
        // One ray per pixel at half resolution, here: the camera follows the pointer.
        if let Some(frame) = render_now(app, rect, ppp * 0.5, 1) {
            show_frame(app, ctx, frame, key, false, size);
        }
        ctx.request_repaint_after(Duration::from_secs_f64(SETTLE));
        return;
    }
    let wait = app.view3d.changed_at.map_or(0.0, |t| SETTLE - (now - t));
    if wait > 0.0 {
        // Mid-resize or the camera just stopped: the last frame stays up (see `frame_rect`).
        ctx.request_repaint_after(Duration::from_secs_f64(wait.min(SETTLE)));
        return;
    }
    if app.view3d.job.as_ref().is_some_and(|j| j.key == key) {
        return;
    }
    start_full(app, ctx, rect, key, size);
}

/// Show a finished full-quality frame if it is still the one wanted; drop (and so cancel) one
/// that is not.
fn collect(app: &mut MaterialCraftApp, ctx: &egui::Context, key: u64) {
    let Some(job) = app.view3d.job.take() else { return };
    if job.key != key {
        return;
    }
    match job.rx.try_recv() {
        Ok(Some(frame)) => show_frame(app, ctx, frame, key, true, job.size),
        Ok(None) | Err(TryRecvError::Disconnected) => {}
        Err(TryRecvError::Empty) => app.view3d.job = Some(job),
    }
}

/// The scene, lighting, maps and settings of the current view (scene and lighting are cached).
fn inputs(app: &mut MaterialCraftApp, supersample: u32) -> Option<(Arc<Scene>, Arc<Lighting>, Maps, Settings)> {
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
        app.view3d.scene = Some((scene_key, Arc::new(scene)));
    }
    if app.view3d.light.as_ref().is_none_or(|l| l.name != v.environment) {
        app.view3d.light = Some(Arc::new(Lighting::new(&v.environment)));
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
        supersample,
        transparency: v.transparency,
        alpha_view: v.alpha_view,
        ..Settings::default()
    };
    let maps = Maps::from_usages(app.session.material_maps());
    let (Some((_, scene)), Some(light)) = (&app.view3d.scene, &app.view3d.light) else { return None };
    Some((scene.clone(), light.clone(), maps, settings))
}

/// Texture pixels for `rect` at `scale` pixels per point.
fn pixels(rect: Rect, scale: f32) -> (u32, u32) {
    (((rect.width() * scale) as u32).clamp(16, 4096), ((rect.height() * scale) as u32).clamp(16, 4096))
}

/// Render a frame on this thread.
fn render_now(app: &mut MaterialCraftApp, rect: Rect, scale: f32, supersample: u32) -> Option<Frame> {
    let (scene, light, maps, settings) = inputs(app, supersample)?;
    let (w, h) = pixels(rect, scale);
    let t0 = crate::now_ms();
    let px = materialcraft_render::render(&scene, &maps, &light, &settings, w, h);
    Some(Frame { w, h, px, ms: (crate::now_ms() - t0) as f32 })
}

/// Start the full-quality frame (2×2 supersampled, full resolution) on a worker thread.
#[cfg(not(target_arch = "wasm32"))]
fn start_full(app: &mut MaterialCraftApp, ctx: &egui::Context, rect: Rect, key: u64, size: (u32, u32)) {
    let Some((scene, light, maps, settings)) = inputs(app, 2) else { return };
    let (w, h) = pixels(rect, ctx.pixels_per_point());
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = std::sync::mpsc::channel();
    let (stop, repaint, viewport) = (cancel.clone(), ctx.clone(), ctx.viewport_id());
    let spawned = std::thread::Builder::new().name("materialcraft-view3d".into()).spawn(move || {
        let t0 = crate::now_ms();
        let px = materialcraft_render::render_cancellable(&scene, &maps, &light, &settings, w, h, &stop);
        let _ = tx.send(px.map(|px| Frame { w, h, px, ms: (crate::now_ms() - t0) as f32 }));
        repaint.request_repaint_of(viewport);
    });
    match spawned {
        Ok(_) => app.view3d.job = Some(Job { key, size, cancel, rx }),
        Err(e) => {
            log::warn!("could not start a 3D view render thread ({e}); rendering here");
            if let Some(frame) = render_now(app, rect, ctx.pixels_per_point(), 2) {
                show_frame(app, ctx, frame, key, true, size);
            }
        }
    }
}

/// The browser has no threads: the full-quality frame renders here (the page pauses).
#[cfg(target_arch = "wasm32")]
fn start_full(app: &mut MaterialCraftApp, ctx: &egui::Context, rect: Rect, key: u64, size: (u32, u32)) {
    if let Some(frame) = render_now(app, rect, ctx.pixels_per_point(), 2) {
        show_frame(app, ctx, frame, key, true, size);
    }
}

/// Upload a frame and remember what it shows.
fn show_frame(app: &mut MaterialCraftApp, ctx: &egui::Context, frame: Frame, key: u64, full: bool, size: (u32, u32)) {
    let st = &mut app.view3d;
    let img = ColorImage::from_rgba_unmultiplied([frame.w as usize, frame.h as usize], &frame.px);
    match &mut st.tex {
        Some(t) => t.set(img, TextureOptions::LINEAR),
        None => st.tex = Some(ctx.load_texture("view3d", img, TextureOptions::LINEAR)),
    }
    st.render_ms = frame.ms;
    st.renders += 1;
    st.shown = Some((frame.w, frame.h, frame.px));
    st.key = Some((key, full));
    st.tex_size = Some(size);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_for_another_size_keeps_its_proportions() {
        let rect = Rect::from_min_size(pos2(10.0, 20.0), vec2(400.0, 200.0));
        assert_eq!(frame_rect(rect, None), rect);
        assert_eq!(frame_rect(rect, Some((400, 200))), rect);
        // Rendered narrower (the view was widened): full height, centred, bars at the sides.
        let r = frame_rect(rect, Some((300, 200)));
        assert_eq!((r.width(), r.height(), r.center()), (300.0, 200.0, rect.center()));
        // A degenerate size never divides by zero.
        assert_eq!(frame_rect(rect, Some((300, 0))), rect);
    }
}
