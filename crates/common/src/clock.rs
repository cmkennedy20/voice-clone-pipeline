//! Deterministic-friendly UTC wall-clock abstraction.

/// Synchronous UTC clock port allowing deterministic time injection in tests.
pub trait Clock: Send + Sync {
    /// Returns the current UTC timestamp.
    fn now_utc(&self) -> chrono::DateTime<chrono::Utc>;
}

/// Production [`Clock`] backed by the operating system's real UTC clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_utc(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeZone, Utc};

    use super::{Clock, SystemClock};

    struct FixedTestClock(DateTime<Utc>);

    impl Clock for FixedTestClock {
        fn now_utc(&self) -> DateTime<Utc> {
            self.0
        }
    }

    #[test]
    fn system_clock_returns_modern_utc_timestamp() {
        let now = SystemClock.now_utc();
        let min_expected = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        assert!(now >= min_expected);
    }

    #[test]
    fn clock_trait_object_preserves_injected_timestamp() {
        let pinned = Utc.with_ymd_and_hms(2026, 11, 18, 4, 0, 0).unwrap();
        let clock: &dyn Clock = &FixedTestClock(pinned);
        assert_eq!(clock.now_utc(), pinned);
    }
}
