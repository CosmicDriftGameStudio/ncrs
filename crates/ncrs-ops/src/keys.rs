//! What a key does while one of the operation overlays is up: the button
//! dialogs, the conflict question, the transfer dialog and the trash view.
//! The app asks the keymap what the key is bound to and passes that in as far
//! as the overlay lets it through.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use crate::message::{ConflictChoice, TransferDialogMessage, TrashMessage};

/// The keys every button dialog shares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKey {
    /// Presses the focused button.
    Activate,
    Focus(isize),
}

/// Arrows and Tab move the focus, Enter presses the focused button.
pub fn dialog_focus_key(key: &Key, modifiers: Modifiers) -> Option<DialogKey> {
    match key.as_ref() {
        Key::Named(Named::Enter) => Some(DialogKey::Activate),
        Key::Named(Named::ArrowLeft) => Some(DialogKey::Focus(-1)),
        Key::Named(Named::ArrowRight) => Some(DialogKey::Focus(1)),
        Key::Named(Named::Tab) if modifiers.shift() => Some(DialogKey::Focus(-1)),
        Key::Named(Named::Tab) => Some(DialogKey::Focus(1)),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKey {
    Answer(ConflictChoice),
    ToggleAll,
    Dialog(DialogKey),
}

/// Keys while a copy asks what to do with a file that exists. `aborts` is
/// asked only for a key the dialog does not know: whether it is bound to the
/// abort of a job.
pub fn conflict_key(
    key: &Key,
    modifiers: Modifiers,
    aborts: impl FnOnce() -> bool,
) -> Option<ConflictKey> {
    match key.as_ref() {
        // Escape means "no" to a question, and "keep" is the no that does
        // not lose data. Cancelling the whole transfer is on the key that
        // already aborts a job (Ctrl+C on a Mac unless rebound).
        Key::Named(Named::Escape) => Some(ConflictKey::Answer(ConflictChoice::ThisKeep)),
        // Space ticks "for all files", as it ticks a checkbox elsewhere.
        Key::Named(Named::Space) => Some(ConflictKey::ToggleAll),
        // The fixed dialog keys before the configured one, so a rebound
        // abort key cannot take Enter or an arrow. The key that aborts a
        // job then cancels the whole operation.
        _ => dialog_focus_key(key, modifiers)
            .map(ConflictKey::Dialog)
            .or_else(|| aborts().then_some(ConflictKey::Answer(ConflictChoice::Cancel))),
    }
}

/// What a press is bound to, as far as the transfer overlays care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    Quit,
    AbortJob,
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TransferOverlayKey {
    Dialog(TransferDialogMessage),
    AbortJob,
    Quit,
}

/// Keys while the transfer dialog or the progress mask is up. `bound` is asked
/// for a key the overlay does not know.
pub fn transfer_overlay_key(
    dialog_open: bool,
    key: &Key,
    not_typed_by_text_field: bool,
    bound: impl FnOnce() -> Bound,
) -> Option<TransferOverlayKey> {
    if dialog_open {
        let dialog = |message| Some(TransferOverlayKey::Dialog(message));
        return match key.as_ref() {
            Key::Named(Named::Escape) => dialog(TransferDialogMessage::Cancel),
            Key::Named(Named::Enter) => dialog(TransferDialogMessage::Confirm),
            Key::Named(Named::Tab) => dialog(TransferDialogMessage::ToggleKind),
            _ => (bound() == Bound::Quit && not_typed_by_text_field)
                .then_some(TransferOverlayKey::Quit),
        };
    }
    // The progress mask has no field. Escape stops the job, as it does with the
    // mask closed; the panel keys do nothing behind it.
    match key.as_ref() {
        Key::Named(Named::Escape) => Some(TransferOverlayKey::AbortJob),
        _ => match bound() {
            Bound::Quit => Some(TransferOverlayKey::Quit),
            Bound::AbortJob => Some(TransferOverlayKey::AbortJob),
            Bound::Other => None,
        },
    }
}

/// What a press is bound to, as far as the trash view cares.
#[derive(Debug, Clone, PartialEq)]
pub enum TrashBound {
    /// A tagging key, as the trash view's own message.
    Tagging(TrashMessage),
    Quit,
    SwitchPanel,
    CommandPalette,
    RestoreLast,
    OpenInFinder,
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TrashKey {
    Message(TrashMessage),
    AbortJob,
    /// A binding that reaches through the trash view.
    Bound(TrashBound),
}

/// Keys while the trash view is up: it owns the cursor keys, Enter puts back,
/// and the tagging keys tag its rows. Of the other bindings only the few that
/// leave the view reach through; everything else, F5 and F8 included, does
/// nothing here.
pub fn trash_key(
    job_running: bool,
    key: &Key,
    modifiers: Modifiers,
    bound: TrashBound,
) -> Option<TrashKey> {
    let trash = |message| Some(TrashKey::Message(message));
    if let TrashBound::Tagging(message) = bound {
        return trash(message);
    }
    if (modifiers - Modifiers::SHIFT).is_empty() {
        match key.as_ref() {
            Key::Named(Named::Escape) if job_running => return Some(TrashKey::AbortJob),
            Key::Named(Named::Escape) => return trash(TrashMessage::Close),
            Key::Named(Named::ArrowUp) => return trash(TrashMessage::Move(-1)),
            Key::Named(Named::ArrowDown) => return trash(TrashMessage::Move(1)),
            Key::Named(Named::PageUp) => return trash(TrashMessage::Page(-1)),
            Key::Named(Named::PageDown) => return trash(TrashMessage::Page(1)),
            Key::Named(Named::Home) => return trash(TrashMessage::First),
            Key::Named(Named::End) => return trash(TrashMessage::Last),
            Key::Named(Named::Enter) => return trash(TrashMessage::Restore),
            _ => {}
        }
    }
    match bound {
        TrashBound::Tagging(_) | TrashBound::Other => None,
        passes => Some(TrashKey::Bound(passes)),
    }
}
