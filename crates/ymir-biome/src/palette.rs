//! Color and texture palette definitions for each biome type.
//!
//! This module defines the full [`Biome`] enum (every variant we'll ever
//! classify across all palettes), the [`BiomePalette`] selector that groups
//! biomes by which atmospheric regime they belong to, and [`palette_for`],
//! the lookup from [`AtmosphereClass`] to [`BiomePalette`] described in
//! design section 5.6.
//!
//! The biome classifier selects a palette for the world based on the
//! atmosphere, then chooses among that palette's biomes using the climate
//! signal (temperature, moisture, elevation). Biomes are strictly grouped
//! by palette: a given [`Biome`] variant appears in exactly one palette.

use serde::{Deserialize, Serialize};
use ymir_atmosphere::composition::AtmosphereClass;

/// Every biome type this crate can ever classify, across all palettes.
///
/// Biomes are grouped by the atmospheric regime they live in (see the
/// [`BiomePalette`] selector). A given variant belongs to exactly one
/// palette; the palette assigned to the world by [`palette_for`] determines
/// which subset of variants the classifier can produce.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Biome {
    // --- Earth-like (oxygen-bearing, biological) ---
    /// Hot, wet, high-productivity equatorial forest.
    TropicalRainforest,
    /// Mid-latitude deciduous/mixed forest.
    TemperateForest,
    /// High-latitude coniferous forest (taiga).
    BorealForest,
    /// Tropical grassland with scattered trees and a pronounced dry season.
    Savanna,
    /// Temperate grassland / prairie / steppe.
    Grassland,
    /// Hot, arid desert (Sahara / Sonoran style).
    HotDesert,
    /// Cold, arid desert (Gobi / Atacama style).
    ColdDesert,
    /// Treeless sub-polar biome with permafrost.
    Tundra,
    /// Continental or polar ice cap.
    IceSheet,
    /// Open ocean surface.
    Ocean,
    /// Shallow coastal waters (continental shelf, reefs, lagoons).
    CoastalShallow,
    /// Marsh / swamp / bog.
    Wetland,
    /// Above-treeline alpine grassland.
    AlpineMeadow,

    // --- Venus-like (thick CO2, no O2) ---
    /// Low-elevation basaltic Venus plains.
    VenusPlains,
    /// Higher-altitude Venus terrain with metallic frosts.
    VenusHighlands,
    /// Active or recently active lava plain.
    LavaPlain,
    /// Uplands with heavy sulfuric-acid weathering.
    SulfuricHighlands,
    /// Evaporitic / chemically precipitated sediment.
    ChemicalSediment,

    // --- Mars-like (thin CO2) ---
    /// Mars-style dust-covered plains.
    MartianDustPlain,
    /// Exposed Martian bedrock and scarps.
    MartianBedrock,
    /// Polar CO2/water ice cap.
    MartianPolarIce,
    /// Heavily cratered regolith / impact terrain.
    ImpactRegolith,

    // --- Titan-like (thick N2 + methane/H2O) ---
    /// Liquid-methane/ethane sea.
    TitanMethaneSea,
    /// Equatorial hydrocarbon dune field.
    TitanDunes,
    /// High-albedo water-ice highland terrain.
    TitanIceHighland,
    /// Cryovolcanic province.
    TitanCryovolcanic,

    // --- Airless ---
    /// Exposed regolith with no significant weathering.
    BareRegolith,
    /// Fresh impact crater floor / ejecta.
    ImpactCrater,
    /// Thermally fractured bedrock (day/night cycling).
    ThermalFracture,
    /// Solar-wind-darkened / space-weathered terrain.
    SolarBleached,

    // --- No surface (gas giants, sub-Neptunes) ---
    /// Placeholder for bodies with no real solid surface.
    NoSurface,
}

