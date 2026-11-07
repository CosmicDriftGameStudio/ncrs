//! Renaming the entry under the cursor in place: the input in its row, what is
//! typed into it, and what a confirmed name asks of the app.

use std::path::{Path, PathBuf};

use iced::widget::{operation, text_input};
use iced::Task;
use ncrs_core::PanelSide;
use ncrs_ui_kit::rename_row::RENAME_INPUT_ID;

use crate::action::Action;
use crate::message::PanelMessage;
use crate::state::Panels;

/// The name input of one row, while it is open. Keyed by the path of the
/// entry rather than its row, since a reload can move the row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameState {
    pub path: PathBuf,
    pub draft: String,
    /// Why the last confirmed name was refused.
    pub error: Option<String>,
    /// The rename of a confirmed name is running.
    pub busy: bool,
}

/// How many graphemes of `name` are selected when the input opens: the name
/// without its extension for a file, everything for a folder, a file without
/// an extension and a dotfile. `None` selects all of it.
pub fn selected_graphemes(name: &str, is_dir: bool) -> Option<usize> {
    if is_dir {
        return None;
    }
    let dot = name.rfind('.')?;
    let has_stem = dot > 0;
    let has_extension = dot + 1 < name.len();
    (has_stem && has_extension).then(|| text_input::Value::new(&name[..dot]).len())
}

impl Panels {
    /// The panel whose name input is open.
    pub fn renaming_side(&self) -> Option<PanelSide> {
        [PanelSide::Left, PanelSide::Right]
            .into_iter()
            .find(|side| self.panel(*side).rename.is_some())
    }

    fn rename_mut(&mut self) -> Option<&mut RenameState> {
        let side = self.renaming_side()?;
        self.panel_mut(side).rename.as_mut()
    }

    /// Opens the name input on the cursor entry of the active panel. The task
    /// focuses it and selects what the user most likely replaces.
    pub fn begin_rename(&mut self) -> Task<PanelMessage> {
        self.end_rename();
        let panel = self.active_panel_mut();
        let Some(entry) = panel.selected_entry().filter(|entry| !entry.is_parent) else {
            return Task::none();
        };
        let name = entry.name.to_string_lossy().into_owned();
        let selection = selected_graphemes(&name, entry.is_dir);
        panel.rename = Some(RenameState {
            path: entry.path.clone(),
            draft: name,
            error: None,
            busy: false,
        });
        let focus = operation::focus(RENAME_INPUT_ID);
        match selection {
            Some(end) => focus.chain(operation::select_range(RENAME_INPUT_ID, 0, end)),
            None => focus.chain(operation::select_all(RENAME_INPUT_ID)),
        }
    }

    pub fn type_rename(&mut self, typed: String) {
        if let Some(rename) = self.rename_mut().filter(|rename| !rename.busy) {
            rename.draft = typed;
            rename.error = None;
        }
    }

    /// Enter in the input. The name of the entry itself changes nothing and
    /// closes the input; any other name is asked of the app.
    pub fn confirm_rename(&mut self) -> Option<Action> {
        let side = self.renaming_side()?;
        let rename = self.rename_mut().filter(|rename| !rename.busy)?;
        let unchanged = rename
            .path
            .file_name()
            .is_some_and(|name| name.to_string_lossy() == rename.draft);
        if unchanged {
            self.end_rename();
            return None;
        }
        rename.busy = true;
        rename.error = None;
        Some(Action::SubmitRename {
            side,
            from: rename.path.clone(),
            new_name: rename.draft.clone(),
        })
    }

    /// The name asked for `from` was refused: its input stays open and says
    /// why. An answer for an entry whose input is no longer open is dropped.
    pub fn refuse_rename(&mut self, from: &Path, text: String) {
        if let Some(rename) = self.rename_mut().filter(|rename| rename.path == from) {
            rename.busy = false;
            rename.error = Some(text);
        }
    }

    /// The rename of `from` is over: its input closes, another entry's input
    /// opened since stays.
    pub fn end_rename_of(&mut self, from: &Path) {
        for panel in [&mut self.left, &mut self.right] {
            if panel
                .rename
                .as_ref()
                .is_some_and(|rename| rename.path == from)
            {
                panel.rename = None;
            }
        }
    }

    /// The input closes, renamed or not.
    pub fn end_rename(&mut self) {
        self.left.rename = None;
        self.right.rename = None;
    }

    /// A hit list is a snapshot of paths, so a renamed file is looked for under
    /// its new path when the list is read again.
    pub fn rename_hit(&mut self, side: PanelSide, from: &Path, to: &Path) {
        let panel = self.panel_mut(side);
        if !panel.is_hit_list() {
            return;
        }
        for entry in panel.entries.iter_mut().filter(|entry| entry.path == from) {
            entry.path = to.to_path_buf();
            if let Some(name) = to.file_name() {
                entry.name = name.to_os_string();
            }
        }
    }

