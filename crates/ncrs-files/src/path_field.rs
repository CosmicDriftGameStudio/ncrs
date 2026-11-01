//! The path field of the status bar: what it holds while it is edited, and the
//! words for a path that cannot be opened.

use std::path::{Path, PathBuf};

use iced::Task;
use ncrs_core::PanelSide;
use ncrs_fs::remote::{RemotePath, ServerId};
use ncrs_fs::{Destination, PathProblem};
use ncrs_i18n::{fill_template, Language, Msg};

use crate::action::Action;
use crate::state::Panels;

/// The answer to the look at a path that Enter submitted.
#[derive(Debug, Clone, PartialEq)]
pub struct PathChecked {
    pub request_id: u64,
    pub side: PanelSide,
    pub target: PathBuf,
    pub result: Result<Destination, PathProblem>,
}

/// What the app tells the path field when Enter is pressed in it.
pub struct TypedPathCtx<'a> {
    pub lang: Language,
    pub home: &'a Path,
    /// The home folder of the open session on a server, if there is one.
    pub session_home: &'a dyn Fn(&ServerId) -> Option<String>,
}

impl std::fmt::Debug for TypedPathCtx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypedPathCtx")
            .field("home", &self.home)
            .finish_non_exhaustive()
    }
}

impl Panels {
    /// Sets what the path field shows, or ends the editing with `None`. Any
    /// change makes an answer still on its way stale.
    pub fn set_path_field(&mut self, value: Option<String>) {
        self.path_check_id = self.path_check_id.wrapping_add(1);
        self.path_field = value;
    }

    /// Enter in the path field: looks at the typed path on the blocking pool and
    /// opens it in the active panel once the answer is in.
    pub fn go_to_typed_path(&mut self, ctx: &TypedPathCtx<'_>) -> (Task<PathChecked>, Vec<Action>) {
        let Some(raw_path) = self.path_field.clone() else {
            return (Task::none(), Vec::new());
        };
        let typed = raw_path.trim();
        if typed.is_empty() {
            self.set_path_field(None);
            return (Task::none(), vec![Action::ReleaseFocus]);
        }
        let side = self.active;
        // Before anything that asks the disk: an address, or a path typed
        // while the panel is on a server, is no path of this computer.
        if let Some(actions) = self.open_typed_remote(side, typed, ctx) {
            return (Task::none(), actions);
        }
        let target = ncrs_fs::resolve_typed_path(&self.active_panel().path, ctx.home, typed);
        self.path_check_id = self.path_check_id.wrapping_add(1);
        let request_id = self.path_check_id;
        let located = Task::perform(ncrs_fs::locate(target.clone()), move |result| PathChecked {
            request_id,
            side,
            target: target.clone(),
            result,
        });
        (located, Vec::new())
    }

    /// What typing `typed` into the path field does when it names a server: an
    /// `sftp://` address opens it (or the connect dialog, when the address has
    /// no user), a path typed over a server's folder is read on that server.
    /// `None` for an ordinary path.
    fn open_typed_remote(
        &mut self,
        side: PanelSide,
        typed: &str,
        ctx: &TypedPathCtx<'_>,
    ) -> Option<Vec<Action>> {
        let is_address = typed
            .get(..7)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("sftp://"));
        let target = if is_address {
            match ncrs_fs::remote::parse_url(typed) {
                Ok(url) => match url.remote_path() {
                    Some(place) => place,
                    None => {
                        self.set_path_field(None);
                        return Some(vec![Action::OpenConnectDialog(url), Action::ReleaseFocus]);
                    }
                },
                Err(_) => {
                    let text = ctx.lang.text(Msg::ErrorRemoteUrlInvalid).to_owned();
                    return Some(vec![Action::ShowError(text)]);
                }
            }
        } else {
            let place = RemotePath::parse(&self.active_panel().path)?;
            let home = (ctx.session_home)(place.server()).unwrap_or_else(|| "/".to_owned());
            place.resolve_typed(typed, &home)
        };
        self.set_path_field(None);
        Some(vec![
            Action::ClearMessages,
            Action::Load {
                side,
                path: target.to_path_buf(),
                select: None,
            },
            Action::ReleaseFocus,
        ])
    }

    /// The answer to [`Self::go_to_typed_path`]: a destination closes the field
    /// and loads it, a problem is reported and leaves the field open. An answer
    /// to text that has since changed, or to a field that is closed, is dropped.
    pub fn finish_path_check(&mut self, lang: Language, checked: PathChecked) -> Vec<Action> {
        if checked.request_id != self.path_check_id || self.path_field.is_none() {
            return Vec::new();
        }
        match checked.result {
            Ok(destination) => {
                self.set_path_field(None);
                vec![
                    Action::ClearMessages,
                    Action::Load {
                        side: checked.side,
                        path: destination.directory,
                        select: destination.select,
                    },
                    Action::ReleaseFocus,
                ]
            }
            Err(problem) => {
                let reason = problem_text(lang, &problem, &checked.target);
                vec![Action::ShowError(reason)]
            }
        }
    }
}

/// Why `target` cannot be opened, for the status bar.
pub fn problem_text(lang: Language, problem: &PathProblem, target: &Path) -> String {
    let shown = target.display().to_string();
    match problem {
        PathProblem::Missing => {
            fill_template(lang.text(Msg::StatusPathMissing), &[("{path}", &shown)])
        }
        PathProblem::Unreadable { reason } => fill_template(
            lang.text(Msg::StatusPathUnreadable),
            &[("{path}", &shown), ("{reason}", reason)],
        ),
    }
}
