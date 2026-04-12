//! The `Sourced<T>` wrapper type that tracks whether a value was derived from
//! simulation or overridden with observational data.

use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::ops::Deref;

/// Provenance tag for every value in the pipeline.
///
/// Records whether a value was computed from upstream stages, injected from
/// real observational data, or assumed as a fallback.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Source {
    /// Computed from upstream stage outputs.
    Derived { from_stage: String },
    /// Injected from real observational data.
    Observed {
        reference: String,
        instrument: String,
        date: String,
        uncertainty: Option<f64>,
    },
    /// Fallback or user-specified value with no observational basis.
    Assumed { reason: String },
}

/// A value with provenance tracking.
///
/// Wraps any `T` alongside a [`Source`] that records how the value was obtained.
/// Implements `Deref<Target = T>` so a `Sourced<f64>` can be used directly in
/// arithmetic expressions.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(bound(deserialize = "T: serde::de::DeserializeOwned"))]
pub struct Sourced<T: Clone + Debug> {
    value: T,
    source: Source,
}

impl<T: Clone + Debug> Sourced<T> {
    /// Create a derived value, computed from the named pipeline stage.
    pub fn derived(value: T, stage: impl Into<String>) -> Self {
        Self {
            value,
            source: Source::Derived {
                from_stage: stage.into(),
            },
        }
    }

    /// Create an observed value from real measurement data.
    pub fn observed(value: T, reference: impl Into<String>, instrument: impl Into<String>) -> Self {
        Self {
            value,
            source: Source::Observed {
                reference: reference.into(),
                instrument: instrument.into(),
                date: String::new(),
                uncertainty: None,
            },
        }
    }

    /// Create an assumed value with a reason string.
    pub fn assumed(value: T, reason: impl Into<String>) -> Self {
        Self {
            value,
            source: Source::Assumed {
                reason: reason.into(),
            },
        }
    }

    /// Returns `true` if this value was derived from upstream computation.
    pub fn is_derived(&self) -> bool {
        matches!(self.source, Source::Derived { .. })
    }

    /// Returns `true` if this value was injected from observational data.
    pub fn is_observed(&self) -> bool {
        matches!(self.source, Source::Observed { .. })
    }

    /// Returns `true` if this value was assumed as a fallback.
    pub fn is_assumed(&self) -> bool {
        matches!(self.source, Source::Assumed { .. })
    }

    /// Borrow the inner value.
    pub fn inner(&self) -> &T {
        &self.value
    }

    /// Consume the wrapper and return the inner value.
    pub fn into_inner(self) -> T {
        self.value
    }

    /// Borrow the source provenance tag.
    pub fn source(&self) -> &Source {
        &self.source
    }
}

impl<T: Clone + Debug> Deref for Sourced<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T: Clone + Debug + PartialEq> PartialEq for Sourced<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value && self.source == other.source
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_variant() {
        let s = Sourced::derived(42.0_f64, "stellar_context");
        assert!(s.is_derived());
        assert!(!s.is_observed());
        assert!(!s.is_assumed());
        assert_eq!(*s.inner(), 42.0);
    }

    #[test]
    fn observed_variant() {
        let s = Sourced::observed(0.163_f64, "Gilbert+ 2023", "TESS + Spitzer");
        assert!(s.is_observed());
        assert!(!s.is_derived());
        assert!(!s.is_assumed());
        if let Source::Observed {
            reference,
            instrument,
            ..
        } = s.source()
        {
            assert_eq!(reference, "Gilbert+ 2023");
            assert_eq!(instrument, "TESS + Spitzer");
        } else {
            panic!("expected Observed variant");
        }
    }

    #[test]
    fn assumed_variant() {
        let s = Sourced::assumed(1.0_f64, "Earth-like default");
        assert!(s.is_assumed());
        assert!(!s.is_derived());
        assert!(!s.is_observed());
    }

    #[test]
    fn deref_allows_arithmetic() {
        let a = Sourced::derived(3.0_f64, "stage_a");
        let b = Sourced::derived(4.0_f64, "stage_b");
        let sum = *a + *b;
        assert!((sum - 7.0).abs() < f64::EPSILON);
    }

    #[test]
    fn into_inner_consumes() {
        let s = Sourced::assumed(String::from("hello"), "test");
        let val: String = s.into_inner();
        assert_eq!(val, "hello");
    }

    #[test]
    fn serde_round_trip_derived() {
        let original = Sourced::derived(99.5_f64, "atmosphere");
        let json = serde_json::to_string(&original).expect("serialize");
        let restored: Sourced<f64> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, restored);
    }

    #[test]
    fn serde_round_trip_observed() {
        let original = Sourced::observed(0.163_f64, "Gilbert+ 2023", "TESS");
        let json = serde_json::to_string(&original).expect("serialize");
        let restored: Sourced<f64> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, restored);
    }

    #[test]
    fn serde_round_trip_assumed() {
        let original = Sourced::assumed(1.0_f64, "Earth-like default");
        let json = serde_json::to_string(&original).expect("serialize");
        let restored: Sourced<f64> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, restored);
    }
}
