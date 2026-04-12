//! Dependency graph for pipeline stages, ensuring correct recomputation order
//! when overrides are applied.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// The seven stages of the Ymir causal pipeline, ordered from stellar context
/// down to regional surface detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Stage {
    StellarContext = 1,
    PlanetarySystem = 2,
    Atmosphere = 3,
    Skeleton = 4,
    Climate = 5,
    Biome = 6,
    RegionalDetail = 7,
}

static ALL_STAGES: [Stage; 7] = [
    Stage::StellarContext,
    Stage::PlanetarySystem,
    Stage::Atmosphere,
    Stage::Skeleton,
    Stage::Climate,
    Stage::Biome,
    Stage::RegionalDetail,
];

impl Stage {
    /// Human-readable name for this pipeline stage.
    pub fn name(&self) -> &'static str {
        match self {
            Stage::StellarContext => "Stellar Context",
            Stage::PlanetarySystem => "Planetary System",
            Stage::Atmosphere => "Atmosphere",
            Stage::Skeleton => "Skeleton",
            Stage::Climate => "Climate",
            Stage::Biome => "Biome",
            Stage::RegionalDetail => "Regional Detail",
        }
    }

    /// 1-based index of this stage in the pipeline.
    pub fn index(&self) -> usize {
        *self as usize
    }

    /// All stages in pipeline order.
    pub fn all() -> &'static [Stage] {
        &ALL_STAGES
    }

    /// Returns all stages at or after the given stage (inclusive).
    pub fn downstream_of(stage: Stage) -> Vec<Stage> {
        ALL_STAGES
            .iter()
            .filter(|s| s.index() >= stage.index())
            .copied()
            .collect()
    }
}

/// Tracks which pipeline stages need recomputation.
///
/// When an override is applied at stage N, that stage and all downstream stages
/// (N through 7) are marked dirty. Stages upstream of N remain unaffected.
pub struct PipelineDirtyState {
    dirty: HashSet<Stage>,
}

impl PipelineDirtyState {
    /// Creates a new dirty state with all stages marked dirty (fresh generation).
    pub fn new() -> Self {
        Self {
            dirty: ALL_STAGES.iter().copied().collect(),
        }
    }

    /// Creates a dirty state with no stages dirty (fully computed pipeline).
    pub fn clean() -> Self {
        Self {
            dirty: HashSet::new(),
        }
    }

    /// Marks the given stage and all downstream stages as dirty.
    /// This is the correct response when an override is applied at `stage`.
    pub fn mark_override_at(&mut self, stage: Stage) {
        for s in Stage::downstream_of(stage) {
            self.dirty.insert(s);
        }
    }

    /// Marks a single stage as clean after it has been recomputed.
    pub fn mark_clean(&mut self, stage: Stage) {
        self.dirty.remove(&stage);
    }

    /// Returns true if the given stage needs recomputation.
    pub fn is_dirty(&self, stage: Stage) -> bool {
        self.dirty.contains(&stage)
    }

    /// Returns all dirty stages in pipeline order.
    pub fn dirty_stages(&self) -> Vec<Stage> {
        let mut stages: Vec<Stage> = self.dirty.iter().copied().collect();
        stages.sort();
        stages
    }

    /// Returns the first dirty stage (lowest index), or None if all clean.
    pub fn next_dirty(&self) -> Option<Stage> {
        ALL_STAGES.iter().find(|s| self.dirty.contains(s)).copied()
    }
}

