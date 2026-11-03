//! UI state: serde data, so the control channel can read and drive it and eframe persists it.

use serde::{Deserialize, Serialize};

use materialcraft_engine::CameraPrefs;

use crate::docking::Dock;
use crate::theme::{Accent, Brightness};

/// Preferences (Edit ▸ Preferences).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Prefs {
    pub brightness: Brightness,
    pub accent: Accent,
    pub ui_scale: f32,
    /// Largest node output while editing (0 = full size).
    pub preview_size: u32,
    /// Minutes between autosaves of a modified package (0 = off).
    pub autosave_minutes: u32,
    /// Show node thumbnails in the graph.
    pub thumbnails: bool,
    /// Library tiles: large icons or a list.
    pub library_list: bool,
    /// Open the home screen when the app starts.
    pub show_home: bool,
    /// 3D view camera: pan limit and speed.
    pub camera: CameraPrefs,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            brightness: Brightness::default(),
            accent: Accent::default(),
            ui_scale: 1.0,
            preview_size: 512,
            autosave_minutes: 5,
            thumbnails: true,
            library_list: false,
            show_home: true,
            camera: CameraPrefs::default(),
        }
    }
}

/// Which dialogs are open.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Windows {
    pub new_graph: bool,
    pub export: bool,
    pub about: bool,
    pub preferences: bool,
    pub shortcuts: bool,
    pub home: bool,
}

/// The graph view's camera: the graph point at the view's top-left corner, and the zoom.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphCamera {
    pub pan: [f32; 2],
    pub zoom: f32,
}

impl Default for GraphCamera {
    fn default() -> Self {
        GraphCamera { pan: [-60.0, -60.0], zoom: 0.8 }
    }
}

/// The 2D view's camera: image zoom (1 = one image pixel per point) and center offset.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageCamera {
    pub zoom: f32,
    pub offset: [f32; 2],
    /// Fit the image to the view (until the user zooms).
    pub fit: bool,
}

impl Default for ImageCamera {
    fn default() -> Self {
        ImageCamera { zoom: 1.0, offset: [0.0, 0.0], fit: true }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiState {
    pub dock: Dock,
    pub prefs: Prefs,
    pub windows: Windows,
    pub graph_camera: GraphCamera,
    pub image_camera: ImageCamera,
    /// Library: the selected category path ("" = all) and the search text.
    pub library_category: String,
    pub library_search: String,
    /// Recently opened or saved packages, newest first.
    pub recent: Vec<String>,
    /// New graph dialog fields.
    pub new_graph_template: String,
    pub new_graph_name: String,
    pub new_graph_size: u32,
    /// Export dialog fields.
    pub export_dir: String,
    pub export_format: String,
    pub export_depth: u32,
    /// Export size in pixels; 0 = the graph's own size.
    pub export_size: u32,
    pub export_pattern: String,
    /// Frame the whole graph on the next frame.
    #[serde(skip)]
    pub frame_all: bool,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            dock: Dock::default(),
            prefs: Prefs::default(),
            windows: Windows::default(),
            graph_camera: GraphCamera::default(),
            image_camera: ImageCamera::default(),
            library_category: String::new(),
            library_search: String::new(),
            recent: Vec::new(),
            new_graph_template: "pbr".into(),
            new_graph_name: "New_graph".into(),
            new_graph_size: 1024,
            export_dir: String::new(),
            export_format: "png".into(),
            export_depth: 0,
            export_size: 0,
            export_pattern: "$(graph)_$(identifier)".into(),
            frame_all: true,
        }
    }
}

impl UiState {
    /// Remember `path` at the top of the recent list.
    pub fn add_recent(&mut self, path: &str) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_string());
        self.recent.truncate(12);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_prefs_persist_and_default_when_missing() {
        // Preferences saved before the camera settings existed still load, with the defaults.
        let old: Prefs = serde_json::from_value(serde_json::json!({"uiScale": 1.25})).unwrap();
        assert_eq!(old.camera, CameraPrefs::default());
        assert_eq!(old.ui_scale, 1.25);
        let p = Prefs { camera: CameraPrefs { pan_limit: 8.0, pan_speed: 0.5 }, ..Prefs::default() };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["camera"], serde_json::json!({"panLimit": 8.0, "panSpeed": 0.5}));
        assert_eq!(serde_json::from_value::<Prefs>(v).unwrap(), p);
    }
}
