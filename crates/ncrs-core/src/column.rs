use crate::sort_key::SortColumn;

/// A column a file panel can show. Name is always shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Column {
    Icon,
    Name,
    Size,
    Modified,
    Created,
    Type,
    Extension,
    Permissions,
    Owner,
    Group,
    Accessed,
    LinkTarget,
}

impl Column {
    pub const ALL: [Column; 12] = [
        Column::Icon,
        Column::Name,
        Column::Size,
        Column::Modified,
        Column::Created,
        Column::Type,
        Column::Extension,
        Column::Permissions,
        Column::Owner,
        Column::Group,
        Column::Accessed,
        Column::LinkTarget,
    ];

    pub const DEFAULT: [Column; 4] = [Column::Icon, Column::Name, Column::Size, Column::Modified];

    /// Stable: this is the spelling in the config file.
    pub fn key(self) -> &'static str {
        match self {
            Column::Icon => "icon",
            Column::Name => "name",
            Column::Size => "size",
            Column::Modified => "modified",
            Column::Created => "created",
            Column::Type => "type",
            Column::Extension => "extension",
            Column::Permissions => "permissions",
            Column::Owner => "owner",
            Column::Group => "group",
            Column::Accessed => "accessed",
            Column::LinkTarget => "link_target",
        }
    }

    pub fn from_key(key: &str) -> Option<Column> {
        Column::ALL.into_iter().find(|column| column.key() == key)
    }

    /// Owner and group exist only where files have a numeric owner.
    pub fn on_this_system(self) -> bool {
        cfg!(unix) || !matches!(self, Column::Owner | Column::Group)
    }

    pub fn sort_column(self) -> Option<SortColumn> {
        match self {
            Column::Name => Some(SortColumn::Name),
            Column::Size => Some(SortColumn::Size),
            Column::Modified => Some(SortColumn::Modified),
            _ => None,
        }
    }
}

/// How a date column writes a time, as it is spelled in the config file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DateFormat {
    /// "5 min ago", "yesterday 14:03", the date from the day before that.
    Relative,
    /// Day and month, with the year when it is not this year.
    Short,
    /// `2026-11-07 14:03`, what the panels showed before the format was a setting.
    #[default]
    Iso,
}

impl DateFormat {
    pub const ALL: [Self; 3] = [Self::Relative, Self::Short, Self::Iso];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Relative => "relative",
            Self::Short => "short",
            Self::Iso => "iso",
        }
    }
}

/// How the size column writes a size, as it is spelled in the config file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SizeFormat {
    Bytes,
    Kilobytes,
    /// `1.4 KB`, what the panels showed before the format was a setting.
    #[default]
    Readable,
}

impl SizeFormat {
    pub const ALL: [Self; 3] = [Self::Bytes, Self::Kilobytes, Self::Readable];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Bytes => "bytes",
            Self::Kilobytes => "kilobytes",
            Self::Readable => "readable",
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_reads_back_to_its_column() {
        for column in Column::ALL {
            assert_eq!(Column::from_key(column.key()), Some(column));
        }
        assert_eq!(Column::from_key("nonsense"), None);
    }
}
