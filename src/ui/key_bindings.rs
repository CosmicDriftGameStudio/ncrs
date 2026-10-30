//! The keyboard shortcuts dialog: a search field, every action with its group
//! and its keys as key caps, and below the list what the last edit needs or
//! said. Pure view over [`KeyBindings`].

use iced::widget::text::Wrapping;
use iced::widget::{column, container, mouse_area, row, text, Row as WidgetRow};
use iced::{alignment, Element, Length, Size};

use super::format;
use super::frame::{self, DialogSize, Frame, PageHint};
use super::kit::{self, ListView, Tone};
use super::theme::{font_size, size, spacing};
use crate::app::fill_template;
use crate::i18n::{FuturamaName, Language, Msg};
use crate::key_bindings::{KeyBindings, Mode, Notice, Row, VISIBLE_ROWS};
use crate::keymap::{binding_label, Binding, KeyRefusal, MAX_KEYS_PER_ACTION};
use crate::messages::{KeyBindingsMessage, Message};
use crate::palette::Palette;

pub const SEARCH_FIELD_ID: &str = "key-bindings-query";

/// The shares of a row: the action, its group, its keys.
const ACTION_PORTION: u16 = 6;
const GROUP_PORTION: u16 = 2;
const KEYS_PORTION: u16 = 4;

/// A theme name cut shorter than this many characters says nothing, so it is
/// left out instead.
const MIN_THEME_NAME_CHARS: usize = 4;

/// The cap that adds a key, on the selected row.
const ADD_CAP: &str = "+";

const HINTS: &[PageHint] = &[
    PageHint {
        key: "⌫",
        verb: Msg::KeyBindingsHintRemove,
    },
    PageHint {
        key: "⇧⌫",
        verb: Msg::KeyBindingsHintReset,
    },
];

fn to_dialog(message: KeyBindingsMessage) -> Message {
    Message::KeyBindings(message)
}

fn label_of(binding: &Binding) -> String {
    binding_label(binding).unwrap_or_default()
}

pub fn view<'a>(
    dialog: &'a KeyBindings,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let field = kit::text_field(
        lang.text(Msg::KeyBindingsSearchPlaceholder),
        dialog.query(),
        SEARCH_FIELD_ID,
        Some(|query| to_dialog(KeyBindingsMessage::SearchChanged(query))),
        to_dialog(KeyBindingsMessage::Record),
        palette,
    );
    let offset = dialog.offset();
    let rows: Vec<Element<'a, Message>> = dialog
        .rows()
        .iter()
        .enumerate()
        .skip(offset)
        .take(VISIBLE_ROWS)
        .map(|(index, entry)| action_row(dialog, index, entry, action_width(window), lang, palette))
        .collect();
    let list = kit::scroll_list(
        header(lang, palette),
        rows,
        ListView {
            len: dialog.rows().len(),
            visible: VISIBLE_ROWS,
            offset,
        },
        |delta| to_dialog(KeyBindingsMessage::Scrolled(delta)),
        palette,
    );
    let mut content = column![field, list].spacing(spacing::M);
    if let Some(line) = status(dialog, lang, palette) {
        content = content.push(line);
    }

    let frame = Frame::new(
        lang.text(Msg::KeyBindingsTitle),
        content,
        DialogSize::Large,
        window,
        lang,
        palette,
    )
    .title_theme(lang.futurama_name(FuturamaName::KeyBindingsTitle))
    .on_close(to_dialog(KeyBindingsMessage::Close))
    .buttons(buttons(dialog, lang, palette));
    match dialog.mode() {
        Mode::Recording { .. } => frame.footer_note(lang.text(Msg::KeyBindingsRecordingNote)),
        Mode::Browsing => frame.page_hints(HINTS),
        Mode::Clash(_) | Mode::ResetAllQuestion => frame,
    }
    .into_element()
}

