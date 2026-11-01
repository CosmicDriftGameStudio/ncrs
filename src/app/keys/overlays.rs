use crate::app::*;

/// Which keys the open delete dialog answers to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum DeleteKeys {
    #[default]
    Closed,
    /// Enter confirms.
    Trash,
    /// Enter cancels; only the permanent-delete key (Shift+F8) pressed again confirms.
    Permanent,
}

/// What Escape sends while one of the small button dialogs is up: the about
/// card, the OCR models card, the AI model card or the folder grant dialog.
pub(in crate::app) fn card_close_message(prompt: &PromptKeyState) -> Option<Message> {
    if let Some(hermes) = ncrs_hermes_ui::keys::hermes_card_close_message(&hermes_key_facts(prompt))
    {
        Some(hermes.into())
    } else if prompt.about {
        Some(Message::About(AboutMessage::Close))
    } else if prompt.ocr_models {
        Some(Message::OcrModels(OcrModelsMessage::Close))
    } else if prompt.ai_model {
        Some(Message::AiModel(AiModelMessage::Close))
    } else if prompt.grant_dialog {
        Some(Message::Grant(GrantMessage::Later))
    } else {
        None
    }
}

/// What the keys of the Hermes pages read of the prompt state.
fn hermes_key_facts(prompt: &PromptKeyState) -> ncrs_hermes_ui::keys::KeyFacts {
    ncrs_hermes_ui::keys::KeyFacts {
        hermes_sidebar: prompt.hermes_sidebar,
        review_list_editing: prompt.review_list_editing,
        review_list_replanning: prompt.review_list_replanning,
        rules_form: prompt.rules_form,
        rules_confirming_delete: prompt.rules_confirming_delete,
        hermes_overview_editing: prompt.hermes_overview_editing,
        start_over_dialog: prompt.start_over_dialog,
        hermes_setup: prompt.hermes_setup,
    }
}

type PageKeyFn = fn(
    &ncrs_hermes_ui::keys::KeyFacts,
    &ncrs_hermes_ui::keys::KeyPress<'_>,
    &dyn Fn() -> ncrs_hermes_ui::keys::Bound,
) -> Option<ncrs_hermes_ui::keys::PageKey>;

/// What a key does on a page of Hermes. The keymap is asked only for a key
/// the page does not know.
pub(in crate::app) fn hermes_page_key(
    page: PageKeyFn,
    prompt: &PromptKeyState,
    key: &Key,
    key_modifiers: Modifiers,
    scope: KeyScope,
) -> Option<Message> {
    use ncrs_hermes_ui::keys::{Bound, KeyPress, PageKey};
    let press = KeyPress {
        key,
        modifiers: key_modifiers,
        not_typed_by_text_field: is_not_typed_by_text_field(key, key_modifiers),
    };
    let bound = || match bound_message(&prompt.keymap, key, key_modifiers, scope) {
        Some(Message::Quit) => Bound::Quit,
        Some(Message::FilingStatus(FilingStatusMessage::Toggle)) => Bound::StatusToggle,
        _ => Bound::Other,
    };
    page(&hermes_key_facts(prompt), &press, &bound).map(|pressed| match pressed {
        PageKey::Message(message) => message.into(),
        PageKey::Focus(focus) => Message::HermesFocus(focus),
        PageKey::Step(step) => Message::HermesStep(step),
        PageKey::Quit => Message::Quit,
    })
}

pub(in crate::app) fn card_dialog_key(
    close: Message,
    key: Key,
    key_modifiers: Modifiers,
) -> Option<Message> {
    match key.as_ref() {
        Key::Named(Named::Escape) => Some(close),
        _ => dialog_focus_key(key, key_modifiers),
    }
}

