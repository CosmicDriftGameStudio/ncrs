//! The card of the first-time setup of Hermes: what Hermes does, the three
//! steps to take, and the button that starts it. Modal, over the scrim.

use std::path::Path;

use iced::widget::text::Wrapping;
use iced::widget::{checkbox, column, container, radio, text, Row, Space};
use iced::{alignment, Color, Element, Length, Size};
use ncrs_ai::LOCAL_MODEL;

use super::filing::radio_look;
use super::hermes_flow::{self, FlowBox, FlowBoxes};
use crate::filing_setup::{shown_path, SetupNote, SetupProblem};
use crate::hermes_setup::{ExistingIndex, FolderSlot, HermesSetup, SetupFocus};
use crate::message::{HermesSetupMessage, Message};
use ncrs_config::config::FilingLocation;
use ncrs_config::palette::Palette;
use ncrs_i18n::fill_template;
use ncrs_i18n::{FuturamaName, Language, Msg};
use ncrs_ui_kit::format;
use ncrs_ui_kit::frame::{self, DialogSize, Frame, KeyHints};
use ncrs_ui_kit::kit::{self, dim, error_line, paragraph, Icon, Tone};
use ncrs_ui_kit::theme::{self, font_size, spacing};

const SIZE: DialogSize = DialogSize::Card;

const BADGE_SIZE: f32 = 12.0;
const HEADLINE_SIZE: f32 = 24.0;
const INTRO_WIDTH: f32 = 720.0;
const TILE_PADDING: f32 = 16.0;
const TILE_GAP: f32 = 16.0;
const FIELD_PADDING: [f32; 2] = [8.0, 10.0];
const OPTION_PADDING: [f32; 2] = [8.0, 10.0];
const FOCUS_RING_PADDING: f32 = 2.0;
/// What a path in a tile keeps free beside the tile's padding and border.
const TILE_RESERVED: f32 = 2.0 * TILE_PADDING + 24.0;

/// What the card shows besides the setup's own state.
#[derive(Debug)]
pub struct SetupView<'a> {
    pub setup: &'a HermesSetup,
    /// The element Enter would press.
    pub focus: Option<SetupFocus>,
    /// The locations are being written, or the module is starting or creating
    /// the index: nothing can be changed.
    pub busy: bool,
    pub creating_index: bool,
    /// Why the module could not create the index.
    pub creation_error: Option<&'a str>,
    pub home: &'a Path,
    pub flow_step: u32,
    pub lang: Language,
    pub window: Size,
}

/// The size of the AI model, as the card and the download line say it.
pub fn model_size_label() -> String {
    ncrs_ui_kit::format::megabytes(LOCAL_MODEL.size)
}

pub(in crate::ui) fn on_this_computer(lang: Language, mac: Msg, other: Msg) -> &'static str {
    lang.text(if cfg!(target_os = "macos") {
        mac
    } else {
        other
    })
}

pub fn view<'a>(card: &SetupView<'a>, palette: &Palette) -> Element<'a, Message> {
    let lang = card.lang;
    let can_start = card.setup.can_start() && !card.busy;
    let mut body = column![].spacing(spacing::XL);
    if card.setup.offer().hermes_was_off {
        body = body.push(paragraph(lang.text(Msg::HermesSetupOffNote), palette));
    }
    body = body
        .push(header(lang, palette))
        .push(hermes_flow::flow(card.flow_step, flow_boxes(lang), palette))
        .push(steps(card, palette));
    if card.setup.offer().model_download {
        body = body.push(model_offer(card, palette));
    }
    if let Some(line) = problem_line(card, palette) {
        body = body.push(line);
    }
    if card.creating_index {
        body = body.push(dim(lang.text(Msg::HermesSetupCreatingIndex), palette));
    }
    if let Some(reason) = card.creation_error {
        body = body.push(error_line(
            fill_template(lang.text(Msg::FilingCreateFailed), &[("{reason}", reason)]),
            palette,
        ));
    }

    let focused = |element| card.focus == Some(element);
    let later = kit::button(
        lang.text(Msg::HermesSetupLater),
        (!card.busy).then_some(setup_message(HermesSetupMessage::Later)),
        Tone::filled_if(focused(SetupFocus::Later)),
        palette,
    );
    let start_press = can_start.then_some(setup_message(HermesSetupMessage::Start));
    let start = kit::enter_button(
        lang.text(Msg::HermesSetupStart),
        start_press,
        Tone::Primary,
        palette,
    );
    let mut framed = Frame::new(
        lang.text(Msg::HermesSetupFrameTitle),
        body,
        SIZE,
        card.window,
        lang,
        palette,
    )
    .title_theme(lang.futurama_name(FuturamaName::HermesSetupFrameTitle))
    .buttons(vec![later, start])
    .hints(KeyHints {
        enter: true,
        tab: true,
    });
    if !card.busy {
        framed = framed.on_close(setup_message(HermesSetupMessage::Later));
    }
    framed.into_element()
}