fn header<'a>(lang: Language, palette: &Palette) -> Element<'a, Message> {
    let title = |msg: Msg, portion: u16| {
        container(
            text(lang.text(msg).to_owned())
                .size(font_size::COLUMN_HEADER)
                .color(palette.dim_text)
                .wrapping(Wrapping::None),
        )
        .width(Length::FillPortion(portion))
        .clip(true)
    };
    row![
        title(Msg::KeyBindingsColumnAction, ACTION_PORTION),
        title(Msg::KeyBindingsColumnGroup, GROUP_PORTION),
        title(Msg::KeyBindingsColumnKeys, KEYS_PORTION),
    ]
    .spacing(spacing::S)
    .padding([0.0, spacing::CELL_PADDING_X])
    .height(size::COLUMN_HEADER)
    .align_y(alignment::Vertical::Center)
    .into()
}

/// The pixels the action column of a row has.
fn action_width(window: Size) -> f32 {
    let row_width = frame::content_width(DialogSize::Large, window)
        - 2.0 * spacing::CELL_PADDING_X
        - 2.0 * spacing::S;
    let portions = f32::from(ACTION_PORTION + GROUP_PORTION + KEYS_PORTION);
    row_width * f32::from(ACTION_PORTION) / portions
}

/// The theme name as it fits behind the label and the changed mark in
/// `width`, cut with "…", or `None` when too little of it would show. The
/// label always comes first.
fn fitted_theme_name(
    label: &str,
    theme_name: &str,
    changed_mark: Option<&str>,
    width: f32,
) -> Option<String> {
    let taken_by = |part: &str| format::estimated_width(part, font_size::ROW) + spacing::S;
    let room = width
        - format::estimated_width(label, font_size::ROW)
        - changed_mark.map_or(0.0, taken_by)
        - spacing::S;
    let fitted = format::fit_end(theme_name, room, font_size::ROW);
    (fitted.chars().count() >= MIN_THEME_NAME_CHARS).then_some(fitted)
}

fn action_row<'a>(
    dialog: &KeyBindings,
    index: usize,
    entry: &Row,
    width: f32,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let selected = index == dialog.cursor();
    let (color, dim_color) = if selected {
        (palette.cursor_text, palette.cursor_text)
    } else {
        (palette.text, palette.dim_text)
    };
    let one_line = |content: String, line_color| {
        text(content)
            .size(font_size::ROW)
            .color(line_color)
            .wrapping(Wrapping::None)
    };
    let changed_mark = (!entry.is_built_in).then(|| lang.text(Msg::KeyBindingsChanged));
    let mut name = row![one_line(entry.label.to_owned(), color)].spacing(spacing::S);
    if let Some(theme_name) = entry
        .theme_name
        .and_then(|theme_name| fitted_theme_name(entry.label, theme_name, changed_mark, width))
    {
        name = name.push(one_line(theme_name, dim_color));
    }
    if let Some(mark) = changed_mark {
        name = name.push(one_line(
            mark.to_owned(),
            if selected {
                palette.cursor_text
            } else {
                palette.accent
            },
        ));
    }
    let content = row![
        container(name)
            .width(Length::FillPortion(ACTION_PORTION))
            .clip(true),
        container(one_line(
            lang.text(entry.group.label()).to_owned(),
            dim_color
        ))
        .width(Length::FillPortion(GROUP_PORTION))
        .clip(true),
        container(key_caps(dialog, index, entry, selected, lang, palette))
            .width(Length::FillPortion(KEYS_PORTION))
            .clip(true),
    ]
    .spacing(spacing::S)
    .align_y(alignment::Vertical::Center);
    kit::list_row(
        content.into(),
        to_dialog(KeyBindingsMessage::Select {
            row: index,
            key: None,
        }),
        selected,
        true,
        palette,
    )
}

