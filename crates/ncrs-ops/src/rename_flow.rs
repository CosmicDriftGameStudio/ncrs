//! Renaming the entry under the cursor: the name is checked, the rename runs
//! as a task, and the answer either closes the input or says why it stays.

use std::path::PathBuf;
use std::sync::Arc;

use iced::Task;
use ncrs_core::PanelSide;
use ncrs_fs as fs;
use ncrs_fs::remote::RemotePath;
use ncrs_i18n::{fill_template, Language, Msg};

use crate::action::Action;
use crate::ctx::FlowCtx;
use crate::delete_flow::{actions, Outcome};
use crate::message::OpsMessage;
use crate::row_failure::RowFailure;
use crate::state::Ops;
use crate::transfer_flow::refusal_text;

/// What APFS, ext4 and NTFS take for one name, in bytes and UTF-16 units.
const MAX_NAME_LENGTH: usize = 255;

/// Why a typed name cannot be used, before the file system is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameProblem {
    Empty,
    /// `.` or `..`.
    Dots,
    Separator,
    /// A character the file system does not allow in a name.
    Character(char),
    Control,
    /// Windows cuts a trailing dot or space off the name.
    TrailingDotOrSpace,
    /// Longer than the file system takes: 255 bytes, on Windows 255 UTF-16 units.
    TooLong,
    /// A name Windows opens as a device, such as `CON` or `NUL`.
    Device,
}

impl NameProblem {
    pub fn text(self, lang: Language) -> String {
        match self {
            NameProblem::Empty => lang.text(Msg::ErrorRenameEmpty).to_owned(),
            NameProblem::Dots => lang.text(Msg::ErrorRenameDots).to_owned(),
            NameProblem::Separator => lang.text(Msg::ErrorRenameSeparator).to_owned(),
            NameProblem::Control => lang.text(Msg::ErrorRenameControl).to_owned(),
            NameProblem::TrailingDotOrSpace => lang.text(Msg::ErrorRenameTrailing).to_owned(),
            NameProblem::TooLong => lang.text(Msg::ErrorRenameTooLong).to_owned(),
            NameProblem::Device => lang.text(Msg::ErrorRenameDevice).to_owned(),
            NameProblem::Character(character) => fill_template(
                lang.text(Msg::ErrorRenameCharacter),
                &[("{character}", &character.to_string())],
            ),
        }
    }
}

/// Checks `name` as one entry name. `windows` selects that system's rules:
/// there the backslash separates, `<>:"|?*` and control characters are
/// refused; elsewhere only the slash is. Everywhere control and direction
/// characters are refused, as the file layer does. A name longer than
/// the file system takes is refused.
pub fn validate_name(name: &str, windows: bool) -> Result<(), NameProblem> {
    if name.is_empty() {
        return Err(NameProblem::Empty);
    }
    if name == "." || name == ".." {
        return Err(NameProblem::Dots);
    }
    let length = if windows {
        name.encode_utf16().count()
    } else {
        name.len()
    };
    if length > MAX_NAME_LENGTH {
        return Err(NameProblem::TooLong);
    }
    for character in name.chars() {
        if character == '/' || (windows && character == '\\') {
            return Err(NameProblem::Separator);
        }
        if ncrs_fs::remote::is_deceptive(character) {
            return Err(NameProblem::Control);
        }
        if windows && "<>:\"|?*".contains(character) {
            return Err(NameProblem::Character(character));
        }
    }
    if windows && name.ends_with(['.', ' ']) {
        return Err(NameProblem::TrailingDotOrSpace);
    }
    if windows && ncrs_fs::archive::is_device_name(name) {
        return Err(NameProblem::Device);
    }
    Ok(())
}

/// Enter in the name input. A refused name goes back to the input at once;
/// otherwise the rename runs and [`rename_done`] gets the answer.
pub fn start_rename(
    ops: &mut Ops,
    side: PanelSide,
    from: PathBuf,
    new_name: String,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    let on_server = RemotePath::parse(&from).is_some();
    if let Err(problem) = validate_name(&new_name, cfg!(windows) && !on_server) {
        return refused(from, problem.text(ctx.lang));
    }
    ops.rename_request_id = ops.rename_request_id.wrapping_add(1);
    let request_id = ops.rename_request_id;
    let pool = Arc::clone(ctx.remote);
    let asked = from.clone();
    let task = Task::perform(fs::rename(pool, asked, new_name), move |result| {
        OpsMessage::Renamed {
            request_id,
            side,
            from: from.clone(),
            result: result.map_err(RowFailure::from_io),
        }
    });
    (task, Vec::new())
}

