//! What the app draws of the Hermes pages: the single pages under the
//! title bar, the keys line under them and the parts of the title and status
//! bar that speak for Hermes.

use std::path::Path;

use iced::widget::Column;
use iced::{Element, Size};
use ncrs_config::config::AiSetting;
use ncrs_config::file_list_tags::FileListTags;
use ncrs_config::palette::Palette;
use ncrs_filing::filing::{FilingModule, Phase};
use ncrs_i18n::{fill_template, FuturamaName, Language, Msg};
use ncrs_pipeline::RunnerState;
use ncrs_ui_kit::setting_rows;

use crate::hermes_overview::{ModuleState, OcrFacts, OverviewFacts, Protection};
use crate::hermes_status::HermesStatus;
use crate::message::{HermesPage, Message};
use crate::state::Hermes;
use crate::ui::{
    filing_status as filing_status_ui, rules as rules_ui, saved_queries as saved_queries_ui,
    tags as tags_ui,
};

/// What drawing the Hermes pages reads from the app.
#[derive(Debug, Clone, Copy)]
pub struct ViewCtx<'a> {
    pub lang: Language,
    pub palette: &'a Palette,
    pub window_size: Size,
    pub file_list_tags: &'a FileListTags,
    /// A job runs or waits.
    pub jobs_busy: bool,
    /// Bytes on disk and bytes in all, while the AI model downloads.
    pub ai_download: Option<(u64, u64)>,
    /// The system reads scans itself, without the OCR models.
    pub system_reads_scans: bool,
}

/// The text of the badge in the status bar, and whether it is red.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilingBadge {
    pub text: String,
    pub failing: bool,
}

/// What the overview and the protection panel of the settings say about the
/// module.
pub fn overview_facts(
    hermes: &Hermes,
    ctx: &ViewCtx<'_>,
    ocr_models_present: bool,
) -> OverviewFacts {
    let module = hermes.filing.as_ref();
    let module_state = if hermes.is_restarting() {
        ModuleState::Starting
    } else {
        module.map_or(ModuleState::Off, |running| match running.phase() {
            Phase::Dismissed if running.key_problem().is_some() => ModuleState::KeyMissing,
            phase => ModuleState::of(phase),
        })
    };
    let at_rest = !ctx.jobs_busy && !hermes.is_restarting();
    let failure = match module.map(FilingModule::phase) {
        Some(Phase::Failed(reason)) => Some(reason.clone()),
        _ => None,
    };
    OverviewFacts {
        module: module_state,
        failure,
        has_locations: !hermes.filing_config.locations.is_empty(),
        counts: hermes.filing_status.hermes_counts().copied(),
        ai_on: hermes.filing_config.ai != AiSetting::Off,
        ai_status: module.map(FilingModule::ai_status).unwrap_or_default(),
        ai_download: ctx.ai_download,
        ocr: OcrFacts::of(ctx.system_reads_scans, ocr_models_present),
        protection: Protection::of(
            module_state,
            module.and_then(FilingModule::index_encryption),
        ),
        key_problem: module.and_then(FilingModule::key_problem),
        can_encrypt: at_rest && module.is_some_and(FilingModule::can_encrypt),
        can_start_over: at_rest && module.is_some_and(FilingModule::can_start_over),
        protection_note: module.and_then(|running| running.protection_note().cloned()),
    }
}

/// The rows of one of the pages the settings window fills from here: the
/// rules, the reports, the tags or the read errors.
///
/// The page for the messages `wrap` makes of its own; a click on a row sends
/// what `on_focus` makes of the row's number.
pub fn page_content<'a, M: Clone + 'a>(
    hermes: &'a Hermes,
    page: HermesPage,
    ctx: &ViewCtx<'a>,
    focus: Option<setting_rows::RowFocus>,
    wrap: impl Fn(Message) -> M + Copy + 'a,
    on_focus: impl Fn(usize, Option<M>) -> Option<M> + Copy + 'a,
) -> Element<'a, M> {
    let mut content = Column::new();
    if page == HermesPage::Inspection {
        if let Some(searched) = hermes.pdf_library_missing.as_deref() {
            content = content.push(
                filing_status_ui::pdf_missing_notice(searched, ctx.lang, ctx.palette).map(wrap),
            );
        }
    }
    let groups = page_groups(hermes, page, ctx)
        .unwrap_or_default()
        .into_iter()
        .map(|group| group.map(wrap))
        .collect();
    for section in
        setting_rows::sections_with_focus(groups, |_| true, None, focus, on_focus, ctx.palette)
    {
        content = content.push(section);
    }
    content.into()
}

