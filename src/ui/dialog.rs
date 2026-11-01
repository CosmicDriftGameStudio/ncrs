//! The modal prompt: a text field and two buttons over a dimmed background.
//!
//! Pure function, like every other component here: it takes the prompt state
//! and the language and returns an `Element`. It never mutates anything.

use iced::widget::{column, container, row};
use iced::{alignment, Background, Element, Length, Size};

use super::frame::{DialogSize, Frame, KeyHints};
use super::hermes_kit;
use super::kit::{self, Check, Tone};
use super::theme::{self, spacing};
use crate::app::fill_template;
use crate::dialog::{Prompt, PromptKind};
use crate::i18n::{Language, Msg};
use crate::messages::Message;
use crate::palette::Palette;
use ncrs_ui_kit::numbered_hint::numbered_hint;

/// The text field carries an id, because `TextInput::id` exists and
/// `iced_selector` can find it. The buttons do not — see the note where they
/// are built.
pub const FIELD_ID: &str = "prompt-field";

/// Renders the prompt for `state`, or nothing when there is no prompt.
///
/// `on_input`, `on_submit` and `on_cancel` are passed in rather than built
/// here, so this component stays a view and the app decides the messages.
pub fn view<'a>(
    state: &'a Prompt,
    window: Size,
    lang: Language,
    palette: &Palette,
    on_input: impl Fn(String) -> Message + 'a,
    on_submit: Message,
    on_cancel: Message,
) -> Option<Element<'a, Message>> {
    let (title, label, confirm) = match state.kind() {
        PromptKind::CreateDir => (
            lang.text(Msg::DialogMkdirTitle),
            lang.text(Msg::DialogMkdirLabel),
            lang.text(Msg::DialogMkdirCreate),
        ),
        // The label and button say "name" and "create", which fit a file too.
        PromptKind::CreateFile => (
            lang.text(Msg::DialogNewFileTitle),
            lang.text(Msg::DialogMkdirLabel),
            lang.text(Msg::DialogMkdirCreate),
        ),
        PromptKind::Paste => (
            lang.text(Msg::DialogPasteTitle),
            lang.text(Msg::DialogMkdirLabel),
            lang.text(Msg::DialogPasteConfirm),
        ),
        PromptKind::Pack => (
            lang.text(Msg::DialogPackTitle),
            lang.text(Msg::DialogPackLabel),
            lang.text(Msg::DialogPackConfirm),
        ),
    };

    let field = kit::text_field(
        label,
        state.name(),
        FIELD_ID,
        Some(on_input),
        on_submit.clone(),
        palette,
    );

    // `on_press_maybe(None)` is how iced disables a button: no message, so a
    // keypress while the operation runs does nothing. There is no `disabled`.
    // The buttons carry no `widget::Id`: `Button::id` does not exist in iced
    // 0.14, and `iced_selector` can only find widgets by id. So they are found
    // by their bounds instead — see `ui_tests`, which clicks at the position
    // the layout reports for them. That is why the layout test and the click
    // test have to agree about where the buttons are.
    let idle = !state.busy();
    let confirm_button =
        kit::enter_button(confirm, idle.then_some(on_submit), Tone::Primary, palette);
    let cancel_button = kit::button(
        lang.text(Msg::DialogCancel),
        idle.then(|| on_cancel.clone()),
        Tone::Secondary,
        palette,
    );

    let mut body = column![field].spacing(spacing::LINE_GAP);
    let hits = state.hits().filter(|_| state.kind() == PromptKind::Pack);
    if let Some(pack) = hits {
        body = body.push(keep_folders_row(pack.keep_folders, lang, palette));
        if !pack.keep_folders {
            body = body.extend(numbered_hint(
                lang.text(Msg::HintNumberedInArchive),
                &pack.numbered,
                Some(&pack.scope),
                lang,
                palette,
            ));
        }
    }

    if let Some(error) = state.error() {
        // A validation reason is a message id, not a sentence: the dialog layer
        // asked the user for something impossible and gets to word it.
        let message: &str = match validation_message(error) {
            Some(mid) => lang.text(mid),
            None => error,
        };
        body = body.push(kit::error_line(message, palette));
    }
    let mut framed = Frame::new(title, body, DialogSize::Small, window, lang, palette)
        .buttons(vec![cancel_button, confirm_button])
        .hints(KeyHints {
            enter: true,
            tab: false,
        });
    if hits.is_some() {
        framed = framed.subtitle(subtitle_of_hits(state, lang));
    }
    if idle {
        framed = framed.on_close(on_cancel);
    }
    Some(framed.into_element())
}

fn subtitle_of_hits(state: &Prompt, lang: Language) -> String {
    fill_template(
        lang.text(Msg::DialogPackSubtitleHits),
        &[("{count}", &state.sources().len().to_string())],
    )
}

/// The checkbox that keeps the folders of the hits in the archive.
fn keep_folders_row<'a>(keep: bool, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let state = if keep { Check::On } else { Check::Off };
    row![
        hermes_kit::checkbox(
            state,
            Some(Message::PromptToggleKeepFolders),
            Some(lang.text(Msg::DialogPackKeepFolders)),
            palette,
        ),
        kit::dim(lang.text(Msg::DialogPackKeepFoldersNote), palette),
    ]
    .spacing(spacing::LINE_GAP)
    .align_y(alignment::Vertical::Center)
    .into()
}

/// Maps a validation reason to its message, or `None` for a message the fs
/// layer already wrote in full.
fn validation_message(reason: &str) -> Option<Msg> {
    match reason {
        "empty_name" => Some(Msg::ErrorEmptyName),
        "reserved_name" => Some(Msg::ErrorReservedName),
        "separator_not_allowed" => Some(Msg::ErrorSeparatorNotAllowed),
        "archive_extension" => Some(Msg::ErrorPackExtension),
        "archive_exists" => Some(Msg::ErrorPackExists),
        "archive_folder_missing" => Some(Msg::ErrorPackFolderMissing),
        _ => None,
    }
}

/// A scrim behind the dialog, so clicks outside it do not reach the panels.
pub fn scrim<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(alignment::Horizontal::Center)
        .align_y(alignment::Vertical::Center)
        .style(|_theme| container::Style {
            background: Some(Background::Color(theme::SCRIM)),
            ..container::Style::default()
        })
        .into()
}
