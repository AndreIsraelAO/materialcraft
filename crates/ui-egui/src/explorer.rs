//! The explorer: the open package, its graphs and its bitmap resources.

use egui::{RichText, Sense, Ui, vec2};
use egui_phosphor::regular as icon;
use serde_json::json;

use crate::MaterialCraftApp;
use crate::i18n::tr;
use crate::theme::Tokens;

pub fn show(app: &mut MaterialCraftApp, ui: &mut Ui) {
    let tk = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if crate::widgets::icon_button(ui, icon::FILE_PLUS, tr("New graph..."), false, 22.0).clicked() {
            app.ui.windows.new_graph = true;
        }
        if crate::widgets::icon_button(ui, icon::FOLDER_OPEN, tr("Open package..."), false, 22.0).clicked() {
            crate::menus::invoke(app, ui.ctx(), "file.open");
        }
        if crate::widgets::icon_button(ui, icon::FLOPPY_DISK, tr("Save package"), false, 22.0).clicked() {
            crate::menus::invoke(app, ui.ctx(), "file.save");
        }
        if crate::widgets::icon_button(ui, icon::EXPORT, tr("Export outputs..."), false, 22.0).clicked() {
            app.ui.windows.export = true;
        }
        if crate::widgets::icon_button(ui, icon::IMAGE, tr("Import bitmap..."), false, 22.0).clicked() {
            crate::menus::invoke(app, ui.ctx(), "file.importBitmap");
        }
    });
    ui.add_space(2.0);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let name = format!("{}{}", app.session.pkg.name, if app.session.modified { "*" } else { "" });
        ui.label(RichText::new(format!("{}  {name}", icon::PACKAGE)).strong().color(tk.text_strong));
        let mut open: Option<usize> = None;
        let mut action: Option<(&str, usize)> = None;
        for (i, g) in app.session.pkg.graphs.iter().enumerate() {
            let on = i == app.session.graph;
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
            if on {
                ui.painter().rect_filled(r, 4.0, tk.row_selected);
                ui.painter().rect_stroke(r, 4.0, egui::Stroke::new(1.0, tk.accent), egui::StrokeKind::Inside);
            } else if resp.hovered() {
                ui.painter().rect_filled(r, 4.0, tk.row_hover);
            }
            ui.painter().text(
                r.left_center() + vec2(18.0, 0.0),
                egui::Align2::LEFT_CENTER,
                format!("{}  {}", icon::TREE_STRUCTURE, g.name),
                egui::FontId::proportional(12.0),
                if on { tk.text_strong } else { tk.text },
            );
            if resp.clicked() {
                open = Some(i);
            }
            resp.context_menu(|ui| {
                for (label, a) in [("Open", "open"), ("Duplicate", "duplicate"), ("Delete", "delete")] {
                    if ui.button(tr(label)).clicked() {
                        action = Some((a, i));
                        ui.close();
                    }
                }
            });
        }
        if let Some(i) = open {
            app.run("graph.open", json!({"graph": i}));
        }
        match action {
            Some(("open", i)) => {
                app.run("graph.open", json!({"graph": i}));
            }
            Some(("duplicate", i)) => {
                app.run("graph.duplicate", json!({"graph": i}));
            }
            Some(("delete", i)) => {
                app.run("graph.delete", json!({"graph": i}));
            }
            _ => {}
        }
        if !app.session.pkg.resources.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new(tr("Resources")).color(tk.text_dim).size(11.0));
            for r in &app.session.pkg.resources {
                ui.label(format!("{}  {}  ({} KB)", icon::IMAGE, r.name, r.bytes.len() / 1024));
            }
        }
    });
}
