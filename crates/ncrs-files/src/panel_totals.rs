//! The totals in the line under a file panel, counted over what the panel
//! shows: a filtered listing or a hit list as it is, not the folder behind it.

use ncrs_core::FileEntry;
use ncrs_i18n::{Language, Msg};
use ncrs_ui_kit::format;

use crate::panel::PanelState;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Totals {
    pub files: usize,
    pub folders: usize,
    /// Files, plus the folders whose size is known.
    pub bytes: u64,
    /// Folders that are not links and have no measured size yet.
    pub folders_without_size: usize,
}

impl Totals {
    fn add(&mut self, entry: &FileEntry) {
        if !entry.is_dir {
            self.files += 1;
            self.bytes += entry.size;
        } else {
            self.folders += 1;
            match entry.folder_size {
                Some(measured) => self.bytes += measured.bytes,
                // Links are not entered, so their size is never measured.
                None if !entry.is_symlink => self.folders_without_size += 1,
                None => {}
            }
        }
    }

    fn entries(&self) -> usize {
        self.files + self.folders
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelTotals {
    pub shown: Totals,
    /// `None` when nothing is tagged.
    pub selected: Option<Totals>,
}

/// The `..` row is not counted.
pub fn panel_totals(panel: &PanelState) -> PanelTotals {
    let mut shown = Totals::default();
    let mut selected = Totals::default();
    for entry in panel.entries.iter().filter(|entry| !entry.is_parent) {
        shown.add(entry);
        if panel.selection.is_tagged(&entry.path) {
            selected.add(entry);
        }
    }
    PanelTotals {
        shown,
        selected: (selected.entries() > 0).then_some(selected),
    }
}

pub fn footer_text(totals: &PanelTotals, lang: Language) -> String {
    let line = match &totals.selected {
        None => lang
            .text(Msg::PanelFooterTotals)
            .replace("{files}", &format::files_counted(totals.shown.files, lang))
            .replace(
                "{folders}",
                &format::folders_counted(totals.shown.folders, lang),
            )
            .replace("{size}", &format::size(totals.shown.bytes)),
        Some(selected) => lang
            .text(Msg::PanelFooterSelected)
            .replace("{selected}", &format::grouped(selected.entries() as u64))
            .replace("{total}", &format::grouped(totals.shown.entries() as u64))
            .replace("{selected_size}", &format::size(selected.bytes))
            .replace("{total_size}", &format::size(totals.shown.bytes)),
    };
    if totals.shown.folders_without_size > 0 {
        lang.text(Msg::PanelFooterWithoutFolderSizes)
            .replace("{line}", &line)
    } else {
        line
    }
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ncrs_core::FolderSize;

    use super::*;

    fn entry(name: &str, is_dir: bool, size: u64) -> FileEntry {
        FileEntry {
            name: name.into(),
            path: PathBuf::from("/d").join(name),
            is_dir,
            is_symlink: false,
            is_parent: false,
            size,
            modified: None,
            folder_size: None,
            details: Default::default(),
        }
    }

    fn measured(name: &str, bytes: u64) -> FileEntry {
        FileEntry {
            folder_size: Some(FolderSize {
                bytes,
                partial: false,
            }),
            ..entry(name, true, 0)
        }
    }

    fn panel(entries: Vec<FileEntry>) -> PanelState {
        let mut panel = PanelState::new(PathBuf::from("/d"));
        panel.entries = entries;
        panel
    }

    #[test]
    fn nothing_selected_counts_files_and_folders_and_skips_the_parent_row() {
        let parent = FileEntry {
            is_parent: true,
            ..entry("..", true, 0)
        };
        let panel = panel(vec![
            parent,
            entry("a", false, 100),
            entry("b", false, 50),
            entry("sub", true, 0),
        ]);
        let totals = panel_totals(&panel);
        assert_eq!(totals.selected, None);
        assert_eq!(totals.shown.files, 2);
        assert_eq!(totals.shown.folders, 1);
        assert_eq!(totals.shown.bytes, 150);
    }

    #[test]
    fn known_folder_sizes_are_added_and_unknown_ones_are_flagged() {
        let panel = panel(vec![
            entry("a", false, 10),
            measured("known", 1000),
            entry("unknown", true, 0),
        ]);
        let shown = panel_totals(&panel).shown;
        assert_eq!(shown.bytes, 1010);
        assert_eq!(shown.folders_without_size, 1);
    }

    #[test]
    fn linked_folders_are_not_reported_as_missing_a_size() {
        let link = FileEntry {
            is_symlink: true,
            ..entry("link", true, 0)
        };
        assert_eq!(
            panel_totals(&panel(vec![link])).shown.folders_without_size,
            0
        );
    }

    #[test]
    fn a_selection_counts_its_entries_against_all() {
        let mut panel = panel(vec![
            entry("a", false, 10),
            entry("b", false, 20),
            measured("d", 100),
        ]);
        panel.selection.toggle(&PathBuf::from("/d/b"));
        panel.selection.toggle(&PathBuf::from("/d/d"));
        let totals = panel_totals(&panel);
        let selected = totals.selected.unwrap();
        assert_eq!((selected.files, selected.folders), (1, 1));
        assert_eq!(selected.bytes, 120);
        assert_eq!(totals.shown.bytes, 130);
    }

    #[test]
    fn a_hit_list_is_counted_as_listed() {
        let mut panel = panel(vec![entry("x", false, 7)]);
        panel.hit_list = Some(crate::hit_list::HitList {
            query: "q".into(),
            scope: PathBuf::from("/d"),
            return_to: PathBuf::from("/e"),
        });
        assert_eq!(panel_totals(&panel).shown.files, 1);
    }

    #[test]
    fn the_text_names_the_missing_folder_sizes() {
        let panel = panel(vec![entry("a", false, 2048), entry("d", true, 0)]);
        let text = footer_text(&panel_totals(&panel), Language::English);
        assert_eq!(text, "1 file, 1 folder · 2.0 KB · without folder sizes");
    }

    #[test]
    fn the_text_of_a_selection_reads_selected_of_all() {
        let mut panel = panel(vec![entry("a", false, 2048), entry("b", false, 2048)]);
        panel.selection.toggle(&PathBuf::from("/d/a"));
        let text = footer_text(&panel_totals(&panel), Language::English);
        assert_eq!(text, "1 of 2 selected · 2.0 KB of 4.0 KB");
    }
}
