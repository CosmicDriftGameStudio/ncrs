//! The pages of the settings tab and what the command list can open on them.

/// A page of the settings tab, one entry of its feature list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsFeature {
    Look,
    Language,
    Programs,
    Keys,
    Servers,
    Columns,
    Favourites,
    Notes,
    /// The repo manager: where it looks for repositories and how its bulk actions run.
    Git,
    /// "Platz schaffen": what counts as safe, what is never touched, when the drive counts as full.
    Space,
    System,
    Updates,
    /// The trial and the purchase of the App Store build.
    FullVersion,
    Index,
    /// The one model of Hermes and the switch that turns him on.
    Ai,
    /// Sources and targets of the filing.
    Hermes,
    /// What Hermes does now and what it filed.
    Activity,
    Tags,
    Reports,
    ReadErrors,
    Usage,
}

/// The heading over a run of pages in the feature list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsGroup {
    General,
    Files,
    Documents,
    Hermes,
}

impl SettingsFeature {
    pub const ALL: [Self; 21] = [
        Self::Look,
        Self::Language,
        Self::Keys,
        Self::System,
        Self::Updates,
        Self::FullVersion,
        Self::Programs,
        Self::Servers,
        Self::Columns,
        Self::Favourites,
        Self::Notes,
        Self::Git,
        Self::Space,
        Self::Index,
        Self::ReadErrors,
        Self::Reports,
        Self::Ai,
        Self::Hermes,
        Self::Activity,
        Self::Tags,
        Self::Usage,
    ];

    /// Pages whose rows the app puts in: they belong to the Hermes module,
    /// which the settings crate does not know.
    pub fn is_filled_by_app(self) -> bool {
        matches!(
            self,
            Self::Hermes
                | Self::Tags
                | Self::Reports
                | Self::ReadErrors
                | Self::Activity
                | Self::Updates
        )
    }

    pub fn group(self) -> SettingsGroup {
        match self {
            Self::Look
            | Self::Language
            | Self::Keys
            | Self::System
            | Self::Updates
            | Self::FullVersion => SettingsGroup::General,
            Self::Programs
            | Self::Servers
            | Self::Columns
            | Self::Favourites
            | Self::Notes
            | Self::Git
            | Self::Space => SettingsGroup::Files,
            Self::Index | Self::ReadErrors | Self::Reports => SettingsGroup::Documents,
            Self::Ai | Self::Hermes | Self::Activity | Self::Tags | Self::Usage => {
                SettingsGroup::Hermes
            }
        }
    }
}

/// What the command list can open: a page, or one setting of a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsTarget {
    pub feature: SettingsFeature,
    /// The label of the row; `None` for the page itself.
    pub row: Option<String>,
    pub label: String,
    pub theme_name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_come_in_the_order_of_their_groups() {
        use SettingsFeature::*;
        assert_eq!(
            SettingsFeature::ALL,
            [
                Look,
                Language,
                Keys,
                System,
                Updates,
                FullVersion,
                Programs,
                Servers,
                Columns,
                Favourites,
                Notes,
                Git,
                Space,
                Index,
                ReadErrors,
                Reports,
                Ai,
                Hermes,
                Activity,
                Tags,
                Usage
            ]
        );
        let expected = [
            (Look, SettingsGroup::General),
            (Language, SettingsGroup::General),
            (Keys, SettingsGroup::General),
            (System, SettingsGroup::General),
            (Updates, SettingsGroup::General),
            (FullVersion, SettingsGroup::General),
            (Programs, SettingsGroup::Files),
            (Servers, SettingsGroup::Files),
            (Columns, SettingsGroup::Files),
            (Favourites, SettingsGroup::Files),
            (Notes, SettingsGroup::Files),
            (Git, SettingsGroup::Files),
            (Space, SettingsGroup::Files),
            (Index, SettingsGroup::Documents),
            (ReadErrors, SettingsGroup::Documents),
            (Reports, SettingsGroup::Documents),
            (Ai, SettingsGroup::Hermes),
            (Hermes, SettingsGroup::Hermes),
            (Activity, SettingsGroup::Hermes),
            (Tags, SettingsGroup::Hermes),
            (Usage, SettingsGroup::Hermes),
        ];
        for (feature, group) in expected {
            assert_eq!(feature.group(), group, "{feature:?}");
        }
        let mut runs: Vec<SettingsGroup> = SettingsFeature::ALL.iter().map(|f| f.group()).collect();
        runs.dedup();
        assert_eq!(runs.len(), 4, "each group is one run in ALL");
    }
}
