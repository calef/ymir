//! Atmospheric modeling: retention, composition, and greenhouse effect calculations.
//!
//! Determines whether a planet retains an atmosphere, derives its likely
//! composition from outgassing and stellar wind stripping models, and computes
//! the resulting greenhouse warming.

pub mod atmosphere_model;
pub mod composition;
pub mod greenhouse;
pub mod retention;