/// High-level grouping of biomes by atmospheric regime.
///
/// A world's palette is chosen up front from its [`AtmosphereClass`] via
/// [`palette_for`]; the biome classifier then picks among the palette's
/// biomes using the climate signal. Palettes partition the [`Biome`] enum,
/// i.e. no biome appears in two palettes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BiomePalette {
    /// Oxygen-bearing, biologically active worlds.
    EarthLike,
    /// Thick-CO2, Venus-like worlds.
    VenusLike,
    /// Thin-CO2, Mars-like worlds.
    MarsLike,
    /// Thick-N2-plus-methane/H2O, Titan-like worlds.
    TitanLike,
    /// Airless / no appreciable atmosphere.
    Airless,
    /// No solid surface (gas giants, sub-Neptunes).
    NoSurface,
}

const EARTH_LIKE: &[Biome] = &[
    Biome::TropicalRainforest,
    Biome::TemperateForest,
    Biome::BorealForest,
    Biome::Savanna,
    Biome::Grassland,
    Biome::HotDesert,
    Biome::ColdDesert,
    Biome::Tundra,
    Biome::IceSheet,
    Biome::Ocean,
    Biome::CoastalShallow,
    Biome::Wetland,
    Biome::AlpineMeadow,
];

const VENUS_LIKE: &[Biome] = &[
    Biome::VenusPlains,
    Biome::VenusHighlands,
    Biome::LavaPlain,
    Biome::SulfuricHighlands,
    Biome::ChemicalSediment,
];

const MARS_LIKE: &[Biome] = &[
    Biome::MartianDustPlain,
    Biome::MartianBedrock,
    Biome::MartianPolarIce,
    Biome::ImpactRegolith,
];

const TITAN_LIKE: &[Biome] = &[
    Biome::TitanMethaneSea,
    Biome::TitanDunes,
    Biome::TitanIceHighland,
    Biome::TitanCryovolcanic,
];

const AIRLESS: &[Biome] = &[
    Biome::BareRegolith,
    Biome::ImpactCrater,
    Biome::ThermalFracture,
    Biome::SolarBleached,
];

const NO_SURFACE: &[Biome] = &[Biome::NoSurface];

impl BiomePalette {
    /// Return the exact set of biomes valid in this palette.
    ///
    /// The returned slice is static and ordered; classifiers should treat
    /// membership as set-like rather than relying on the order.
    pub fn biomes(&self) -> &'static [Biome] {
        match self {
            BiomePalette::EarthLike => EARTH_LIKE,
            BiomePalette::VenusLike => VENUS_LIKE,
            BiomePalette::MarsLike => MARS_LIKE,
            BiomePalette::TitanLike => TITAN_LIKE,
            BiomePalette::Airless => AIRLESS,
            BiomePalette::NoSurface => NO_SURFACE,
        }
    }

    /// True iff `biome` is one of the variants produced by this palette.
    pub fn contains(&self, biome: Biome) -> bool {
        self.biomes().contains(&biome)
    }

    /// A sensible fallback biome for this palette, used when classification
    /// fails or lacks data. Chosen as the most "typical" member of the
    /// palette.
    pub fn default_biome(&self) -> Biome {
        match self {
            BiomePalette::EarthLike => Biome::Ocean,
            BiomePalette::VenusLike => Biome::VenusPlains,
            BiomePalette::MarsLike => Biome::MartianDustPlain,
            BiomePalette::TitanLike => Biome::TitanIceHighland,
            BiomePalette::Airless => Biome::BareRegolith,
            BiomePalette::NoSurface => Biome::NoSurface,
        }
    }
}

