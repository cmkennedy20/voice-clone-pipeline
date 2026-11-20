//! Idempotent structured `tracing` subscriber initialization.

use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Initializes the global `tracing` subscriber once per process from an explicit filter directive.
///
/// Subsequent calls in integration test suites safely ignore the already-initialized error.
pub fn init_telemetry(directive: &str) {
    let filter = EnvFilter::try_new(directive).unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer())
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::init_telemetry;

    #[test]
    fn repeated_init_telemetry_calls_are_idempotent() {
        init_telemetry("info");
        init_telemetry("debug");
    }
}
