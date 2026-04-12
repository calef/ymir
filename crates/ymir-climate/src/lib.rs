//! Global climate simulation: temperature, moisture, and wind field computation.
//!
//! Computes latitude-dependent temperature gradients, moisture transport,
//! and prevailing wind patterns from the atmosphere model and surface topology.

pub mod climate_field;
pub mod moisture;
pub mod temperature;
pub mod wind;

pub use climate_field::{ClimateConfig, ClimateMap};
pub use moisture::{MoistureConfig, MoistureField, build_moisture_field};
pub use temperature::{TemperatureConfig, TemperatureField, build_temperature_field};
pub use wind::{WindConfig, WindField, WindVector, build_wind_field};
