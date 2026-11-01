use iced::widget::operation;
use iced::Task;
use ncrs_config::config::{Config, FilingWriteError, KeyChange};
use ncrs_i18n::Msg;
use ncrs_ui_kit::focus::release_focus;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::key_bindings::view::SEARCH_FIELD_ID;
use crate::key_bindings::KeyBindings;
use crate::message::{KeyBindingsMessage, Message};
use crate::state::Settings;
use crate::write::{write_failure, write_key_changes};

pub fn update(
    settings: &mut Settings,
    message: KeyBindingsMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    use KeyBindingsMessage as Shortcuts;
    match message {
        Shortcuts::Open => return open(settings, ctx, actions),
        Shortcuts::Close => return close(settings),
        Shortcuts::Written(result) => {
            written(settings, result, ctx, actions);
            return Task::none();
        }
        _ => {}
    }
    let keymap = std::sync::Arc::clone(ctx.keymap);
    let Some(shortcuts) = settings.key_bindings.as_mut() else {
        return Task::none();
    };
    let edit = match message {
        Shortcuts::SearchChanged(query) => {
            shortcuts.set_query(query, &keymap);
            None
        }
        Shortcuts::Typed(typed) => {
            let query = format!("{}{typed}", shortcuts.query());
            shortcuts.set_query(query, &keymap);
            return operation::focus(SEARCH_FIELD_ID)
                .chain(operation::move_cursor_to_end(SEARCH_FIELD_ID));
        }
        Shortcuts::SearchByKey(binding) => {
            shortcuts.search_by_key(binding, &keymap);
            None
        }
        // The list takes the keys from the field: Left, Right and the
        // keys that remove would edit the text otherwise.
        Shortcuts::Move(delta) => {
            shortcuts.move_cursor(delta);
            return release_focus();
        }
        Shortcuts::Select { row, key } => {
            shortcuts.select(row, key);
            return release_focus();
        }
        Shortcuts::MoveKey(delta) => {
            shortcuts.move_key_cursor(delta);
            None
        }
        Shortcuts::Scrolled(delta) => {
            shortcuts.scroll(delta);
            None
        }
        Shortcuts::Record => {
            let changes = shortcuts.activate(&keymap);
            if changes.is_none() {
                return release_focus();
            }
            changes
        }
        Shortcuts::RecordAdditional => {
            shortcuts.record_additional();
            return release_focus();
        }
        Shortcuts::Recorded(binding) => shortcuts.recorded(binding, &keymap),
        Shortcuts::CancelRecording => {
            shortcuts.cancel();
            None
        }
        Shortcuts::RemoveKey => shortcuts.remove_key(),
        Shortcuts::Reset => shortcuts.reset(&keymap),
        Shortcuts::ResetAll => {
            shortcuts.ask_reset_all();
            None
        }
        Shortcuts::Confirm => shortcuts.confirm(&keymap),
        Shortcuts::Dismiss => {
            shortcuts.dismiss();
            None
        }
        Shortcuts::Open | Shortcuts::Close | Shortcuts::Written(_) => None,
    };
    edit.map_or_else(Task::none, |changes| write(settings, changes, ctx))
}

/// The key, the command list or the settings tab. Pressed while the
/// dialog is up, it only gives the search field the focus again.
fn open(settings: &mut Settings, ctx: &Ctx<'_>, actions: &mut Vec<Action>) -> Task<Message> {
    let focus = operation::focus(SEARCH_FIELD_ID);
    if settings.key_bindings.is_some() {
        return focus;
    }
    if ctx.blocking_overlay_open {
        return Task::none();
    }
    actions.push(Action::CloseContextMenu);
    settings.key_bindings = Some(KeyBindings::new(ctx.lang, ctx.keymap));
    focus
}

/// Escape and the close button: a clash or a question goes first.
fn close(settings: &mut Settings) -> Task<Message> {
    let Some(shortcuts) = settings.key_bindings.as_mut() else {
        return Task::none();
    };
    if shortcuts.escape() {
        return Task::none();
    }
    settings.key_bindings = None;
    release_focus()
}

/// Writes the `[keys]` lines of an edit on a blocking thread; the keymap
/// read back is the one in effect from then on.
fn write(settings: &mut Settings, changes: Vec<KeyChange>, ctx: &Ctx<'_>) -> Task<Message> {
    let Some(config_path) = ctx.config_path.map(std::path::Path::to_path_buf) else {
        let failure = ctx.lang.text(Msg::FilingSetupNoConfigPath).to_owned();
        if let Some(shortcuts) = settings.key_bindings.as_mut() {
            shortcuts.written(ctx.keymap, Some(failure));
        }
        return Task::none();
    };
    if let Some(shortcuts) = settings.key_bindings.as_mut() {
        shortcuts.start_writing();
    }
    let written_path = config_path.clone();
    Task::perform(
        tokio::task::spawn_blocking(move || write_key_changes(&written_path, &changes)),
        move |joined| {
            Message::KeyBindings(KeyBindingsMessage::Written(joined.unwrap_or_else(|join| {
                Err(FilingWriteError::Io {
                    path: config_path,
                    reason: join.to_string(),
                })
            })))
        },
    )
}

/// Takes over the keys the file now has: the panels, the function key
/// bar and the menu answer them at once.
fn written(
    settings: &mut Settings,
    result: Result<Box<Config>, FilingWriteError>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) {
    let mut keymap = std::sync::Arc::clone(ctx.keymap);
    let failure = match result {
        Ok(config) => {
            keymap = config.keymap;
            actions.push(Action::KeymapChanged(std::sync::Arc::clone(&keymap)));
            None
        }
        Err(error) => Some(write_failure(&error, ctx)),
    };
    if let Some(shortcuts) = settings.key_bindings.as_mut() {
        shortcuts.written(&keymap, failure);
    }
}
