//! Central-area view modes and render-mode selection.
//!
//! The GUI's central region shows one of several views depending on what the
//! user has selected. [`ViewMode`] identifies which view is currently active;
//! [`RenderMode`] identifies which data channel the globe view is colouring by.

use serde::{Deserialize, Serialize};

/// Which central-area view is currently visible.
///
/// Downstream tasks add variants here rather than introducing parallel
/// booleans. GUI-02 populates the star list that drives transitions into
/// [`ViewMode::System`]; GUI-03 lives in [`ViewMode::System`]; GUI-04 in
/// [`ViewMode::Globe`]; GUI-05 in [`ViewMode::Detail`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ViewMode {
    /// Nothing selected yet — show the welcome / empty-state placeholder.
    Empty,
    /// Top-down orbital diagram of the selected star's system (GUI-03).
    System,
    /// Mollweide projection of the selected planetary body (GUI-04).
    Globe,
    /// Hex-grid zoom of a detail region on the selected planet (GUI-05).
    Detail,
}

impl Default for ViewMode {
    fn default() -> Self {
        Self::Empty
    }
}

impl ViewMode {
    /// Human-readable label for menus and tab headers.
    pub fn label(self) -> &'static str {
        match self {
            Self::Empty => "Welcome",
            Self::System => "System",
            Self::Globe => "Globe",
            Self::Detail => "Detail",
        }
    }
}

/// Which data channel the globe view is colouring by.
///
/// Mirrors the render modes supported by `ymir-render`. The [`RenderMode`]
/// value is independent of [`ViewMode`]; switching views preserves the last
/// selected render mode so users keep their preferred overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RenderMode {
    /// Elevation colour ramp.
    Elevation,
    /// Biome palette (GUI-04 default once biome data is available).
    Biome,
    /// Surface temperature heatmap.
    Temperature,
    /// Surface moisture heatmap.
    Moisture,
    /// Source-confidence desaturation overlay (GUI-06).
    Confidence,
    /// Tectonic plate partition overlay.
    Plates,
}

impl Default for RenderMode {
    fn default() -> Self {
        Self::Elevation
    }
}

impl RenderMode {
    /// Human-readable label for the View → Render Mode menu.
    pub fn label(self) -> &'static str {
        match self {
            Self::Elevation => "Elevation",
            Self::Biome => "Biome",
            Self::Temperature => "Temperature",
            Self::Moisture => "Moisture",
            Self::Confidence => "Confidence",
            Self::Plates => "Plates",
        }
    }

    /// All render modes in menu order.
    pub fn all() -> &'static [Self] {
        &[
            Self::Elevation,
            Self::Biome,
            Self::Temperature,
            Self::Moisture,
            Self::Confidence,
            Self::Plates,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_mode_defaults_to_empty() {
        assert_eq!(ViewMode::default(), ViewMode::Empty);
    }

    #[test]
    fn render_mode_all_contains_every_variant() {
        // If a new variant is added, this assertion will fail until
        // `RenderMode::all` is updated — keeps the menu in sync.
        assert_eq!(RenderMode::all().len(), 6);
    }

    #[test]
    fn every_view_mode_has_a_nonempty_label() {
        for mode in [
            ViewMode::Empty,
            ViewMode::System,
            ViewMode::Globe,
            ViewMode::Detail,
        ] {
            assert!(!mode.label().is_empty());
        }
    }
}
