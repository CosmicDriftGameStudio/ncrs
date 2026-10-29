//! The overview page of Hermes: what it does, whether it runs, and the
//! settings a user without technical knowledge needs. Pure view over
//! [`OverviewFacts`] and [`OverviewState`]; the app gathers both.

use std::path::Path;

use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, text, Row, Space};
use iced::{alignment, Color, Element, Length, Size};
use ncrs_pipeline::{Load, RunnerState};

use super::frame::{DialogSize, Frame, KeyHints, PageHint};
use super::hermes::{self, Badges};
use super::hermes_flow::{self, FlowBox, FlowBoxes};
use super::hermes_setup::{model_size_label, on_this_computer};
use super::kit::{self, dim, error_line, paragraph, Check, Tone};
use super::theme::{self, font_size, spacing};
use crate::app::fill_template;
use crate::filing_setup::shown_path;
use crate::hermes_overview::{
    built_in_pattern, AiTile, ModuleState, OverviewFacts, OverviewState, PatternNote, Pill,
    ReviewTile, Tone as TileTone,
};
use crate::i18n::{Language, Msg};
use crate::messages::{FilingStatusMessage, HermesOverviewMessage, HermesPage, Message};
use crate::palette::Palette;

const SIZE: DialogSize = DialogSize::Card;
const HEADLINE_SIZE: f32 = 24.0;
const INTRO_WIDTH: f32 = 720.0;
const TILE_PADDING: f32 = 16.0;
const TILE_GAP: f32 = 16.0;
pub const PATTERN_FIELD_ID: &str = "hermes-overview-pattern";

/// The keys of the page, named in the footer where there is room.
const PAGE_HINTS: &[PageHint] = &[
    PageHint {
        key: "E",
        verb: Msg::HermesOverviewHintPattern,
    },
    PageHint {
        key: "P",
        verb: Msg::HermesOverviewHintPause,
    },
    PageHint {
        key: "1 2 3",
        verb: Msg::HermesOverviewHintLoad,
    },
    PageHint {
        key: "+ -",
        verb: Msg::HermesOverviewHintWorkers,
    },
];
const PATTERN_CONTROL_WIDTH: f32 = 420.0;
const LOAD_CONTROL_WIDTH: f32 = 380.0;
const ROLE_WIDTH: f32 = 140.0;

/// Where the folders are, by their role.
#[derive(Debug, Clone, Copy, Default)]
pub struct Folders<'a> {
    pub inbox: Option<&'a Path>,
    pub filing_location: Option<&'a Path>,
    pub archive: Option<&'a Path>,
}

/// What the page shows besides the facts.
pub struct OverviewView<'a> {
    pub facts: OverviewFacts,
    pub state: &'a OverviewState,
    pub folders: Folders<'a>,
    pub home: &'a Path,
    /// The load in the config.
    pub load: Option<Load>,
    /// The pipeline's state and worker target, while one runs.
    pub pipeline: Option<(RunnerState, usize)>,
    pub flow_step: u32,
    pub badges: Badges,
    pub window: Size,
    pub lang: Language,
}

fn send(message: HermesOverviewMessage) -> Message {
    Message::HermesOverview(message)
}

fn tone_color(tone: TileTone, palette: &Palette) -> Color {
    match tone {
        TileTone::Good => palette.success,
        TileTone::Needs => palette.accent,
        TileTone::Quiet => palette.border_inactive,
    }
}

pub fn view<'a>(page: &OverviewView<'a>, palette: &Palette) -> Element<'a, Message> {
    let lang = page.lang;
    let module = page.facts.module;
    let mut body = column![header(page, palette)].spacing(spacing::XL);
    if let Some(banner) = banner(page, palette) {
        body = body.push(banner);
    }
    if !matches!(module, ModuleState::Off | ModuleState::Dismissed) {
        body = body
            .push(hermes_flow::flow(page.flow_step, flow_boxes(page), palette))
            .push(tiles(page, palette));
        if let Some(read_again) = read_again_button(page, palette) {
            body = body.push(read_again);
        }
        body = body
            .push(folders_section(page, palette))
            .push(filing_section(page, palette))
            .push(ai_section(page, palette))
            .push(load_section(page, palette))
            .push(protection_section(page, palette));
    }
    Frame::new(
        lang.text(Msg::HermesNavOverview),
        body,
        SIZE,
        page.window,
        lang,
        palette,
    )
    .title_theme(lang.text(Msg::HermesNavOverviewTheme))
    .sidebar(hermes::nav(
        HermesPage::Overview,
        page.badges,
        lang,
        palette,
    ))
    .on_close(send(HermesOverviewMessage::Close))
    .hints(KeyHints::default())
    .page_hints(PAGE_HINTS)
    .into_element()
}

