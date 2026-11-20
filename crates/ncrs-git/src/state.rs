/// What git says about one path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileState {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Ignored,
    Conflicted,
}

impl FileState {
    /// Which state wins when one path has several (staged and changed again, say).
    fn rank(self) -> u8 {
        match self {
            Self::Conflicted => 6,
            Self::Modified => 5,
            Self::Deleted => 4,
            Self::Renamed => 3,
            Self::Added => 2,
            Self::Untracked => 1,
            Self::Ignored => 0,
        }
    }

    pub(crate) fn strongest(self, other: Self) -> Self {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }

    /// What a folder shows for a file inside it. Ignored files do not count: a
    /// folder is only ignored when it is ignored itself.
    pub(crate) fn as_folder_state(self) -> Option<Self> {
        match self {
            Self::Conflicted => Some(Self::Conflicted),
            Self::Modified | Self::Added | Self::Deleted | Self::Renamed => Some(Self::Modified),
            Self::Untracked => Some(Self::Untracked),
            Self::Ignored => None,
        }
    }
}