/// The rows of the page that answer the keyboard.
pub fn page_keys<'a>(
    hermes: &'a Hermes,
    page: HermesPage,
    ctx: &ViewCtx<'a>,
) -> Vec<setting_rows::RowKey<Message>> {
    setting_rows::keyed_rows(page_groups(hermes, page, ctx).unwrap_or_default(), |_| true)
}

fn page_groups<'a>(
    hermes: &'a Hermes,
    page: HermesPage,
    ctx: &ViewCtx<'a>,
) -> Option<Vec<setting_rows::Group<'a, Message>>> {
    let (lang, palette) = (ctx.lang, ctx.palette);
    match page {
        HermesPage::Rules => hermes
            .rules
            .as_ref()
            .map(|state| rules_ui::groups(state, lang, palette)),
        HermesPage::SavedQueries => hermes
            .saved_queries
            .as_ref()
            .map(|state| saved_queries_ui::groups(state, lang, palette)),
        HermesPage::Tags => hermes
            .tags
            .as_ref()
            .map(|state| tags_ui::groups(state, ctx.file_list_tags, lang, palette)),
        HermesPage::Inspection => Some(filing_status_ui::groups(
            &hermes.filing_status,
            lang,
            palette,
        )),
    }
}

/// The dialogs of the pages above, to be stacked over the window; at most
/// one of them is up.
pub fn dialogs<'a>(hermes: &'a Hermes, ctx: &ViewCtx<'a>) -> Vec<Element<'a, Message>> {
    let (window, lang, palette) = (ctx.window_size, ctx.lang, ctx.palette);
    [
        hermes
            .rules
            .as_ref()
            .and_then(|state| rules_ui::dialog(state, window, lang, palette)),
        hermes
            .saved_queries
            .as_ref()
            .and_then(|state| saved_queries_ui::dialog(state, window, lang, palette)),
        hermes
            .tags
            .as_ref()
            .and_then(|state| tags_ui::dialog(state, window, lang, palette)),
    ]
    .into_iter()
    .flatten()
    .collect()
}

pub fn hermes_status(hermes: &Hermes) -> Option<HermesStatus> {
    HermesStatus::of(
        hermes.filing_status.snapshot(),
        hermes.filing_status.hermes_counts(),
        hermes.pdf_library_missing.is_some(),
    )
}

/// The title bar chip: Hermes is on, and how fast it files. Absent while the
/// module is off.
pub fn hermes_chip(hermes: &Hermes, lang: Language) -> Option<String> {
    let text = match hermes.filing_status.snapshot().state {
        RunnerState::Off => return None,
        RunnerState::Paused => lang.text(Msg::TitleBarHermesPaused).to_owned(),
        RunnerState::Running | RunnerState::Idle => {
            let busy = hermes_status(hermes).is_some_and(|status| status.is_busy());
            match hermes.filing_status.documents_per_minute().filter(|_| busy) {
                Some(rate) => fill_template(
                    lang.text(Msg::TitleBarHermesRate),
                    &[("{rate}", &rate.to_string())],
                ),
                None => lang
                    .futurama_name(FuturamaName::ActionFilingSetup)
                    .unwrap_or_else(|| lang.text(Msg::GroupHermes))
                    .to_owned(),
            }
        }
    };
    Some(text)
}

/// The badge of Hermes in the status bar. It opens the status page.
pub fn filing_badge(hermes: &Hermes, lang: Language) -> Option<FilingBadge> {
    if hermes.filing_restart.is_some() {
        return Some(FilingBadge {
            text: lang.text(Msg::StatusHermesRestarting).to_owned(),
            failing: false,
        });
    }
    let status = hermes_status(hermes)?;
    Some(FilingBadge {
        text: hermes
            .reading_again
            .line(lang)
            .unwrap_or_else(|| status.line(lang)),
        failing: status.is_failing(),
    })
}

/// What Hermes says about the file under the cursor of the active panel.
pub fn hermes_line(hermes: &Hermes, cursor: Option<&Path>, lang: Language) -> Option<String> {
    if !hermes.is_active() {
        return None;
    }
    hermes.hermes_column.status_line(cursor?, lang)
}