fn setup_message(message: HermesSetupMessage) -> Message {
    Message::HermesSetup(message)
}

fn flow_boxes(lang: Language) -> FlowBoxes<'static> {
    let plain = |title: &'static str| FlowBox {
        title: title.into(),
        note: None,
    };
    let with_note = |title: &'static str, note: &'static str| FlowBox {
        title: title.into(),
        note: Some(note.into()),
    };
    FlowBoxes {
        inbox: plain(lang.text(Msg::HermesSetupFlowInbox)),
        archive: plain(lang.text(Msg::HermesSetupFlowArchive)),
        reading: with_note(
            lang.text(Msg::HermesSetupFlowRead),
            lang.text(Msg::HermesSetupFlowReadNote),
        ),
        rules: with_note(
            lang.text(Msg::HermesSetupFlowRules),
            lang.text(Msg::HermesSetupFlowRulesNote),
        ),
        filed: with_note(
            lang.text(Msg::HermesSetupFlowFiled),
            lang.text(Msg::HermesSetupFlowFiledNote),
        ),
    }
}

fn header<'a>(lang: Language, palette: &Palette) -> Element<'a, Message> {
    let intro = container(
        text(on_this_computer(
            lang,
            Msg::HermesSetupIntroMac,
            Msg::HermesSetupIntroOther,
        ))
        .size(font_size::ROW)
        .color(palette.dim_text)
        .wrapping(Wrapping::Word)
        .align_x(alignment::Horizontal::Center),
    )
    .max_width(INTRO_WIDTH);
    column![
        text(lang.text(Msg::HermesSetupBadge))
            .size(BADGE_SIZE)
            .color(palette.success),
        text(lang.text(Msg::HermesSetupTitle))
            .size(HEADLINE_SIZE)
            .color(palette.text)
            .wrapping(Wrapping::Word),
        intro,
    ]
    .spacing(spacing::M)
    .align_x(alignment::Horizontal::Center)
    .width(Length::Fill)
    .into()
}

fn steps<'a>(card: &SetupView<'a>, palette: &Palette) -> Element<'a, Message> {
    let lang = card.lang;
    Row::new()
        .push(tile(
            lang.text(Msg::HermesSetupStepInbox),
            palette.accent,
            lang.text(Msg::HermesSetupStepInboxText),
            folder_choice(card, FolderSlot::Inbox, palette),
            palette,
        ))
        .push(tile(
            lang.text(Msg::HermesSetupStepLocation),
            palette.success,
            lang.text(Msg::HermesSetupStepLocationText),
            folder_choice(card, FolderSlot::FilingLocation, palette),
            palette,
        ))
        .push(tile(
            lang.text(Msg::HermesSetupStepProtection),
            palette.text,
            on_this_computer(
                lang,
                Msg::HermesSetupStepProtectionTextMac,
                Msg::HermesSetupStepProtectionTextOther,
            ),
            protection(card, palette),
            palette,
        ))
        .spacing(TILE_GAP)
        .into()
}

