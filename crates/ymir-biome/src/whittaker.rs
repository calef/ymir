//! Whittaker-diagram biome classification from temperature and humidity axes.
//!
//! This module provides [`classify`], which maps a single tile's
//! `(temperature_k, humidity, palette)` triple to a concrete [`Biome`].
//! For the Earth-like palette it follows a Whittaker-style temperature ×
//! moisture lookup; abiotic palettes use simpler physical-state thresholds
//! dominated by temperature. See design doc section 5.6.
//!
//! Classification here sees only temperature and relative humidity; it has
//! no knowledge of elevation, coastline, or edge context. Biomes that
//! require that context (`Ocean`, `CoastalShallow`, `Wetland`,
//! `AlpineMeadow`, `ImpactRegolith`, `ImpactCrater`, `ChemicalSediment`,
//! `TitanCryovolcanic`) are reserved for later placement in BIOME-04 and
//! are never returned from this function, even though they appear in the
//! palette's [`BiomePalette::biomes`] set.

use crate::palette::{Biome, BiomePalette};

/// Humidity below which a tile is considered "dry" on the Whittaker diagram.
const DRY: f64 = 0.2;
/// Humidity above which a tile is considered "mid-moist" / typical.
const MID: f64 = 0.5;
/// Humidity above which a tile is considered "wet" / rainforest-suitable.
const WET: f64 = 0.75;

/// Convert Kelvin to Celsius.
fn celsius(k: f64) -> f64 {
    k - 273.15
}

/// Classify a single tile's biome from temperature, humidity, and palette.
///
/// Temperature is in Kelvin; humidity is a relative 0.0..1.0 value. The
/// returned [`Biome`] is guaranteed to be a member of `palette.biomes()`.
///
/// This function only considers temperature and humidity. Biomes that
/// depend on elevation or edge-aware context (e.g. `Ocean`,
/// `CoastalShallow`, `Wetland`, `AlpineMeadow`, `ImpactRegolith`,
/// `ImpactCrater`, `ChemicalSediment`, `TitanCryovolcanic`) are never
/// produced here; they are applied later by BIOME-04 overlays.
pub fn classify(temp_k: f64, humidity: f64, palette: BiomePalette) -> Biome {
    match palette {
        BiomePalette::EarthLike => classify_earth(celsius(temp_k), humidity),
        BiomePalette::VenusLike => classify_venus(celsius(temp_k)),
        BiomePalette::MarsLike => classify_mars(celsius(temp_k), humidity),
        BiomePalette::TitanLike => classify_titan(celsius(temp_k)),
        BiomePalette::Airless => classify_airless(celsius(temp_k)),
        BiomePalette::NoSurface => Biome::NoSurface,
    }
}

/// Earth-like Whittaker lookup on temperature (Celsius) × humidity.
fn classify_earth(t_c: f64, h: f64) -> Biome {
    if t_c < -15.0 {
        // Very cold: ice sheet in the driest conditions, tundra otherwise.
        if h < DRY {
            Biome::IceSheet
        } else {
            Biome::Tundra
        }
    } else if t_c < 0.0 {
        // Sub-freezing but not polar: tundra by default, boreal where wetter.
        if h >= MID {
            Biome::BorealForest
        } else {
            Biome::Tundra
        }
    } else if t_c < 10.0 {
        // Cool temperate: boreal if wet, grassland if mid, cold desert if dry.
        if h >= MID {
            Biome::BorealForest
        } else if h >= DRY {
            Biome::Grassland
        } else {
            Biome::ColdDesert
        }
    } else if t_c < 20.0 {
        // Warm temperate: forest if wet, grassland if mid, cold desert if dry.
        if h >= MID {
            Biome::TemperateForest
        } else if h >= DRY {
            Biome::Grassland
        } else {
            Biome::ColdDesert
        }
    } else if t_c < 30.0 {
        // Subtropical/tropical: rainforest, savanna, hot desert.
        if h >= WET {
            Biome::TropicalRainforest
        } else if h >= DRY {
            Biome::Savanna
        } else {
            Biome::HotDesert
        }
    } else {
        // Very hot: rainforest only at wet extremes, otherwise savanna or
        // hot desert.
        if h >= WET {
            Biome::TropicalRainforest
        } else if h >= MID {
            Biome::Savanna
        } else {
            Biome::HotDesert
        }
    }
}

/// Mars-like lookup. Humidity is near-zero almost everywhere; the split
/// between dust plain and bedrock uses "bone dry" as a proxy for wind-
/// scoured exposed rock.
fn classify_mars(t_c: f64, h: f64) -> Biome {
    // CO2 sublimation point is roughly -125 C; colder than that freezes out
    // into the polar ice cap.
    if t_c < -125.0 {
        Biome::MartianPolarIce
    } else if h <= 0.0 {
        Biome::MartianBedrock
    } else {
        Biome::MartianDustPlain
    }
}

/// Venus-like lookup. We treat temperature as a rough proxy for both
/// surface chemistry and elevation (highlands run cooler on Venus).
fn classify_venus(t_c: f64) -> Biome {
    if t_c > 450.0 {
        Biome::LavaPlain
    } else if t_c > 400.0 {
        Biome::VenusHighlands
    } else if t_c > 350.0 {
        Biome::SulfuricHighlands
    } else {
        Biome::VenusPlains
    }
}

