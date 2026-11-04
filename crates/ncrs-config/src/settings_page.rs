//! The pages of the settings tab and what the command list can open on them.

/// A page of the settings tab, one entry of its feature list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsFeature {
    Look,
    Language,
    Programs,
    Keys,
    Servers,
    Index,
    Hermes,
}

/// The heading over a run of pages in the feature list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsGroup {
    General,
    Documents,
}

impl SettingsFeature {
    pub const ALL: [Self; 7] = [
        Self::Look,
        Self::Language,
        Self::Programs,
        Self::Keys,
        Self::Servers,
        Self::Index,
        Self::Hermes,
    ];

    pub fn group(self) -> SettingsGroup {
        match self {
            Self::Look | Self::Language | Self::Programs | Self::Keys | Self::Servers => {
                SettingsGroup::General
            }
            Self::Index | Self::Hermes => SettingsGroup::Documents,
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
