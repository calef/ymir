//! Shared traits for cross-crate abstractions.
//!
//! These traits define the interfaces that sibling crates (ymir-climate, ymir-render, etc.)
//! depend on instead of concrete types from each other, enabling loose coupling across
//! the pipeline.

/// Abstraction for a tile on a geodesic grid.
///
/// Provides the minimal geographic and geometric properties that any grid tile
/// must expose, regardless of the underlying tessellation scheme.
pub trait GeoTile {
    /// Latitude of the tile center in degrees.
    fn lat(&self) -> f64;
    /// Longitude of the tile center in degrees.
    fn lon(&self) -> f64;
    /// Normalized elevation in the range [0, 1].
    fn elevation(&self) -> f64;
    /// Relative tile area (ratio to mean tile area, or absolute in steradians).
    fn area(&self) -> f64;
}

/// Abstraction for a stage in the causal pipeline.
///
/// Each pipeline stage transforms an input into an output given some configuration.
/// Stages are composable: one stage's `Output` becomes the next stage's `Input`.
pub trait PipelineStage {
    /// The input data this stage consumes.
    type Input;
    /// The output data this stage produces.
    type Output;
    /// Configuration controlling stage behavior.
    type Config;

    /// Human-readable name for logging and diagnostics.
    fn name(&self) -> &'static str;
    /// Execute the stage, producing output from the given input and config.
    fn execute(&self, input: &Self::Input, config: &Self::Config) -> Self::Output;
}

/// A point on a sphere, used across surface, climate, and render crates.
///
/// The canonical representation is in radians; degree conversions are provided
/// as default methods.
pub trait SphericalPoint {
    /// Latitude in radians.
    fn lat_rad(&self) -> f64;
    /// Longitude in radians.
    fn lon_rad(&self) -> f64;
    /// Latitude in degrees (default: converts from radians).
    fn lat_deg(&self) -> f64 {
        self.lat_rad().to_degrees()
    }
    /// Longitude in degrees (default: converts from radians).
    fn lon_deg(&self) -> f64 {
        self.lon_rad().to_degrees()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    /// Mock struct implementing both GeoTile and SphericalPoint.
    struct MockTile {
        lat_radians: f64,
        lon_radians: f64,
        elev: f64,
        tile_area: f64,
    }

    impl GeoTile for MockTile {
        fn lat(&self) -> f64 {
            self.lat_radians.to_degrees()
        }
        fn lon(&self) -> f64 {
            self.lon_radians.to_degrees()
        }
        fn elevation(&self) -> f64 {
            self.elev
        }
        fn area(&self) -> f64 {
            self.tile_area
        }
    }

    impl SphericalPoint for MockTile {
        fn lat_rad(&self) -> f64 {
            self.lat_radians
        }
        fn lon_rad(&self) -> f64 {
            self.lon_radians
        }
    }

    #[test]
    fn spherical_point_default_methods() {
        let tile = MockTile {
            lat_radians: PI / 4.0,
            lon_radians: -PI / 2.0,
            elev: 0.5,
            tile_area: 1.0,
        };
        let eps = 1e-10;
        assert!((tile.lat_deg() - 45.0).abs() < eps);
        assert!((tile.lon_deg() - (-90.0)).abs() < eps);
    }

    #[test]
    fn geo_tile_values() {
        let tile = MockTile {
            lat_radians: 0.0,
            lon_radians: PI,
            elev: 0.75,
            tile_area: 1.2,
        };
        assert!((tile.lat() - 0.0).abs() < 1e-10);
        assert!((tile.lon() - 180.0).abs() < 1e-10);
        assert!((tile.elevation() - 0.75).abs() < 1e-10);
        assert!((tile.area() - 1.2).abs() < 1e-10);
    }

    /// Verify PipelineStage can be implemented with concrete types.
    struct DoubleStage;

    impl PipelineStage for DoubleStage {
        type Input = f64;
        type Output = f64;
        type Config = ();

        fn name(&self) -> &'static str {
            "double"
        }

        fn execute(&self, input: &Self::Input, _config: &Self::Config) -> Self::Output {
            input * 2.0
        }
    }

    #[test]
    fn pipeline_stage_concrete_types() {
        let stage = DoubleStage;
        assert_eq!(stage.name(), "double");
        assert!((stage.execute(&3.5, &()) - 7.0).abs() < 1e-10);
    }
}