fn pill_text(pill: Pill) -> (Msg, TileTone) {
    match pill {
        Pill::Running => (Msg::HermesOverviewPillRunning, TileTone::Good),
        Pill::NeedsYou => (Msg::HermesOverviewPillNeedsYou, TileTone::Needs),
        Pill::Off => (Msg::HermesOverviewPillOff, TileTone::Quiet),
        Pill::Starting => (Msg::HermesOverviewPillStarting, TileTone::Quiet),
    }
}

fn header<'a>(page: &OverviewView<'a>, palette: &Palette) -> Element<'a, Message> {
    let lang = page.lang;
    let (label, tone) = pill_text(page.facts.pill());
    let color = match tone {
        TileTone::Quiet => palette.dim_text,
        other => tone_color(other, palette),
    };
    let pill = container(text(lang.text(label)).size(font_size::BODY).color(color))
        .padding([spacing::XS, spacing::L])
        .style(theme::outlined_in(color));
    let intro = container(
        text(on_this_computer(
            lang,
            Msg::HermesOverviewIntroMac,
            Msg::HermesOverviewIntroOther,
        ))
        .size(font_size::ROW)
        .color(palette.dim_text)
        .wrapping(Wrapping::Word)
        .align_x(alignment::Horizontal::Center),
    )
    .max_width(INTRO_WIDTH);
    column![
        pill,
        text(lang.text(Msg::HermesOverviewTitle))
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

/// The sentence and the button for a module that does not run: what is wrong
/// and the one step that helps.
fn banner<'a>(page: &OverviewView<'a>, palette: &Palette) -> Option<Element<'a, Message>> {
    let lang = page.lang;
    let facts = &page.facts;
    let (sentence, button) = match facts.module {
        ModuleState::Off => (
            lang.text(Msg::HermesOverviewOffNote).to_owned(),
            Some((
                Msg::HermesOverviewSwitchOn,
                HermesOverviewMessage::SwitchHermesOn,
            )),
        ),
        ModuleState::Dismissed => (
            lang.text(Msg::HermesOverviewDismissedNote).to_owned(),
            Some((
                Msg::HermesOverviewContinueSetup,
                HermesOverviewMessage::ContinueSetup,
            )),
        ),
        ModuleState::Failed => (
            fill_template(
                lang.text(Msg::HermesOverviewFailedNote),
                &[("{reason}", facts.failure.as_deref().unwrap_or_default())],
            ),
            Some((Msg::HermesOverviewTryAgain, HermesOverviewMessage::TryAgain)),
        ),
        ModuleState::WaitsForYou => (lang.text(Msg::HermesOverviewWaitsNote).to_owned(), None),
        ModuleState::KeyMissing | ModuleState::Encrypting | ModuleState::StartingOver => {
            return None
        }
        ModuleState::Ready | ModuleState::Starting if !facts.has_locations => (
            lang.text(Msg::HermesOverviewNoFoldersNote).to_owned(),
            Some((
                Msg::HermesOverviewTileFoldersButton,
                HermesOverviewMessage::ChangeFolders,
            )),
        ),
        ModuleState::Ready | ModuleState::Starting => return None,
    };
    let line = if facts.module == ModuleState::Failed {
        error_line(sentence, palette)
    } else {
        paragraph(sentence, palette)
    };
    let mut content = Row::new()
        .push(container(line).width(Length::Fill))
        .spacing(spacing::XL)
        .align_y(alignment::Vertical::Center);
    if let Some((label, message)) = button {
        content = content.push(kit::button(
            lang.text(label),
            Some(send(message)),
            Tone::Primary,
            palette,
        ));
    }
    Some(
        container(content)
            .padding([spacing::L, spacing::XL])
            .width(Length::Fill)
            .style(theme::outlined_in(palette.accent))
            .into(),
    )
}

