//! A fixed store state for trying the lock and the settings page without a
//! signed build.

use crate::status::{Environment, StoreStatus};
use crate::TRIAL_DAYS;

const SECONDS_PER_DAY: i64 = 86_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Simulation {
    /// Trial not started.
    New,
    /// Trial running with this many days left.
    Trial(u32),
    Expired,
    Full,
    /// Downloaded before the app became free.
    Earlier,
}

impl Simulation {
    pub fn parse(text: &str) -> Option<Self> {
        let trimmed = text.trim();
        if let Some(rest) = trimmed.strip_prefix("trial:") {
            let days: u32 = rest.trim().parse().ok()?;
            return (1..=TRIAL_DAYS)
                .contains(&days)
                .then_some(Self::Trial(days));
        }
        match trimmed {
            "new" => Some(Self::New),
            "expired" => Some(Self::Expired),
            "full" => Some(Self::Full),
            "earlier" => Some(Self::Earlier),
            _ => None,
        }
    }

    pub fn status(self, now_unix: i64) -> StoreStatus {
        let base = StoreStatus {
            environment: Some(Environment::Sandbox),
            original_app_version: Some("1.0".to_owned()),
            ..StoreStatus::default()
        };
        let started_days_ago = |days: i64| Some(now_unix - days * SECONDS_PER_DAY);
        match self {
            Self::New => base,
            Self::Trial(days_left) => StoreStatus {
                trial_started_at: started_days_ago(i64::from(TRIAL_DAYS - days_left)),
                ..base
            },
            Self::Expired => StoreStatus {
                trial_started_at: started_days_ago(i64::from(TRIAL_DAYS) + 6),
                ..base
            },
            Self::Full => StoreStatus {
                full_purchased: true,
                ..base
            },
            Self::Earlier => StoreStatus {
                environment: Some(Environment::Production),
                original_app_version: Some("1.6.0".to_owned()),
                ..base
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entitlement::{entitlement, Entitlement, FullKind};

    #[test]
    fn parses_the_words() {
        assert_eq!(Simulation::parse("new"), Some(Simulation::New));
        assert_eq!(Simulation::parse(" trial:5 "), Some(Simulation::Trial(5)));
        assert_eq!(Simulation::parse("expired"), Some(Simulation::Expired));
        assert_eq!(Simulation::parse("full"), Some(Simulation::Full));
        assert_eq!(Simulation::parse("earlier"), Some(Simulation::Earlier));
    }

    #[test]
    fn rejects_the_rest() {
        for text in [
            "", "trial", "trial:", "trial:x", "trial:0", "trial:15", "trial:-1", "Full",
        ] {
            assert_eq!(Simulation::parse(text), None, "{text}");
        }
    }

    #[test]
    fn each_simulation_gives_its_entitlement() {
        let now = 1_700_000_000;
        let decide =
            |simulation: Simulation| entitlement(Ok(simulation.status(now)), None, now, None);
        assert_eq!(decide(Simulation::New), Entitlement::TrialNotStarted);
        assert_eq!(
            decide(Simulation::Trial(5)),
            Entitlement::Trial { days_left: 5 }
        );
        assert_eq!(
            decide(Simulation::Trial(14)),
            Entitlement::Trial { days_left: 14 }
        );
        assert_eq!(decide(Simulation::Expired), Entitlement::Expired);
        assert_eq!(
            decide(Simulation::Full),
            Entitlement::Full(FullKind::Purchased)
        );
        assert_eq!(
            decide(Simulation::Earlier),
            Entitlement::Full(FullKind::EarlierPurchase)
        );
    }
}
