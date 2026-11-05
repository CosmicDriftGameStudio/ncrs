//! The pages of the settings tab and what the command list can open on them.

/// A page of the settings tab, one entry of its feature list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsFeature {
    Look,
    Language,
    Programs,
    Keys,
    Servers,
    System,
    Index,
    Hermes,
    Rules,
    Tags,
    Reports,
    ReadErrors,
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
    pub const ALL: [Self; 12] = [
        Self::Look,
        Self::Language,
        Self::Keys,
        Self::System,
        Self::Programs,
        Self::Servers,
        Self::Index,
        Self::ReadErrors,
        Self::Reports,
        Self::Hermes,
        Self::Rules,
        Self::Tags,
    ];

    /// Pages whose rows the app puts in: they belong to the Hermes module,
    /// which the settings crate does not know.
    pub fn is_filled_by_app(self) -> bool {
        matches!(
            self,
            Self::Rules | Self::Tags | Self::Reports | Self::ReadErrors
        )
    }

    pub fn group(self) -> SettingsGroup {
        match self {
            Self::Look | Self::Language | Self::Keys | Self::System => SettingsGroup::General,
            Self::Programs | Self::Servers => SettingsGroup::Files,
            Self::Index | Self::ReadErrors | Self::Reports => SettingsGroup::Documents,
            Self::Hermes | Self::Rules | Self::Tags => SettingsGroup::Hermes,
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
                Look, Language, Keys, System, Programs, Servers, Index, ReadErrors, Reports,
                Hermes, Rules, Tags
            ]
        );
        let expected = [
            (Look, SettingsGroup::General),
            (Language, SettingsGroup::General),
            (Keys, SettingsGroup::General),
            (System, SettingsGroup::General),
            (Programs, SettingsGroup::Files),
            (Servers, SettingsGroup::Files),
            (Index, SettingsGroup::Documents),
            (ReadErrors, SettingsGroup::Documents),
            (Reports, SettingsGroup::Documents),
            (Hermes, SettingsGroup::Hermes),
            (Rules, SettingsGroup::Hermes),
            (Tags, SettingsGroup::Hermes),
        ];
        for (feature, group) in expected {
            assert_eq!(feature.group(), group, "{feature:?}");
        }
        let mut runs: Vec<SettingsGroup> = SettingsFeature::ALL.iter().map(|f| f.group()).collect();
        runs.dedup();
        assert_eq!(runs.len(), 4, "each group is one run in ALL");
    }
}