fn count_text(template: Msg, count: u64, lang: Language) -> String {
    fill_template(lang.text(template), &[("{count}", &count.to_string())])
}

fn flow_boxes<'a>(page: &OverviewView<'a>) -> FlowBoxes<'a> {
    let lang = page.lang;
    let facts = &page.facts;
    let counts = facts.counts.unwrap_or_default();
    let inbox_name = page
        .folders
        .inbox
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let inbox_note = fill_template(
        lang.text(Msg::HermesOverviewFlowInboxNote),
        &[
            ("{name}", &inbox_name),
            ("{count}", &counts.inbox.new.to_string()),
        ],
    );
    let archive_note = if page.folders.archive.is_some() {
        count_text(
            Msg::HermesOverviewFlowArchiveNote,
            counts.archive_documents,
            lang,
        )
    } else {
        lang.text(Msg::HermesOverviewFlowArchiveNone).to_owned()
    };
    let reading_note = match facts.ai_tile() {
        tile if tile.reads() => Msg::HermesOverviewFlowReadAi,
        AiTile::Off => Msg::HermesOverviewFlowReadNoAi,
        _ => Msg::HermesOverviewFlowReadModelMissing,
    };
    let rules_note = if counts.confirmed_rules == 1 {
        lang.text(Msg::HermesOverviewFlowRulesNoteOne).to_owned()
    } else {
        count_text(
            Msg::HermesOverviewFlowRulesNote,
            counts.confirmed_rules,
            lang,
        )
    };
    let pattern = page.state.pattern_draft();
    let pattern_text = if pattern == built_in_pattern() {
        lang.text(Msg::HermesOverviewFlowFiledDefault)
    } else {
        pattern
    };
    let filed_note = fill_template(
        lang.text(Msg::HermesOverviewFlowFiledNote),
        &[
            (
                "{path}",
                &page
                    .folders
                    .filing_location
                    .map(|path| shown_path(path, page.home))
                    .unwrap_or_default(),
            ),
            ("{pattern}", pattern_text),
        ],
    );
    let titled = |title: Msg, note: String| FlowBox {
        title: lang.text(title).into(),
        note: Some(note.into()),
    };
    FlowBoxes {
        inbox: titled(Msg::HermesSetupFlowInbox, inbox_note),
        archive: titled(Msg::HermesSetupFlowArchive, archive_note),
        reading: titled(Msg::HermesSetupFlowRead, lang.text(reading_note).to_owned()),
        rules: titled(Msg::HermesSetupFlowRules, rules_note),
        filed: titled(Msg::HermesSetupFlowFiled, filed_note),
    }
}

fn tile<'a>(
    title: &str,
    tone: TileTone,
    lines: Vec<Element<'a, Message>>,
    palette: &Palette,
) -> Element<'a, Message> {
    let color = tone_color(tone, palette);
    container(
        column![
            text(title.to_owned()).size(font_size::ROW).color(color),
            column(lines).spacing(spacing::M),
        ]
        .spacing(spacing::M),
    )
    .padding(TILE_PADDING)
    .width(Length::FillPortion(1))
    .style(theme::outlined_in(color))
    .into()
}

fn tiles<'a>(page: &OverviewView<'a>, palette: &Palette) -> Element<'a, Message> {
    Row::new()
        .push(folders_tile(page, palette))
        .push(ocr_tile(page, palette))
        .push(ai_tile(page, palette))
        .push(review_tile(page, palette))
        .spacing(TILE_GAP)
        .height(Length::Shrink)
        .into()
}

