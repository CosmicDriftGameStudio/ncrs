//! Putting files on the system clipboard.

use std::path::PathBuf;

use ncrs_i18n::{fill_template, Language, Msg};

use crate::action::Action;

/// Puts `sources` on the system clipboard as files. Only files of this
/// computer's disk can go there (`all_on_local_disk`).
pub fn copy_files_to_clipboard(
    lang: Language,
    sources: &[PathBuf],
    all_on_local_disk: bool,
    write: &mut dyn FnMut(&[PathBuf]) -> Result<(), String>,
) -> Vec<Action> {
    if sources.is_empty() {
        return Vec::new();
    }
    if !all_on_local_disk {
        let text = lang.text(Msg::StatusClipboardFilesLocalOnly).to_owned();
        return vec![Action::ShowNotice(text)];
    }
    if let Err(reason) = write(sources) {
        let text = fill_template(
            lang.text(Msg::ErrorClipboardWriteFailed),
            &[("{reason}", &reason)],
        );
        return vec![Action::ShowError(text)];
    }
    let text = match sources.len() {
        1 => lang.text(Msg::StatusFilesCopiedToClipboardOne).to_owned(),
        count => fill_template(
            lang.text(Msg::StatusFilesCopiedToClipboardMany),
            &[("{count}", &count.to_string())],
        ),
    };
    vec![Action::ClearStatus, Action::ShowNotice(text)]
}
