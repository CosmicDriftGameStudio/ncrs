//! The `[git_view]` section: what the Git tab remembers between runs.

use std::collections::BTreeMap;

/// What the Git tab remembers: how wide its commit columns are, and per
/// repository which groups of the left list are folded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitViewSettings {
    pub author_width: Option<u16>,
    pub date_width: Option<u16>,
    pub sha_width: Option<u16>,
    /// The folded groups of each repository, by the folder of its root.
    pub folded: BTreeMap<String, Vec<String>>,
}

impl GitViewSettings {
    /// The groups folded in the repository at `root`.
    pub fn folded_in(&self, root: &str) -> &[String] {
        self.folded.get(root).map_or(&[], Vec::as_slice)
    }

    /// These settings with the folded groups of `root` replaced; none left drops the entry.
    pub fn with_folded(&self, root: &str, folded: Vec<String>) -> Self {
        let mut settings = self.clone();
        if folded.is_empty() {
            settings.folded.remove(root);
        } else {
            settings.folded.insert(root.to_owned(), folded);
        }
        settings
    }
}
