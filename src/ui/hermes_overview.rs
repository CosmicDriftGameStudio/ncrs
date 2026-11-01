//! The overview page of Hermes, a tab: what it does, whether it runs, and the
//! settings a user without technical knowledge needs. Pure view over
//! [`OverviewFacts`] and [`OverviewState`]; the app gathers both.

use std::path::Path;

use iced::widget::column;
use iced::{Color, Element, Size};
use ncrs_pipeline::RunnerState;

use super::filing_pieces::{target_view, ExampleLine, TargetHost};
use super::frame::{DialogSize, Frame, KeyHints};
use super::hermes::{self, Badges};
use super::hermes_kit::{self as page_kit, MenuItem, MenuLook};
use super::hermes_setup::{model_size_label, on_this_computer};
use super::kit::{self, paragraph, Check, Tone};
use super::theme::spacing;
use crate::app::fill_template;
use crate::filing::{KeyProblem, ProtectionNote};
use crate::filing_setup::shown_path;
use crate::hermes_overview::{
    AiTile, ModuleState, OcrTile, OverviewFacts, OverviewState, PatternNote, Pill, Protection,
    Tone as TileTone,
};
use crate::i18n::{FuturamaName, Language, Msg};
use crate::messages::{FilingStatusMessage, HermesOverviewMessage, HermesPage, Message};
use crate::palette::Palette;

pub const FOLDER_NAME_FIELD_ID: &str = "hermes-overview-folder-name";

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
    /// The state of the pipeline, and how many files it works on.
    pub runner: RunnerState,
    pub in_progress: usize,
    pub badges: Badges,
    pub lang: Language,
}

fn send(message: HermesOverviewMessage) -> Message {
    Message::HermesOverview(message)
}

fn tone_color(tone: TileTone, palette: &Palette) -> Color {
    match tone {
        TileTone::Good => palette.success,
        TileTone::Needs => palette.accent,
        TileTone::Quiet => palette.dim_text,
    }
}

/// The page with its navigation, header, content and action bar.
pub fn view<'a>(page: &OverviewView<'a>, palette: &Palette) -> Element<'a, Message> {
    let lang = page.lang;
    let module = page.facts.module;
    let active = !matches!(module, ModuleState::Off | ModuleState::Dismissed);
    let bar = active.then(|| action_bar(page, palette));
    page_kit::tab_page(
        hermes::tab_nav(HermesPage::Overview, page.badges, lang, palette),
        header(page, palette),
        content(page, palette),
        bar,
        palette,
    )
}

fn pill_text(pill: Pill) -> (Msg, TileTone) {
    match pill {
        Pill::Running => (Msg::HermesOverviewPillRunning, TileTone::Good),
        Pill::NeedsYou => (Msg::HermesOverviewPillNeedsYou, TileTone::Needs),
        Pill::Off => (Msg::HermesOverviewPillOff, TileTone::Quiet),
        Pill::Starting => (Msg::HermesOverviewPillStarting, TileTone::Quiet),
    }
}

/// The word behind the title: running in green, paused dimmed, a failure red.
fn state_word<'a>(page: &OverviewView<'a>, palette: &Palette) -> Element<'a, Message> {
    let lang = page.lang;
    let (label, tone) = pill_text(page.facts.pill());
    let paused = page.runner == RunnerState::Paused && tone == TileTone::Good;
    let (shown, color) = if paused {
        (Msg::FilingStatusStatePaused, palette.dim_text)
    } else if matches!(
        page.facts.module,
        ModuleState::Failed | ModuleState::KeyMissing
    ) {
        (label, palette.error)
    } else {
        (label, tone_color(tone, palette))
    };
    page_kit::value_in(lang.text(shown), color)
}

fn header<'a>(page: &OverviewView<'a>, palette: &Palette) -> Element<'a, Message> {
    let lang = page.lang;
    let more = page_kit::menu_button(
        "",
        &[
            MenuItem::new(
                lang.text(Msg::HermesOverviewOwnServer),
                send(HermesOverviewMessage::ShowOwnServerSettings),
            ),
            MenuItem::new(
                lang.text(Msg::HermesOverviewMenuProtection),
                send(HermesOverviewMessage::ShowProtectionSettings),
            ),
        ],
        MenuLook::Icon,
        palette,
    );
    page_kit::page_header(
        lang.text(Msg::HermesNavOverview),
        lang.futurama_name(FuturamaName::HermesNavOverview),
        Some(
            on_this_computer(
                lang,
                Msg::HermesOverviewIntroMac,
                Msg::HermesOverviewIntroOther,
            )
            .to_owned(),
        ),
        Some(state_word(page, palette)),
        vec![more],
        palette,
    )
}

