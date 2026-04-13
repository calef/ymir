//! Left-sidebar star browser panel.
//!
//! Renders a searchable, filterable table over a [`ymir_catalog::Catalog`].
//! Supports text search by name or Gaia source ID, spectral-class toggle
//! buttons, a distance range slider, and a "confirmed HZ planet" checkbox.
//!
//! # Virtual scrolling
//!
//! Only visible rows are rendered. The panel computes how many rows fit in the
//! visible area (using a fixed `ROW_HEIGHT`) and offsets into the filtered
//! result set accordingly. This keeps frame cost proportional to the visible
//! row count rather than the total catalog size, making 500k-row catalogs
//! usable.
//!
//! # Filter evaluation
//!
//! [`BrowserFilter`] is the pure, `Clone`-able predicate state. Call
//! [`BrowserFilter::matches`] to test a single [`StarSummary`]. The panel
//! caches the filtered index into `filtered_rows` and only rebuilds it when
//! [`BrowserFilter`] has changed (tracked via the `filter_dirty` flag).
//!
//! # Selection API (for GUI-03+)
//!
//! Row click writes `AppState::selected_star_gaia_id`. Downstream panels read
//! that field to know which star is selected. Keyboard up/down arrow navigation
//! adjusts the selection within the filtered result set and also writes to
//! `AppState::selected_star_gaia_id`.

use egui::Key;

use crate::app::AppState;
use crate::panel::{Panel, PanelRegion};
use ymir_catalog::catalog_index::IndexedSpectralType;
use ymir_catalog::star_context::SpectralClass;
use ymir_catalog::{CatalogQuery, StarSummary};

/// Pixel height of a single table row. Increasing this gives more breathing
/// room; decreasing it lets more rows fit in the sidebar.
const ROW_HEIGHT: f32 = 22.0;

/// All Harvard spectral classes in temperature order (hot to cool).
const ALL_CLASSES: &[SpectralClass] = &[
    SpectralClass::O,
    SpectralClass::B,
    SpectralClass::A,
    SpectralClass::F,
    SpectralClass::G,
    SpectralClass::K,
    SpectralClass::M,
];

/// Maximum displayable distance in parsecs for the distance slider.
const MAX_DISTANCE_PC: f64 = 500.0;

/// Pure filter predicate for the star browser.
///
/// Holds all UI filter settings in a cheap-to-clone struct so the panel can
/// detect changes and know when to rebuild its cached filtered index.
///
/// # Text search
///
/// The text filter matches case-insensitively against the star's common name (if
/// any) or its Gaia source ID rendered as a decimal string. A blank filter
/// matches every row.
///
/// # Spectral class
///
/// `spectral_enabled` is a parallel array aligned with the Harvard class
/// sequence O, B, A, F, G, K, M. When every element is `true` (default) the
/// filter is treated as "any class" (inclusive). When at least one element is
/// `false` only the enabled classes pass.
#[derive(Debug, Clone, PartialEq)]
pub struct BrowserFilter {
    /// Lower-cased free-text search string.
    pub text: String,
    /// Per-class toggles in Harvard O-through-M order.
    pub spectral_enabled: [bool; 7],
    /// Minimum distance in parsecs (`0.0` = no lower bound).
    pub min_distance_pc: f64,
    /// Maximum distance in parsecs (`MAX_DISTANCE_PC` = no upper bound).
    pub max_distance_pc: f64,
    /// If `true`, only stars with a confirmed HZ planet are shown.
    pub hz_only: bool,
}

impl Default for BrowserFilter {
    fn default() -> Self {
        Self {
            text: String::new(),
            spectral_enabled: [true; 7],
            min_distance_pc: 0.0,
            max_distance_pc: MAX_DISTANCE_PC,
            hz_only: false,
        }
    }
}

