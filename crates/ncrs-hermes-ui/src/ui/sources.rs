//! The group of the sources on the page Einsortieren: the heading with the
//! button that adds one, and a row for each folder that is filed from.

use std::path::Path;

use iced::widget::{container, Row};
use iced::{alignment, Element};
use ncrs_config::config::{Filing, SourceSetting};
use ncrs_config::palette::Palette;
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_intake::{LocationTransfer, OnNoText};
use ncrs_ui_kit::format::shown_path;
use ncrs_ui_kit::hermes_kit::{self as page_kit, ChipKind, GAP_S, GAP_XS};
use ncrs_ui_kit::kit::{self, Tone};
use ncrs_ui_kit::setting_rows::{Group, RowButton, SettingRow};

use crate::message::Message;
use crate::sorting_page::{SortingMessage, SortingPage, SourceState};
use crate::target_dialog::target_name;

fn send(message: SortingMessage) -> Message {
    Message::Sorting(message)
}

pub fn group<'a>(
    page: &SortingPage,
    filing: &Filing,
    home: &Path,
    lang: Language,
    palette: &Palette,
) -> Group<'a, Message> {
    let mut rows: Vec<SettingRow<'a, Message>> = filing
        .sources
        .iter()
        .enumerate()
        .map(|(index, setting)| {
            let state = page.states.get(index).copied();
            source_row(index, setting, state, filing, home, lang, palette)
        })
        .collect();
    if rows.is_empty() {
        rows.push(SettingRow::text(
            lang.text(Msg::SortingSourcesEmpty).to_owned(),
            None,
            container("").into(),
        ));
    }
    Group::headed(
        lang.text(Msg::SortingSourcesTitle),
        Some(lang.text(Msg::SortingSourcesNote)),
        vec![RowButton::new(
            lang.text(Msg::SortingAddSource),
            Some(send(SortingMessage::AddSource)),
            Tone::Secondary,
        )],
        rows,
        palette,
    )
}

fn source_row<'a>(
    index: usize,
    setting: &SourceSetting,
    state: Option<SourceState>,
    filing: &Filing,
    home: &Path,
    lang: Language,
    palette: &Palette,
) -> SettingRow<'a, Message> {
    let source = &setting.source;
    let name = source
        .path
        .file_name()
        .unwrap_or(source.path.as_os_str())
        .to_string_lossy()
        .into_owned();
    let status = state_text(state, source_summary(setting, lang), lang);
    let mut buttons = Vec::new();
    if let Some(SourceState::Ready {
        questions: questions @ 1..,
    }) = state
    {
        buttons.push(RowButton::new(
            questions_text(questions, lang),
            Some(send(SortingMessage::OpenQuestions(index))),
            Tone::Secondary,
        ));
    }
    if state.is_some_and(SourceState::folder_is_out_of_reach) {
        buttons.push(RowButton::new(
            lang.text(Msg::SortingRepickFolder),
            Some(send(SortingMessage::RepickSource(index))),
            Tone::Primary,
        ));
    }
    buttons.push(RowButton::new(
        lang.text(Msg::SortingChange),
        Some(send(SortingMessage::ChangeSource(index))),
        Tone::Secondary,
    ));
    buttons.push(RowButton::new(
        lang.text(Msg::SortingRemove),
        Some(send(SortingMessage::RemoveSource(index))),
        Tone::Secondary,
    ));
    SettingRow::listed_columns(
        name,
        Some(shown_path(&source.path, home)),
        |colors| route(setting, filing, colors),
        Some(status),
        buttons,
        palette,
    )
}

/// "[*.pdf] → [T1 Documents] [T2 Firm]"
fn route<'a>(setting: &SourceSetting, filing: &Filing, palette: &Palette) -> Element<'a, Message> {
    let source = &setting.source;
    let mut line = Row::new()
        .spacing(GAP_S)
        .align_y(alignment::Vertical::Center);
    if !source.include.is_empty() {
        line = line.push(page_kit::chip(
            &source.include,
            ChipKind::Automatic,
            None,
            palette,
        ));
    }
    let targets: Vec<Element<'a, Message>> = source
        .targets
        .iter()
        .filter_map(|id| {
            let at = filing
                .targets
                .iter()
                .position(|listed| listed.target.id == *id)?;
            let name = target_name(&filing.targets.get(at)?.target);
            let label = format!("T{} {name}", at + 1);
            Some(page_kit::target_chip(at, &label, palette))
        })
        .collect();
    if !targets.is_empty() {
        line = line.push(kit::dim("→", palette)).extend(targets);
    }
    line.wrap().vertical_spacing(GAP_XS).into()
}

/// "move · without text: ask", the line of a source that works.
fn source_summary(setting: &SourceSetting, lang: Language) -> String {
    let source = &setting.source;
    let files = match source.transfer {
        LocationTransfer::Move => Msg::SortingSourceMove,
        LocationTransfer::Copy => Msg::SortingSourceCopy,
    };
    let no_text = match source.no_text {
        OnNoText::Ask => Msg::SortingSourceNoTextAsk,
        OnNoText::Skip => Msg::SortingSourceNoTextSkip,
    };
    fill_template(
        lang.text(Msg::SortingSourceSummary),
        &[
            ("{files}", lang.text(files)),
            ("{no_text}", lang.text(no_text)),
        ],
    )
}

fn questions_text(questions: usize, lang: Language) -> String {
    if questions == 1 {
        return lang.text(Msg::SortingStateQuestionsOne).to_owned();
    }
    fill_template(
        lang.text(Msg::SortingStateQuestionsOther),
        &[("{count}", &questions.to_string())],
    )
}

fn state_text(state: Option<SourceState>, summary: String, lang: Language) -> String {
    match state {
        None | Some(SourceState::Ready { .. }) => summary,
        Some(SourceState::Paused) => lang.text(Msg::SortingStatePaused).to_owned(),
        Some(SourceState::DriveMissing) => lang.text(Msg::SortingStateDriveMissing).to_owned(),
        Some(SourceState::FolderMissing) => lang.text(Msg::SortingStateFolderMissing).to_owned(),
        Some(SourceState::Unreadable) => lang.text(Msg::SortingStateUnreadable).to_owned(),
        Some(SourceState::TargetWaiting) => lang.text(Msg::SortingStateTargetWaiting).to_owned(),
        Some(SourceState::GlobError) => lang.text(Msg::SortingStateGlobError).to_owned(),
    }
}
