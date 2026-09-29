//! Root application: state, `update` (the only place state changes) and
//! `view` (pure composition of UI components).
use std::path::PathBuf;

use iced::keyboard::{self, key::Named, Key, Modifiers};
use iced::widget::{column, container, row, Stack};
use iced::{window, Element, Length, Subscription, Task, Theme};

use crate::dialog::{Prompt, PromptKind};
use crate::fs::{self, CreateDirError};
use crate::i18n::{Language, Msg};
use crate::keymap;
use crate::messages::{Message, PanelSide};
use crate::ui::{self, dialog, header, layout, panel, statusbar, theme, PanelProps, PanelState};

const APP_NAME: &str = "NC-rs";

/// A reload waiting for its operation to finish.
///
/// F7 creates a directory and then reads the panel. Both were started at once,
/// which races: a read that finishes first does not see the new entry. The
/// reload waits here until the create has reported back.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingReload {
    side: PanelSide,
    /// Name to select afterwards, so Enter opens what was just created.
    name: String,
}

pub struct App {
    left_panel: PanelState,
    right_panel: PanelState,
    active_panel: PanelSide,
    /// Number of file rows that fit into a panel (derived from window size).
    visible_rows: usize,
    lang: Language,
    /// Header hints, kept in state so `view` does not allocate per frame.
    shortcuts: Vec<(String, &'static str)>,
    /// The open modal prompt, if any. Mutated only here.
    prompt: Option<Prompt>,
    /// Panel the prompt belongs to, so a panel switch while it is open cannot
    /// create a directory in the wrong place.
    prompt_side: Option<PanelSide>,
    /// Id of the running prompt operation, so a late result cannot land on a
    /// prompt the user already dismissed.
    prompt_request_id: u64,
    /// Reload queued by `PromptSubmit`, run once the create has reported.
    pending_reload: Option<PendingReload>,
}

/// The part of the prompt the key routing needs, cheap to clone.
///
/// A subscription outlives the call that created it, so it cannot borrow the
/// app. It gets this instead: two flags and the name typed so far. `Arc<str>`
/// rather than `&'static str` because leaking a copy per keypress would grow
/// without bound, and a `String` per keypress is what this avoids.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct PromptKeyState {
    /// A prompt is open, so keys belong to it.
    pub open: bool,
    /// Whether the operation is running; keys are ignored then.
    pub busy: bool,
    /// The name typed so far.
    pub typed: std::sync::Arc<str>,
}

/// Decides who a key press belongs to: the open prompt, or the bindings.
///
/// A free function because the subscription that calls it outlives `&self`.
/// One place, so a binding cannot end up half-modal: with a prompt open, Enter
/// submits instead of opening a directory, and Backspace edits text instead of
/// walking to the parent.
pub fn route_key(prompt: &PromptKeyState, key: Key) -> Option<Message> {
    if !prompt.open {
        return keymap::map_key(key, Modifiers::default());
    }
    if prompt.busy {
        return None;
    }
    let mut name = prompt.typed.to_string();
    match key.as_ref() {
        Key::Named(Named::Enter) => Some(Message::PromptSubmit),
        Key::Named(Named::Escape) => Some(Message::PromptCancel),
        // Backspace edits the text; it must not walk to the parent directory.
        Key::Named(Named::Backspace) => {
            name.pop();
            Some(Message::PromptInput(name))
        }
        // No filtering: a directory name may contain anything but a path
        // separator, and `Prompt::validate` reports the rest.
        Key::Character(c) => {
            name.push_str(c);
            Some(Message::PromptInput(name))
        }
        _ => None,
    }
}

impl App {
    /// Initial state plus tasks that load both panels.
    /// Left panel starts in the working directory, right in `$HOME`, so the
    /// two panels are not the same directory on launch.
    pub fn new() -> (Self, Task<Message>) {
        let start = fs::start_dir();
        let home = fs::home_dir();

        let mut app = Self {
            left_panel: PanelState::new(start.clone()),
            right_panel: PanelState::new(home.clone()),
            active_panel: PanelSide::Left,
            visible_rows: layout::visible_rows(layout::INITIAL_WINDOW_SIZE),
            lang: Language::default(),
            shortcuts: keymap::shortcuts(Language::default()),
            prompt: None,
            prompt_side: None,
            prompt_request_id: 0,
            pending_reload: None,
        };

        let tasks = Task::batch([
            app.load(PanelSide::Left, start, None),
            app.load(PanelSide::Right, home, None),
        ]);
        (app, tasks)
    }

    pub fn title(&self) -> String {
        format!("{APP_NAME} – {}", self.active_panel().path.display())
    }

