//! Provenance metadata types that record whether each value was derived,
//! observed, or overridden, and from which pipeline stage.
//!
//! The main entry point is [`ProvenanceReport`], which is written to
//! `<world>/provenance.json` after each generation or regeneration run.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

use crate::{Source, Sourced};

// ---------------------------------------------------------------------------
// Per-stage counts
// ---------------------------------------------------------------------------

/// Source counts for a single pipeline stage.
///
/// Records how many fields within the stage carry each [`Source`] variant.
/// Useful for quick health checks: a fully-observed stage should show zero
/// `derived` counts, while a freshly-simulated stage shows zero `observed`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StageCounts {
    /// Fields tagged [`Source::Observed`].
    pub observed: usize,
    /// Fields tagged [`Source::Derived`].
    pub derived: usize,
    /// Fields tagged [`Source::Assumed`].
    pub assumed: usize,
}

impl StageCounts {
    /// Tally a single [`Source`] variant.
    pub fn tally(&mut self, source: &Source) {
        match source {
            Source::Observed { .. } => self.observed += 1,
            Source::Derived { .. } => self.derived += 1,
            Source::Assumed { .. } => self.assumed += 1,
        }
    }

    /// Add counts from another [`StageCounts`] into `self`.
    pub fn merge(&mut self, other: &StageCounts) {
        self.observed += other.observed;
        self.derived += other.derived;
        self.assumed += other.assumed;
    }

    /// Total number of tracked fields.
    pub fn total(&self) -> usize {
        self.observed + self.derived + self.assumed
    }
}

// ---------------------------------------------------------------------------
// Per-stage provenance node
// ---------------------------------------------------------------------------

/// Provenance data for a single pipeline stage.
///
/// Contains both the source-count histogram and the full nested
/// `{ value, source }` field tree for the stage's struct.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StageProvenance {
    /// Histogram of field-level source counts for this stage.
    pub counts: StageCounts,
    /// Full field tree: a nested JSON object where every `Sourced<T>` field
    /// appears as `{ "value": ..., "source": ... }`.
    ///
    /// For aggregate stages (skeleton, climate, biome) that work on large
    /// tile arrays this is a compact summary node rather than the full array.
    pub fields: Value,
}

// ---------------------------------------------------------------------------
// Full provenance report
// ---------------------------------------------------------------------------

/// Per-world provenance report, written to `<world>/provenance.json`.
///
/// The top-level object maps stage names to their [`StageProvenance`].
/// Stages are ordered by the pipeline execution sequence:
/// `stellar`, `orbital_body`, `atmosphere`, `skeleton`, `climate`, `biome`.
///
/// # JSON shape
///
/// ```json
/// {
///   "stellar": {
///     "counts": { "observed": 7, "derived": 5, "assumed": 0 },
///     "fields": { "effective_temp": { "value": 5778.0, "source": { "Observed": { ... } } }, ... }
///   },
///   "orbital_body": { ... },
///   "atmosphere": { ... },
///   "skeleton": {
///     "counts": { "observed": 0, "derived": 1, "assumed": 0 },
///     "fields": { "source": { "Derived": { "from_stage": "skeleton" } } }
///   },
///   "climate": { ... },
///   "biome": { ... }
/// }
/// ```
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProvenanceReport {
    /// Ordered map from stage name to its provenance data.
    ///
    /// Uses [`BTreeMap`] for stable JSON key ordering.
    pub stages: BTreeMap<String, StageProvenance>,
}

impl ProvenanceReport {
    /// Create an empty report.
    pub fn new() -> Self {
        Self::default()
    }

    /// Walk all `Sourced<T>` leaves in a JSON value and tally their sources.
    fn tally_json(v: &Value, counts: &mut StageCounts) {
        match v {
            Value::Object(map) => {
                if map.len() == 2 && map.contains_key("value") && map.contains_key("source") {
                    // This looks like a serialized `Sourced<T>`. Parse the source tag.
                    if let Some(source_val) = map.get("source") {
                        if let Ok(source) = serde_json::from_value::<Source>(source_val.clone()) {
                            counts.tally(&source);
                            return; // Don't recurse into the value/source sub-tree.
                        }
                    }
                }
                // Not a Sourced leaf: recurse into object values.
                for v in map.values() {
                    Self::tally_json(v, counts);
                }
            }
            Value::Array(arr) => {
                for v in arr {
                    Self::tally_json(v, counts);
                }
            }
            _ => {}
        }
    }

    /// Add a stage whose output struct uses per-field [`Sourced<T>`] wrapping.
    ///
    /// Serializes `output` as JSON, then walks every `{ value, source }` leaf
    /// to build the count histogram. The full serialized tree is stored in
    /// `fields` so `ymir provenance --full` can display it.
    ///
    /// # Errors
    ///
    /// Returns `Err(String)` if `output` cannot be serialized to JSON.
    pub fn add_perfield_stage<T>(
        &mut self,
        stage_name: impl Into<String>,
        output: &T,
    ) -> Result<(), String>
    where
        T: Serialize,
    {
        let name = stage_name.into();
        let fields = serde_json::to_value(output)
            .map_err(|e| format!("provenance serialize {name}: {e}"))?;

        let mut counts = StageCounts::default();
        Self::tally_json(&fields, &mut counts);

        self.stages.insert(name, StageProvenance { counts, fields });
        Ok(())
    }

