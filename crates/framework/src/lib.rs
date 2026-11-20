//! Tier 3 infrastructure, configuration, database pool, DSP codecs, and external clients.

pub mod auth;
pub mod clients;
pub mod config;
pub mod context;
pub mod db;
pub mod desktop;
pub mod dsp;
pub mod telemetry;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
