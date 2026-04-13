//! Panel trait and layout regions.
//!
//! Every widget that claims a region of the main window implements [`Panel`].
//! The trait stays deliberately small: a region identifier, a human-readable
//! title, and an egui render hook. Downstream tasks (GUI-02..08) implement
//! [`Panel`] for their widgets and mount them from
//! [`crate::app::YmirApp`]'s `update` loop.

/// Which region of the three-pane layout a panel occupies.
///
/// The app mounts at most one panel per region at a time. [`PanelRegion`] is
/// [`Copy`] so the app can dispatch on it without borrowing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PanelRegion {
    /// Left sidebar (star browser in GUI-02).
    LeftSidebar,
    /// Central area (system / globe / detail views, GUI-03..05).
    Central,
    /// Right sidebar (inspector, GUI-07/08).
    Inspector,
}

/// A mountable panel.
///
/// Panels own their own UI state. They must not own application state that
/// other panels need to read — put that in [`crate::app::AppState`] instead.
/// The `ui` method is called every frame from
/// [`eframe::App::update`].
pub trait Panel {
    /// The layout region this panel occupies.
    fn region(&self) -> PanelRegion;

    /// A short human-readable title, shown in headers and debug inspectors.
    fn title(&self) -> &str;

    /// Draws the panel into the given egui UI.
    ///
    /// The panel is responsible for its own scrolling, spacing, and internal
    /// layout. The app chrome provides only the outer frame.
    fn ui(&mut self, ui: &mut egui::Ui);
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Stub;
    impl Panel for Stub {
        fn region(&self) -> PanelRegion {
            PanelRegion::Central
        }
        fn title(&self) -> &str {
            "stub"
        }
        fn ui(&mut self, _ui: &mut egui::Ui) {}
    }

    #[test]
    fn panel_trait_is_object_safe() {
        let _boxed: Box<dyn Panel> = Box::new(Stub);
    }

    #[test]
    fn panel_region_is_copy() {
        fn takes_copy<T: Copy>(_: T) {}
        takes_copy(PanelRegion::LeftSidebar);
    }
}