fn folders_tile<'a>(page: &OverviewView<'a>, palette: &Palette) -> Element<'a, Message> {
    let lang = page.lang;
    let tile_state = page.facts.folder_tile();
    let sentence = match tile_state {
        crate::hermes_overview::FolderTile::Ready => Msg::HermesOverviewTileFoldersOk,
        crate::hermes_overview::FolderTile::Missing => Msg::HermesOverviewTileFoldersMissing,
    };
    let mut lines = vec![dim(lang.text(sentence), palette)];
    if tile_state.tone() == TileTone::Needs {
        lines.push(kit::button(
            lang.text(Msg::HermesOverviewTileFoldersButton),
            Some(send(HermesOverviewMessage::ChangeFolders)),
            Tone::Secondary,
            palette,
        ));
    }
    tile(
        lang.text(Msg::HermesOverviewTileFoldersTitle),
        tile_state.tone(),
        lines,
        palette,
    )
}

fn ocr_tile<'a>(page: &OverviewView<'a>, palette: &Palette) -> Element<'a, Message> {
    let lang = page.lang;
    let tile_state = page.facts.ocr_tile();
    let (sentence, button) = match tile_state {
        crate::hermes_overview::OcrTile::Ready => (Msg::HermesOverviewTileOcrReady, false),
        crate::hermes_overview::OcrTile::Missing => (Msg::HermesOverviewTileOcrMissing, true),
    };
    let mut lines = vec![dim(lang.text(sentence), palette)];
    if button {
        lines.push(kit::button(
            lang.text(Msg::HermesOverviewTileOcrButton),
            Some(send(HermesOverviewMessage::LoadOcrModels)),
            Tone::Secondary,
            palette,
        ));
    }
    tile(
        lang.text(Msg::HermesOverviewTileOcrTitle),
        tile_state.tone(),
        lines,
        palette,
    )
}

fn ai_tile<'a>(page: &OverviewView<'a>, palette: &Palette) -> Element<'a, Message> {
    let lang = page.lang;
    let state = page.facts.ai_tile();
    let lines: Vec<Element<'a, Message>> = match &state {
        AiTile::Off => vec![dim(lang.text(Msg::HermesOverviewTileAiOff), palette)],
        AiTile::NotLoaded => vec![
            dim(lang.text(Msg::HermesOverviewTileAiNotLoaded), palette),
            kit::button(
                &fill_template(
                    lang.text(Msg::HermesOverviewTileAiLoad),
                    &[("{size}", &model_size_label())],
                ),
                Some(send(HermesOverviewMessage::LoadAiModel)),
                Tone::Secondary,
                palette,
            ),
        ],
        AiTile::Downloading { received, total } => {
            let fraction = if *total == 0 {
                0.0
            } else {
                *received as f32 / *total as f32
            };
            vec![
                kit::progress_bar(fraction, palette),
                dim(download_line(*received, *total, lang), palette),
            ]
        }
        AiTile::Ready => vec![dim(lang.text(Msg::HermesOverviewTileAiReady), palette)],
        AiTile::OwnServer { model } => vec![dim(
            fill_template(
                lang.text(Msg::HermesOverviewTileAiOwnServer),
                &[("{model}", model)],
            ),
            palette,
        )],
        AiTile::TakesEffectAtRestart => {
            vec![dim(lang.text(Msg::HermesOverviewTileAiRestart), palette)]
        }
        AiTile::Failed(reason) => vec![dim(
            fill_template(
                lang.text(Msg::HermesOverviewTileAiFailed),
                &[("{reason}", reason)],
            ),
            palette,
        )],
    };
    tile(
        lang.text(Msg::HermesOverviewTileAiTitle),
        state.tone(),
        lines,
        palette,
    )
}