fn tile<'a>(
    heading: &'a str,
    heading_color: Color,
    explanation: &'a str,
    choice: Element<'a, Message>,
    palette: &Palette,
) -> Element<'a, Message> {
    container(
        column![
            text(heading).size(font_size::ROW).color(heading_color),
            dim(explanation, palette),
            Space::new().height(Length::Fill),
            choice,
        ]
        .spacing(spacing::L)
        .height(Length::Fill),
    )
    .padding(TILE_PADDING)
    .width(Length::FillPortion(1))
    .height(Length::Fixed(TILE_HEIGHT))
    .style(theme::outlined(palette))
    .into()
}

/// Tall enough for the explanation of the narrowest card and the choice below.
const TILE_HEIGHT: f32 = 224.0;

/// Around an element the keys can reach: a ring in the accent color while it
/// has the focus, and the same room when it has not.
fn focus_ring<'a>(
    content: Element<'a, Message>,
    focused: bool,
    palette: &Palette,
) -> Element<'a, Message> {
    let ring = if focused {
        palette.accent
    } else {
        Color::TRANSPARENT
    };
    container(content)
        .padding(FOCUS_RING_PADDING)
        .style(theme::outlined_in(ring))
        .into()
}

fn folder_choice<'a>(
    card: &SetupView<'a>,
    slot: FolderSlot,
    palette: &Palette,
) -> Element<'a, Message> {
    let lang = card.lang;
    let chars = frame::content_chars(SIZE, card.window, 0.0)
        .div_euclid(3)
        .saturating_sub(frame::chars_that_fit(TILE_RESERVED));
    let field: Element<'a, Message> = match card.setup.folder(slot) {
        Some(path) => container(
            text(format::ellipsize_middle(
                &shown_path(path, card.home),
                chars,
            ))
            .size(font_size::BODY)
            .color(palette.text)
            .wrapping(Wrapping::None),
        )
        .padding(FIELD_PADDING)
        .width(Length::Fill)
        .style(theme::outlined(palette))
        .into(),
        None => container(
            text(lang.text(Msg::HermesSetupNotChosen))
                .size(font_size::BODY)
                .color(palette.dim_text)
                .wrapping(Wrapping::None),
        )
        .padding(FIELD_PADDING)
        .width(Length::Fill)
        .style(theme::outlined_in(palette.border_inactive))
        .into(),
    };
    let element = match slot {
        FolderSlot::Inbox => SetupFocus::ChooseInbox,
        FolderSlot::FilingLocation => SetupFocus::ChooseFilingLocation,
    };
    let choose = kit::button(
        lang.text(Msg::HermesSetupChooseFolder),
        (!card.busy).then_some(setup_message(HermesSetupMessage::PickFolder(slot))),
        Tone::filled_if(card.focus == Some(element)),
        palette,
    );
    column![field, choose].spacing(spacing::M).into()
}

fn protection<'a>(card: &SetupView<'a>, palette: &Palette) -> Element<'a, Message> {
    let lang = card.lang;
    if let Some(existing) = card.setup.offer().existing_index {
        let sentence = match existing {
            ExistingIndex::Encrypted => Msg::HermesSetupIndexExistsEncrypted,
            ExistingIndex::Plain => Msg::HermesSetupIndexExistsPlain,
            ExistingIndex::Unknown => Msg::HermesSetupIndexExists,
        };
        return paragraph(lang.text(sentence), palette);
    }
    let encrypted = card.setup.encrypted();
    let option = |label: Msg, value: bool, element: SetupFocus| {
        let chosen = encrypted == value;
        let choice = radio(lang.text(label), value, Some(encrypted), |picked| {
            setup_message(HermesSetupMessage::ChooseEncrypted(picked))
        })
        .size(14)
        .text_size(font_size::BODY)
        .style(radio_look(palette));
        let outline = if chosen {
            palette.success
        } else {
            palette.border_inactive
        };
        let row = container(choice)
            .padding(OPTION_PADDING)
            .width(Length::Fill)
            .style(theme::outlined_in(outline));
        focus_ring(row.into(), card.focus == Some(element), palette)
    };
    column![
        option(
            Msg::HermesSetupProtectEncrypted,
            true,
            SetupFocus::Encrypted
        ),
        option(Msg::HermesSetupProtectPlain, false, SetupFocus::Plain),
    ]
    .spacing(spacing::XS)
    .into()
}

