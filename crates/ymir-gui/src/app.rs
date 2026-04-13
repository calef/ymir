//! Top-level eframe application and shared app state.
//!
//! [`YmirApp`] is the root [`eframe::App`] implementation. It owns the three
//! panel slots (left sidebar, central, inspector) and a single [`AppState`]
//! value that panels read from and write to.
//!
//! The scaffold wires up the menu bar, the three-pane layout, and a stub
//! inspector. The left sidebar and central area are placeholder text until
//! GUI-02/03/04 fill them in.

use crate::inspector::InspectorPanel;
use crate::panel::Panel;
use crate::view::{RenderMode, ViewMode};
use crate::world::LoadedWorld;
use serde::{Deserialize, Serialize};

/// Shared, serde-persistent application state.
///
/// Panels read from and write to `AppState` rather than holding their own
/// selection state. This keeps "select a star in the left sidebar,
/// re-draw the central view, update the inspector" a single-frame
/// single-source-of-truth update.
///
/// # Persistence
///
/// `AppState` derives [`Serialize`] / [`Deserialize`] so eframe's
/// `persistence` feature can restore the last view mode, render mode, and
/// recently-opened world path between launches. Fields that are not safe to
/// persist (in-memory catalogs, textures) live on [`YmirApp`] instead, not
/// here.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppState {
    /// Which view is currently shown in the central region.
    pub view_mode: ViewMode,
    /// Which render channel the globe view uses.
    pub render_mode: RenderMode,
    /// Whether the source-confidence overlay is toggled on (GUI-06).
    pub confidence_overlay: bool,
    /// Last opened world directory, used to repopulate File → Recent.
    pub last_world_path: Option<std::path::PathBuf>,
}

/// Root application struct mounted into [`eframe::run_native`].
///
/// Non-serialisable handles (the loaded world, in-memory textures, the
/// inspector panel) live here; serialisable preferences live on
/// [`AppState`]. Panels take `&mut` borrows of both during `update`.
pub struct YmirApp {
    /// Persistent preferences and selection state.
    pub state: AppState,
    /// Currently open world, if any.
    pub world: Option<LoadedWorld>,
    /// Inspector panel (right sidebar). Always present.
    pub inspector: InspectorPanel,
}

impl Default for YmirApp {
    fn default() -> Self {
        Self {
            state: AppState::default(),
            world: None,
            inspector: InspectorPanel::new(),
        }
    }
}

impl YmirApp {
    /// Constructs the app, restoring state from eframe persistence if
    /// available.
    ///
    /// GUI-01 keeps restoration best-effort: a corrupt stored state resets
    /// to defaults rather than panicking.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let state = cc
            .storage
            .and_then(|s| eframe::get_value::<AppState>(s, eframe::APP_KEY))
            .unwrap_or_default();
        Self {
            state,
            world: None,
            inspector: InspectorPanel::new(),
        }
    }

    /// Renders the top menu bar: File, View, Help.
    fn menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("ymir_menu_bar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open World...").clicked() {
                        // GUI-01 leaves the picker unwired; GUI-02 hooks this
                        // up to `rfd` or a hand-rolled path input.
                        ui.close_kind(egui::UiKind::Menu);
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("View", |ui| {
                    ui.label("Render Mode");
                    for mode in RenderMode::all() {
                        if ui
                            .radio(self.state.render_mode == *mode, mode.label())
                            .clicked()
                        {
                            self.state.render_mode = *mode;
                        }
                    }
                    ui.separator();
                    ui.checkbox(&mut self.state.confidence_overlay, "Confidence overlay");
                });
                ui.menu_button("Help", |ui| {
                    ui.label(concat!("Ymir GUI v", env!("CARGO_PKG_VERSION")));
                    ui.label("Scaffold — GUI-01");
                });
            });
        });
    }

    /// Renders the left sidebar placeholder. GUI-02 replaces this body.
    fn left_sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("ymir_star_browser")
            .resizable(true)
            .default_width(260.0)
            .show(ctx, |ui| {
                ui.heading("Star Browser");
                ui.label("Star list lands in GUI-02.");
            });
    }

    /// Renders the right inspector.
    fn right_inspector(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("ymir_inspector")
            .resizable(true)
            .default_width(320.0)
            .show(ctx, |ui| {
                ui.heading(self.inspector.title());
                self.inspector.ui(ui);
            });
    }

    /// Renders the central-area view placeholder. GUI-03..05 replace this
    /// body per [`ViewMode`].
    fn central_area(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(self.state.view_mode.label());
            match self.state.view_mode {
                ViewMode::Empty => {
                    ui.label("Open a world via File → Open World to begin.");
                }
                ViewMode::System => {
                    ui.label("System view lands in GUI-03.");
                }
                ViewMode::Globe => {
                    ui.label("Globe view lands in GUI-04.");
                }
                ViewMode::Detail => {
                    ui.label("Detail view lands in GUI-05.");
                }
            }
            ui.separator();
            ui.label(format!(
                "Render mode: {} (confidence overlay: {})",
                self.state.render_mode.label(),
                if self.state.confidence_overlay {
                    "on"
                } else {
                    "off"
                }
            ));
            if let Some(world) = self.world.as_ref() {
                ui.label(format!("World: {}", world.path().display()));
            }
        });
    }
}

impl eframe::App for YmirApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.state);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.menu_bar(ctx);
        self.left_sidebar(ctx);
        self.right_inspector(ctx);
        self.central_area(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_starts_in_empty_view() {
        let state = AppState::default();
        assert_eq!(state.view_mode, ViewMode::Empty);
        assert_eq!(state.render_mode, RenderMode::Elevation);
        assert!(!state.confidence_overlay);
        assert!(state.last_world_path.is_none());
    }

    #[test]
    fn app_state_round_trips_through_json() {
        let state = AppState {
            view_mode: ViewMode::Globe,
            render_mode: RenderMode::Biome,
            confidence_overlay: true,
            ..Default::default()
        };
        let json = serde_json::to_string(&state).unwrap();
        let restored: AppState = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.view_mode, ViewMode::Globe);
        assert_eq!(restored.render_mode, RenderMode::Biome);
        assert!(restored.confidence_overlay);
    }
}
