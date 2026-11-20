//! Handing a file to a program and starting a terminal: what F3, F4, Enter
//! and "Open terminal here" do. The launcher is the app's; what the flow
//! cannot do itself (fetch, unpack, view, ask) comes back as actions.

use std::path::{Path, PathBuf};

use ncrs_fs::{Launcher, OpenCommand, OpenKind, OpenPrograms, OpenWith};
use ncrs_i18n::{fill_template, Language, Msg};

use crate::action::Action;
use crate::open_plan::{open_refused_text, CopyOf, OpenPlan, OpenSource};
use crate::state::Panels;

/// What the open flow reads from the app.
pub struct OpenCtx<'a> {
    pub lang: Language,
    pub programs: &'a OpenPrograms,
    pub launcher: &'a dyn Launcher,
    /// The labels of the keys that view and edit, for the text of a refusal.
    pub view_key: Option<String>,
    pub edit_key: Option<String>,
}

impl std::fmt::Debug for OpenCtx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenCtx")
            .field("lang", &self.lang)
            .finish_non_exhaustive()
    }
}

/// F3 / F4: the row under the cursor, or the search hit, goes to the program
/// for `kind`; a file on a server or in an archive is fetched or unpacked
/// first and only viewed.
pub fn open_external(
    panels: &Panels,
    kind: OpenKind,
    source: OpenSource,
    ctx: &OpenCtx<'_>,
) -> Vec<Action> {
    let from_panel = matches!(source, OpenSource::ActivePanel);
    match panels.plan_open(kind, source) {
        OpenPlan::Nothing => Vec::new(),
        OpenPlan::Refused(msg) => vec![Action::ShowError(ctx.lang.text(msg).to_owned())],
        OpenPlan::ViewFromServer(remote) => vec![Action::ViewFromServer(remote)],
        OpenPlan::ViewFromArchive {
            source: archive_source,
            archive,
        } => vec![Action::ViewFromArchive {
            source: archive_source,
            archive,
        }],
        OpenPlan::Launch { path, line } => {
            if kind == OpenKind::View {
                if let Some(with) = ctx.programs.open_with_for(&path) {
                    return open_by_type(with, path, line, ctx);
                }
            }
            let executable = if from_panel {
                is_executable_row(panels, &path)
            } else {
                ncrs_fs::could_execute(&path)
            };
            launch_external(kind, path, line, CopyOf::Nothing, executable, ctx)
        }
    }
}

/// Enter or a double click on a file: opened as the Finder does, by the
/// system opener whatever `[open]` says. A file that could run asks first.
/// A file in an archive or on a server is viewed, as with F3.
pub fn open_like_the_finder(panels: &Panels, ctx: &OpenCtx<'_>) -> Vec<Action> {
    match panels.plan_open(OpenKind::View, OpenSource::ActivePanel) {
        OpenPlan::Nothing => Vec::new(),
        OpenPlan::Refused(msg) => vec![Action::ShowError(ctx.lang.text(msg).to_owned())],
        OpenPlan::ViewFromServer(remote) => vec![Action::ViewFromServer(remote)],
        OpenPlan::ViewFromArchive {
            source: archive_source,
            archive,
        } => vec![Action::ViewFromArchive {
            source: archive_source,
            archive,
        }],
        OpenPlan::Launch { path, .. } => {
            // ponytail: the exec bit alone also asks for files on exFAT or a NAS share,
            // where every file has it; narrow it if that becomes a nuisance.
            if is_executable_row(panels, &path) {
                vec![Action::OpenBundle(path)]
            } else {
                open_with_system(&path, ctx)
            }
        }
    }
}

/// Hands `path` to the system's own opener, never to a configured program.
pub fn open_with_system(path: &Path, ctx: &OpenCtx<'_>) -> Vec<Action> {
    start_command(&ncrs_fs::system_open_command(path), path, ctx)
}