    /// Closes the input when the entry it belongs to is no longer listed, as
    /// after a reload that found it gone.
    pub fn drop_rename_of_missing_entry(&mut self) {
        for side in [PanelSide::Left, PanelSide::Right] {
            let panel = self.panel_mut(side);
            let missing = panel
                .rename
                .as_ref()
                .is_some_and(|rename| !panel.entries.iter().any(|entry| entry.path == rename.path));
            if missing {
                panel.rename = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_selects_its_name_without_the_extension() {
        assert_eq!(selected_graphemes("report.pdf", false), Some(6));
        assert_eq!(selected_graphemes("archive.tar.gz", false), Some(11));
    }

    #[test]
    fn the_stem_is_counted_in_graphemes_not_bytes() {
        assert_eq!(selected_graphemes("Rechnung März.pdf", false), Some(13));
        assert_eq!(selected_graphemes("Marz\u{308}.txt", false), Some(4));
    }

    #[test]
    fn everything_is_selected_for_a_folder_a_name_without_extension_and_a_dotfile() {
        assert_eq!(selected_graphemes("photos.2026", true), None);
        assert_eq!(selected_graphemes("Makefile", false), None);
        assert_eq!(selected_graphemes(".env", false), None);
        assert_eq!(selected_graphemes("trailing.", false), None);
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod panels_tests {
    use ncrs_core::FileEntry;

    use super::*;
    use crate::panel::PanelState;

    fn entry(name: &str, is_dir: bool, is_parent: bool) -> FileEntry {
        FileEntry {
            name: name.into(),
            path: PathBuf::from("/d").join(name),
            is_dir,
            is_symlink: false,
            is_parent,
            size: 0,
            modified: None,
            folder_size: None,
            details: Default::default(),
        }
    }

    fn panels_on(cursor: usize) -> Panels {
        let mut left = PanelState::new(PathBuf::from("/d"));
        left.entries = vec![
            entry("..", true, true),
            entry("notes.txt", false, false),
            entry("photos", true, false),
        ];
        left.selected = cursor;
        Panels::new(left, PanelState::new(PathBuf::from("/e")), PanelSide::Left)
    }

    #[test]
    fn the_input_opens_on_the_cursor_entry_with_its_name() {
        let mut panels = panels_on(1);
        let _focus = panels.begin_rename();
        let rename = panels.left.rename.clone().unwrap();
        assert_eq!(rename.path, PathBuf::from("/d/notes.txt"));
        assert_eq!(rename.draft, "notes.txt");
        assert_eq!(panels.renaming_side(), Some(PanelSide::Left));
    }

    #[test]
    fn the_parent_row_cannot_be_renamed() {
        let mut panels = panels_on(0);
        let _focus = panels.begin_rename();
        assert_eq!(panels.renaming_side(), None);
    }

    #[test]
    fn enter_asks_for_the_typed_name_once_and_waits_for_the_answer() {
        let mut panels = panels_on(1);
        let _focus = panels.begin_rename();
        panels.type_rename("todo.txt".to_owned());

        let asked = panels.confirm_rename();

        assert_eq!(
            asked,
            Some(Action::SubmitRename {
                side: PanelSide::Left,
                from: PathBuf::from("/d/notes.txt"),
                new_name: "todo.txt".to_owned(),
            })
        );
        assert_eq!(panels.confirm_rename(), None);
        panels.type_rename("ignored".to_owned());
        assert_eq!(panels.left.rename.as_ref().unwrap().draft, "todo.txt");
    }

    #[test]
    fn a_refused_name_stays_in_the_input_with_its_reason_until_typing_goes_on() {
        let mut panels = panels_on(1);
        let _focus = panels.begin_rename();
        panels.type_rename("a/b".to_owned());
        let _asked = panels.confirm_rename();

        panels.refuse_rename(Path::new("/d/notes.txt"), "no slash".to_owned());
        let rename = panels.left.rename.clone().unwrap();
        assert_eq!(rename.error.as_deref(), Some("no slash"));
        assert!(!rename.busy);

        panels.type_rename("ab".to_owned());
        assert_eq!(panels.left.rename.as_ref().unwrap().error, None);
    }

    #[test]
    fn the_name_of_the_entry_itself_closes_the_input_without_asking() {
        let mut panels = panels_on(2);
        let _focus = panels.begin_rename();

        assert_eq!(panels.confirm_rename(), None);
        assert_eq!(panels.renaming_side(), None);
    }

    #[test]
    fn cancelling_closes_the_input() {
        let mut panels = panels_on(1);
        let _focus = panels.begin_rename();
        panels.end_rename();
        assert_eq!(panels.renaming_side(), None);
    }

    #[test]
    fn a_reload_that_lost_the_entry_closes_its_input() {
        let mut panels = panels_on(1);
        let _focus = panels.begin_rename();
        panels.left.entries.remove(1);

        panels.drop_rename_of_missing_entry();

        assert_eq!(panels.renaming_side(), None);
    }
}