fn review_tile<'a>(page: &OverviewView<'a>, palette: &Palette) -> Element<'a, Message> {
    let lang = page.lang;
    let state = page.facts.review_tile();
    let (title, text_msg, button) = match state {
        ReviewTile::Waiting(1) => (
            lang.text(Msg::HermesOverviewTileReviewWaitingOne)
                .to_owned(),
            Msg::HermesOverviewTileReviewText,
            true,
        ),
        ReviewTile::Waiting(count) => (
            count_text(Msg::HermesOverviewTileReviewWaiting, count, lang),
            Msg::HermesOverviewTileReviewText,
            true,
        ),
        ReviewTile::Clear => (
            lang.text(Msg::HermesOverviewTileReviewClear).to_owned(),
            Msg::HermesOverviewTileReviewClearText,
            false,
        ),
    };
    let mut lines = vec![dim(lang.text(text_msg), palette)];
    if button {
        lines.push(kit::button(
            lang.text(Msg::HermesOverviewTileReviewButton),
            Some(Message::HermesPage(HermesPage::Review)),
            Tone::Secondary,
            palette,
        ));
    }
    tile(&title, state.tone(), lines, palette)
}

fn read_again_button<'a>(
    page: &OverviewView<'a>,
    palette: &Palette,
) -> Option<Element<'a, Message>> {
    if !page.facts.can_read_reviews_again() {
        return None;
    }
    let lang = page.lang;
    let count = page.facts.open_reviews();
    let label = if count == 1 {
        lang.text(Msg::HermesOverviewReadAgainOne).to_owned()
    } else {
        count_text(Msg::HermesOverviewReadAgain, count, lang)
    };
    Some(
        row![kit::button(
            &label,
            Some(send(HermesOverviewMessage::ReadReviewsAgainWithAi)),
            Tone::Secondary,
            palette,
        )]
        .into(),
    )
}

fn role_line<'a>(
    role: Msg,
    path: Option<&Path>,
    none: Msg,
    page: &OverviewView<'a>,
    palette: &Palette,
) -> Element<'a, Message> {
    let lang = page.lang;
    let value: Element<'a, Message> = match path {
        Some(folder) => text(shown_path(folder, page.home))
            .size(font_size::BODY)
            .font(iced::Font::MONOSPACE)
            .color(palette.text)
            .wrapping(Wrapping::None)
            .into(),
        None => dim(lang.text(none), palette),
    };
    row![
        container(dim(lang.text(role), palette)).width(ROLE_WIDTH),
        container(value).width(Length::Fill),
    ]
    .spacing(spacing::M)
    .into()
}

fn folders_section<'a>(page: &OverviewView<'a>, palette: &Palette) -> Element<'a, Message> {
    let lang = page.lang;
    let folders = page.folders;
    let lines = kit::stack([
        role_line(
            Msg::HermesOverviewRoleInbox,
            folders.inbox,
            Msg::HermesSetupNotChosen,
            page,
            palette,
        ),
        role_line(
            Msg::HermesOverviewRoleFilingLocation,
            folders.filing_location,
            Msg::HermesSetupNotChosen,
            page,
            palette,
        ),
        role_line(
            Msg::HermesOverviewRoleArchive,
            folders.archive,
            Msg::HermesOverviewArchiveNone,
            page,
            palette,
        ),
    ]);
    let content = row![
        container(lines).width(Length::Fill),
        kit::button(
            lang.text(Msg::HermesOverviewFoldersChange),
            Some(send(HermesOverviewMessage::ChangeFolders)),
            Tone::Secondary,
            palette,
        ),
    ]
    .spacing(spacing::XL)
    .align_y(alignment::Vertical::Center);
    kit::section(
        lang.text(Msg::HermesOverviewSectionFolders),
        content,
        palette,
    )
}

