//! Global climate simulation: temperature, moisture, and wind field computation.
//!
//! Computes latitude-dependent temperature gradients, moisture transport,
//! and prevailing wind patterns from the atmosphere model and surface topology.

pub mod climate_field;
pub mod moisture;
pub mod temperature;
pub mod wind;