/// The keys of a row. On the selected row the focused one is drawn apart,
/// the slot that adds a key follows the last, and while recording the key
/// being replaced or added reads "press the new key combination".
fn key_caps<'a>(
    dialog: &KeyBindings,
    index: usize,
    entry: &Row,
    selected: bool,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let recording = selected && dialog.is_recording();
    let focused = selected.then(|| dialog.key_cursor());
    let cap = |label: &str, slot: usize| -> Element<'a, Message> {
        let drawn = if focused == Some(slot) {
            kit::key_cap_focused(label, palette)
        } else {
            kit::key_cap_in_row(label, selected, palette)
        };
        mouse_area(drawn)
            .on_press(to_dialog(KeyBindingsMessage::Select {
                row: index,
                key: Some(slot),
            }))
            .into()
    };
    let prompt = || -> Element<'a, Message> {
        text(lang.text(Msg::KeyBindingsRecording).to_owned())
            .size(font_size::ROW)
            .color(palette.cursor_text)
            .wrapping(Wrapping::None)
            .into()
    };
    let mut caps: Vec<Element<'a, Message>> = Vec::new();
    for (slot, binding) in entry.keys.iter().enumerate() {
        if recording && focused == Some(slot) {
            caps.push(prompt());
        } else {
            caps.push(cap(&label_of(binding), slot));
        }
    }
    let add_slot = entry.keys.len();
    if recording && focused.is_some_and(|slot| slot >= add_slot) {
        caps.push(prompt());
    } else if selected && add_slot < MAX_KEYS_PER_ACTION {
        caps.push(cap(ADD_CAP, add_slot));
    }
    if entry.keys.is_empty() && !recording {
        let color = if selected {
            palette.cursor_text
        } else {
            palette.dim_text
        };
        caps.insert(
            0,
            text(lang.text(Msg::KeyBindingsNoKey).to_owned())
                .size(font_size::ROW)
                .color(color)
                .wrapping(Wrapping::None)
                .into(),
        );
    }
    WidgetRow::with_children(caps)
        .spacing(spacing::XS)
        .align_y(alignment::Vertical::Center)
        .into()
}

/// The line under the list: the clash or the question waiting for an
/// answer, or why the last edit did not happen.
fn status<'a>(
    dialog: &KeyBindings,
    lang: Language,
    palette: &Palette,
) -> Option<Element<'a, Message>> {
    match dialog.mode() {
        Mode::Clash(clash) => {
            let lines = clash.taken.iter().map(|(holder, binding)| {
                let holder_label = dialog.action_name(*holder);
                fill_template(
                    lang.text(Msg::KeyBindingsClash),
                    &[("{key}", &label_of(binding)), ("{action}", holder_label)],
                )
            });
            return Some(kit::warning_box(
                lines.collect::<Vec<_>>().join("\n"),
                palette,
            ));
        }
        Mode::ResetAllQuestion => {
            return Some(kit::warning_box(
                lang.text(Msg::KeyBindingsResetAllQuestion).to_owned(),
                palette,
            ));
        }
        Mode::Browsing | Mode::Recording { .. } => {}
    }
    let shown = match dialog.notice()? {
        Notice::Refused(KeyRefusal::Unnamed) => {
            lang.text(Msg::KeyBindingsRefusedUnnamed).to_owned()
        }
        Notice::Refused(KeyRefusal::NeedsModifier) => {
            lang.text(Msg::KeyBindingsRefusedNeedsModifier).to_owned()
        }
        Notice::Refused(KeyRefusal::Escape) => lang.text(Msg::KeyBindingsRefusedEscape).to_owned(),
        Notice::Refused(KeyRefusal::FixedInSearch) => {
            lang.text(Msg::KeyBindingsRefusedFixedInSearch).to_owned()
        }
        Notice::TooMany => fill_template(
            lang.text(Msg::KeyBindingsTooMany),
            &[("{count}", &MAX_KEYS_PER_ACTION.to_string())],
        ),
        Notice::AlreadyBound(binding) => fill_template(
            lang.text(Msg::KeyBindingsAlreadyBound),
            &[("{key}", &label_of(binding))],
        ),
        Notice::NotWritten(reason) => fill_template(
            lang.text(Msg::KeyBindingsNotWritten),
            &[("{reason}", reason)],
        ),
    };
    Some(kit::error_line(shown, palette))
}