fn filing_section<'a>(page: &OverviewView<'a>, palette: &Palette) -> Element<'a, Message> {
    let lang = page.lang;
    let state = page.state;
    // The field exists only while it is being edited: a click into a text
    // field cannot be told to the page, and the page's letters must stay off
    // while the field has the focus.
    let field = if state.is_editing() {
        kit::mono_text_field(
            built_in_pattern(),
            state.pattern_draft(),
            PATTERN_FIELD_ID,
            Some(|typed| send(HermesOverviewMessage::PatternEdited(typed))),
            send(HermesOverviewMessage::PatternSubmit),
            palette,
        )
    } else {
        kit::button(
            state.pattern_draft(),
            Some(send(HermesOverviewMessage::FocusPattern)),
            Tone::Secondary,
            palette,
        )
    };
    let problem = state.pattern_note().map(|note| match note {
        PatternNote::Invalid => lang.text(Msg::HermesOverviewPatternInvalid).to_owned(),
        PatternNote::WriteFailed(reason) => fill_template(
            lang.text(Msg::HermesOverviewPatternWriteFailed),
            &[("{reason}", reason)],
        ),
    });
    let mut content = column![kit::form_row_with_problem(
        lang.text(Msg::HermesOverviewPatternLabel),
        None,
        Some(lang.text(Msg::HermesOverviewPatternNote)),
        problem.as_deref(),
        field,
        PATTERN_CONTROL_WIDTH,
        palette,
    )]
    .spacing(spacing::S);
    let example = page
        .folders
        .filing_location
        .and_then(|location| state.example_target(location));
    if let Some(target) = example {
        content = content.push(dim(
            fill_template(
                lang.text(Msg::HermesOverviewPatternExample),
                &[("{path}", &shown_path(&target, page.home))],
            ),
            palette,
        ));
    }
    kit::section(
        lang.text(Msg::HermesOverviewSectionFiling),
        content,
        palette,
    )
}

fn ai_section<'a>(page: &OverviewView<'a>, palette: &Palette) -> Element<'a, Message> {
    let lang = page.lang;
    let on = page.facts.ai_on;
    let word = lang.text(if on {
        Msg::SettingsOn
    } else {
        Msg::SettingsOff
    });
    let switch = row![
        kit::checkbox(
            if on { Check::On } else { Check::Off },
            Some(send(HermesOverviewMessage::SetAiEnabled(!on))),
            palette,
        ),
        dim(word, palette),
    ]
    .spacing(spacing::S)
    .align_y(alignment::Vertical::Center);
    let mut content = column![kit::form_row(
        lang.text(Msg::HermesOverviewAiSwitch),
        None,
        Some(lang.text(Msg::HermesOverviewAiSwitchNote)),
        switch.into(),
        palette,
    )]
    .spacing(spacing::S);
    let state = page.facts.ai_tile();
    if on && !matches!(state, AiTile::Off) {
        content = content.push(dim(ai_sentence(&state, lang), palette));
    }
    content = content.push(row![kit::button(
        lang.text(Msg::HermesOverviewOwnServer),
        Some(send(HermesOverviewMessage::ShowOwnServerSettings)),
        Tone::Secondary,
        palette,
    )]);
    kit::section(lang.text(Msg::HermesOverviewSectionAi), content, palette)
}

fn download_line(received: u64, total: u64, lang: Language) -> String {
    let (done, all, percent) = crate::ui::format::download_figures(received, total);
    fill_template(
        lang.text(Msg::HermesOverviewTileAiDownloading),
        &[
            ("{received}", done.as_str()),
            ("{total}", all.as_str()),
            ("{percent}", percent.as_str()),
        ],
    )
}

fn ai_sentence(state: &AiTile, lang: Language) -> String {
    match state {
        AiTile::Off => String::new(),
        AiTile::NotLoaded => lang.text(Msg::HermesOverviewTileAiNotLoaded).to_owned(),
        AiTile::Downloading { received, total } => download_line(*received, *total, lang),
        AiTile::Ready => lang.text(Msg::HermesOverviewTileAiReady).to_owned(),
        AiTile::OwnServer { model } => fill_template(
            lang.text(Msg::HermesOverviewTileAiOwnServer),
            &[("{model}", model)],
        ),
        AiTile::TakesEffectAtRestart => lang.text(Msg::HermesOverviewTileAiRestart).to_owned(),
        AiTile::Failed(reason) => fill_template(
            lang.text(Msg::HermesOverviewTileAiFailed),
            &[("{reason}", reason)],
        ),
    }
}

