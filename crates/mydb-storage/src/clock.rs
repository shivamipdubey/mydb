//! Where the current time comes from.
//!
//! Injected rather than read directly, because
//! docs/18-testing-strategy.md requires proving that a recovery bin entry
//! expires and purges after 30 days "with a manipulated clock, not a real
//! 30-day wait". A retention rule that can only be tested by waiting a month
//! is a retention rule nobody tests.

use chrono::{DateTime, Duration, Utc};

/// The source of the current time.
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;

    /// The current time as stored: RFC 3339, seconds precision, UTC.
    fn now_rfc3339(&self) -> String {
        self.now()
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    }
}

/// The real clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// A clock that reports whatever it is told to.
///
/// Public rather than test-only: the retention rule it exists to test lives
/// in this crate, but the tasks that build on it are in others, and a
/// retention window nobody can advance is a retention window nobody can
/// verify.
#[derive(Debug, Clone)]
pub struct FixedClock {
    now: DateTime<Utc>,
}

impl FixedClock {
    pub fn at(now: DateTime<Utc>) -> Self {
        Self { now }
    }

    /// A fixed, arbitrary starting point, so a test's dates read the same on
    /// every run.
    pub fn epoch() -> Self {
        Self::at(
            DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                .map(|value| value.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
        )
    }

    pub fn advance_days(&mut self, days: i64) {
        self.now += Duration::days(days);
    }

    pub fn advance_seconds(&mut self, seconds: i64) {
        self.now += Duration::seconds(seconds);
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.now
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn a_fixed_clock_does_not_move_on_its_own() {
        let clock = FixedClock::epoch();
        assert_eq!(clock.now(), clock.now());
        assert_eq!(clock.now_rfc3339(), "2026-01-01T00:00:00Z");
    }

    #[test]
    fn a_fixed_clock_advances_only_when_told() {
        let mut clock = FixedClock::epoch();
        clock.advance_days(30);
        assert_eq!(clock.now_rfc3339(), "2026-01-31T00:00:00Z");
    }
}