    /// Add a stage that uses a single aggregate [`Sourced<T>`] tag for its
    /// entire output (e.g., skeleton, climate, biome).
    ///
    /// Emits a compact summary node instead of a per-tile field dump.
    pub fn add_aggregate_stage(
        &mut self,
        stage_name: impl Into<String>,
        aggregate_source: &Source,
    ) {
        let name = stage_name.into();
        let mut counts = StageCounts::default();
        counts.tally(aggregate_source);

        let fields = serde_json::json!({
            "source": serde_json::to_value(aggregate_source)
                .unwrap_or(Value::Null)
        });

        self.stages.insert(name, StageProvenance { counts, fields });
    }

    /// Add a stage sourced from a [`Sourced<T>`] wrapper directly.
    ///
    /// Convenience wrapper around [`Self::add_aggregate_stage`].
    pub fn add_sourced_stage<T: Clone + std::fmt::Debug>(
        &mut self,
        stage_name: impl Into<String>,
        sourced: &Sourced<T>,
    ) {
        self.add_aggregate_stage(stage_name, sourced.source());
    }

    /// Pretty-print the full JSON report to a [`String`].
    ///
    /// # Errors
    ///
    /// Returns `Err(String)` if serialization fails.
    pub fn to_pretty_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| format!("provenance to_json: {e}"))
    }

    /// Print a per-stage count histogram to stdout.
    ///
    /// One line per stage, format:
    /// `  stellar: 12 Observed / 9 Derived / 0 Assumed`
    pub fn print_summary(&self) {
        // Print in a stable order that mirrors pipeline execution sequence.
        let pipeline_order = [
            "stellar",
            "orbital_body",
            "atmosphere",
            "skeleton",
            "climate",
            "biome",
        ];

        let mut printed: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();

        for &stage in &pipeline_order {
            if let Some(sp) = self.stages.get(stage) {
                let c = &sp.counts;
                println!(
                    "  {stage}: {} Observed / {} Derived / {} Assumed",
                    c.observed, c.derived, c.assumed
                );
                printed.insert(stage);
            }
        }

        // Any stages not in the canonical order (shouldn't happen in Phase 1).
        for (name, sp) in &self.stages {
            if !printed.contains(name.as_str()) {
                let c = &sp.counts;
                println!(
                    "  {name}: {} Observed / {} Derived / {} Assumed",
                    c.observed, c.derived, c.assumed
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Sourced;

    #[test]
    fn stage_counts_tally_and_merge() {
        let mut a = StageCounts::default();
        a.tally(&Source::Observed {
            reference: "ref".into(),
            instrument: "inst".into(),
            date: "".into(),
            uncertainty: None,
        });
        a.tally(&Source::Derived {
            from_stage: "foo".into(),
        });
        assert_eq!(a.observed, 1);
        assert_eq!(a.derived, 1);
        assert_eq!(a.assumed, 0);
        assert_eq!(a.total(), 2);

        let mut b = StageCounts::default();
        b.tally(&Source::Assumed {
            reason: "test".into(),
        });

        a.merge(&b);
        assert_eq!(a.total(), 3);
    }

    #[test]
    fn perfield_stage_counts_sourced_leaves() {
        #[derive(Serialize)]
        struct Fake {
            a: Sourced<f64>,
            b: Sourced<f64>,
            c: String, // not Sourced — should not count
        }

        let fake = Fake {
            a: Sourced::observed(1.0, "ref", "inst"),
            b: Sourced::derived(2.0, "stage"),
            c: "plain".into(),
        };

        let mut report = ProvenanceReport::new();
        report.add_perfield_stage("test_stage", &fake).unwrap();

        let sp = report.stages.get("test_stage").unwrap();
        assert_eq!(sp.counts.observed, 1);
        assert_eq!(sp.counts.derived, 1);
        assert_eq!(sp.counts.assumed, 0);
    }

    #[test]
    fn aggregate_stage_adds_summary_node() {
        let source = Source::Derived {
            from_stage: "skeleton".into(),
        };
        let mut report = ProvenanceReport::new();
        report.add_aggregate_stage("skeleton", &source);

        let sp = report.stages.get("skeleton").unwrap();
        assert_eq!(sp.counts.derived, 1);
        assert_eq!(sp.counts.observed, 0);

        // Fields object must have a "source" key.
        assert!(sp.fields.get("source").is_some());
    }

    #[test]
    fn report_round_trips_json() {
        let source = Source::Assumed {
            reason: "fallback".into(),
        };
        let mut report = ProvenanceReport::new();
        report.add_aggregate_stage("biome", &source);

        let json = report.to_pretty_json().unwrap();
        let restored: ProvenanceReport = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.stages.get("biome").unwrap().counts.assumed, 1);
    }
}