fn load_section<'a>(page: &OverviewView<'a>, palette: &Palette) -> Element<'a, Message> {
    let lang = page.lang;
    let loads = [
        (Msg::FilingStatusLoadGentle, Load::Gentle),
        (Msg::FilingStatusLoadBalanced, Load::Balanced),
        (Msg::FilingStatusLoadFull, Load::Full),
    ];
    let options: Vec<(&str, Message)> = loads
        .iter()
        .map(|(label, target)| {
            (
                lang.text(*label),
                send(HermesOverviewMessage::SetLoad(*target)),
            )
        })
        .collect();
    let selected = loads
        .iter()
        .position(|(_, target)| page.load == Some(*target))
        .unwrap_or(loads.len());
    let load_row = kit::form_row_wide(
        lang.text(Msg::FilingStatusLoadLabel),
        None,
        Some(lang.text(Msg::FilingStatusLoadExplain)),
        kit::segmented(&options, selected, true, palette),
        LOAD_CONTROL_WIDTH,
        palette,
    );
    let mut content = column![load_row].spacing(spacing::S);
    if let Some((state, workers)) = page.pipeline {
        content = content.push(pipeline_controls(state, workers, lang, palette));
    }
    kit::section(lang.text(Msg::HermesOverviewSectionLoad), content, palette)
}

fn pipeline_controls<'a>(
    state: RunnerState,
    workers: usize,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let running = state != RunnerState::Off;
    let pause_label = if state == RunnerState::Paused {
        Msg::FilingStatusResume
    } else {
        Msg::FilingStatusPause
    };
    let pause = kit::button(
        lang.text(pause_label),
        running.then(|| Message::FilingStatus(FilingStatusMessage::TogglePause)),
        Tone::Secondary,
        palette,
    );
    let step = |label: &str, delta: i8| {
        kit::button(
            label,
            running.then(|| Message::FilingStatus(FilingStatusMessage::StepWorkers(delta))),
            Tone::Secondary,
            palette,
        )
    };
    let workers_row = row![
        step("-", -1),
        paragraph(workers.to_string(), palette),
        step("+", 1)
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    column![
        kit::form_row(
            lang.text(Msg::FilingStatusStateLabel),
            None,
            None,
            pause,
            palette
        ),
        kit::form_row(
            lang.text(Msg::FilingStatusWorkersLabel),
            None,
            Some(lang.text(Msg::FilingStatusLoadNote)),
            workers_row.into(),
            palette,
        ),
    ]
    .spacing(spacing::S)
    .into()
}