/// Decides who a key press belongs to: the open prompt, or the bindings.
///
/// A free function because the subscription that calls it outlives `&self`.
/// One place, so a binding cannot end up half-modal: with a prompt open, Enter
/// submits instead of opening a directory, and Backspace edits text instead of
/// walking to the parent.
/// Keys while no prompt is open: the conflict dialog first, then the running
/// job, then the bindings.
///
/// The order matters and is not arbitrary. A conflict is asked *while* a job
/// runs, so if the job branch came first, Escape would stop the copy instead of
/// answering the question the app is waiting on.
pub(in crate::app) fn conflict_key(
    prompt: &PromptKeyState,
    key: Key,
    key_modifiers: Modifiers,
    scope: KeyScope,
) -> Option<Message> {
    let aborts =
        || bound_message(&prompt.keymap, &key, key_modifiers, scope) == Some(Message::AbortJob);
    ncrs_ops::keys::conflict_key(&key, key_modifiers, aborts).map(|pressed| match pressed {
        ConflictKey::Answer(choice) => Message::TransferConflict(choice),
        ConflictKey::ToggleAll => Message::ToggleConflictAll,
        ConflictKey::Dialog(dialog) => dialog_message(dialog),
    })
}

pub(in crate::app) fn transfer_overlay_key(
    prompt: &PromptKeyState,
    key: &Key,
    key_modifiers: Modifiers,
    scope: KeyScope,
) -> Option<Message> {
    let bound = || match bound_message(&prompt.keymap, key, key_modifiers, scope) {
        Some(Message::Quit) => OpsBound::Quit,
        Some(Message::AbortJob) => OpsBound::AbortJob,
        _ => OpsBound::Other,
    };
    ncrs_ops::keys::transfer_overlay_key(
        prompt.transfer_dialog,
        key,
        is_not_typed_by_text_field(key, key_modifiers),
        bound,
    )
    .map(|pressed| match pressed {
        TransferOverlayKey::Dialog(message) => Message::TransferDialog(message),
        TransferOverlayKey::AbortJob => Message::AbortJob,
        TransferOverlayKey::Quit => Message::Quit,
    })
}

pub(in crate::app) fn context_menu_key(prompt: &PromptKeyState, key: &Key) -> Option<Message> {
    ncrs_files::keys::context_menu_key(prompt.context_submenu, key).map(|pressed| match pressed {
        ContextMenuKey::CloseSub => Message::ContextMenuCloseSub,
        ContextMenuKey::Close => Message::ContextMenuClose,
        ContextMenuKey::OpenSub => Message::ContextMenuOpenSub,
        ContextMenuKey::Activate => Message::ContextMenuActivate,
        ContextMenuKey::Move(delta) => Message::ContextMenuMove(delta),
        ContextMenuKey::First => Message::ContextMenuFirst,
        ContextMenuKey::Last => Message::ContextMenuLast,
    })
}

/// The keys of the connect dialog: its fields take the typing and Enter, Tab
/// goes from one to the next, Escape closes it. Of the bindings only quitting
/// reaches past it.
pub(in crate::app) fn connect_dialog_key(
    prompt: &PromptKeyState,
    key: &Key,
    key_modifiers: Modifiers,
    scope: KeyScope,
) -> Option<Message> {
    match key.as_ref() {
        Key::Named(Named::Escape) => Some(Message::Remote(RemoteMessage::Cancel)),
        Key::Named(Named::Enter) => Some(Message::Remote(RemoteMessage::Submit)),
        Key::Named(Named::Tab) => Some(Message::Remote(RemoteMessage::FocusField {
            backwards: key_modifiers.shift(),
        })),
        _ => bound_message(&prompt.keymap, key, key_modifiers, scope)
            .filter(|message| *message == Message::Quit)
            .filter(|_| is_not_typed_by_text_field(key, key_modifiers)),
    }
}

pub(in crate::app) fn theme_fetch_key(
    prompt: &PromptKeyState,
    key: &Key,
    key_modifiers: Modifiers,
    scope: KeyScope,
) -> Option<Message> {
    let quits = || {
        bound_message(&prompt.keymap, key, key_modifiers, scope)
            .is_some_and(|message| message == Message::Quit)
            && is_not_typed_by_text_field(key, key_modifiers)
    };
    Some(
        match ncrs_settings::keys::theme_fetch_key(key, key_modifiers, quits)? {
            ThemeFetchKey::Message(message) => Message::ThemeFetch(message),
            ThemeFetchKey::Quit => Message::Quit,
        },
    )
}