impl BrowserFilter {
    /// Return `true` if `summary` satisfies every active filter predicate.
    ///
    /// This is the hot path when rebuilding the filtered index; it is called
    /// once per catalog row. Keep it allocation-free and branch-light.
    pub fn matches(&self, summary: &StarSummary) -> bool {
        // Distance bounds.
        if summary.distance_pc < self.min_distance_pc {
            return false;
        }
        if summary.distance_pc > self.max_distance_pc {
            return false;
        }

        // HZ host filter.
        if self.hz_only && !summary.has_hz_planet {
            return false;
        }

        // Spectral class filter. Only apply when not all classes are enabled.
        if !self.all_classes_enabled() {
            let idx = class_index(&summary.spectral.class);
            if !self.spectral_enabled[idx] {
                return false;
            }
        }

        // Text filter — skip the allocation when the field is empty.
        if !self.text.is_empty() {
            // Check common name first, then Gaia ID as a decimal string.
            let name_hit = summary
                .common_name
                .as_deref()
                .is_some_and(|n| n.to_ascii_lowercase().contains(&self.text));
            if !name_hit {
                let id_str = summary.gaia_id.to_string();
                if !id_str.contains(&self.text) {
                    return false;
                }
            }
        }

        true
    }

    /// Returns `true` when every spectral-class toggle is enabled (all-classes
    /// mode). The spectral filter is skipped in this case.
    pub fn all_classes_enabled(&self) -> bool {
        self.spectral_enabled.iter().all(|&b| b)
    }

    /// Returns the [`CatalogQuery`] that best represents the current filter
    /// state so the panel can ask the catalog for a pre-filtered list before
    /// running the in-panel predicate.
    ///
    /// The catalog query handles distance and single-class filtering cheaply;
    /// text search and multi-class filtering are handled in-panel after the
    /// catalog returns its narrowed list.
    pub fn to_catalog_query(&self) -> CatalogQuery {
        // Only supply a single spectral class to the catalog when exactly one
        // class is enabled. Multi-class filtering runs in-panel.
        let enabled_count = self.spectral_enabled.iter().filter(|&&b| b).count();
        let spectral = if enabled_count == 1 {
            let idx = self.spectral_enabled.iter().position(|&b| b).unwrap();
            Some(ALL_CLASSES[idx].clone())
        } else {
            None
        };

        CatalogQuery {
            spectral,
            min_distance_pc: if self.min_distance_pc > 0.0 {
                Some(self.min_distance_pc)
            } else {
                None
            },
            max_distance_pc: if self.max_distance_pc < MAX_DISTANCE_PC {
                Some(self.max_distance_pc)
            } else {
                None
            },
            hz_hosts_only: self.hz_only,
        }
    }
}

/// Return the index into [`ALL_CLASSES`] for the given class letter.
///
/// Panics in debug builds if a future spectral class variant is added without
/// updating this mapping; the compiler will catch it via the exhaustive match.
fn class_index(class: &SpectralClass) -> usize {
    match class {
        SpectralClass::O => 0,
        SpectralClass::B => 1,
        SpectralClass::A => 2,
        SpectralClass::F => 3,
        SpectralClass::G => 4,
        SpectralClass::K => 5,
        SpectralClass::M => 6,
    }
}

/// Format a [`StarSummary`] as a display name: prefer the common name,
/// fall back to `Gaia <id>`.
fn display_name(summary: &StarSummary) -> String {
    summary
        .common_name
        .clone()
        .unwrap_or_else(|| format!("Gaia {}", summary.gaia_id))
}

/// Format an [`IndexedSpectralType`] as a short string, e.g. `"G2"`.
fn spectral_label(s: &IndexedSpectralType) -> String {
    format!("{}{}", s.class, s.subtype)
}

/// Left-sidebar star browser panel.
///
/// Owns the filter widgets and the cached list of matching [`StarSummary`]
/// rows. The catalog itself lives on [`crate::app::YmirApp`] (not here) and is
/// passed in on each [`Panel::ui`] call via the dedicated
/// [`StarBrowserPanel::draw`] method.
///
/// GUI-03 and later tasks read the selected star by inspecting
/// `AppState::selected_star_gaia_id`.
pub struct StarBrowserPanel {
    /// Raw text typed into the search box (pre-lowercasing).
    search_text: String,
    /// Current filter predicate. When it differs from `last_filter`, the panel
    /// rebuilds `filtered_rows`.
    filter: BrowserFilter,
    /// Snapshot of the filter used to build `filtered_rows`. Used to detect
    /// when a rebuild is needed.
    last_filter: BrowserFilter,
    /// All rows that satisfy the current filter, in catalog order.
    filtered_rows: Vec<StarSummary>,
    /// Whether `filtered_rows` needs to be rebuilt this frame.
    filter_dirty: bool,
    /// Index into `filtered_rows` of the keyboard-focused row, if any.
    keyboard_cursor: Option<usize>,
}