/// The rename has finished. A late answer, for a rename the user has since
/// replaced by another, still reloads the panels after a success and shows a
/// failure, but leaves the input of the newer rename alone.
pub fn rename_done(
    ops: &Ops,
    request_id: u64,
    side: PanelSide,
    from: &std::path::Path,
    result: Result<PathBuf, RowFailure>,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    let late = request_id != ops.rename_request_id;
    let old_name = from
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let failed = |text: String| {
        if late {
            return actions(vec![Action::ShowError(text)]);
        }
        actions(vec![
            Action::ShowError(text),
            Action::RenameEnded {
                side,
                from: from.to_path_buf(),
                renamed_to: None,
            },
        ])
    };
    match result {
        Ok(_) if late => actions(vec![Action::ReloadPanels]),
        Ok(to) => actions(vec![Action::RenameEnded {
            side,
            from: from.to_path_buf(),
            renamed_to: Some(to),
        }]),
        Err(RowFailure::Conflict(_reason)) if late => actions(vec![Action::ShowError(
            ctx.lang.text(Msg::ErrorRenameTaken).to_owned(),
        )]),
        Err(RowFailure::Conflict(_reason)) => refused(
            from.to_path_buf(),
            ctx.lang.text(Msg::ErrorRenameTaken).to_owned(),
        ),
        Err(RowFailure::Refused(refusal)) => failed(refusal_text(ctx.lang, refusal)),
        Err(RowFailure::UnsafeName(name)) => failed(fill_template(
            ctx.lang.text(Msg::ErrorRemoteUnsafeName),
            &[("{name}", &name)],
        )),
        Err(failure @ (RowFailure::Io { .. } | RowFailure::Other(_))) => failed(fill_template(
            ctx.lang.text(Msg::ErrorRenameFailed),
            &[
                ("{name}", &old_name),
                ("{reason}", &failure.reason_text(ctx.lang)),
            ],
        )),
    }
}

fn refused(from: PathBuf, text: String) -> Outcome {
    actions(vec![Action::RenameRefused { from, text }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_control_or_direction_character_is_refused_on_every_system() {
        for windows in [false, true] {
            assert_eq!(
                validate_name("a\u{202e}b.txt", windows),
                Err(NameProblem::Control)
            );
            assert_eq!(validate_name("a\nb", windows), Err(NameProblem::Control));
        }
    }

    #[test]
    fn a_name_beyond_the_limit_is_refused() {
        assert_eq!(validate_name(&"a".repeat(255), false), Ok(()));
        assert_eq!(
            validate_name(&"a".repeat(256), false),
            Err(NameProblem::TooLong)
        );
        // 128 two-byte letters are 256 bytes but only 128 UTF-16 units.
        let umlauts = "ä".repeat(128);
        assert_eq!(validate_name(&umlauts, false), Err(NameProblem::TooLong));
        assert_eq!(validate_name(&umlauts, true), Ok(()));
    }

    #[test]
    fn windows_refuses_device_names_only_there() {
        assert_eq!(validate_name("CON.txt", true), Err(NameProblem::Device));
        assert_eq!(validate_name("CON.txt", false), Ok(()));
    }

    #[test]
    fn an_empty_name_and_the_two_dot_names_are_refused() {
        assert_eq!(validate_name("", false), Err(NameProblem::Empty));
        assert_eq!(validate_name(".", false), Err(NameProblem::Dots));
        assert_eq!(validate_name("..", false), Err(NameProblem::Dots));
        assert_eq!(validate_name("...", false), Ok(()));
        assert_eq!(validate_name(".env", false), Ok(()));
    }

    #[test]
    fn a_slash_is_a_separator_everywhere_and_a_backslash_on_windows_only() {
        assert_eq!(validate_name("a/b", false), Err(NameProblem::Separator));
        assert_eq!(validate_name("a/b", true), Err(NameProblem::Separator));
        assert_eq!(validate_name(r"a\b", true), Err(NameProblem::Separator));
        assert_eq!(validate_name(r"a\b", false), Ok(()));
    }

    #[test]
    fn windows_refuses_its_reserved_characters_and_every_system_control_characters() {
        for character in ['<', '>', ':', '"', '|', '?', '*'] {
            let name = format!("a{character}b");
            assert_eq!(
                validate_name(&name, true),
                Err(NameProblem::Character(character))
            );
            assert_eq!(validate_name(&name, false), Ok(()));
        }
        assert_eq!(validate_name("a\u{7}b", true), Err(NameProblem::Control));
        assert_eq!(validate_name("a\u{7}b", false), Err(NameProblem::Control));
        assert_eq!(
            validate_name("a\u{202e}b", false),
            Err(NameProblem::Control)
        );
    }

    #[test]
    fn windows_refuses_a_trailing_dot_or_space_which_it_would_cut_off() {
        for name in ["report.", "report ", "a.txt."] {
            assert_eq!(
                validate_name(name, true),
                Err(NameProblem::TrailingDotOrSpace)
            );
            assert_eq!(validate_name(name, false), Ok(()));
        }
        assert_eq!(validate_name(".hidden", true), Ok(()));
    }

    #[test]
    fn a_nul_is_refused_everywhere() {
        assert_eq!(validate_name("a\0b", false), Err(NameProblem::Control));
        assert_eq!(validate_name("a\0b", true), Err(NameProblem::Control));
    }

    #[test]
    fn a_name_with_spaces_and_unicode_passes() {
        assert_eq!(validate_name("Rechnung März 2026.pdf", true), Ok(()));
    }
}