/// Keys while the command line has the focus. The field types characters and
/// digits, so of the bindings only quitting and the key that focused the line
/// reach through, and only as keys the field does not type. Escape is
/// `InputState`'s; it arrives here only when the field lost its own focus.
pub(in crate::app) fn command_line_key(
    prompt: &PromptKeyState,
    key: &Key,
    key_modifiers: Modifiers,
    scope: KeyScope,
) -> Option<Message> {
    let bound = || {
        let Some(message) = bound_message(&prompt.keymap, key, key_modifiers, scope)
            .filter(|_| is_not_typed_by_text_field(key, key_modifiers))
        else {
            return CommandLineBound::Other;
        };
        if message == Message::Quit {
            CommandLineBound::Quit
        } else if message == KeyAction::CommandLine.message() {
            CommandLineBound::CommandLine
        } else {
            CommandLineBound::Other
        }
    };
    Some(
        match ncrs_palette::keys::command_line_key(key, key_modifiers, bound)? {
            CommandLineKey::Message(message) => Message::CommandLine(message),
            CommandLineKey::Quit => Message::Quit,
        },
    )
}

/// The keyboard shortcuts dialog: see `ncrs_settings::keys::key_bindings_key`.
pub(in crate::app) fn key_bindings_key(
    prompt: &PromptKeyState,
    key: &Key,
    key_modifiers: Modifiers,
) -> Option<Message> {
    ncrs_settings::keys::key_bindings_key(prompt.key_bindings_recording, key, key_modifiers)
        .map(Message::KeyBindings)
}

/// The tab shows no panel, so the panel keys do nothing. Of the bindings the
/// command list, the key that closes the tab and quitting reach through. A
/// card opened from the tab has the Escape first.
pub(in crate::app) fn settings_key(
    prompt: &PromptKeyState,
    key: &Key,
    key_modifiers: Modifiers,
    scope: KeyScope,
) -> Option<Message> {
    let bound = || {
        bound_message(&prompt.keymap, key, key_modifiers, scope)
            .filter(|message| {
                *message == Message::Quit
                    || *message == KeyAction::CommandPalette.message()
                    || *message == KeyAction::Settings.message()
                    || *message == KeyAction::KeyBindings.message()
            })
            .filter(|_| is_not_typed_by_text_field(key, key_modifiers))
    };
    match ncrs_settings::keys::settings_key(key, || bound().is_some())? {
        SettingsKey::Close if prompt.filing_setup => {
            Some(Message::FilingSetup(FilingSetupMessage::Close))
        }
        SettingsKey::Close => Some(Message::Settings(SettingsMessage::Close)),
        SettingsKey::Bound => bound(),
    }
}

// A visible card whose footer promises "Esc: close" goes before the job
// (same thought as the delete dialog): the filing setup, then a filing
// card, then a running job, then the config error. Unbound rather than
// bound to something that does nothing — a key that means different things
// depending on invisible state is worse than one that is simply free.
pub(in crate::app) fn escape_message(prompt: &PromptKeyState) -> Option<Message> {
    if prompt.filing_setup {
        Some(Message::FilingSetup(FilingSetupMessage::Close))
    } else if prompt.filing_card {
        Some(Message::Filing(FilingMessage::Dismiss))
    } else if prompt.job_running {
        Some(Message::AbortJob)
    } else if prompt.config_error {
        Some(Message::DismissConfigError)
    } else if prompt.hit_list {
        Some(Message::HitList(HitListMessage::BackToSearch))
    } else {
        None
    }
}