impl Default for StarBrowserPanel {
    fn default() -> Self {
        Self {
            search_text: String::new(),
            filter: BrowserFilter::default(),
            last_filter: BrowserFilter::default(),
            filtered_rows: Vec::new(),
            filter_dirty: true, // Force initial population on first draw.
            keyboard_cursor: None,
        }
    }
}

impl StarBrowserPanel {
    /// Create a new, empty star browser panel.
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the catalog query representing the current filter state.
    ///
    /// The caller (typically [`crate::app::YmirApp`]) uses this to fetch
    /// pre-filtered candidates from the catalog *before* calling [`Self::draw`],
    /// avoiding a double-borrow of `self` inside a closure.
    pub fn current_query(&self) -> ymir_catalog::CatalogQuery {
        self.filter.to_catalog_query()
    }

    /// Return `true` when the filter has changed since the last call to
    /// [`Self::draw`] with candidates. The caller can use this to decide
    /// whether to re-query the catalog.
    pub fn filter_changed(&self) -> bool {
        self.filter_dirty || self.filter != self.last_filter
    }

    /// Draw the browser, taking a mutable borrow of `AppState` for selection
    /// write-back and a pre-fetched candidate list from the caller.
    ///
    /// # Candidate list
    ///
    /// `candidates` is `Some(rows)` when the caller has re-queried the catalog
    /// because [`Self::filter_changed`] returned `true`. The panel applies its
    /// remaining in-panel predicates (multi-class, text) and caches the result
    /// in `filtered_rows`.
    ///
    /// When `candidates` is `None` the panel either:
    /// - Keeps its cached `filtered_rows` if the filter is unchanged (normal
    ///   per-frame call with no catalog change).
    /// - Clears `filtered_rows` when the panel is first drawn without a
    ///   catalog (the initial empty state).
    ///
    /// Separating the catalog query from this call lets [`crate::app::YmirApp`]
    /// borrow `self.catalog` and `self.star_browser` in sequence rather than
    /// simultaneously, working around the Rust borrow checker.
    pub fn draw(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut AppState,
        candidates: Option<Vec<StarSummary>>,
    ) {
        // --- Filter widgets (may set filter_dirty) ---
        self.draw_filter_bar(ui);

        // --- Rebuild if new candidates were supplied ---
        if let Some(rows) = candidates {
            self.filtered_rows = rows
                .into_iter()
                .filter(|s| self.filter.matches(s))
                .collect();
            self.last_filter = self.filter.clone();
            self.filter_dirty = false;
            // Clamp keyboard cursor to the new result set.
            if let Some(k) = self.keyboard_cursor {
                if k >= self.filtered_rows.len() {
                    self.keyboard_cursor = if self.filtered_rows.is_empty() {
                        None
                    } else {
                        Some(self.filtered_rows.len() - 1)
                    };
                }
            }
        }

        ui.separator();
        ui.label(format!("{} stars shown", self.filtered_rows.len()));
        ui.separator();

        // --- Keyboard navigation ---
        self.handle_keyboard(ui, state);

        // --- Virtualized row list ---
        self.draw_rows(ui, state);
    }