fn count_text(template: Msg, count: u64, lang: Language) -> String {
    fill_template(lang.text(template), &[("{count}", &count.to_string())])
}

fn waiting_text(count: u64, lang: Language) -> String {
    match count {
        0 => lang.text(Msg::HermesOverviewTileReviewClear).to_owned(),
        1 => lang
            .text(Msg::HermesOverviewTileReviewWaitingOne)
            .to_owned(),
        many => count_text(Msg::HermesOverviewTileReviewWaiting, many, lang),
    }
}

fn action_bar<'a>(page: &OverviewView<'a>, palette: &Palette) -> Element<'a, Message> {
    let lang = page.lang;
    let waiting = page.facts.open_reviews();
    page_kit::action_bar(
        Some(waiting_text(waiting, lang)),
        vec![page_kit::primary(
            lang.text(Msg::HermesOverviewCheckReviews),
            Some(kit::ENTER_KEY),
            (waiting > 0).then(|| send(HermesOverviewMessage::ShowReviews)),
            palette,
        )],
        palette,
    )
}

fn content<'a>(page: &OverviewView<'a>, palette: &Palette) -> Element<'a, Message> {
    let lang = page.lang;
    let facts = &page.facts;
    let (sentence, button) = match facts.module {
        ModuleState::Off => (
            Msg::HermesOverviewOffNote,
            Some((
                Msg::HermesOverviewSwitchOn,
                HermesOverviewMessage::SwitchHermesOn,
            )),
        ),
        ModuleState::Dismissed => (
            Msg::HermesOverviewDismissedNote,
            Some((
                Msg::HermesOverviewContinueSetup,
                HermesOverviewMessage::ContinueSetup,
            )),
        ),
        _ => return sections(page, palette),
    };
    page_kit::empty_state(
        lang.text(Msg::HermesOverviewTitle),
        lang.text(sentence),
        button.map(|(label, message)| {
            page_kit::primary(lang.text(label), None, Some(send(message)), palette)
        }),
        palette,
    )
}

/// What is wrong with the module and the one step that helps, above the rows.
fn problem<'a>(page: &OverviewView<'a>, palette: &Palette) -> Option<Element<'a, Message>> {
    let lang = page.lang;
    let facts = &page.facts;
    let (line, links): (
        Element<'a, Message>,
        Vec<(Msg, HermesOverviewMessage, bool)>,
    ) = match facts.module {
        ModuleState::Failed => (
            page_kit::error_line(
                fill_template(
                    lang.text(Msg::HermesOverviewFailedNote),
                    &[("{reason}", facts.failure.as_deref().unwrap_or_default())],
                ),
                palette,
            ),
            vec![(
                Msg::HermesOverviewTryAgain,
                HermesOverviewMessage::TryAgain,
                true,
            )],
        ),
        ModuleState::KeyMissing => (
            page_kit::error_line(key_sentence(facts, lang), palette),
            vec![
                (
                    Msg::HermesOverviewEnterRecoveryKey,
                    HermesOverviewMessage::EnterRecoveryKey,
                    facts.can_start_over,
                ),
                (
                    Msg::HermesOverviewTryAgain,
                    HermesOverviewMessage::TryAgain,
                    facts.can_start_over,
                ),
            ],
        ),
        ModuleState::WaitsForYou => (
            page_kit::hint(lang.text(Msg::HermesOverviewWaitsNote), palette),
            Vec::new(),
        ),
        ModuleState::Ready | ModuleState::Starting if !facts.has_locations => (
            page_kit::hint(lang.text(Msg::HermesOverviewNoFoldersNote), palette),
            vec![(
                Msg::HermesOverviewTileFoldersButton,
                HermesOverviewMessage::ChangeFolders,
                true,
            )],
        ),
        _ => return None,
    };
    let mut cell = vec![line];
    cell.extend(links.into_iter().map(|(label, message, enabled)| {
        page_kit::link(lang.text(label), enabled.then(|| send(message)), palette)
    }));
    Some(page_kit::cell(cell))
}

