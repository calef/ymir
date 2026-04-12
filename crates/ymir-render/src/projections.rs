//! Map projection implementations: equirectangular, Mollweide, orthographic, etc.
//!
//! All projections produce normalized coordinates in `[-1, 1]` for both axes,
//! with the natural aspect ratio reported via [`MapProjection::width_height_ratio`].
//! Inverse projections (used by the rasterizer) live alongside the forward
//! transforms on the concrete projection types.

use std::f64::consts::PI;

/// A forward map projection from (latitude, longitude) on the unit sphere to
/// normalized 2D map coordinates.
///
/// The output `(x, y)` is in `[-1, 1]` along each axis when the point lies on
/// the projected map; points off-map (which can occur for projections whose
/// natural extent does not fill the bounding rectangle) yield `None`.
pub trait MapProjection {
    /// Project a (latitude, longitude) pair (radians) to normalized map
    /// coordinates in `[-1, 1] x [-1, 1]`. Returns `None` for points that fall
    /// outside the projection's valid region.
    fn project(&self, lat_rad: f64, lon_rad: f64) -> Option<(f64, f64)>;

    /// Aspect ratio (width / height) of the natural projection rectangle.
    fn width_height_ratio(&self) -> f64;
}

/// Mollweide equal-area pseudocylindrical projection.
///
/// The forward transform solves `2*theta + sin(2*theta) = pi*sin(lat)` for the
/// auxiliary angle `theta` via Newton-Raphson iteration, then computes
/// `x = (2*sqrt(2)/pi) * lon * cos(theta)` and `y = sqrt(2) * sin(theta)`.
/// Coordinates are normalized to `[-1, 1]` by dividing by their natural
/// half-extents (`2*sqrt(2)` and `sqrt(2)` respectively).
#[derive(Debug, Clone, Copy, Default)]
pub struct Mollweide;

impl Mollweide {
    /// Maximum number of Newton-Raphson iterations for the auxiliary-angle solve.
    const MAX_ITERS: usize = 16;
    /// Convergence threshold on the iterate delta (radians).
    const TOL: f64 = 1.0e-10;

    /// Solve `2*theta + sin(2*theta) = pi*sin(lat)` for `theta`, the Mollweide
    /// auxiliary angle. The poles (`|lat| == pi/2`) yield `theta = ±pi/2`
    /// directly without iteration.
    fn auxiliary_angle(lat_rad: f64) -> f64 {
        let lat = lat_rad.clamp(-PI / 2.0, PI / 2.0);
        // Pole shortcut: 2*theta + sin(2*theta) = ±pi at theta = ±pi/2.
        if (lat.abs() - PI / 2.0).abs() < 1.0e-12 {
            return lat.signum() * PI / 2.0;
        }

        let target = PI * lat.sin();
        let mut theta = lat; // good initial guess
        for _ in 0..Self::MAX_ITERS {
            let f = 2.0 * theta + (2.0 * theta).sin() - target;
            let fp = 2.0 + 2.0 * (2.0 * theta).cos();
            // fp is in [0, 4]; only zero at theta = ±pi/2 which we handled above.
            let delta = f / fp;
            theta -= delta;
            if delta.abs() < Self::TOL {
                break;
            }
        }
        theta
    }

    /// Inverse Mollweide: recover (lat, lon) in radians from normalized map
    /// coordinates `(nx, ny)` in `[-1, 1] x [-1, 1]`. Returns `None` if the
    /// point falls outside the projected ellipse.
    pub fn inverse(nx: f64, ny: f64) -> Option<(f64, f64)> {
        // Denormalize back to natural Mollweide extents.
        let sqrt2 = 2.0_f64.sqrt();
        let x = nx * 2.0 * sqrt2;
        let y = ny * sqrt2;

        // Recover auxiliary angle from y.
        let s = y / sqrt2;
        if !(-1.0..=1.0).contains(&s) {
            return None;
        }
        let theta = s.asin();

        // Latitude.
        let sin_lat = (2.0 * theta + (2.0 * theta).sin()) / PI;
        if !(-1.0..=1.0).contains(&sin_lat) {
            return None;
        }
        let lat = sin_lat.asin();

        // Longitude. cos(theta) is zero only at the poles; there longitude
        // is degenerate, so just pick zero.
        let cos_theta = theta.cos();
        let lon = if cos_theta.abs() < 1.0e-12 {
            0.0
        } else {
            PI * x / (2.0 * sqrt2 * cos_theta)
        };

        if !(-PI..=PI).contains(&lon) {
            return None;
        }

        Some((lat, lon))
    }
}

