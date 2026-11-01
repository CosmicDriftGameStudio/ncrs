//! What the Hermes column shows for a file and a folder, as plain data.

use ncrs_i18n::{Language, Msg};
use ncrs_index::StatusCounts;

/// What the status column shows for a file, strongest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HermesFileState {
    /// Indexed, but no rule or reading has placed it yet.
    Unread,
    /// A rule placed it; it is filed on the next round.
    Planned,
    Reading,
    Filed,
    /// Hermes asked the user a question about the file.
    Inquiry,
    Error,
    Excluded,
}

/// The part a configured folder plays for Hermes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderRole {
    Inbox,
    /// An archive that files are moved into.
    FilingLocation,
    /// An archive that is only read.
    Archive,
}

/// The counts and flags of one folder. `None` counts are not read yet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderSummary {
    pub direct: Option<StatusCounts>,
    pub deep: Option<StatusCounts>,
    pub excluded: bool,
    /// Errors of the current pipeline run for files in or below the folder.
    pub errors: usize,
    /// Files in or below the folder that a worker is reading right now.
    pub reading: usize,
}

impl FolderSummary {
    /// The deep counts once they are there, until then the direct ones.
    fn shown_counts(&self) -> Option<StatusCounts> {
        self.deep.or(self.direct)
    }
}

/// How many files are in one state, for the summary of a folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SummaryItem {
    pub state: HermesFileState,
    pub count: u64,
}

/// The states that occur in the folder, in the order unread, planned, reading, filed,
/// inquiry, error. Empty while nothing is counted.
pub fn summary_items(summary: &FolderSummary) -> Vec<SummaryItem> {
    let Some(counts) = summary.shown_counts() else {
        return Vec::new();
    };
    [
        (HermesFileState::Unread, counts.new),
        (HermesFileState::Planned, counts.planned),
        (HermesFileState::Reading, summary.reading as u64),
        (HermesFileState::Filed, counts.filed),
        (HermesFileState::Inquiry, counts.in_review),
        (HermesFileState::Error, summary.errors as u64),
    ]
    .into_iter()
    .filter(|(_, count)| *count > 0)
    .map(|(state, count)| SummaryItem { state, count })
    .collect()
}

/// Up to 9999 as it is, then with k and M: 12.4k, 31k, 1.2M. Cut, not rounded.
pub fn abbreviated(count: u64) -> String {
    const THOUSAND: u64 = 1_000;
    const MILLION: u64 = 1_000_000;
    const SHOWN_IN_FULL: u64 = 9_999;
    const ONE_DECIMAL_BELOW: u64 = 100_000;
    if count <= SHOWN_IN_FULL {
        return count.to_string();
    }
    let (unit, suffix) = if count >= MILLION {
        (MILLION, 'M')
    } else {
        (THOUSAND, 'k')
    };
    let whole = count.div_euclid(unit);
    let tenth = count.rem_euclid(unit).div_euclid(unit.div_euclid(10));
    if tenth == 0 || (suffix == 'k' && count >= ONE_DECIMAL_BELOW) {
        format!("{whole}{suffix}")
    } else {
        format!("{whole}.{tenth}{suffix}")
    }
}

pub const fn state_message(state: HermesFileState) -> Msg {
    match state {
        HermesFileState::Unread => Msg::HermesColumnStateUnread,
        HermesFileState::Planned => Msg::HermesColumnStatePlanned,
        HermesFileState::Reading => Msg::HermesColumnStateReading,
        HermesFileState::Filed => Msg::HermesColumnStateFiled,
        HermesFileState::Inquiry => Msg::HermesColumnStateInquiry,
        HermesFileState::Error => Msg::HermesColumnStateError,
        HermesFileState::Excluded => Msg::HermesColumnStateExcluded,
    }
}

pub fn state_label(state: HermesFileState, lang: Language) -> &'static str {
    lang.text(state_message(state))
}

pub const fn role_message(role: FolderRole) -> Msg {
    match role {
        FolderRole::Inbox => Msg::HermesColumnRoleInbox,
        FolderRole::FilingLocation => Msg::HermesColumnRoleFilingLocation,
        FolderRole::Archive => Msg::HermesColumnRoleArchive,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(counts: StatusCounts, reading: usize, errors: usize) -> FolderSummary {
        FolderSummary {
            direct: None,
            deep: Some(counts),
            excluded: false,
            errors,
            reading,
        }
    }

    #[test]
    fn counts_above_9999_are_abbreviated() {
        let shown: Vec<String> = [
            0, 7, 9_999, 10_000, 12_400, 31_000, 99_950, 100_000, 1_250_000,
        ]
        .into_iter()
        .map(abbreviated)
        .collect();
        assert_eq!(
            shown,
            ["0", "7", "9999", "10k", "12.4k", "31k", "99.9k", "100k", "1.2M"]
        );
    }

    #[test]
    fn the_summary_lists_only_occurring_states_in_a_fixed_order() {
        let counts = StatusCounts {
            new: 2,
            planned: 1,
            in_review: 4,
            filed: 0,
        };

        let items = summary_items(&summary(counts, 1, 5));

        let states: Vec<(HermesFileState, u64)> =
            items.iter().map(|item| (item.state, item.count)).collect();
        assert_eq!(
            states,
            [
                (HermesFileState::Unread, 2),
                (HermesFileState::Planned, 1),
                (HermesFileState::Reading, 1),
                (HermesFileState::Inquiry, 4),
                (HermesFileState::Error, 5),
            ]
        );
        assert!(summary_items(&FolderSummary::default()).is_empty());
    }

    #[test]
    fn deep_counts_replace_the_direct_ones() {
        let direct = StatusCounts {
            new: 1,
            ..StatusCounts::default()
        };
        let deep = StatusCounts {
            new: 9,
            ..StatusCounts::default()
        };
        let mut shown = FolderSummary {
            direct: Some(direct),
            ..FolderSummary::default()
        };
        assert_eq!(
            summary_items(&shown).first().map(|item| item.count),
            Some(1)
        );

        shown.deep = Some(deep);

        assert_eq!(
            summary_items(&shown).first().map(|item| item.count),
            Some(9)
        );
    }
}
