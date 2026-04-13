//! Desktop GUI for browsing Ymir-generated worlds.
//!
//! `ymir-gui` is the viewer layer at the top of the crate dependency graph. It
//! depends on every other `ymir-*` crate as a read-only producer and is not
//! depended on by any of them. The GUI opens a pre-generated world directory
//! (see [`storage::WorldDir`](ymir_storage)) and surfaces its contents through
//! three regions:
//!
//! 1. **Left sidebar** — the star browser (filled in by GUI-02).
//! 2. **Central area** — system view, globe view, and detail view
//!    (GUI-03/04/05), swapped via the current [`ViewMode`].
//! 3. **Right inspector** — provenance report and, later, the override editor
//!    (GUI-07/08), tabbed via [`InspectorTab`].
//!
//! The framework choice is `egui` via `eframe`. Rationale: small dependency
//! footprint, native + wasm out of the box, ergonomic for data-heavy panels,
//! trivial PNG embedding for pre-rendered Mollweide previews. A `wgpu` backend
//! can replace `glow` later when the Phase-6 3D globe view lands.
//!
//! # Scaffold status (GUI-01)
//!
//! This is the initial scaffold. It wires up the window, the three-region
//! layout, the top menu bar, and the panel traits, but the individual panels
//! are stubs. GUI-02..08 fill in the real widgets.
//!
//! # Extensibility points
//!
//! Downstream tasks should extend the GUI by:
//!
//! - Implementing [`panel::Panel`] for new widgets and registering them inside
//!   [`app::YmirApp`]'s `update` loop against the relevant layout region.
//! - Adding variants to [`view::ViewMode`] / [`view::RenderMode`] /
//!   [`inspector::InspectorTab`] rather than introducing ad-hoc booleans.
//! - Reading from [`app::AppState`] and writing mutations through the same
//!   struct — the app's single source of truth for selection, view mode, and
//!   the currently loaded world.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod app;
pub mod inspector;
pub mod panel;
pub mod view;
pub mod world;

pub use app::{AppState, YmirApp};
pub use inspector::{InspectorPanel, InspectorTab};
pub use panel::{Panel, PanelRegion};
pub use view::{RenderMode, ViewMode};
pub use world::{LoadedWorld, WorldLoadError};