impl MapProjection for Mollweide {
    fn project(&self, lat_rad: f64, lon_rad: f64) -> Option<(f64, f64)> {
        if !lat_rad.is_finite() || !lon_rad.is_finite() {
            return None;
        }
        if lat_rad.abs() > PI / 2.0 + 1.0e-9 {
            return None;
        }
        if lon_rad.abs() > PI + 1.0e-9 {
            return None;
        }
        let theta = Self::auxiliary_angle(lat_rad);
        let sqrt2 = 2.0_f64.sqrt();
        let x = (2.0 * sqrt2 / PI) * lon_rad * theta.cos();
        let y = sqrt2 * theta.sin();
        // Normalize.
        let nx = x / (2.0 * sqrt2);
        let ny = y / sqrt2;
        // Clamp tiny FP overshoot at the boundary back into [-1, 1].
        let nx = nx.clamp(-1.0, 1.0);
        let ny = ny.clamp(-1.0, 1.0);
        Some((nx, ny))
    }

    fn width_height_ratio(&self) -> f64 {
        2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1.0e-9;

    #[test]
    fn equator_projects_to_y_zero() {
        let proj = Mollweide;
        for lon_deg in [-180.0_f64, -90.0, -45.0, 0.0, 45.0, 90.0, 180.0] {
            let (_, ny) = proj.project(0.0, lon_deg.to_radians()).unwrap();
            assert!(ny.abs() < EPS, "lon {lon_deg}: y = {ny}");
        }
    }

    #[test]
    fn poles_project_to_y_plus_minus_one() {
        let proj = Mollweide;
        let (nx_n, ny_n) = proj.project(PI / 2.0, 0.0).unwrap();
        let (nx_s, ny_s) = proj.project(-PI / 2.0, 0.0).unwrap();
        assert!((ny_n - 1.0).abs() < 1.0e-8, "north pole y = {ny_n}");
        assert!((ny_s + 1.0).abs() < 1.0e-8, "south pole y = {ny_s}");
        // x at the poles collapses to zero (cos(theta) = 0).
        assert!(nx_n.abs() < 1.0e-8);
        assert!(nx_s.abs() < 1.0e-8);
    }

    #[test]
    fn origin_projects_to_zero_zero() {
        let proj = Mollweide;
        let (nx, ny) = proj.project(0.0, 0.0).unwrap();
        assert!(nx.abs() < EPS && ny.abs() < EPS);
    }

    #[test]
    fn coordinates_within_unit_box() {
        let proj = Mollweide;
        let mut lat = -PI / 2.0;
        while lat <= PI / 2.0 {
            let mut lon = -PI;
            while lon <= PI {
                let (nx, ny) = proj.project(lat, lon).unwrap();
                assert!(nx.abs() <= 1.0 + 1.0e-9, "lat {lat} lon {lon}: x = {nx}");
                assert!(ny.abs() <= 1.0 + 1.0e-9, "lat {lat} lon {lon}: y = {ny}");
                lon += PI / 12.0;
            }
            lat += PI / 12.0;
        }
    }

    #[test]
    fn aspect_ratio_is_two() {
        assert_eq!(Mollweide.width_height_ratio(), 2.0);
    }

    #[test]
    fn round_trip_sample_points() {
        let proj = Mollweide;
        let cases: [(f64, f64); 8] = [
            (0.0, 0.0),
            (30.0, 45.0),
            (-30.0, -45.0),
            (60.0, 120.0),
            (-60.0, -120.0),
            (15.5, -33.3),
            (75.0, 170.0),
            (-75.0, -170.0),
        ];
        for (lat_deg, lon_deg) in cases {
            let lat = lat_deg.to_radians();
            let lon = lon_deg.to_radians();
            let (nx, ny) = proj.project(lat, lon).unwrap();
            let (lat2, lon2) = Mollweide::inverse(nx, ny).unwrap();
            assert!(
                (lat - lat2).abs() < 1.0e-6,
                "lat mismatch: {lat} vs {lat2} (lon = {lon})"
            );
            assert!(
                (lon - lon2).abs() < 1.0e-6,
                "lon mismatch: {lon} vs {lon2} (lat = {lat})"
            );
        }
    }

    #[test]
    fn inverse_outside_ellipse_returns_none() {
        // Top-right corner of bounding box should be off-map.
        assert!(Mollweide::inverse(0.99, 0.99).is_none());
        assert!(Mollweide::inverse(-0.99, 0.99).is_none());
        assert!(Mollweide::inverse(0.99, -0.99).is_none());
        assert!(Mollweide::inverse(-0.99, -0.99).is_none());
    }

    #[test]
    fn auxiliary_angle_satisfies_defining_equation() {
        for lat_deg in [-89.0_f64, -60.0, -30.0, 0.0, 15.0, 45.0, 75.0, 89.0] {
            let lat = lat_deg.to_radians();
            let theta = Mollweide::auxiliary_angle(lat);
            let lhs = 2.0 * theta + (2.0 * theta).sin();
            let rhs = PI * lat.sin();
            assert!(
                (lhs - rhs).abs() < 1.0e-9,
                "lat {lat_deg}: lhs = {lhs}, rhs = {rhs}"
            );
        }
    }
}