impl Default for PipelineDirtyState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_state_all_dirty() {
        let state = PipelineDirtyState::new();
        for stage in Stage::all() {
            assert!(state.is_dirty(*stage), "{:?} should be dirty", stage);
        }
        assert_eq!(state.dirty_stages().len(), 7);
    }

    #[test]
    fn clean_state_none_dirty() {
        let state = PipelineDirtyState::clean();
        for stage in Stage::all() {
            assert!(!state.is_dirty(*stage), "{:?} should be clean", stage);
        }
        assert_eq!(state.dirty_stages().len(), 0);
        assert_eq!(state.next_dirty(), None);
    }

    #[test]
    fn override_at_atmosphere_marks_3_through_7() {
        let mut state = PipelineDirtyState::clean();
        state.mark_override_at(Stage::Atmosphere);

        assert!(!state.is_dirty(Stage::StellarContext));
        assert!(!state.is_dirty(Stage::PlanetarySystem));
        assert!(state.is_dirty(Stage::Atmosphere));
        assert!(state.is_dirty(Stage::Skeleton));
        assert!(state.is_dirty(Stage::Climate));
        assert!(state.is_dirty(Stage::Biome));
        assert!(state.is_dirty(Stage::RegionalDetail));

        assert_eq!(state.dirty_stages().len(), 5);
    }

    #[test]
    fn override_at_stage_1_all_dirty() {
        let mut state = PipelineDirtyState::clean();
        state.mark_override_at(Stage::StellarContext);

        for stage in Stage::all() {
            assert!(state.is_dirty(*stage), "{:?} should be dirty", stage);
        }
    }

    #[test]
    fn override_at_stage_7_only_regional_detail() {
        let mut state = PipelineDirtyState::clean();
        state.mark_override_at(Stage::RegionalDetail);

        for stage in &Stage::all()[..6] {
            assert!(!state.is_dirty(*stage), "{:?} should be clean", stage);
        }
        assert!(state.is_dirty(Stage::RegionalDetail));
    }

    #[test]
    fn mark_clean_single_stage() {
        let mut state = PipelineDirtyState::clean();
        state.mark_override_at(Stage::Atmosphere);
        state.mark_clean(Stage::Atmosphere);

        assert!(!state.is_dirty(Stage::Atmosphere));
        assert!(state.is_dirty(Stage::Skeleton));
        assert!(state.is_dirty(Stage::Climate));
        assert!(state.is_dirty(Stage::Biome));
        assert!(state.is_dirty(Stage::RegionalDetail));
    }

    #[test]
    fn next_dirty_returns_lowest_index() {
        let mut state = PipelineDirtyState::clean();
        state.mark_override_at(Stage::Climate);
        assert_eq!(state.next_dirty(), Some(Stage::Climate));

        state.mark_clean(Stage::Climate);
        assert_eq!(state.next_dirty(), Some(Stage::Biome));
    }

    #[test]
    fn multiple_overrides() {
        let mut state = PipelineDirtyState::clean();
        state.mark_override_at(Stage::Atmosphere);
        state.mark_override_at(Stage::Climate);

        // Stages 3-7 should still be dirty (override at 3 already covered 5-7)
        assert!(!state.is_dirty(Stage::StellarContext));
        assert!(!state.is_dirty(Stage::PlanetarySystem));
        assert!(state.is_dirty(Stage::Atmosphere));
        assert!(state.is_dirty(Stage::Skeleton));
        assert!(state.is_dirty(Stage::Climate));
        assert!(state.is_dirty(Stage::Biome));
        assert!(state.is_dirty(Stage::RegionalDetail));
    }

    #[test]
    fn stage_names() {
        assert_eq!(Stage::StellarContext.name(), "Stellar Context");
        assert_eq!(Stage::RegionalDetail.name(), "Regional Detail");
    }

    #[test]
    fn stage_indices() {
        assert_eq!(Stage::StellarContext.index(), 1);
        assert_eq!(Stage::RegionalDetail.index(), 7);
    }

    #[test]
    fn downstream_of_middle_stage() {
        let downstream = Stage::downstream_of(Stage::Skeleton);
        assert_eq!(
            downstream,
            vec![
                Stage::Skeleton,
                Stage::Climate,
                Stage::Biome,
                Stage::RegionalDetail
            ]
        );
    }

    #[test]
    fn dirty_stages_ordered() {
        let mut state = PipelineDirtyState::clean();
        state.mark_override_at(Stage::Climate);
        let dirty = state.dirty_stages();
        assert_eq!(
            dirty,
            vec![Stage::Climate, Stage::Biome, Stage::RegionalDetail]
        );
    }
}
