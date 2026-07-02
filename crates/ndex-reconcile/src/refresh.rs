//! Opportunistic pre-search reconciliation (PRD §6).

use std::time::Duration;

use ndex_core::error::Result;
use ndex_store::Store;

/// Index staleness relative to the configured thresholds (PRD §6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Staleness {
    /// Younger than `threshold` — skip refresh.
    Fresh,
    /// Between `threshold` and `warn_threshold` — run a quick reconcile.
    Stale,
    /// Older than `warn_threshold`, or never reconciled — warn the user.
    Warn,
}

/// Classify index staleness from the last reconciliation time and `now` (PRD §6.2).
pub fn staleness(
    last_reconciled_ns: Option<i64>,
    now_ns: i64,
    threshold: Duration,
    warn_threshold: Duration,
) -> Staleness {
    let Some(last) = last_reconciled_ns else {
        return Staleness::Warn;
    };
    let age_ns = (i128::from(now_ns) - i128::from(last)).max(0) as u128;
    if age_ns < threshold.as_nanos() {
        Staleness::Fresh
    } else if age_ns < warn_threshold.as_nanos() {
        Staleness::Stale
    } else {
        Staleness::Warn
    }
}

/// Run a time-boxed quick reconcile (Phase 1 + 2 + new-file Phase 3) under a non-blocking lock,
/// skipping silently if another writer holds it (PRD §6.2).
pub fn quick_reconcile(store: &mut Store, budget: Duration) -> Result<()> {
    let _ = (store, budget);
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR_NS: i64 = 3_600_000_000_000;
    const DAY_NS: i64 = 24 * HOUR_NS;

    #[test]
    fn staleness_classification() {
        let threshold = Duration::from_secs(3_600); // 1h
        let warn = Duration::from_secs(604_800); // 7d
        let now = 1_000_000_000_000_000;

        assert_eq!(staleness(Some(now), now, threshold, warn), Staleness::Fresh);
        assert_eq!(
            staleness(Some(now - 2 * HOUR_NS), now, threshold, warn),
            Staleness::Stale
        );
        assert_eq!(
            staleness(Some(now - 10 * DAY_NS), now, threshold, warn),
            Staleness::Warn
        );
        assert_eq!(staleness(None, now, threshold, warn), Staleness::Warn);
    }
}