/// Titan-like lookup on temperature alone (humidity is methane vapor and
/// is not meaningfully mapped here).
///
/// Titan's surface sits in a narrow thermal band (roughly 90..95 K). We
/// split that band into a cold "ice highland" tier below ~93 K, a narrow
/// dune-field tier just above, and a liquid-methane "sea" tier at the warm
/// end where methane and ethane are pool-stable.
fn classify_titan(t_c: f64) -> Biome {
    if t_c < -180.0 {
        Biome::TitanIceHighland
    } else if t_c < -179.0 {
        Biome::TitanDunes
    } else {
        Biome::TitanMethaneSea
    }
}

/// Airless lookup on temperature alone.
fn classify_airless(t_c: f64) -> Biome {
    if t_c > 200.0 {
        Biome::SolarBleached
    } else if t_c < -150.0 {
        Biome::ThermalFracture
    } else {
        Biome::BareRegolith
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_PALETTES: &[BiomePalette] = &[
        BiomePalette::EarthLike,
        BiomePalette::VenusLike,
        BiomePalette::MarsLike,
        BiomePalette::TitanLike,
        BiomePalette::Airless,
        BiomePalette::NoSurface,
    ];

    #[test]
    fn earth_hot_wet_is_tropical_rainforest() {
        assert_eq!(
            classify(303.0, 0.9, BiomePalette::EarthLike),
            Biome::TropicalRainforest
        );
    }

    #[test]
    fn earth_cold_dry_is_tundra_or_ice() {
        let b = classify(250.0, 0.1, BiomePalette::EarthLike);
        assert!(
            matches!(b, Biome::Tundra | Biome::IceSheet),
            "expected Tundra or IceSheet, got {b:?}"
        );
    }

    #[test]
    fn earth_moderate_wet_is_temperate_forest() {
        assert_eq!(
            classify(285.0, 0.8, BiomePalette::EarthLike),
            Biome::TemperateForest
        );
    }

    #[test]
    fn earth_moderate_dry_is_grassland_or_desert() {
        let b = classify(285.0, 0.15, BiomePalette::EarthLike);
        assert!(
            matches!(b, Biome::ColdDesert | Biome::Grassland),
            "expected ColdDesert or Grassland, got {b:?}"
        );
    }

    #[test]
    fn earth_hot_dry_is_hot_desert() {
        assert_eq!(
            classify(308.0, 0.1, BiomePalette::EarthLike),
            Biome::HotDesert
        );
    }

    #[test]
    fn mars_default_is_dust_plain() {
        // Mild humidity to distinguish from bone-dry bedrock case.
        assert_eq!(
            classify(220.0, 0.01, BiomePalette::MarsLike),
            Biome::MartianDustPlain
        );
    }

    #[test]
    fn mars_extreme_cold_is_polar_ice() {
        assert_eq!(
            classify(140.0, 0.0, BiomePalette::MarsLike),
            Biome::MartianPolarIce
        );
    }

    #[test]
    fn venus_extreme_hot_is_lava() {
        assert_eq!(
            classify(730.0, 0.0, BiomePalette::VenusLike),
            Biome::LavaPlain
        );
    }

    #[test]
    fn venus_default_is_plains() {
        assert_eq!(
            classify(620.0, 0.0, BiomePalette::VenusLike),
            Biome::VenusPlains
        );
    }

    #[test]
    fn titan_cold_is_ice_highland() {
        assert_eq!(
            classify(90.0, 0.0, BiomePalette::TitanLike),
            Biome::TitanIceHighland
        );
    }

    #[test]
    fn titan_warm_is_methane_sea() {
        assert_eq!(
            classify(95.0, 0.0, BiomePalette::TitanLike),
            Biome::TitanMethaneSea
        );
    }

    #[test]
    fn airless_hot_is_solar_bleached() {
        assert_eq!(
            classify(500.0, 0.0, BiomePalette::Airless),
            Biome::SolarBleached
        );
    }

    #[test]
    fn airless_cold_is_thermal_fracture() {
        assert_eq!(
            classify(100.0, 0.0, BiomePalette::Airless),
            Biome::ThermalFracture
        );
    }

    #[test]
    fn airless_moderate_is_regolith() {
        assert_eq!(
            classify(250.0, 0.0, BiomePalette::Airless),
            Biome::BareRegolith
        );
    }

    #[test]
    fn no_surface_always_returns_no_surface() {
        // Span absurd extremes just to be sure nothing sneaks through.
        for &t in &[0.0_f64, 100.0, 288.0, 730.0, 10_000.0] {
            for &h in &[0.0_f64, 0.25, 0.5, 0.75, 1.0] {
                assert_eq!(classify(t, h, BiomePalette::NoSurface), Biome::NoSurface);
            }
        }
    }

    #[test]
    fn every_result_is_in_palette() {
        // Sweep a grid of realistic-ish T/H values across every palette
        // and make sure classify never produces an out-of-palette biome.
        let temps_k: &[f64] = &[
            50.0, 90.0, 140.0, 200.0, 220.0, 250.0, 273.15, 285.0, 303.0, 310.0, 400.0, 500.0,
            620.0, 730.0,
        ];
        let humidities: &[f64] = &[0.0, 0.05, 0.15, 0.3, 0.5, 0.7, 0.8, 0.95, 1.0];

        for &p in ALL_PALETTES {
            for &t in temps_k {
                for &h in humidities {
                    let b = classify(t, h, p);
                    assert!(
                        p.contains(b),
                        "classify({t}K, {h}, {p:?}) = {b:?} not in palette"
                    );
                }
            }
        }
    }
}