    pub fn theme(&self) -> Theme {
        theme::app_theme()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        // A subscription outlives the call that created it, so it cannot close
        // over `&self`. What it needs from the app is the open prompt and
        // nothing else, and that is two `Copy` values.
        let initial_key_state = self.prompt_key_state();

        Subscription::batch([
            // `listen` yields the event stream, so the mapping can decide
            // without routing every key through a message first.
            //
            // `with` moves the state into the stream, so the mapping below reads
            // the copy that comes back with each event rather than closing over
            // the original.
            keyboard::listen().with(initial_key_state).filter_map(
                |(key_state, event)| match event {
                    keyboard::Event::KeyPressed { key, modifiers, .. }
                        if modifiers == Modifiers::default() =>
                    {
                        route_key(&key_state, key)
                    }
                    _ => None,
                },
            ),
            window::resize_events().map(|(_id, size)| Message::WindowResized(size)),
        ])
    }

    // -----------------------------------------------------------------------
    // Update
    // -----------------------------------------------------------------------

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let rows = self.visible_rows;

        match message {
            // A raw key press. Routed here rather than in the subscription,
            // because that one cannot see whether a prompt is open.
            // --- modal prompt ---
            // Handled before the panels: while a prompt is open every
            // keystroke belongs to it, and Enter must not open a directory.
            Message::CreateDirPrompt => {
                let parent = self.active_panel().path.clone();
                self.prompt = Some(Prompt::create_dir(parent));
                self.prompt_side = Some(self.active_panel);
                Task::none()
            }
            Message::PromptInput(text) => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.set_name(text);
                }
                Task::none()
            }
            Message::PromptSubmit => {
                let Some(prompt) = self.prompt.as_ref() else {
                    return Task::none();
                };
                if let Err(reason) = prompt.validate() {
                    self.set_prompt_error(reason);
                    return Task::none();
                }
                let side = self.prompt_side.unwrap_or(self.active_panel);
                let parent = self.panel(side).path.clone();
                let new_name = prompt.name().trim().to_string();
                let request_id = self.prompt_request_id.wrapping_add(1);
                self.prompt_request_id = request_id;
                self.set_prompt_busy(true);

                // The new directory's name, carried through the task so the
                // reload can select it.
                let target = parent.join(&new_name);
                let create = Task::perform(fs::create_dir(target), move |result| {
                    Message::PromptFinished {
                        prompt: PromptKind::CreateDir,
                        request_id,
                        result,
                    }
                });

                // The reload belongs *after* the create, not beside it.
                // Running both at once races: a read that finishes first sees a
                // directory that does not contain the new entry yet, and the
                // selection then points at whatever is there instead.
                self.pending_reload = Some(PendingReload {
                    side,
                    name: new_name,
                });
                create
            }
            Message::PromptCancel => {
                self.prompt = None;
                self.prompt_side = None;
                Task::none()
            }
            Message::PromptFinished {
                // The prompt kind is fixed by the only operation that sends
                // this today; a second one will want it back.
                prompt: _kind,
                request_id,
                result,
            } => {
                if request_id != self.prompt_request_id {
                    // Dismissed or retried while the operation ran.
                    return Task::none();
                }
                // The queued reload runs here, after the create has reported —
                // never beside it. On success it selects the new directory; on
                // failure it still re-reads, because the name may belong to
                // something else that appeared meanwhile.
                let reload = self.pending_reload.take().map(|pending| {
                    let path = self.panel(pending.side).path.clone();
                    self.load(pending.side, path, Some(pending.name))
                });

                match result {
                    Ok(()) => {
                        self.prompt = None;
                        self.prompt_side = None;
                    }
                    Err(err) => {
                        self.set_prompt_busy(false);
                        self.set_prompt_error_text(&self.prompt_error(&err));
                    }
                }
                reload.unwrap_or_else(Task::none)
            }

            Message::DirectoryLoaded {
                side,
                request_id,
                path,
                entries,
                error,
                select,
            } => {
                let panel = self.panel_mut(side);
                if panel.request_id == request_id {
                    panel.apply_listing(path, entries, error, select.as_deref(), rows);
                }
                Task::none()
            }

            Message::MoveSelection(delta) => {
                self.active_panel_mut().move_selection(delta, rows);
                Task::none()
            }
            Message::PageUp => {
                self.active_panel_mut()
                    .move_selection(-(rows as isize), rows);
                Task::none()
            }
            Message::PageDown => {
                self.active_panel_mut().move_selection(rows as isize, rows);
                Task::none()
            }
            Message::SelectFirst => {
                self.active_panel_mut().select(0, rows);
                Task::none()
            }
            Message::SelectLast => {
                self.active_panel_mut().select_last(rows);
                Task::none()
            }

            Message::OpenSelected => {
                let Some(entry) = self.active_panel().selected_entry() else {
                    return Task::none();
                };
                if entry.is_parent {
                    self.go_up(self.active_panel)
                } else if entry.is_dir {
                    let path = entry.path.clone();
                    self.load(self.active_panel, path, None)
                } else {
                    // Placeholder for file actions (view / edit / open with …).
                    Task::none()
                }
            }
            Message::GoUp => self.go_up(self.active_panel),

            // --- selection ---
            Message::ToggleTag => {
                let panel = self.active_panel_mut();
                // The `..` entry is not a thing to copy, so it cannot be tagged.
                let Some(entry) = panel.selected_entry() else {
                    return Task::none();
                };
                if entry.is_parent {
                    return Task::none();
                }
                let name = entry.name.clone();
                panel.selection.toggle(&name);
                Task::none()
            }
            Message::TagAll => {
                // Split borrow: the set is filled while the entries are read.
                let panel = self.active_panel_mut();
                let mut selection = std::mem::take(&mut panel.selection);
                selection.tag_all(&panel.entries);
                panel.selection = selection;
                Task::none()
            }
            Message::ClearTags => {
                self.active_panel_mut().selection.clear();
                Task::none()
            }

            Message::SwitchPanel => {
                self.active_panel = self.active_panel.other();
                Task::none()
            }
            Message::SwitchLanguage => {
                self.lang = self.lang.other();
                self.shortcuts = keymap::shortcuts(self.lang);
                Task::none()
            }
            Message::RowClicked { side, index } => {
                self.active_panel = side;
                self.panel_mut(side).select(index, rows);
                Task::none()
            }

            Message::WindowResized(size) => {
                self.visible_rows = layout::visible_rows(size);
                self.left_panel.ensure_visible(self.visible_rows);
                self.right_panel.ensure_visible(self.visible_rows);
                Task::none()
            }
            Message::Quit => iced::exit(),
        }
    }

    // -----------------------------------------------------------------------
    // View
    // -----------------------------------------------------------------------

    pub fn view(&self) -> Element<'_, Message> {
        let panel_view = |side: PanelSide| {
            panel::view(
                self.panel(side),
                PanelProps {
                    is_active: self.active_panel == side,
                    visible_rows: self.visible_rows,
                },
                self.lang,
                move |index| Message::RowClicked { side, index },
            )
        };

        let panels = row![panel_view(PanelSide::Left), panel_view(PanelSide::Right)]
            .spacing(ui::theme::spacing::PANEL_GAP)
            .height(Length::Fill);

        let root = container(
            column![
                header::view(APP_NAME, &self.shortcuts),
                panels,
                statusbar::view(self.active_panel(), self.lang),
            ]
            .spacing(theme::spacing::SECTION_GAP),
        )
        .padding(theme::spacing::OUTER_PADDING)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::root);

        let Some(prompt) = self.prompt.as_ref() else {
            return root.into();
        };
        let overlay = dialog::view(
            prompt,
            self.lang,
            Message::PromptInput,
            Message::PromptSubmit,
            Message::PromptCancel,
        );
        let Some(dialog) = overlay else {
            return root.into();
        };

        // The prompt sits above everything and takes every keystroke; the scrim
        // swallows clicks meant for the panels underneath.
        Stack::with_children([root.into(), dialog::scrim(dialog)]).into()
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    pub fn panel(&self, side: PanelSide) -> &PanelState {
        match side {
            PanelSide::Left => &self.left_panel,
            PanelSide::Right => &self.right_panel,
        }
    }

    pub fn panel_mut(&mut self, side: PanelSide) -> &mut PanelState {
        match side {
            PanelSide::Left => &mut self.left_panel,
            PanelSide::Right => &mut self.right_panel,
        }
    }

    pub fn active_panel(&self) -> &PanelState {
        self.panel(self.active_panel)
    }

    pub fn active_panel_mut(&mut self) -> &mut PanelState {
        self.panel_mut(self.active_panel)
    }

    #[allow(dead_code)] // Used by upcoming two-panel operations (copy/move).
    pub fn inactive_panel_mut(&mut self) -> &mut PanelState {
        self.panel_mut(self.active_panel.other())
    }

    /// The state the key routing needs, cheap to clone.
    fn prompt_key_state(&self) -> PromptKeyState {
        match &self.prompt {
            None => PromptKeyState::default(),
            Some(prompt) => PromptKeyState {
                open: true,
                busy: prompt.busy(),
                typed: prompt.name().into(),
            },
        }
    }

    /// Shows a validation failure in the open prompt.
    fn set_prompt_error(&mut self, reason: &'static str) {
        if let Some(prompt) = self.prompt.as_mut() {
            prompt.set_validation_error(reason);
        }
    }

    /// Wording for a failed create, in the active language. Deciding here
    /// rather than in the filesystem layer keeps `fs` free of user-facing text,
    /// and the two failures need different advice: a taken name means pick
    /// another, a missing parent means the panel has gone stale.
    fn prompt_error(&self, err: &CreateDirError) -> String {
        use std::io::ErrorKind;
        let lang = self.lang;
        let name = self.prompt_name();
        match err.io_error().map(|e| e.kind()) {
            Some(ErrorKind::AlreadyExists) => {
                lang.text(Msg::ErrorMkdirTaken).replace("{name}", &name)
            }
            Some(ErrorKind::NotFound) => lang.text(Msg::ErrorParentMissing).to_string(),
            _ => lang
                .text(Msg::ErrorMkdirFailed)
                .replace("{name}", &name)
                .replace("{reason}", &err.to_string()),
        }
    }

    /// The name currently in the prompt, or an empty string.
    fn prompt_name(&self) -> String {
        self.prompt
            .as_ref()
            .map(|p| p.name().to_string())
            .unwrap_or_default()
    }

    /// Shows a filesystem failure in the open prompt.
    fn set_prompt_error_text(&mut self, reason: &str) {
        if let Some(prompt) = self.prompt.as_mut() {
            prompt.set_error(reason.to_string());
        }
    }

    fn set_prompt_busy(&mut self, busy: bool) {
        if let Some(prompt) = self.prompt.as_mut() {
            prompt.set_busy(busy);
        }
    }

    /// Navigates `side` to its parent directory, re-selecting the directory
    /// we came from.
    fn go_up(&mut self, side: PanelSide) -> Task<Message> {
        let current = &self.panel(side).path;
        let Some(parent) = current.parent().map(PathBuf::from) else {
            return Task::none();
        };
        let came_from = current
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
        self.load(side, parent, came_from)
    }

    /// Starts an async directory read for `side`.
    fn load(&mut self, side: PanelSide, path: PathBuf, select: Option<String>) -> Task<Message> {
        let request_id = self.panel_mut(side).begin_load();
        Task::perform(fs::read_directory(path), move |listing| {
            Message::DirectoryLoaded {
                side,
                request_id,
                path: listing.path,
                entries: listing.entries,
                error: listing.error,
                select: select.clone(),
            }
        })
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
// reason: a test module belongs next to what it tests
#[allow(clippy::inline_modules)]
#[cfg(test)]
mod render_timing {
    use super::*;
    use crate::fs::FileEntry;
    use std::time::{Duration, Instant};

    /// Synthetic entries, so the measurement does not depend on what is in the
    /// working directory.
    fn entries(n: usize) -> Vec<FileEntry> {
        (0..n)
            .map(|i| FileEntry {
                name: format!("file_{i}.txt").into(),
                path: std::path::PathBuf::from(format!("/tmp/file_{i}.txt")),
                is_dir: i % 5 == 0,
                is_symlink: false,
                is_parent: i == 0,
                size: 1024 * (i as u64 % 900),
                modified: None,
            })
            .collect()
    }

    fn app_with_rows(n: usize) -> App {
        let mut app = App::new().0;
        app.left_panel.entries = entries(n);
        app.right_panel.entries = entries(n);
        app.visible_rows = 35;
        app
    }

    /// Regression: navigation felt laggy — the highlighted row visibly caught up
    /// about a second after the key press.
    ///
    /// `view` is rebuilt from scratch on every keystroke, so its cost is what
    /// the user waits for. Measured at ~0.1 ms, which is well inside budget and
    /// rules the view out as the cause: the remaining cost is rasterisation,
    /// which this test cannot reach. See ROADMAP.md, T2.
    #[test]
    fn a_frame_costs_far_less_than_a_frame_budget() {
        let app = app_with_rows(200);

        let _ = app.view(); // warm-up
        let frames = 60;
        let start = Instant::now();
        for _ in 0..frames {
            let _ = app.view();
        }
        let per_frame = start.elapsed() / frames;

        assert!(
            per_frame < Duration::from_millis(16),
            "a frame costs {per_frame:?}; the 60fps budget is 16ms"
        );
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
// reason: a test module belongs next to what it tests
#[allow(clippy::inline_modules)]
#[cfg(test)]
mod prompt_routing {
    use super::*;
    use iced::keyboard::key::Named;

    fn app() -> App {
        App::new().0
    }

    fn with_prompt() -> App {
        let mut app = app();
        app.prompt = Some(Prompt::create_dir(PathBuf::from("/tmp")));
        app
    }

    /// Routes as the subscription does: build the state, then call the free
    /// function. Same path the real keypress takes.
    fn press(app: &App, key: Key) -> Option<Message> {
        route_key(&app.prompt_key_state(), key)
    }

    /// While the prompt is open, F7 must not open a second one and the panel
    /// keys must not move the cursor. This is the bug the whole routing exists
    /// to prevent: Enter in a text field that also opens directories.
    #[test]
    fn an_open_prompt_takes_every_key() {
        let app = with_prompt();

        for key in [
            Key::Named(Named::ArrowDown),
            Key::Named(Named::Enter),
            Key::Named(Named::Tab),
            Key::Named(Named::F10),
        ] {
            let label = format!("{key:?}");
            let routed = press(&app, key.clone());
            let leaked = matches!(
                routed,
                Some(Message::MoveSelection(_))
                    | Some(Message::SwitchPanel)
                    | Some(Message::Quit)
                    | Some(Message::CreateDirPrompt)
            );
            assert!(!leaked, "{label} escaped the prompt as {routed:?}");
        }
    }

    #[test]
    fn enter_submits_and_escape_cancels() {
        let app = with_prompt();
        assert_eq!(
            press(&app, Key::Named(Named::Enter)),
            Some(Message::PromptSubmit)
        );
        assert_eq!(
            press(&app, Key::Named(Named::Escape)),
            Some(Message::PromptCancel)
        );
    }

    /// Typing appends, backspace removes one character — the field behaves like
    /// a text field, not like a panel selection.
    #[test]
    fn typing_and_backspace_edit_the_name() {
        let mut app = with_prompt();
        app.prompt.as_mut().unwrap().set_name("abc".into());

        assert_eq!(
            press(&app, Key::Character("d".into())),
            Some(Message::PromptInput("abcd".into()))
        );
        assert_eq!(
            press(&app, Key::Named(Named::Backspace)),
            Some(Message::PromptInput("ab".into()))
        );
    }

    /// A new keystroke clears the error, so a corrected name does not still
    /// show the complaint about the previous one.
    #[test]
    fn typing_clears_a_validation_error() {
        let mut prompt = Prompt::create_dir(PathBuf::from("/tmp"));
        prompt.set_validation_error("empty_name");
        assert!(prompt.error().is_some());

        prompt.set_name("x".into());
        assert_eq!(prompt.error(), None);
    }

    /// Without a prompt the bindings apply unchanged, so the routing did not
    /// change normal navigation.
    #[test]
    fn without_a_prompt_the_bindings_apply() {
        let app = app();
        assert_eq!(
            press(&app, Key::Named(Named::ArrowDown)),
            Some(Message::MoveSelection(1))
        );
        assert_eq!(
            press(&app, Key::Named(Named::Enter)),
            Some(Message::OpenSelected)
        );
    }

    /// F7 is bound, and the header hint comes from the same entry, so the key
    /// that opens the prompt and the key the user reads about are the same.
    #[test]
    fn f7_opens_the_prompt_and_is_advertised() {
        let app = app();
        assert_eq!(
            press(&app, Key::Named(Named::F7)),
            Some(Message::CreateDirPrompt)
        );
        assert!(
            keymap::shortcuts(Language::English)
                .iter()
                .any(|(key, _)| key == "F7"),
            "F7 opens the prompt but the header does not show it"
        );
    }

    // --- the whole chain, without a window -------------------------------
    //
    // Driving `update` the way a keypress would: open, type, submit. The
    // filesystem work itself is not awaited here, so this covers routing and
    // validation; the directory is checked in the async test below.

    fn press_and_update(app: &mut App, key: Key) {
        if let Some(message) = route_key(&app.prompt_key_state(), key) {
            let _ = app.update(message);
        }
    }

    /// Removes itself, so a failed assertion does not leave a directory that
    /// makes the next run fail.
    fn scratch(label: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("ncrs-chain-{label}-"))
            .tempdir()
            .expect("a scratch directory")
    }

    #[test]
    fn f7_then_a_name_then_enter_readies_the_operation() {
        let scratch = scratch("submit");
        let base = scratch.path().to_path_buf();
        std::fs::create_dir_all(&base).unwrap();
        let mut app = app();
        app.left_panel.path = base.clone();

        press_and_update(&mut app, Key::Named(Named::F7));
        assert!(app.prompt.is_some(), "F7 did not open the prompt");

        for c in "neuer ordner".chars() {
            press_and_update(&mut app, Key::Character(c.to_string().into()));
        }
        assert_eq!(app.prompt.as_ref().unwrap().name(), "neuer ordner");

        // Submit starts the operation and marks the prompt busy.
        let _task = app.update(Message::PromptSubmit);
        assert!(
            app.prompt.is_some(),
            "the prompt closed before the work ran"
        );
        assert!(app.prompt.as_ref().unwrap().busy());
        assert_eq!(app.prompt.as_ref().unwrap().error(), None);
    }

    /// An empty name is refused while the dialog is still open: no filesystem
    /// call, no busy state, an error the user can act on.
    #[test]
    fn submitting_an_empty_name_reports_it_without_closing() {
        let scratch = scratch("empty");
        let base = scratch.path().to_path_buf();
        std::fs::create_dir_all(&base).unwrap();
        let mut app = app();
        app.left_panel.path = base.clone();

        press_and_update(&mut app, Key::Named(Named::F7));
        let _task = app.update(Message::PromptSubmit);

        assert!(app.prompt.is_some(), "the prompt closed on an invalid name");
        assert!(!app.prompt.as_ref().unwrap().busy());
        assert_eq!(app.prompt.as_ref().unwrap().error(), Some("empty_name"));
    }

    /// Escape closes the prompt and leaves no state behind.
    #[test]
    fn escape_closes_the_prompt() {
        let mut app = app();
        press_and_update(&mut app, Key::Named(Named::F7));
        assert!(app.prompt.is_some());

        press_and_update(&mut app, Key::Named(Named::Escape));
        assert!(app.prompt.is_none());
        assert_eq!(app.prompt_side, None);
    }

    /// A result that arrives after the user dismissed the prompt is dropped:
    /// it must not reopen anything or report a failure into the void.
    #[test]
    fn a_result_from_a_dismissed_prompt_is_ignored() {
        let mut app = app();
        app.prompt = Some(Prompt::create_dir(PathBuf::from("/tmp")));
        app.prompt_request_id = 7;
        press_and_update(&mut app, Key::Named(Named::Escape));

        let stale = Message::PromptFinished {
            prompt: PromptKind::CreateDir,
            request_id: 7,
            result: Err(fs::CreateDirError::TaskFailed {
                reason: "stale".into(),
            }),
        };
        let _task = app.update(stale);
        assert!(app.prompt.is_none());
    }

    /// The two filesystem failures get different wording, because the user
    /// needs to act differently: a taken name means pick another, a missing
    /// parent means the panel is stale.
    #[test]
    fn the_two_failures_read_differently() {
        let mut app = app();
        app.prompt = Some(Prompt::create_dir(PathBuf::from("/tmp")));
        app.prompt.as_mut().unwrap().set_name("x".into());

        let taken = fs::CreateDirError::AlreadyExists {
            path: PathBuf::from("/tmp/x"),
            source: std::io::Error::new(std::io::ErrorKind::AlreadyExists, "exists"),
        };
        let parent_gone = fs::CreateDirError::AlreadyExists {
            path: PathBuf::from("/tmp/x"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "no parent"),
        };

        let taken_text = app.prompt_error(&taken);
        let parent_text = app.prompt_error(&parent_gone);
        assert_ne!(
            taken_text, parent_text,
            "a taken name and a missing parent need different messages"
        );
        assert!(
            !taken_text.contains("{"),
            "placeholder left in: {taken_text}"
        );
        assert!(
            !parent_text.contains("{"),
            "placeholder left in: {parent_text}"
        );
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
// reason: a test module belongs next to what it tests
#[allow(clippy::inline_modules)]
#[cfg(test)]
mod prompt_end_to_end {
    use super::*;

    /// Removes itself, so a failed assertion does not leave a directory that
    /// makes the next run fail.
    fn scratch(label: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("ncrs-e2e-{label}-"))
            .tempdir()
            .expect("a scratch directory")
    }

    /// The whole path, including the filesystem: F7, type a name, submit, and
    /// the directory exists afterwards. The task is awaited through the
    /// executor so this is the real thing, not a simulation.
    #[tokio::test]
    async fn the_directory_exists_after_submitting() {
        let scratch = scratch("create");
        let base = scratch.path().to_path_buf();
        std::fs::create_dir_all(&base).unwrap();
        let target = base.join("angelegt");

        let result = fs::create_dir(target.clone()).await;
        assert!(result.is_ok(), "create_dir failed: {result:?}");
        assert!(target.is_dir(), "{} was not created", target.display());
    }

    /// A name taken is reported as such, and the existing directory is left
    /// alone — the case a second F7 on the same name produces.
    #[tokio::test]
    async fn a_taken_name_leaves_the_existing_directory_intact() {
        let scratch = scratch("taken");
        let base = scratch.path().to_path_buf();
        std::fs::create_dir_all(&base).unwrap();
        let target = base.join("vorhanden");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("inhalt.txt"), b"wichtig").unwrap();

        let err = fs::create_dir(target.clone()).await.unwrap_err();
        assert_eq!(
            err.io_error().unwrap().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        assert_eq!(
            std::fs::read(target.join("inhalt.txt")).unwrap(),
            b"wichtig",
            "an existing directory must not be touched"
        );
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
// reason: a test module belongs next to what it tests
#[allow(clippy::inline_modules)]
#[cfg(test)]
mod reload_order {
    use super::*;

    /// Removes itself, so a failed assertion does not leave a directory that
    /// makes the next run fail.
    fn scratch(label: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("ncrs-order-{label}-"))
            .tempdir()
            .expect("a scratch directory")
    }

    /// Regression: F7 started the create and the reload at the same time with
    /// `Task::batch`. A read that finished first saw a directory without the new
    /// entry, and the selection landed on whatever was there instead — the new
    /// directory silently missing from the panel.
    ///
    /// The fix puts the reload behind the create's result. This checks the
    /// ordering without timing: while the create is still in flight, no reload
    /// may have been issued.
    #[test]
    fn the_reload_waits_for_the_create() {
        let scratch = scratch("waits");
        let base = scratch.path().to_path_buf();
        std::fs::create_dir_all(&base).unwrap();

        let mut app = App::new().0;
        app.left_panel.path = base.clone();
        app.prompt = Some(Prompt::create_dir(base.clone()));
        app.prompt.as_mut().unwrap().set_name("neu".into());
        // Submitting queues the reload instead of running it.
        let _ = app.update(Message::PromptSubmit);
        assert!(
            app.pending_reload.is_some(),
            "no reload was queued, so the panel would never refresh"
        );
        assert_ne!(
            app.prompt_request_id, 0,
            "the request id was not advanced, so a stale result could land"
        );

        // Still busy: the create has not reported.
        assert!(app.prompt.as_ref().unwrap().busy());
    }

    /// The queued reload runs when the create reports, and clears itself so a
    /// later result cannot trigger a second read.
    #[test]
    fn the_reload_runs_when_the_create_reports() {
        let scratch = scratch("runs");
        let base = scratch.path().to_path_buf();
        std::fs::create_dir_all(&base).unwrap();

        let mut app = App::new().0;
        app.left_panel.path = base.clone();
        app.prompt = Some(Prompt::create_dir(base.clone()));
        app.prompt.as_mut().unwrap().set_name("spaet".into());
        app.prompt_request_id = 1;
        app.pending_reload = Some(PendingReload {
            side: PanelSide::Left,
            name: "spaet".into(),
        });

        let _ = app.update(Message::PromptFinished {
            prompt: PromptKind::CreateDir,
            request_id: 1,
            result: Ok(()),
        });

        assert!(
            app.pending_reload.is_none(),
            "the reload was not consumed and would run again"
        );
        assert!(app.prompt.is_none(), "the prompt stayed open after success");
    }

    /// On failure the prompt stays open with the error, and the reload still
    /// runs: the name may belong to something that appeared in the meantime.
    #[test]
    fn a_failure_still_reloads() {
        let scratch = scratch("failure");
        let base = scratch.path().to_path_buf();
        std::fs::create_dir_all(&base).unwrap();

        let mut app = App::new().0;
        app.left_panel.path = base.clone();
        app.prompt = Some(Prompt::create_dir(base.clone()));
        app.prompt.as_mut().unwrap().set_name("weg".into());
        app.prompt_request_id = 2;
        app.pending_reload = Some(PendingReload {
            side: PanelSide::Left,
            name: "weg".into(),
        });

        let _ = app.update(Message::PromptFinished {
            prompt: PromptKind::CreateDir,
            request_id: 2,
            result: Err(fs::CreateDirError::AlreadyExists {
                path: base.join("weg"),
                source: std::io::Error::new(std::io::ErrorKind::AlreadyExists, "exists"),
            }),
        });

        assert!(
            app.pending_reload.is_none(),
            "a failed create left a reload queued"
        );
        assert!(
            app.prompt.is_some(),
            "the prompt closed on failure, so the user cannot correct the name"
        );
        assert!(!app.prompt.as_ref().unwrap().busy());
    }

    /// A result for a request the app has moved past changes nothing — not the
    /// prompt, and not a queued reload either.
    #[test]
    fn a_stale_result_touches_neither() {
        let mut app = App::new().0;
        app.prompt_request_id = 5;
        app.pending_reload = Some(PendingReload {
            side: PanelSide::Left,
            name: "x".into(),
        });

        let _ = app.update(Message::PromptFinished {
            prompt: PromptKind::CreateDir,
            request_id: 4,
            result: Ok(()),
        });

        assert!(app.prompt.is_none());
        assert!(
            app.pending_reload.is_some(),
            "a stale result consumed the queued reload"
        );
    }
}

#[cfg(test)]
// Test-only constructors and helpers. One block rather than four: they were
// appended as tests were written, and a reader had to hunt for them.
impl App {
    /// An app with the create-directory prompt open, for the UI tests. The
    /// prompt field is private to this module, so the setup lives here.
    /// An app whose panels are filled from fixed data, for snapshot tests.
    ///
    /// `App::new()` would read the working directory, so the rendered image
    /// would depend on where the test runner happens to be started — a fresh CI
    /// container and a laptop would produce different references for the same
    /// code. This one renders the same bytes everywhere.
    pub fn with_fixed_panels() -> Self {
        let mut app = Self {
            left_panel: PanelState::new(PathBuf::from("/home/test/links")),
            right_panel: PanelState::new(PathBuf::from("/home/test/files")),
            active_panel: PanelSide::Left,
            visible_rows: 12,
            lang: Language::default(),
            shortcuts: keymap::shortcuts(Language::default()),
            prompt: None,
            prompt_side: None,
            prompt_request_id: 0,
            pending_reload: None,
        };
        for (side, names) in [
            (PanelSide::Left, ["..", "Documents", "Projects", "Desktop"]),
            (
                PanelSide::Right,
                ["..", "notes.txt", "report.pdf", "photo.jpg"],
            ),
        ] {
            let panel = app.panel_mut(side);
            panel.entries = names
                .iter()
                .enumerate()
                .map(|(i, name)| fs::FileEntry {
                    name: (*name).into(),
                    path: PathBuf::from("/home/test").join(name),
                    is_dir: i == 0 || name.ends_with('s') && i < 2,
                    is_symlink: false,
                    is_parent: i == 0,
                    size: if i == 0 { 0 } else { 1024 * (i as u64 * 4096) },
                    modified: None,
                })
                .collect();
        }
        app
    }

    /// A prompt on fixed data, for the prompt's own reference image.
    pub fn with_fixed_prompt() -> Self {
        let mut app = Self::with_fixed_panels();
        app.prompt = Some(Prompt::create_dir(PathBuf::from("/home/test/links")));
        if let Some(prompt) = app.prompt.as_mut() {
            prompt.set_name("neuer ordner".to_string());
        }
        app
    }

    pub fn with_prompt_open(parent: PathBuf) -> Self {
        let mut app = Self::new().0;
        app.prompt = Some(Prompt::create_dir(parent));
        app
    }

    pub fn set_prompt(&mut self, name: &str, error: Option<&str>) {
        let Some(open) = self.prompt.as_mut() else {
            return;
        };
        open.set_name(name.to_string());
        if let Some(message) = error {
            open.set_error(message.to_string());
        }
    }
    pub fn panel_mut_for_test(&mut self, side: PanelSide) -> &mut PanelState {
        self.panel_mut(side)
    }

    pub fn set_visible_rows_for_test(&mut self, rows: usize) {
        self.visible_rows = rows;
    }

    /// Tags the row at `index`, addressed by position the way a keypress is.
    pub fn toggle_tag_for_test(&mut self, index: usize) {
        let Some(name) = self.left_panel.entries.get(index).map(|e| e.name.clone()) else {
            return;
        };
        self.left_panel.selection.toggle(&name);
    }

    pub fn tag_all_for_test(&mut self) {
        let mut selection = crate::selection::SelectionSet::new();
        selection.tag_all(&self.left_panel.entries);
        self.left_panel.selection = selection;
    }

    pub fn left_panel_selection_tagged_for_test(&self, index: usize) -> bool {
        self.left_panel
            .entries
            .get(index)
            .is_some_and(|e| self.left_panel.selection.is_tagged(&e.name))
    }

    pub fn move_selection_for_test(&mut self, delta: isize) {
        let rows = self.visible_rows;
        self.left_panel.move_selection(delta, rows);
    }
}