/// Keys while the search view is up: see `ncrs_search::search_key`.
pub(in crate::app) fn search_key(
    prompt: &PromptKeyState,
    key: &Key,
    key_modifiers: Modifiers,
) -> Option<Message> {
    let bound = bound_message(&prompt.keymap, key, key_modifiers, KeyScope::Search);
    let facts = SearchKeyFacts {
        tag_or_toggle: bound.as_ref().and_then(tag_or_toggle),
        bound_untyped: bound.is_some() && is_not_typed_by_text_field(key, key_modifiers),
        type_ahead: match type_ahead_message(key, key_modifiers) {
            Some(Message::Panel(PanelMessage::TypeAhead(typed))) => Some(typed),
            _ => None,
        },
    };
    match ncrs_search::search_key(key, key_modifiers, facts)? {
        SearchKey::Message(message) => Some(Message::Search(message)),
        SearchKey::Bound => bound,
    }
}

/// Keys while the trash view is up: it owns the cursor keys, Enter puts back,
/// and the tagging keys tag its rows. Of the other bindings only the few that
/// leave the view reach through; everything else, F5 and F8 included, does
/// nothing here.
pub(in crate::app) fn trash_key(
    prompt: &PromptKeyState,
    key: &Key,
    key_modifiers: Modifiers,
) -> Option<Message> {
    let bound = bound_message(&prompt.keymap, key, key_modifiers, KeyScope::Panel);
    let kind = match bound.as_ref() {
        Some(message) => {
            if let Some(tagging) = trash_tagging(message) {
                TrashBound::Tagging(tagging)
            } else if *message == Message::Quit {
                TrashBound::Quit
            } else if *message == Message::Panel(PanelMessage::SwitchPanel) {
                TrashBound::SwitchPanel
            } else if *message == KeyAction::CommandPalette.message() {
                TrashBound::CommandPalette
            } else if *message == KeyAction::RestoreLastDeletion.message() {
                TrashBound::RestoreLast
            } else if *message == KeyAction::OpenTrashInFinder.message() {
                TrashBound::OpenInFinder
            } else {
                TrashBound::Other
            }
        }
        None => TrashBound::Other,
    };
    let pressed = ncrs_ops::keys::trash_key(prompt.job_running, key, key_modifiers, kind)?;
    match pressed {
        TrashKey::Message(message) => Some(Message::Trash(message)),
        TrashKey::AbortJob => Some(Message::AbortJob),
        TrashKey::Bound(_) => bound,
    }
}

/// The trash view's version of a bound tagging message.
fn trash_tagging(message: &Message) -> Option<TrashMessage> {
    match message {
        Message::Panel(PanelMessage::ToggleTag) => Some(TrashMessage::ToggleTag),
        Message::Panel(PanelMessage::TagMove(delta)) => Some(TrashMessage::TagMove(*delta)),
        Message::Panel(PanelMessage::TagAll) => Some(TrashMessage::TagAll),
        Message::Panel(PanelMessage::ClearTags) => Some(TrashMessage::ClearTags),
        _ => None,
    }
}

/// The search's version of a bound tagging or toggle message.
fn tag_or_toggle(message: &Message) -> Option<SearchMessage> {
    match message {
        Message::Panel(PanelMessage::ToggleTag) => Some(SearchMessage::ToggleTag),
        Message::Panel(PanelMessage::TagMove(delta)) => Some(SearchMessage::TagMove(*delta)),
        Message::Panel(PanelMessage::TagAll) => Some(SearchMessage::TagAll),
        Message::Panel(PanelMessage::ClearTags) => Some(SearchMessage::ClearTags),
        Message::Search(toggle) if toggle.is_toggle() => Some(toggle.clone()),
        _ => None,
    }
}

/// The search toggle a press is bound to, resolved as the key routing does it.
/// The search fields use it to claim the press before they type its character.
pub fn search_toggle_for_press(
    keymap: &Keymap,
    key: Key,
    physical_key: keyboard::key::Physical,
    modifiers: Modifiers,
) -> Option<SearchMessage> {
    let bound_key = binding_key(key, physical_key, modifiers);
    match bound_message(keymap, &bound_key, modifiers, KeyScope::Search)? {
        Message::Search(toggle) if toggle.is_toggle() => Some(toggle),
        _ => None,
    }
}
