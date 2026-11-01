use serde::{Deserialize, Serialize};

/// Identifies one of the two file panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PanelSide {
    Left,
    Right,
}

impl PanelSide {
    pub fn other(self) -> Self {
        match self {
            PanelSide::Left => PanelSide::Right,
            PanelSide::Right => PanelSide::Left,
        }
    }
}
