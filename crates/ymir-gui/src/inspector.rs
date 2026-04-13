//! Right-sidebar inspector panel: tabbed view of the current selection.
//!
//! The inspector is always visible. Its content depends on what is selected in
//! the central area: a star, an orbital body, a tile, or a hex. Tabs within
//! the inspector surface different facets of that selection — raw fields,
//! provenance, override history.

use crate::panel::{Panel, PanelRegion};
use serde::{Deserialize, Serialize};
use ymir_catalog::StarSummary;

/// Inspector-panel tabs.
///
/// Later tasks add variants as new tabs become relevant. GUI-07 fills in
/// [`InspectorTab::Provenance`]; GUI-08 introduces [`InspectorTab::Override`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InspectorTab {
    /// Raw selected-object fields (spectral type, mass, elevation, etc.).
    Fields,
    /// Full provenance tree for the selected value (GUI-07).
    Provenance,
    /// Override editor for Derived / Assumed fields (GUI-08).
    Override,
}

impl Default for InspectorTab {
    fn default() -> Self {
        Self::Fields
    }
}

impl InspectorTab {
    /// Tab label for the inspector header.
    pub fn label(self) -> &'static str {
        match self {
            Self::Fields => "Fields",
            Self::Provenance => "Provenance",
            Self::Override => "Override",
        }
    }

    /// All inspector tabs in display order.
    pub fn all() -> &'static [Self] {
        &[Self::Fields, Self::Provenance, Self::Override]
    }
}

/// Scaffolding inspector panel. Renders a tab strip and a placeholder body.
///
/// GUI-07 swaps the body out for the real provenance tree; GUI-08 adds the
/// override form on the [`InspectorTab::Override`] tab.
///
/// GUI-02 populates the Fields tab with the currently selected star's
/// [`StarSummary`] when one is set via [`InspectorPanel::set_selected_star`].
#[derive(Debug, Default)]
pub struct InspectorPanel {
    current: InspectorTab,
    /// The selected star's summary, set by the star browser on row selection.
    /// `None` when no star is selected.
    selected_star: Option<StarSummary>,
}

impl InspectorPanel {
    /// Creates a new inspector defaulted to [`InspectorTab::Fields`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the currently active tab.
    pub fn current_tab(&self) -> InspectorTab {
        self.current
    }

    /// Switches to the given tab.
    pub fn set_tab(&mut self, tab: InspectorTab) {
        self.current = tab;
    }

    /// Sets (or clears) the selected star displayed in the Fields tab.
    ///
    /// Called each frame by [`crate::app::YmirApp`] after the star browser
    /// updates [`crate::app::AppState::selected_star_gaia_id`]. Passing `None`
    /// returns the inspector to its "no selection" placeholder.
    pub fn set_selected_star(&mut self, summary: Option<StarSummary>) {
        self.selected_star = summary;
    }

    /// Returns a reference to the currently selected star summary, if any.
    pub fn selected_star(&self) -> Option<&StarSummary> {
        self.selected_star.as_ref()
    }
}

impl InspectorPanel {
    /// Render the star fields table for the selected star.
    fn draw_star_fields(ui: &mut egui::Ui, star: &StarSummary) {
        let name = star.common_name.as_deref().unwrap_or("(no common name)");
        ui.heading(name);
        ui.separator();
        egui::Grid::new("star_fields_grid")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                ui.label("Gaia ID:");
                ui.monospace(star.gaia_id.to_string());
                ui.end_row();

                ui.label("Spectral type:");
                ui.label(format!("{}{}", star.spectral.class, star.spectral.subtype));
                ui.end_row();

                ui.label("Distance:");
                ui.label(format!("{:.3} pc", star.distance_pc));
                ui.end_row();

                ui.label("T_eff:");
                ui.label(format!("{:.0} K", star.teff_k));
                ui.end_row();

                ui.label("Luminosity:");
                ui.label(format!("{:.4} L\u{2609}", star.luminosity_sun));
                ui.end_row();

                ui.label("RA:");
                ui.label(format!("{:.6}°", star.ra_deg));
                ui.end_row();

                ui.label("Dec:");
                ui.label(format!("{:.6}°", star.dec_deg));
                ui.end_row();

                ui.label("HZ planet:");
                ui.label(if star.has_hz_planet { "yes" } else { "no" });
                ui.end_row();
            });
    }
}

impl Panel for InspectorPanel {
    fn region(&self) -> PanelRegion {
        PanelRegion::Inspector
    }

    fn title(&self) -> &str {
        "Inspector"
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for tab in InspectorTab::all() {
                let selected = *tab == self.current;
                if ui.selectable_label(selected, tab.label()).clicked() {
                    self.current = *tab;
                }
            }
        });
        ui.separator();
        match self.current {
            InspectorTab::Fields => {
                if let Some(star) = &self.selected_star {
                    Self::draw_star_fields(ui, star);
                } else {
                    ui.label("No selection. Pick a star from the browser on the left.");
                }
            }
            InspectorTab::Provenance => {
                ui.label("Provenance tree lands in GUI-07.");
            }
            InspectorTab::Override => {
                ui.label("Override editor lands in GUI-08.");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspector_defaults_to_fields_tab() {
        let panel = InspectorPanel::new();
        assert_eq!(panel.current_tab(), InspectorTab::Fields);
    }

    #[test]
    fn set_tab_updates_current() {
        let mut panel = InspectorPanel::new();
        panel.set_tab(InspectorTab::Provenance);
        assert_eq!(panel.current_tab(), InspectorTab::Provenance);
    }

    #[test]
    fn all_tabs_have_labels() {
        for tab in InspectorTab::all() {
            assert!(!tab.label().is_empty());
        }
    }
}