/// Hands `path` to the program for `kind`, at `line` when the program takes
/// one. `from` says whether `path` is a temporary copy made for viewing, and
/// of what; `executable` that the file, or the one it was copied from, could
/// run.
pub fn launch_external(
    kind: OpenKind,
    path: PathBuf,
    line: Option<u64>,
    from: CopyOf,
    executable: bool,
    ctx: &OpenCtx<'_>,
) -> Vec<Action> {
    let Some(command) = ncrs_fs::open_command(ctx.programs, kind, &path, executable, line) else {
        let message = open_refused_text(
            ctx.lang,
            &shown_name(&path),
            from,
            ctx.view_key.clone(),
            ctx.edit_key.clone(),
        );
        return vec![Action::ClearMessages, Action::ShowError(message)];
    };
    start_command(&command, &path, ctx)
}

/// "Open terminal here": the terminal from `[open] terminal`, else the first
/// one found, started in the folder under the cursor or the panel's folder.
/// Where the panel has no local folder the update refuses before this runs.
pub fn open_terminal(panels: &Panels, search_is_open: bool, ctx: &OpenCtx<'_>) -> Vec<Action> {
    let mut actions = vec![Action::ClearMessages];
    if search_is_open {
        return actions;
    }
    let Some(folder) = panels.local_folder_target() else {
        return actions;
    };
    // Only needed when no terminal is set.
    let detected = match ctx.programs.terminal {
        Some(_) => &[],
        None => ncrs_fs::detected_terminals(),
    };
    let command = match ncrs_fs::terminal_command(ctx.programs.terminal.as_ref(), detected, &folder)
    {
        Ok(command) => command,
        Err(ncrs_fs::NoTerminal) => {
            actions.push(Action::ShowError(
                ctx.lang.text(Msg::ErrorTerminalNone).to_owned(),
            ));
            return actions;
        }
    };
    if let Err(reason) = ctx.launcher.launch(&command) {
        let name = folder
            .file_name()
            .map_or_else(|| folder.to_string_lossy(), |name| name.to_string_lossy());
        actions.push(Action::ShowError(fill_template(
            ctx.lang.text(Msg::ErrorTerminalFailed),
            &[("{name}", &name), ("{reason}", &reason)],
        )));
    }
    actions
}

/// F3 on a local file a `[[open.by_type]]` rule names.
fn open_by_type(
    with: OpenWith,
    path: PathBuf,
    line: Option<u64>,
    ctx: &OpenCtx<'_>,
) -> Vec<Action> {
    match with {
        OpenWith::Internal => vec![Action::OpenInViewer(path)],
        OpenWith::System => open_with_system(&path, ctx),
        OpenWith::Program(program) => {
            start_command(&ncrs_fs::program_command(&program, &path, line), &path, ctx)
        }
    }
}

/// Whether the row at `path` could run, read from the listing; a path that is
/// not the row under the cursor is asked of the disk.
fn is_executable_row(panels: &Panels, path: &Path) -> bool {
    panels
        .active_panel()
        .selected_entry()
        .filter(|entry| entry.path == path)
        .is_some_and(ncrs_fs::could_execute_listed)
        // The listing can be stale (mode changed, file swapped for a link):
        // a "no" from it is checked on disk before opening without asking.
        || ncrs_fs::could_execute(path)
}

/// Clears the status line, then starts `command`; a refusal of the system is
/// the error of the status bar.
fn start_command(command: &OpenCommand, path: &Path, ctx: &OpenCtx<'_>) -> Vec<Action> {
    let mut actions = vec![Action::ClearMessages];
    if let Err(reason) = ctx.launcher.launch(command) {
        actions.push(Action::ShowError(fill_template(
            ctx.lang.text(Msg::ErrorOpenFailed),
            &[("{name}", &shown_name(path)), ("{reason}", &reason)],
        )));
    }
    actions
}

fn shown_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}