    /// Draw the filter bar (search box, spectral toggles, distance slider, HZ
    /// checkbox). Mutates `self.filter` and marks `filter_dirty` on any change.
    fn draw_filter_bar(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;

        // Text search.
        ui.label("Search:");
        let resp = ui.text_edit_singleline(&mut self.search_text);
        if resp.changed() {
            self.filter.text = self.search_text.to_ascii_lowercase();
            changed = true;
        }

        ui.add_space(4.0);

        // Spectral class toggles.
        ui.label("Spectral class:");
        ui.horizontal_wrapped(|ui| {
            for (i, class) in ALL_CLASSES.iter().enumerate() {
                let label = format!("{class}");
                let active = self.filter.spectral_enabled[i];
                let btn = egui::Button::new(&label)
                    .selected(active)
                    .min_size(egui::vec2(24.0, 20.0));
                if ui.add(btn).clicked() {
                    self.filter.spectral_enabled[i] = !active;
                    changed = true;
                }
            }
            // "All" reset button.
            if ui.small_button("All").clicked() {
                self.filter.spectral_enabled = [true; 7];
                changed = true;
            }
        });

        ui.add_space(4.0);

        // Distance slider.
        ui.label("Max distance (pc):");
        let mut max_dist = self.filter.max_distance_pc as f32;
        if ui
            .add(egui::Slider::new(&mut max_dist, 0.0..=(MAX_DISTANCE_PC as f32)).suffix(" pc"))
            .changed()
        {
            self.filter.max_distance_pc = max_dist as f64;
            changed = true;
        }

        ui.add_space(2.0);

        // HZ checkbox.
        let mut hz = self.filter.hz_only;
        if ui.checkbox(&mut hz, "Confirmed HZ planet only").changed() {
            self.filter.hz_only = hz;
            changed = true;
        }

        if changed {
            self.filter_dirty = true;
        }
    }

    /// Handle up/down arrow key navigation within the filtered list.
    fn handle_keyboard(&mut self, ui: &mut egui::Ui, state: &mut AppState) {
        if self.filtered_rows.is_empty() {
            return;
        }

        let down = ui.input(|i| i.key_pressed(Key::ArrowDown));
        let up = ui.input(|i| i.key_pressed(Key::ArrowUp));

        if down || up {
            let cursor = self.keyboard_cursor.unwrap_or(0);
            let new_cursor = if down {
                (cursor + 1).min(self.filtered_rows.len() - 1)
            } else {
                cursor.saturating_sub(1)
            };
            self.keyboard_cursor = Some(new_cursor);
            state.selected_star_gaia_id = Some(self.filtered_rows[new_cursor].gaia_id);
        }
    }

    /// Draw the virtualized row list using a fixed-height scroll area.
    fn draw_rows(&mut self, ui: &mut egui::Ui, state: &mut AppState) {
        let total_rows = self.filtered_rows.len();
        if total_rows == 0 {
            ui.label("No stars match the current filters.");
            return;
        }

        let total_height = total_rows as f32 * ROW_HEIGHT;

        // Column header.
        ui.horizontal(|ui| {
            ui.set_min_width(ui.available_width());
            ui.monospace(format!(
                "{:<22} {:>4} {:>7} {:>7}",
                "Name", "Spc", "Dist pc", "T_eff K"
            ));
        });

        egui::ScrollArea::vertical()
            .id_salt("star_browser_scroll")
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                // Allocate the full virtual height so the scrollbar is correct.
                ui.set_height(total_height);

                // Determine the range of rows in the visible viewport.
                let first_visible = (viewport.min.y / ROW_HEIGHT).floor() as usize;
                let last_visible =
                    ((viewport.max.y / ROW_HEIGHT).ceil() as usize + 1).min(total_rows);

                // Spacer above visible rows.
                let top_offset = first_visible as f32 * ROW_HEIGHT;
                ui.add_space(top_offset);

                for row_idx in first_visible..last_visible {
                    let summary = &self.filtered_rows[row_idx];
                    let is_selected = state.selected_star_gaia_id == Some(summary.gaia_id);
                    let is_cursor = self.keyboard_cursor == Some(row_idx);

                    let name = display_name(summary);
                    let spc = spectral_label(&summary.spectral);
                    let dist = format!("{:.1}", summary.distance_pc);
                    let teff = format!("{:.0}", summary.teff_k);
                    let planet_badge = if summary.has_hz_planet { " *" } else { "" };

                    let label_text = format!(
                        "{:<20}{} {:>4} {:>7} {:>7}",
                        // Truncate long names so the row stays on one line.
                        if name.len() > 20 {
                            format!("{:.17}...", &name[..17])
                        } else {
                            name.clone()
                        },
                        planet_badge,
                        spc,
                        dist,
                        teff,
                    );

                    let highlight = is_selected || is_cursor;
                    let resp = ui.selectable_label(highlight, &label_text);
                    if resp.clicked() {
                        state.selected_star_gaia_id = Some(summary.gaia_id);
                        // Sync keyboard cursor to the clicked row.
                        self.keyboard_cursor = Some(row_idx);
                    }
                    if resp.hovered() {
                        resp.on_hover_text(format!(
                            "{}\nGaia ID: {}\nRA: {:.4}°  Dec: {:.4}°\n{}{} · {:.1} pc · {:.0} K",
                            name,
                            summary.gaia_id,
                            summary.ra_deg,
                            summary.dec_deg,
                            spc,
                            if summary.has_hz_planet {
                                " · HZ planet"
                            } else {
                                ""
                            },
                            summary.distance_pc,
                            summary.teff_k,
                        ));
                    }
                }
            });
    }
}