fn model_offer<'a>(card: &SetupView<'a>, palette: &Palette) -> Element<'a, Message> {
    let lang = card.lang;
    let toggle = if card.busy {
        None
    } else {
        Some(|_| setup_message(HermesSetupMessage::ToggleLoadModel))
    };
    let load_now = Row::new()
        .push(
            checkbox(card.setup.load_model())
                .on_toggle_maybe(toggle)
                .style(theme::dialog_checkbox(palette)),
        )
        .push(
            text(lang.text(Msg::HermesSetupLoadModelNow))
                .size(font_size::BODY)
                .color(palette.text),
        )
        .spacing(spacing::M)
        .align_y(alignment::Vertical::Center);
    container(
        Row::new()
            .push(kit::icon(Icon::CircleHelp, palette.accent))
            .push(
                container(dim(
                    fill_template(
                        lang.text(Msg::HermesSetupOptionalModel),
                        &[("{size}", &model_size_label())],
                    ),
                    palette,
                ))
                .width(Length::Fill),
            )
            .push(focus_ring(
                load_now.into(),
                card.focus == Some(SetupFocus::LoadModel),
                palette,
            ))
            .spacing(spacing::XL)
            .align_y(alignment::Vertical::Center),
    )
    .padding([spacing::L, spacing::XL])
    .width(Length::Fill)
    .style(theme::outlined(palette))
    .into()
}

/// Why the folders chosen do not go together, or what went wrong with the
/// last pick or the write.
fn problem_line<'a>(card: &SetupView<'a>, palette: &Palette) -> Option<Element<'a, Message>> {
    let setup = card.setup;
    if let Some((problem, list)) = setup.problem() {
        return Some(error_line(problem_text(problem, &list, card), palette));
    }
    let note = setup.note()?;
    let line = note_text(note, &setup.locations(), card);
    Some(match note {
        SetupNote::AlreadyAdded { .. } => paragraph(line, palette),
        SetupNote::FolderUnusable { .. }
        | SetupNote::NoConfigPath
        | SetupNote::SaveFailed(_)
        | SetupNote::GrantFailed { .. } => error_line(line, palette),
    })
}

fn problem_text(problem: SetupProblem, list: &[FilingLocation], card: &SetupView<'_>) -> String {
    super::filing_setup::problem_sentence(problem, list, card.lang, card.home)
}

fn note_text(note: &SetupNote, list: &[FilingLocation], card: &SetupView<'_>) -> String {
    let lang = card.lang;
    let shown = |path: &Path| shown_path(path, card.home);
    match note {
        SetupNote::AlreadyAdded { folder } => fill_template(
            lang.text(Msg::FilingSetupAlreadyAdded),
            &[("{path}", &shown(folder))],
        ),
        SetupNote::NoConfigPath => lang.text(Msg::FilingSetupNoConfigPath).to_owned(),
        SetupNote::FolderUnusable { folder, reason } => fill_template(
            lang.text(Msg::FilingSetupFolderUnusable),
            &[("{path}", &shown(folder)), ("{reason}", reason)],
        ),
        SetupNote::SaveFailed(error) => {
            super::filing_setup::write_failure_sentence(error, list, lang, card.home)
        }
        SetupNote::GrantFailed { folder, reason } => fill_template(
            lang.text(Msg::FilingSetupGrantFailed),
            &[("{path}", &shown(folder)), ("{reason}", reason)],
        ),
    }
}
