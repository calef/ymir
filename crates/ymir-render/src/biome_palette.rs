//! Static color table mapping every [`Biome`] variant to a display RGB
//! triplet.
//!
//! Colors are hand-picked for visual plausibility on a Mollweide map:
//! Earth-like biomes use familiar satellite-tone blues, greens, and tans;
//! Venus-like biomes use yellows and ochres; Mars-like biomes use rusts and
//! dusty reds; Titan-like biomes use pale blues and methane-dark tones;
//! airless palettes use gray regolith shades; `NoSurface` gets a deep violet
//! sentinel so it stands out visually if it ever shows up.
//!
//! The implementation uses an exhaustive `match`, so adding a new [`Biome`]
//! variant is a compile error here rather than a runtime surprise.

use ymir_biome::Biome;

/// RGB color for a given [`Biome`] variant.
///
/// The mapping is static and total over [`Biome`]; the match is exhaustive by
/// design so the build breaks when a new variant is introduced without a
/// corresponding color decision.
pub fn biome_color(biome: Biome) -> [u8; 3] {
    match biome {
        // --- Earth-like ---
        Biome::TropicalRainforest => [20, 90, 40],
        Biome::TemperateForest => [50, 120, 60],
        Biome::BorealForest => [40, 85, 55],
        Biome::Savanna => [190, 170, 90],
        Biome::Grassland => [150, 180, 90],
        Biome::HotDesert => [225, 200, 130],
        Biome::ColdDesert => [190, 175, 140],
        Biome::Tundra => [180, 190, 180],
        Biome::IceSheet => [240, 245, 250],
        Biome::Ocean => [20, 70, 140],
        Biome::CoastalShallow => [80, 150, 200],
        Biome::Wetland => [80, 120, 90],
        Biome::AlpineMeadow => [130, 160, 110],

        // --- Venus-like ---
        Biome::VenusPlains => [180, 150, 80],
        Biome::VenusHighlands => [160, 130, 95],
        Biome::LavaPlain => [90, 40, 30],
        Biome::SulfuricHighlands => [200, 180, 100],
        Biome::ChemicalSediment => [210, 195, 160],

        // --- Mars-like ---
        Biome::MartianDustPlain => [190, 110, 70],
        Biome::MartianBedrock => [140, 70, 50],
        Biome::MartianPolarIce => [230, 220, 220],
        Biome::ImpactRegolith => [160, 95, 70],

        // --- Titan-like ---
        Biome::TitanMethaneSea => [30, 35, 55],
        Biome::TitanDunes => [110, 90, 70],
        Biome::TitanIceHighland => [200, 220, 230],
        Biome::TitanCryovolcanic => [170, 190, 210],

        // --- Airless ---
        Biome::BareRegolith => [150, 150, 150],
        Biome::ImpactCrater => [95, 95, 95],
        Biome::ThermalFracture => [120, 115, 110],
        Biome::SolarBleached => [175, 160, 130],

        // --- No surface sentinel ---
        Biome::NoSurface => [60, 20, 80],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `Biome` variant used to exercise `biome_color` at least once.
    const ALL_BIOMES: &[Biome] = &[
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
        Biome::VenusPlains,
        Biome::VenusHighlands,
        Biome::LavaPlain,
        Biome::SulfuricHighlands,
        Biome::ChemicalSediment,
        Biome::MartianDustPlain,
        Biome::MartianBedrock,
        Biome::MartianPolarIce,
        Biome::ImpactRegolith,
        Biome::TitanMethaneSea,
        Biome::TitanDunes,
        Biome::TitanIceHighland,
        Biome::TitanCryovolcanic,
        Biome::BareRegolith,
        Biome::ImpactCrater,
        Biome::ThermalFracture,
        Biome::SolarBleached,
        Biome::NoSurface,
    ];

    #[test]
    fn color_table_covers_all_biomes() {
        // Just calling `biome_color` for every variant proves the match is
        // total; the exhaustive match in the function body does the real
        // work. If someone adds a new variant to `Biome`, the build breaks
        // in biome_palette.rs, not here.
        for &b in ALL_BIOMES {
            let _ = biome_color(b);
        }
    }

    #[test]
    fn ocean_is_blue_dominant() {
        let [r, g, b] = biome_color(Biome::Ocean);
        assert!(
            b > r && b > g,
            "ocean expected blue-dominant: [{r},{g},{b}]"
        );
    }

    #[test]
    fn temperate_forest_is_green_dominant() {
        let [r, g, b] = biome_color(Biome::TemperateForest);
        assert!(
            g > r && g > b,
            "temperate forest expected green-dominant: [{r},{g},{b}]"
        );
    }

    #[test]
    fn martian_dust_plain_is_red_dominant() {
        let [r, g, b] = biome_color(Biome::MartianDustPlain);
        assert!(
            r > g && r > b,
            "martian dust plain expected red-dominant: [{r},{g},{b}]"
        );
    }

    #[test]
    fn ice_sheet_is_bright() {
        let [r, g, b] = biome_color(Biome::IceSheet);
        assert!(r > 200 && g > 200 && b > 200);
    }
}
