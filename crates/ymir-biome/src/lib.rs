//! Biome classification and palette assignment from climate data.
//!
//! Maps temperature and moisture values to biome types using a Whittaker-diagram
//! approach, with Markov-chain transitions and weighted palette selection for
//! visual variety.

pub mod markov;
pub mod palette;
pub mod weight_schema;
pub mod whittaker;

pub use markov::{SmoothingConfig, smooth_biomes};
pub use palette::{Biome, BiomePalette, palette_for};
pub use weight_schema::{BiomeTransitions, default_transitions};
pub use whittaker::classify;