fn sections<'a>(page: &OverviewView<'a>, palette: &Palette) -> Element<'a, Message> {
    let lang = page.lang;
    let mut children: Vec<Element<'a, Message>> = Vec::new();
    children.extend(problem(page, palette));
    children.push(page_kit::section(
        lang.text(Msg::HermesOverviewSectionState),
        palette,
    ));
    children.push(state_row(page, palette));
    children.push(ocr_row(page, palette));
    children.push(ai_row(page, palette));
    children.push(page_kit::section(
        lang.text(Msg::HermesOverviewSectionFolders),
        palette,
    ));
    children.extend(folder_rows(page, palette));
    children.push(page_kit::section(
        lang.text(Msg::HermesOverviewPatternLabel),
        palette,
    ));
    children.push(page_kit::hint(
        lang.text(Msg::HermesOverviewPatternNote),
        palette,
    ));
    let host = TargetHost {
        wrap: |edit| send(HermesOverviewMessage::TargetEdited(edit)),
        name_field_id: FOLDER_NAME_FIELD_ID,
        folder_hint: None,
        example: ExampleLine::Row,
        lang,
        palette,
    };
    children.push(target_view(page.state.target(), &host));
    if let Some(PatternNote::WriteFailed(reason)) = page.state.pattern_note() {
        children.push(page_kit::error_line(
            fill_template(
                lang.text(Msg::HermesOverviewPatternWriteFailed),
                &[("{reason}", reason)],
            ),
            palette,
        ));
    }
    page_kit::form_column(children)
}

fn runner_word(page: &OverviewView<'_>) -> String {
    let lang = page.lang;
    let word = lang.text(match page.runner {
        RunnerState::Running => Msg::FilingStatusStateRunning,
        RunnerState::Paused => Msg::FilingStatusStatePaused,
        RunnerState::Idle => Msg::FilingStatusStateIdle,
        RunnerState::Off => Msg::FilingStatusStateOff,
    });
    if page.runner == RunnerState::Running && page.in_progress > 0 {
        let busy = fill_template(
            lang.text(Msg::HermesOverviewInProgress),
            &[("{count}", &page.in_progress.to_string())],
        );
        format!("{word} · {busy}")
    } else {
        word.to_owned()
    }
}

fn state_row<'a>(page: &OverviewView<'a>, palette: &Palette) -> Element<'a, Message> {
    let lang = page.lang;
    let mut cell = vec![page_kit::value(runner_word(page), palette)];
    if page.runner != RunnerState::Off {
        let label = if page.runner == RunnerState::Paused {
            Msg::FilingStatusResume
        } else {
            Msg::FilingStatusPause
        };
        cell.push(page_kit::link(
            lang.text(label),
            Some(Message::FilingStatus(FilingStatusMessage::TogglePause)),
            palette,
        ));
    }
    page_kit::form_row(
        lang.text(Msg::FilingStatusStateLabel),
        page_kit::cell(cell),
        None,
        palette,
    )
}

fn ocr_row<'a>(page: &OverviewView<'a>, palette: &Palette) -> Element<'a, Message> {
    let lang = page.lang;
    let cell = match page.facts.ocr_tile() {
        OcrTile::Ready => vec![page_kit::value(
            lang.text(Msg::HermesOverviewTileOcrReady),
            palette,
        )],
        OcrTile::Missing => vec![
            page_kit::value(lang.text(Msg::HermesOverviewTileOcrMissing), palette),
            page_kit::link(
                lang.text(Msg::HermesOverviewTileOcrButton),
                Some(send(HermesOverviewMessage::LoadOcrModels)),
                palette,
            ),
        ],
    };
    page_kit::form_row(
        lang.text(Msg::HermesOverviewTileOcrTitle),
        page_kit::cell(cell),
        None,
        palette,
    )
}

fn ai_row<'a>(page: &OverviewView<'a>, palette: &Palette) -> Element<'a, Message> {
    let lang = page.lang;
    let on = page.facts.ai_on;
    let state = page.facts.ai_tile();
    let mut cell = vec![page_kit::checkbox(
        if on { Check::On } else { Check::Off },
        Some(send(HermesOverviewMessage::SetAiEnabled(!on))),
        Some(lang.text(Msg::HermesOverviewAiSwitch)),
        palette,
    )];
    if matches!(state, AiTile::NotLoaded) {
        cell.push(page_kit::link(
            &fill_template(
                lang.text(Msg::HermesOverviewTileAiLoad),
                &[("{size}", &model_size_label())],
            ),
            Some(send(HermesOverviewMessage::LoadAiModel)),
            palette,
        ));
    }
    let note = match &state {
        AiTile::Off => lang.text(Msg::HermesOverviewTileAiOff).to_owned(),
        other => ai_sentence(other, lang),
    };
    page_kit::form_row(
        lang.text(Msg::HermesOverviewTileAiTitle),
        page_kit::cell(cell),
        Some(page_kit::hint(note, palette)),
        palette,
    )
}