/// The protection: its state, what it means, why the last change failed, and
/// the buttons that change it. While a change runs the buttons are inactive.
fn protection_section<'a>(page: &OverviewView<'a>, palette: &Palette) -> Element<'a, Message> {
    use crate::filing::{KeyProblem, ProtectionNote};
    use crate::hermes_overview::Protection;

    let lang = page.lang;
    let facts = &page.facts;
    let key_missing = facts.module == ModuleState::KeyMissing;
    let (state_word, sentence) = match facts.protection {
        Protection::Encrypted if key_missing => (
            Some(Msg::HermesOverviewProtectionEncrypted),
            match facts.key_problem {
                Some(KeyProblem::Mismatch) => lang.text(Msg::HermesOverviewKeyMismatch),
                _ => on_this_computer(
                    lang,
                    Msg::HermesOverviewKeyMissingMac,
                    Msg::HermesOverviewKeyMissingOther,
                ),
            },
        ),
        Protection::Encrypted => (
            Some(Msg::HermesOverviewProtectionEncrypted),
            on_this_computer(
                lang,
                Msg::HermesOverviewProtectionEncryptedMac,
                Msg::HermesOverviewProtectionEncryptedOther,
            ),
        ),
        Protection::Plain => (
            Some(Msg::HermesOverviewProtectionPlain),
            on_this_computer(
                lang,
                Msg::HermesOverviewProtectionPlainMac,
                Msg::HermesOverviewProtectionPlainOther,
            ),
        ),
        Protection::Encrypting => (
            Some(Msg::HermesOverviewProtectionEncrypting),
            lang.text(Msg::HermesOverviewProtectionWorking),
        ),
        Protection::StartingOver => (
            Some(Msg::HermesOverviewProtectionStartingOver),
            lang.text(Msg::HermesOverviewProtectionWorking),
        ),
        Protection::Unknown => (None, lang.text(Msg::HermesOverviewProtectionNone)),
    };
    let headline_color = match facts.protection {
        Protection::Encrypted if !key_missing => palette.success,
        _ => palette.accent,
    };
    let mut lines = column![].spacing(spacing::S);
    if let Some(headline) = state_word {
        lines = lines.push(
            text(lang.text(headline))
                .size(font_size::ROW)
                .color(headline_color),
        );
    }
    lines = lines.push(dim(sentence, palette));
    if let Some(note) = facts.protection_note.as_ref() {
        let (template, reason) = match note {
            ProtectionNote::EncryptFailed(reason) => (Msg::HermesOverviewEncryptFailed, reason),
            ProtectionNote::StartOverFailed(reason) => (Msg::HermesOverviewStartOverFailed, reason),
            ProtectionNote::KeyEntryNotRemoved(reason) => (Msg::HermesOverviewKeyEntryKept, reason),
        };
        lines = lines.push(error_line(
            fill_template(lang.text(template), &[("{reason}", reason)]),
            palette,
        ));
    }
    let button = |label: Msg, message: HermesOverviewMessage, enabled: bool, tone: Tone| {
        kit::button(
            lang.text(label),
            enabled.then(|| send(message)),
            tone,
            palette,
        )
    };
    let mut buttons = Row::new().spacing(spacing::M);
    if key_missing {
        buttons = buttons
            .push(button(
                Msg::HermesOverviewEnterRecoveryKey,
                HermesOverviewMessage::EnterRecoveryKey,
                facts.can_start_over,
                Tone::Primary,
            ))
            .push(button(
                Msg::HermesOverviewTryAgain,
                HermesOverviewMessage::TryAgain,
                facts.can_start_over,
                Tone::Secondary,
            ));
    } else if facts.can_encrypt || facts.protection == Protection::Encrypting {
        buttons = buttons.push(button(
            Msg::HermesOverviewEncryptNow,
            HermesOverviewMessage::EncryptNow,
            facts.can_encrypt,
            Tone::Secondary,
        ));
    }
    let changing = matches!(
        facts.protection,
        Protection::Encrypting | Protection::StartingOver
    );
    if facts.can_start_over || key_missing || changing {
        buttons = buttons.push(button(
            Msg::HermesOverviewStartOver,
            HermesOverviewMessage::AskStartOver,
            facts.can_start_over,
            Tone::Secondary,
        ));
    }
    let content = row![
        container(lines).width(Length::Fill),
        Space::new().width(spacing::M),
        buttons,
    ]
    .align_y(alignment::Vertical::Center);
    kit::section(
        lang.text(Msg::HermesOverviewSectionProtection),
        content,
        palette,
    )
}

/// Buttons: Cancel `0`, "Remove index" `1`.
pub const START_OVER_CANCEL_FOCUS: usize = 0;
pub const START_OVER_CONFIRM_FOCUS: usize = 1;

/// The question before the index and its key are removed. Opens on Cancel,
/// so Enter alone never removes anything.
pub fn start_over_dialog<'a>(
    focus: usize,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let body = column![
        paragraph(lang.text(Msg::HermesOverviewStartOverLoss), palette),
        paragraph(lang.text(Msg::HermesOverviewStartOverFiles), palette),
    ]
    .spacing(spacing::LINE_GAP);
    let cancel = send(HermesOverviewMessage::CancelStartOver);
    Frame::new(
        lang.text(Msg::HermesOverviewStartOverTitle),
        body,
        DialogSize::Medium,
        window,
        lang,
        palette,
    )
    .buttons(vec![
        kit::button(
            lang.text(Msg::DialogCancel),
            Some(cancel.clone()),
            Tone::filled_if(focus == START_OVER_CANCEL_FOCUS),
            palette,
        ),
        kit::button(
            lang.text(Msg::HermesOverviewStartOverConfirm),
            Some(send(HermesOverviewMessage::ConfirmStartOver)),
            Tone::Danger,
            palette,
        ),
    ])
    .on_close(cancel)
    .hints(KeyHints {
        enter: false,
        tab: true,
    })
    .into_element()
}