fn buttons<'a>(
    dialog: &KeyBindings,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    let button = |msg: Msg, message: Option<KeyBindingsMessage>| {
        kit::button(
            lang.text(msg),
            message.map(to_dialog),
            Tone::Secondary,
            palette,
        )
    };
    let enter = |msg: Msg, message: KeyBindingsMessage| {
        kit::enter_button(
            lang.text(msg),
            Some(to_dialog(message)),
            Tone::Primary,
            palette,
        )
    };
    match dialog.mode() {
        Mode::Browsing => {
            let selected = dialog.selected();
            let can_reset = selected.is_some_and(|entry| !entry.is_built_in);
            let has_selection = selected.is_some();
            vec![
                button(Msg::KeyBindingsResetAll, Some(KeyBindingsMessage::ResetAll)),
                button(
                    Msg::KeyBindingsReset,
                    can_reset.then_some(KeyBindingsMessage::Reset),
                ),
                button(
                    Msg::KeyBindingsAddKey,
                    has_selection.then_some(KeyBindingsMessage::RecordAdditional),
                ),
                kit::enter_button(
                    lang.text(Msg::KeyBindingsChangeKey),
                    has_selection.then_some(to_dialog(KeyBindingsMessage::Record)),
                    Tone::Primary,
                    palette,
                ),
            ]
        }
        Mode::Recording { .. } => vec![button(
            Msg::DialogCancel,
            Some(KeyBindingsMessage::CancelRecording),
        )],
        Mode::Clash(_) => vec![
            button(Msg::DialogCancel, Some(KeyBindingsMessage::Dismiss)),
            enter(Msg::KeyBindingsTake, KeyBindingsMessage::Confirm),
        ],
        Mode::ResetAllQuestion => vec![
            button(Msg::DialogCancel, Some(KeyBindingsMessage::Dismiss)),
            enter(Msg::KeyBindingsResetAll, KeyBindingsMessage::Confirm),
        ],
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_theme_name_with_room_shows_whole() {
        assert_eq!(
            fitted_theme_name("Copy", "Teleporter", None, 400.0).as_deref(),
            Some("Teleporter")
        );
    }

    #[test]
    fn a_theme_name_without_room_is_cut_with_an_ellipsis() {
        let label = "Unmark all";
        let theme_name = "Remove every stamp from the crates";
        let width = format::estimated_width(label, font_size::ROW)
            + spacing::S
            + format::estimated_width("Remove every", font_size::ROW);
        let fitted = fitted_theme_name(label, theme_name, None, width).unwrap();
        assert!(fitted.ends_with('…'), "{fitted}");
        assert!(theme_name.starts_with(fitted.trim_end_matches('…')));
    }

    #[test]
    fn a_theme_name_that_would_show_only_a_stub_is_left_out() {
        let label = "Mark the row or remove its mark";
        let width = format::estimated_width(label, font_size::ROW) + 20.0;
        assert_eq!(fitted_theme_name(label, "Stamp", None, width), None);
    }

    #[test]
    fn the_changed_mark_takes_its_room_before_the_theme_name() {
        let label = "Copy";
        let width = format::estimated_width(label, font_size::ROW)
            + spacing::S
            + format::estimated_width("Teleporter", font_size::ROW)
            + 1.0;
        assert_eq!(
            fitted_theme_name(label, "Teleporter", None, width).as_deref(),
            Some("Teleporter")
        );
        assert_ne!(
            fitted_theme_name(label, "Teleporter", Some("changed"), width).as_deref(),
            Some("Teleporter")
        );
    }
}
