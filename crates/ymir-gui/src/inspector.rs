//! Right-sidebar inspector panel: tabbed view of the current selection.
//!
//! The inspector is always visible. Its content depends on what is selected in
//! the central area: a star, an orbital body, a tile, or a hex. Tabs within
//! the inspector surface different facets of that selection — raw fields,
//! provenance, override history.

use crate::panel::{Panel, PanelRegion};
use serde::{Deserialize, Serialize};

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
#[derive(Debug, Default)]
pub struct InspectorPanel {
    current: InspectorTab,
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
                ui.label("No selection. Pick a star from the browser on the left.");
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