impl Panel for StarBrowserPanel {
    fn region(&self) -> PanelRegion {
        PanelRegion::LeftSidebar
    }

    fn title(&self) -> &str {
        "Star Browser"
    }

    fn ui(&mut self, _ui: &mut egui::Ui) {
        // The Panel trait's `ui` method is not used directly; callers should
        // use `draw(ui, state, catalog_rows)` instead to pass the app state
        // and catalog accessor. This stub satisfies the trait for object-safe
        // boxing if needed by future infrastructure.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ymir_catalog::catalog_index::IndexedSpectralType;

    fn make_summary(
        gaia_id: u64,
        common_name: Option<&str>,
        distance_pc: f64,
        teff_k: f64,
        class: SpectralClass,
        has_hz_planet: bool,
    ) -> StarSummary {
        StarSummary {
            gaia_id,
            common_name: common_name.map(str::to_string),
            ra_deg: 0.0,
            dec_deg: 0.0,
            distance_pc,
            teff_k,
            luminosity_sun: 1.0,
            spectral: IndexedSpectralType { class, subtype: 2 },
            has_hz_planet,
        }
    }

    #[test]
    fn default_filter_matches_everything() {
        let f = BrowserFilter::default();
        let s = make_summary(1, Some("Test Star"), 10.0, 5778.0, SpectralClass::G, false);
        assert!(f.matches(&s));
    }

    #[test]
    fn text_filter_matches_common_name_case_insensitively() {
        let f = BrowserFilter {
            text: "proxima".to_string(),
            ..Default::default()
        };
        let hit = make_summary(
            1,
            Some("Proxima Centauri"),
            1.3,
            3000.0,
            SpectralClass::M,
            false,
        );
        let miss = make_summary(
            2,
            Some("Alpha Centauri"),
            1.3,
            5800.0,
            SpectralClass::G,
            false,
        );
        assert!(f.matches(&hit));
        assert!(!f.matches(&miss));
    }

    #[test]
    fn text_filter_falls_through_to_gaia_id() {
        let f = BrowserFilter {
            text: "5853498713190525696".to_string(),
            ..Default::default()
        };
        let hit = make_summary(
            5853498713190525696,
            None,
            1.3,
            3000.0,
            SpectralClass::M,
            false,
        );
        let miss = make_summary(1234567890, None, 1.3, 3000.0, SpectralClass::M, false);
        assert!(f.matches(&hit));
        assert!(!f.matches(&miss));
    }

    #[test]
    fn spectral_filter_excludes_disabled_classes() {
        // Disable all except G.
        let mut enabled = [false; 7];
        enabled[class_index(&SpectralClass::G)] = true;
        let f = BrowserFilter {
            spectral_enabled: enabled,
            ..Default::default()
        };

        let g_star = make_summary(1, None, 10.0, 5778.0, SpectralClass::G, false);
        let m_star = make_summary(2, None, 10.0, 3000.0, SpectralClass::M, false);
        assert!(f.matches(&g_star));
        assert!(!f.matches(&m_star));
    }

    #[test]
    fn all_classes_enabled_returns_true_by_default() {
        let f = BrowserFilter::default();
        assert!(f.all_classes_enabled());
    }

    #[test]
    fn all_classes_enabled_returns_false_when_one_disabled() {
        let mut enabled = [true; 7];
        enabled[0] = false;
        let f = BrowserFilter {
            spectral_enabled: enabled,
            ..Default::default()
        };
        assert!(!f.all_classes_enabled());
    }

    #[test]
    fn distance_filter_excludes_out_of_range() {
        let f = BrowserFilter {
            max_distance_pc: 10.0,
            ..Default::default()
        };
        let near = make_summary(1, None, 5.0, 5778.0, SpectralClass::G, false);
        let far = make_summary(2, None, 15.0, 5778.0, SpectralClass::G, false);
        assert!(f.matches(&near));
        assert!(!f.matches(&far));
    }

    #[test]
    fn min_distance_filter_excludes_too_close() {
        let f = BrowserFilter {
            min_distance_pc: 5.0,
            ..Default::default()
        };
        let near = make_summary(1, None, 2.0, 5778.0, SpectralClass::G, false);
        let far = make_summary(2, None, 10.0, 5778.0, SpectralClass::G, false);
        assert!(!f.matches(&near));
        assert!(f.matches(&far));
    }

    #[test]
    fn hz_only_filter_excludes_non_hz_stars() {
        let f = BrowserFilter {
            hz_only: true,
            ..Default::default()
        };
        let hz = make_summary(1, None, 5.0, 5778.0, SpectralClass::G, true);
        let no_hz = make_summary(2, None, 5.0, 5778.0, SpectralClass::G, false);
        assert!(f.matches(&hz));
        assert!(!f.matches(&no_hz));
    }

    #[test]
    fn to_catalog_query_single_class_passes_spectral() {
        let mut enabled = [false; 7];
        enabled[class_index(&SpectralClass::G)] = true;
        let f = BrowserFilter {
            spectral_enabled: enabled,
            ..Default::default()
        };
        let q = f.to_catalog_query();
        assert!(matches!(q.spectral, Some(SpectralClass::G)));
    }

    #[test]
    fn to_catalog_query_multi_class_omits_spectral() {
        // G and K both enabled — can't express in a single catalog query field.
        let mut enabled = [false; 7];
        enabled[class_index(&SpectralClass::G)] = true;
        enabled[class_index(&SpectralClass::K)] = true;
        let f = BrowserFilter {
            spectral_enabled: enabled,
            ..Default::default()
        };
        let q = f.to_catalog_query();
        assert!(q.spectral.is_none());
    }

    #[test]
    fn to_catalog_query_distance_bounds() {
        let f = BrowserFilter {
            min_distance_pc: 2.0,
            max_distance_pc: 50.0,
            ..Default::default()
        };
        let q = f.to_catalog_query();
        assert_eq!(q.min_distance_pc, Some(2.0));
        assert_eq!(q.max_distance_pc, Some(50.0));
    }

    #[test]
    fn to_catalog_query_default_omits_bounds() {
        let f = BrowserFilter::default();
        let q = f.to_catalog_query();
        assert!(q.min_distance_pc.is_none());
        assert!(q.max_distance_pc.is_none());
        assert!(q.spectral.is_none());
        assert!(!q.hz_hosts_only);
    }

    #[test]
    fn class_index_covers_all_variants() {
        for class in ALL_CLASSES {
            let idx = class_index(class);
            assert!(idx < 7, "class_index out of range for {class}");
            assert_eq!(&ALL_CLASSES[idx], class);
        }
    }

    #[test]
    fn display_name_prefers_common_name() {
        let s = make_summary(
            42,
            Some("Alpha Centauri"),
            1.3,
            5800.0,
            SpectralClass::G,
            false,
        );
        assert_eq!(display_name(&s), "Alpha Centauri");
    }

    #[test]
    fn display_name_falls_back_to_gaia_id() {
        let s = make_summary(9999, None, 5.0, 5000.0, SpectralClass::K, false);
        assert_eq!(display_name(&s), "Gaia 9999");
    }

    #[test]
    fn spectral_label_formats_correctly() {
        let ist = IndexedSpectralType {
            class: SpectralClass::G,
            subtype: 2,
        };
        assert_eq!(spectral_label(&ist), "G2");
    }

    #[test]
    fn browser_panel_implements_panel_trait() {
        let panel = StarBrowserPanel::new();
        assert_eq!(panel.region(), PanelRegion::LeftSidebar);
        assert_eq!(panel.title(), "Star Browser");
    }
}
