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
    Documents,
}

impl SettingsFeature {
    pub const ALL: [Self; 12] = [
        Self::Look,
        Self::Language,
        Self::Programs,
        Self::Keys,
        Self::Servers,
        Self::System,
        Self::Index,
        Self::Hermes,
        Self::Rules,
        Self::Tags,
        Self::Reports,
        Self::ReadErrors,
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
            Self::Look
            | Self::Language
            | Self::Programs
            | Self::Keys
            | Self::Servers
            | Self::System => SettingsGroup::General,
            Self::Index
            | Self::Hermes
            | Self::Rules
            | Self::Tags
            | Self::Reports
            | Self::ReadErrors => SettingsGroup::Documents,
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
