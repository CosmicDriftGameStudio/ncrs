use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortColumn {
    #[default]
    Name,
    Size,
    Modified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SortKey {
    pub column: SortColumn,
    pub descending: bool,
}

impl SortKey {
    /// The key after asking for `column`: the same column again reverses the
    /// direction, another column starts ascending.
    pub fn requested(self, column: SortColumn) -> Self {
        Self {
            column,
            descending: self.column == column && !self.descending,
        }
    }
}