/// Select the biome palette appropriate for a given [`AtmosphereClass`].
///
/// This is the lookup from design section 5.6. The mapping is total over
/// [`AtmosphereClass`] variants.
pub fn palette_for(class: AtmosphereClass) -> BiomePalette {
    match class {
        AtmosphereClass::NitrogenOxygen => BiomePalette::EarthLike,
        AtmosphereClass::ThickCO2 => BiomePalette::VenusLike,
        AtmosphereClass::ThinCO2 => BiomePalette::MarsLike,
        AtmosphereClass::ThickN2H2O => BiomePalette::TitanLike,
        AtmosphereClass::None => BiomePalette::Airless,
        AtmosphereClass::HydrogenHelium => BiomePalette::NoSurface,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Every [`AtmosphereClass`] variant; kept in sync by hand so the tests
    /// catch any new class lacking a palette mapping.
    const ALL_CLASSES: &[AtmosphereClass] = &[
        AtmosphereClass::None,
        AtmosphereClass::ThinCO2,
        AtmosphereClass::ThickCO2,
        AtmosphereClass::ThickN2H2O,
        AtmosphereClass::NitrogenOxygen,
        AtmosphereClass::HydrogenHelium,
    ];

    const ALL_PALETTES: &[BiomePalette] = &[
        BiomePalette::EarthLike,
        BiomePalette::VenusLike,
        BiomePalette::MarsLike,
        BiomePalette::TitanLike,
        BiomePalette::Airless,
        BiomePalette::NoSurface,
    ];

    #[test]
    fn every_palette_is_nonempty() {
        for p in ALL_PALETTES {
            assert!(!p.biomes().is_empty(), "palette {p:?} has no biomes listed");
        }
    }

    #[test]
    fn palette_for_covers_all_atmosphere_classes() {
        // Hitting every variant exercises the exhaustive match in
        // `palette_for`; if a new variant is added to AtmosphereClass,
        // compilation fails there and this test calls it out by name.
        for class in ALL_CLASSES {
            let _ = palette_for(*class);
        }

        assert_eq!(
            palette_for(AtmosphereClass::NitrogenOxygen),
            BiomePalette::EarthLike
        );
        assert_eq!(
            palette_for(AtmosphereClass::ThickCO2),
            BiomePalette::VenusLike
        );
        assert_eq!(
            palette_for(AtmosphereClass::ThinCO2),
            BiomePalette::MarsLike
        );
        assert_eq!(
            palette_for(AtmosphereClass::ThickN2H2O),
            BiomePalette::TitanLike
        );
        assert_eq!(palette_for(AtmosphereClass::None), BiomePalette::Airless);
        assert_eq!(
            palette_for(AtmosphereClass::HydrogenHelium),
            BiomePalette::NoSurface
        );
    }

    #[test]
    fn default_biome_is_in_palette() {
        for p in ALL_PALETTES {
            assert!(
                p.contains(p.default_biome()),
                "default_biome() of {p:?} not in biomes()"
            );
        }
    }

    #[test]
    fn palette_biome_sets_are_disjoint() {
        let mut seen: HashSet<Biome> = HashSet::new();
        for p in ALL_PALETTES {
            for &b in p.biomes() {
                assert!(
                    seen.insert(b),
                    "biome {b:?} appears in more than one palette"
                );
            }
        }
    }

    #[test]
    fn no_surface_biome_only_in_no_surface_palette() {
        for p in ALL_PALETTES {
            let has = p.contains(Biome::NoSurface);
            if *p == BiomePalette::NoSurface {
                assert!(has, "NoSurface palette missing NoSurface biome");
            } else {
                assert!(!has, "NoSurface biome leaked into {p:?}");
            }
        }
    }

    #[test]
    fn contains_matches_biomes_slice() {
        let earth = BiomePalette::EarthLike;
        assert!(earth.contains(Biome::Ocean));
        assert!(earth.contains(Biome::TropicalRainforest));
        assert!(!earth.contains(Biome::VenusPlains));
        assert!(!earth.contains(Biome::NoSurface));
    }

    #[test]
    fn biome_serde_round_trip() {
        for p in ALL_PALETTES {
            for &b in p.biomes() {
                let s = serde_json::to_string(&b).expect("serialize");
                let back: Biome = serde_json::from_str(&s).expect("deserialize");
                assert_eq!(b, back);
            }
        }
    }

    #[test]
    fn biome_palette_serde_round_trip() {
        for p in ALL_PALETTES {
            let s = serde_json::to_string(p).expect("serialize");
            let back: BiomePalette = serde_json::from_str(&s).expect("deserialize");
            assert_eq!(*p, back);
        }
    }
}