fn folder_value<'a>(
    path: Option<&Path>,
    none: Msg,
    change: bool,
    page: &OverviewView<'a>,
    palette: &Palette,
) -> Element<'a, Message> {
    let lang = page.lang;
    let mut cell = vec![match path {
        Some(folder) => page_kit::mono(
            shown_path(folder, page.home),
            page_kit::TEXT_BODY,
            palette.text,
        ),
        None => page_kit::value_dim(lang.text(none), palette),
    }];
    if change {
        cell.push(page_kit::link(
            lang.text(Msg::HermesOverviewFoldersChange),
            Some(send(HermesOverviewMessage::ChangeFolders)),
            palette,
        ));
    }
    page_kit::cell(cell)
}

fn folder_rows<'a>(page: &OverviewView<'a>, palette: &Palette) -> Vec<Element<'a, Message>> {
    let lang = page.lang;
    let folders = page.folders;
    vec![
        page_kit::form_row(
            lang.text(Msg::HermesOverviewRoleInbox),
            folder_value(
                folders.inbox,
                Msg::HermesSetupNotChosen,
                true,
                page,
                palette,
            ),
            None,
            palette,
        ),
        page_kit::form_row(
            lang.text(Msg::HermesOverviewRoleFilingLocation),
            folder_value(
                folders.filing_location,
                Msg::HermesSetupNotChosen,
                false,
                page,
                palette,
            ),
            None,
            palette,
        ),
        page_kit::form_row(
            lang.text(Msg::HermesOverviewRoleArchive),
            folder_value(
                folders.archive,
                Msg::HermesOverviewArchiveNone,
                false,
                page,
                palette,
            ),
            None,
            palette,
        ),
    ]
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

fn key_sentence(facts: &OverviewFacts, lang: Language) -> &'static str {
    match facts.key_problem {
        Some(KeyProblem::Mismatch) => lang.text(Msg::HermesOverviewKeyMismatch),
        _ => on_this_computer(
            lang,
            Msg::HermesOverviewKeyMissingMac,
            Msg::HermesOverviewKeyMissingOther,
        ),
    }
}

/// How the index is protected, for the settings page: the state word and
/// whether it is a good one, what it means, and why the last change failed.
pub struct ProtectionText {
    pub state: Option<&'static str>,
    pub good: bool,
    pub sentence: &'static str,
    pub failure: Option<String>,
}

pub fn protection_text(facts: &OverviewFacts, lang: Language) -> ProtectionText {
    let key_missing = facts.module == ModuleState::KeyMissing;
    let (state, sentence) = match facts.protection {
        Protection::Encrypted if key_missing => (
            Some(Msg::HermesOverviewProtectionEncrypted),
            key_sentence(facts, lang),
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
    let failure = facts.protection_note.as_ref().map(|note| {
        let (template, reason) = match note {
            ProtectionNote::EncryptFailed(reason) => (Msg::HermesOverviewEncryptFailed, reason),
            ProtectionNote::StartOverFailed(reason) => (Msg::HermesOverviewStartOverFailed, reason),
            ProtectionNote::KeyEntryNotRemoved(reason) => (Msg::HermesOverviewKeyEntryKept, reason),
        };
        fill_template(lang.text(template), &[("{reason}", reason)])
    });
    ProtectionText {
        state: state.map(|word| lang.text(word)),
        good: facts.protection == Protection::Encrypted && !key_missing,
        sentence,
        failure,
    }
}

/// One thing the user can do about the protection.
pub struct ProtectionAction {
    pub label: Msg,
    pub message: HermesOverviewMessage,
    pub enabled: bool,
    pub primary: bool,
}

/// The buttons for the protection. While a change runs they are inactive.
pub fn protection_actions(facts: &OverviewFacts) -> Vec<ProtectionAction> {
    let key_missing = facts.module == ModuleState::KeyMissing;
    let action = |label, message, enabled, primary| ProtectionAction {
        label,
        message,
        enabled,
        primary,
    };
    let mut actions = Vec::new();
    if key_missing {
        actions.push(action(
            Msg::HermesOverviewEnterRecoveryKey,
            HermesOverviewMessage::EnterRecoveryKey,
            facts.can_start_over,
            true,
        ));
        actions.push(action(
            Msg::HermesOverviewTryAgain,
            HermesOverviewMessage::TryAgain,
            facts.can_start_over,
            false,
        ));
    } else if facts.can_encrypt || facts.protection == Protection::Encrypting {
        actions.push(action(
            Msg::HermesOverviewEncryptNow,
            HermesOverviewMessage::EncryptNow,
            facts.can_encrypt,
            false,
        ));
    }
    let changing = matches!(
        facts.protection,
        Protection::Encrypting | Protection::StartingOver
    );
    if facts.can_start_over || key_missing || changing {
        actions.push(action(
            Msg::HermesOverviewStartOver,
            HermesOverviewMessage::AskStartOver,
            facts.can_start_over,
            false,
        ));
    }
    actions
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
