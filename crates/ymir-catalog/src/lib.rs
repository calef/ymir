//! Stellar catalog ingestion from Gaia DR3 and the NASA Exoplanet Archive.
//!
//! Provides parsers and index structures for loading real star data, querying
//! by position or identifier, and producing `StarContext` records for the
//! downstream pipeline.

pub mod catalog_index;
pub mod exoplanets;
pub mod gaia;
pub mod star_context;
