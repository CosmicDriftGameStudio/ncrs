//! The `[git]` section: how a pull joins what it fetched, and whether the Git
//! tab fetches in the background.

use serde::Deserialize;

/// What a pull does when the branches have diverged. Chosen by the user once,
/// the Pull menu picks another way for a single pull.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PullDefault {
    Merge,
    Rebase,
    /// Only fast-forward: a pull that would need a merge stops and asks.
    #[default]
    FastForwardOnly,
}

impl PullDefault {
    pub const ALL: [Self; 3] = [Self::Merge, Self::Rebase, Self::FastForwardOnly];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Rebase => "rebase",
            Self::FastForwardOnly => "ff_only",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|choice| choice.name() == name)
    }
}

/// How often the Git tab fetches the repos the user opened, in the background.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FetchInterval {
    Off,
    #[default]
    Every15Minutes,
    Hourly,
}

impl FetchInterval {
    pub const ALL: [Self; 3] = [Self::Off, Self::Every15Minutes, Self::Hourly];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Every15Minutes => "15min",
            Self::Hourly => "hourly",
        }
    }

    /// The time between two fetches; `None` when it is off.
    pub const fn period(self) -> Option<std::time::Duration> {
        match self {
            Self::Off => None,
            Self::Every15Minutes => Some(std::time::Duration::from_secs(15 * 60)),
            Self::Hourly => Some(std::time::Duration::from_secs(60 * 60)),
        }
    }

    fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|choice| choice.name() == name)
    }
}

/// The `[git]` section as written, unchecked.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitSection {
    /// `merge`, `rebase` or `ff_only`; anything else is the default.
    #[serde(default)]
    pub pull: Option<String>,
    /// `off`, `15min` or `hourly`; anything else is the default.
    #[serde(default)]
    pub fetch: Option<String>,
}

/// What the Git tab reads from `[git]`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GitSettings {
    pub pull: PullDefault,
    pub fetch: FetchInterval,
}

pub fn resolve_git(section: &GitSection) -> GitSettings {
    GitSettings {
        pull: section
            .pull
            .as_deref()
            .and_then(PullDefault::parse)
            .unwrap_or_default(),
        fetch: section
            .fetch
            .as_deref()
            .and_then(FetchInterval::parse)
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_written_is_fast_forward_only_and_every_quarter_hour() {
        let settings = resolve_git(&GitSection::default());
        assert_eq!(settings.pull, PullDefault::FastForwardOnly);
        assert_eq!(settings.fetch, FetchInterval::Every15Minutes);
    }

    #[test]
    fn a_value_that_is_not_known_is_the_default() {
        let section = GitSection {
            pull: Some("squash".to_owned()),
            fetch: Some("daily".to_owned()),
        };
        assert_eq!(resolve_git(&section), GitSettings::default());
    }

    #[test]
    fn every_choice_is_read_back_from_its_name() {
        for pull in PullDefault::ALL {
            let section = GitSection {
                pull: Some(pull.name().to_owned()),
                fetch: None,
            };
            assert_eq!(resolve_git(&section).pull, pull);
        }
        for fetch in FetchInterval::ALL {
            let section = GitSection {
                pull: None,
                fetch: Some(fetch.name().to_owned()),
            };
            assert_eq!(resolve_git(&section).fetch, fetch);
        }
    }
}
