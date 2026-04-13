//! Stellar catalog ingestion from Gaia DR3 and the NASA Exoplanet Archive.
//!
//! Provides parsers and index structures for loading real star data, querying
//! by position or identifier, and producing `StarContext` records for the
//! downstream pipeline.

pub mod catalog;
pub mod catalog_index;
pub mod exoplanet;
pub mod exoplanets;
pub mod gaia;
pub mod sol;
pub mod star_context;

pub use catalog::{Catalog, CatalogError, CatalogQuery, StarSummary};
pub use catalog_index::{CatalogIndex, CatalogIndexError, IndexedSpectralType, StarInfo};
pub use exoplanet::{ExoplanetCatalog, ExoplanetError, ExoplanetRecord};
pub use gaia::{GaiaError, GaiaReader, StarRow};
pub use sol::sol_context;
pub use star_context::GAIA_DR3_RELEASE_DATE;
