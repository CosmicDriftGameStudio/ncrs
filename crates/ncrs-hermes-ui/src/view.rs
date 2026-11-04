//! What the app draws of the Hermes pages: the single pages under the
//! title bar, the keys line under them and the parts of the title and status
//! bar that speak for Hermes.

use std::path::Path;

use iced::widget::{column, container};
use iced::{Element, Length, Size};
use ncrs_config::config::AiSetting;
use ncrs_config::file_list_tags::FileListTags;
use ncrs_config::palette::Palette;
use ncrs_filing::filing::{FilingModule, Phase};
use ncrs_i18n::{fill_template, FuturamaName, Language, Msg};
use ncrs_pipeline::RunnerState;

use crate::hermes_overview::{ModuleState, OcrFacts, OverviewFacts, Protection};
use crate::hermes_status::HermesStatus;
use crate::message::Message;
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

fn filing_status_page<'a>(hermes: &'a Hermes, ctx: &ViewCtx<'a>) -> Option<Element<'a, Message>> {
    if !hermes.filing_status.page_open() {
        return None;
    }
    let index_open = hermes
        .filing
        .as_ref()
        .is_some_and(|module| matches!(module.phase(), Phase::Ready));
    Some(filing_status_ui::view(
        &hermes.filing_status,
        filing_status_ui::Setting {
            load: hermes.filing.as_ref().map(FilingModule::load),
            index_open,
            pdf_library_missing: hermes.pdf_library_missing.as_deref(),
        },
        ctx.window_size,
        ctx.lang,
        ctx.palette,
    ))
}

/// The open single page below the title bar, with the status bar of Hermes
/// under it.
pub fn ablage_area<'a>(hermes: &'a Hermes, ctx: &ViewCtx<'a>) -> Option<Element<'a, Message>> {
    let lang = ctx.lang;
    let space = lang.text(Msg::HermesKitKeySpace);
    let (page, hints): (_, Vec<(&str, Msg)>) = if let Some(state) = hermes.rules.as_ref() {
        (
            rules_ui::view(state, lang, ctx.palette),
            vec![
                ("N", Msg::RulesHintNew),
                (space, Msg::RulesHintToggle),
                ("Alt ↑↓", Msg::RulesHintOrder),
                ("Del", Msg::RulesHintDelete),
            ],
        )
    } else if let Some(state) = hermes.tags.as_ref() {
        (
            tags_ui::view(state, ctx.file_list_tags, lang, ctx.palette),
            vec![
                ("↑↓", Msg::HermesKitHintSelect),
                (space, Msg::RulesHintToggle),
                ("Esc", Msg::DialogClose),
            ],
        )
    } else if let Some(state) = hermes.saved_queries.as_ref() {
        (
            saved_queries_ui::view(state, lang, ctx.palette),
            vec![
                ("↑↓", Msg::HermesKitHintSelect),
                ("N", Msg::RulesHintNew),
                ("Esc", Msg::DialogClose),
            ],
        )
    } else {
        (
            filing_status_page(hermes, ctx)?,
            vec![
                ("↑↓", Msg::HermesKitHintSelect),
                (space, Msg::HermesKitHintCheck),
                ("Esc", Msg::DialogClose),
            ],
        )
    };
    let bar = ncrs_ui_kit::hermes_kit::status_bar(
        lang.text(Msg::GroupHermes),
        &hints,
        hermes_status(hermes)
            .map(|status| status.line(lang))
            .unwrap_or_default(),
        lang,
        ctx.palette,
    );
    Some(
        container(column![page, bar])
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    )
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
