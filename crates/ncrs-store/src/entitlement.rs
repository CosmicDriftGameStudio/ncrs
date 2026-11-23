//! The rule: what the store says, what the app allows. Pure; the time and the
//! version override come in as parameters.

use crate::status::{Environment, StoreError, StoreStatus};
use crate::{FIRST_LOCKED_VERSION, TRIAL_DAYS};

const SECONDS_PER_DAY: i64 = 86_400;
/// A trial with this many days left or fewer shows the reminder.
pub const REMINDER_DAYS: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FullKind {
    Purchased,
    /// Downloaded before the app became free.
    EarlierPurchase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entitlement {
    Unrestricted,
    Full(FullKind),
    Trial { days_left: u32 },
    TrialNotStarted,
    Expired,
}

impl Entitlement {
    pub fn allows_changes(self) -> bool {
        match self {
            Self::Unrestricted | Self::Full(_) | Self::Trial { .. } => true,
            Self::TrialNotStarted | Self::Expired => false,
        }
    }

    pub fn shows_reminder(self) -> bool {
        match self {
            Self::Trial { days_left } => days_left <= REMINDER_DAYS,
            Self::Expired => true,
            Self::Unrestricted | Self::Full(_) | Self::TrialNotStarted => false,
        }
    }
}

/// `fetched` is the answer of the store just now, `cached` the last good one.
/// production only.
/// environment.
pub fn entitlement(
    fetched: Result<StoreStatus, StoreError>,
    cached: Option<StoreStatus>,
    now_unix: i64,
    original_version_override: Option<&str>,
) -> Entitlement {
    let status = match fetched {
        Ok(status) => status,
        Err(_) => match cached {
            Some(status) => status,
            // ponytail: fail open. Without an answer and without a cache we
            // cannot tell a buyer from a stranger, and locking a paying
            // customer out is worse than letting one free rider through.
            None => return Entitlement::Unrestricted,
        },
    };
    if status.unsupported {
        return Entitlement::Unrestricted;
    }
    if status.full_purchased {
        return Entitlement::Full(FullKind::Purchased);
    }
    if bought_before_free(&status, original_version_override) {
        return Entitlement::Full(FullKind::EarlierPurchase);
    }
    match status.trial_started_at {
        Some(started) => trial(started, now_unix),
        None => Entitlement::TrialNotStarted,
    }
}

fn bought_before_free(status: &StoreStatus, original_version_override: Option<&str>) -> bool {
    let version = match (status.environment, original_version_override) {
        // Production reports the real original version, signed by Apple; nothing may replace it.
        // Every download before FIRST_LOCKED_VERSION was a purchase, so a missing value
        // counts for the buyer.
        (Some(Environment::Production), _) => match status.original_app_version.as_deref() {
            Some(version) => version,
            None => return true,
        },
        // Sandbox and Xcode report a made-up "1.0"; a debug build may override it.
        (_, Some(version)) => version,
        _ => return false,
    };
    match (parse_version(version), parse_version(FIRST_LOCKED_VERSION)) {
        (Some(original), Some(first_locked)) => original < first_locked,
        // Production: unreadable counts for the buyer, like a missing value.
        // Sandbox and Xcode: an unreadable override proves nothing.
        _ => matches!(status.environment, Some(Environment::Production)),
    }
}

fn parse_version(text: &str) -> Option<Vec<u32>> {
    let mut numbers: Vec<u32> = text
        .trim()
        .split('.')
        .map(|part| part.parse().ok())
        .collect::<Option<_>>()?;
    // "1.0" compares like "1.0.0".
    while numbers.len() < 3 {
        numbers.push(0);
    }
    Some(numbers)
}

fn trial(started: i64, now_unix: i64) -> Entitlement {
    let end = started.saturating_add(i64::from(TRIAL_DAYS) * SECONDS_PER_DAY);
    let left = end.saturating_sub(now_unix);
    if left <= 0 {
        return Entitlement::Expired;
    }
    let days = left.unsigned_abs().div_ceil(SECONDS_PER_DAY.unsigned_abs());
    Entitlement::Trial {
        days_left: u32::try_from(days).unwrap_or(TRIAL_DAYS).min(TRIAL_DAYS),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = SECONDS_PER_DAY;
    const START: i64 = 1_700_000_000;

    fn status(environment: Environment, version: &str) -> StoreStatus {
        StoreStatus {
            environment: Some(environment),
            original_app_version: Some(version.to_owned()),
            ..StoreStatus::default()
        }
    }

    fn started(mut status: StoreStatus) -> StoreStatus {
        status.trial_started_at = Some(START);
        status
    }

    fn decide(status: StoreStatus, now: i64) -> Entitlement {
        entitlement(Ok(status), None, now, None)
    }

    #[test]
    fn trial_not_started() {
        assert_eq!(
            decide(status(Environment::Production, "1.7.0"), START),
            Entitlement::TrialNotStarted
        );
        assert!(!Entitlement::TrialNotStarted.allows_changes());
    }

    #[test]
    fn day_13_still_works_with_a_reminder() {
        let result = decide(
            started(status(Environment::Production, "1.7.0")),
            START + 13 * DAY,
        );
        assert_eq!(result, Entitlement::Trial { days_left: 1 });
        assert!(result.allows_changes());
        assert!(result.shows_reminder());
    }

    #[test]
    fn day_15_is_expired() {
        let result = decide(
            started(status(Environment::Production, "1.7.0")),
            START + 15 * DAY,
        );
        assert_eq!(result, Entitlement::Expired);
        assert!(!result.allows_changes());
        assert!(result.shows_reminder());
    }

    #[test]
    fn exactly_14_days_is_expired_and_start_has_14_left() {
        let base = started(status(Environment::Production, "1.7.0"));
        assert_eq!(decide(base.clone(), START + 14 * DAY), Entitlement::Expired);
        assert_eq!(
            decide(base.clone(), START),
            Entitlement::Trial { days_left: 14 }
        );
        // A clock set back never gives more than 14 days.
        assert_eq!(
            decide(base, START - 3 * DAY),
            Entitlement::Trial { days_left: 14 }
        );
    }

    #[test]
    fn early_trial_has_no_reminder() {
        let result = decide(
            started(status(Environment::Production, "1.7.0")),
            START + 5 * DAY,
        );
        assert_eq!(result, Entitlement::Trial { days_left: 9 });
        assert!(!result.shows_reminder());
    }

    #[test]
    fn bought_beats_an_expired_trial() {
        let mut bought = started(status(Environment::Production, "1.7.0"));
        bought.full_purchased = true;
        assert_eq!(
            decide(bought, START + 40 * DAY),
            Entitlement::Full(FullKind::Purchased)
        );
    }

    #[test]
    fn refund_drops_back_to_the_trial_state() {
        // A refunded purchase is missing from the entitlements.
        let refunded = started(status(Environment::Production, "1.7.0"));
        assert_eq!(
            decide(refunded.clone(), START + DAY),
            Entitlement::Trial { days_left: 13 }
        );
        assert_eq!(decide(refunded, START + 20 * DAY), Entitlement::Expired);
    }

    #[test]
    fn old_production_download_is_full() {
        let result = decide(status(Environment::Production, "1.6.2"), START);
        assert_eq!(result, Entitlement::Full(FullKind::EarlierPurchase));
        assert_eq!(
            decide(status(Environment::Production, "1.0"), START),
            result
        );
        assert_eq!(
            decide(status(Environment::Production, "1.7.0"), START),
            Entitlement::TrialNotStarted
        );
        assert_eq!(
            decide(status(Environment::Production, "2.0"), START),
            Entitlement::TrialNotStarted
        );
    }

    #[test]
    fn sandbox_and_xcode_ignore_the_made_up_version() {
        assert_eq!(
            decide(status(Environment::Sandbox, "1.0"), START),
            Entitlement::TrialNotStarted
        );
        assert_eq!(
            decide(status(Environment::Xcode, "1.0"), START),
            Entitlement::TrialNotStarted
        );
    }

    #[test]
    fn override_applies_outside_production_only() {
        let sandbox = status(Environment::Sandbox, "1.0");
        assert_eq!(
            entitlement(Ok(sandbox.clone()), None, START, Some("1.6.0")),
            Entitlement::Full(FullKind::EarlierPurchase)
        );
        assert_eq!(
            entitlement(Ok(sandbox), None, START, Some("1.7.1")),
            Entitlement::TrialNotStarted
        );
        let production = status(Environment::Production, "1.0");
        // Production keeps its real version: the override is ignored.
        assert_eq!(
            entitlement(Ok(production.clone()), None, START, Some("1.7.0")),
            Entitlement::Full(FullKind::EarlierPurchase)
        );
        let late = status(Environment::Production, "1.7.0");
        assert_eq!(
            entitlement(Ok(late), None, START, Some("1.0")),
            Entitlement::TrialNotStarted
        );
    }

    #[test]
    fn production_with_an_unreadable_version_counts_for_the_buyer() {
        let earlier = Entitlement::Full(FullKind::EarlierPurchase);
        assert_eq!(
            decide(status(Environment::Production, "beta"), START),
            earlier
        );
        assert_eq!(decide(status(Environment::Production, ""), START), earlier);
        let mut missing = status(Environment::Production, "1.0");
        missing.original_app_version = None;
        assert_eq!(decide(missing, START), earlier);
    }

    #[test]
    fn sandbox_without_a_readable_version_grants_nothing() {
        let mut missing = status(Environment::Sandbox, "1.0");
        missing.original_app_version = None;
        assert_eq!(decide(missing, START), Entitlement::TrialNotStarted);
        let sandbox = status(Environment::Sandbox, "1.0");
        assert_eq!(
            entitlement(Ok(sandbox), None, START, Some("beta")),
            Entitlement::TrialNotStarted
        );
    }

    #[test]
    fn unsupported_system_is_unrestricted() {
        let unsupported = StoreStatus {
            unsupported: true,
            ..StoreStatus::default()
        };
        assert_eq!(decide(unsupported, START), Entitlement::Unrestricted);
        assert!(Entitlement::Unrestricted.allows_changes());
    }

    #[test]
    fn failed_fetch_uses_the_cache() {
        let cached = started(status(Environment::Production, "1.7.0"));
        let failed = Err(StoreError::Store("offline".to_owned()));
        assert_eq!(
            entitlement(failed, Some(cached), START + 2 * DAY, None),
            Entitlement::Trial { days_left: 12 }
        );
    }

    #[test]
    fn failed_fetch_without_cache_fails_open() {
        let failed = Err(StoreError::Store("offline".to_owned()));
        assert_eq!(
            entitlement(failed, None, START, None),
            Entitlement::Unrestricted
        );
    }
}
