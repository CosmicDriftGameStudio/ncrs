//! What the app draws of the Hermes pages: the pages themselves under the
//! title bar, the keys line under them and the parts of the title and status
//! bar that speak for Hermes.

use std::path::Path;

use iced::widget::{column, container};
use iced::{Element, Length, Size};
use ncrs_config::config::AiSetting;
use ncrs_config::file_list_tags::FileListTags;
use ncrs_config::palette::Palette;
use ncrs_filing::filing::{AiStatus, FilingModule, Phase};
use ncrs_i18n::{fill_template, FuturamaName, Language, Msg};
use ncrs_intake::LocationMode;
use ncrs_pipeline::RunnerState;

use crate::hermes_overview::{ModuleState, OcrFacts, OverviewFacts, Protection};
use crate::hermes_status::HermesStatus;
use crate::message::{HermesFocus, Message};
use crate::review_list::ReviewList;
use crate::state::Hermes;
use crate::ui::hermes::Badges;
use crate::ui::{
    filing_status as filing_status_ui, hermes_overview as hermes_overview_ui,
    reorganisation_plan as reorganisation_plan_ui, review_list as review_list_ui,
    rule_suggestions as rule_suggestions_ui, rules as rules_ui, saved_queries as saved_queries_ui,
    tags as tags_ui,
};

/// What drawing the Hermes pages reads from the app.
#[derive(Debug, Clone, Copy)]
pub struct ViewCtx<'a> {
    pub lang: Language,
    pub palette: &'a Palette,
    pub home: &'a Path,
    pub window_size: Size,
    pub hermes_focus: HermesFocus,
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

/// The counts the sidebar shows, from what the state already holds.
pub fn hermes_badges(hermes: &Hermes, ctx: &ViewCtx<'_>) -> Badges {
    let figures = hermes
        .filing_status
        .index_figures()
        .and_then(|figures| figures.as_ref().ok())
        .and_then(|figures| usize::try_from(figures.statistics.open_reviews).ok());
    Badges {
        focus: ctx.hermes_focus,
        counts: hermes.filing_status.hermes_counts().copied(),
        review: hermes
            .review_list
            .as_ref()
            .and_then(ReviewList::open_count)
            .or(figures),
        rule_suggestions: hermes
            .rule_suggestions
            .as_ref()
            .filter(|view| !view.is_busy())
            .map(|view| view.rows().len()),
    }
}

fn review_field_guide(hermes: &Hermes) -> review_list_ui::FieldGuide {
    review_list_ui::FieldGuide {
        confirmed_rules: hermes
            .filing_status
            .hermes_counts()
            .map(|counts| counts.confirmed_rules),
        ai_is_ready: !matches!(
            hermes.filing_status.ai(),
            AiStatus::Off | AiStatus::NotDownloaded | AiStatus::Failed(_)
        ),
    }
}

/// The inbox, the filing location and the archive of the config, by role.
fn overview_folders(hermes: &Hermes) -> hermes_overview_ui::Folders<'_> {
    let find = |mode: LocationMode, is_target: bool| {
        hermes
            .filing_config
            .locations
            .iter()
            .find(|location| {
                location.mode == mode
                    && (mode == LocationMode::Inbox || location.is_target == is_target)
            })
            .map(|location| location.path.as_path())
    };
    hermes_overview_ui::Folders {
        inbox: find(LocationMode::Inbox, false),
        filing_location: find(LocationMode::Archive, true),
        archive: find(LocationMode::Archive, false),
    }
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

fn overview_page<'a>(hermes: &'a Hermes, ctx: &ViewCtx<'a>) -> Option<Element<'a, Message>> {
    let state = hermes.overview.as_ref()?;
    let snapshot = hermes.filing_status.snapshot();
    let page = hermes_overview_ui::OverviewView {
        facts: overview_facts(hermes, ctx, state.ocr_models_present()),
        state,
        folders: overview_folders(hermes),
        home: ctx.home,
        runner: snapshot.state,
        in_progress: snapshot.open_files(),
        badges: hermes_badges(hermes, ctx),
        lang: ctx.lang,
    };
    Some(hermes_overview_ui::view(&page, ctx.palette))
}

fn review_list_page<'a>(hermes: &'a Hermes, ctx: &ViewCtx<'a>) -> Option<Element<'a, Message>> {
    let state = hermes.review_list.as_ref()?;
    Some(review_list_ui::view(
        state,
        hermes_badges(hermes, ctx),
        review_field_guide(hermes),
        review_list_ui::PreviewRoot {
            archive: hermes
                .filing_config
                .locations
                .iter()
                .find(|location| location.is_target)
                .map(|location| location.path.as_path()),
            home: ctx.home,
        },
        ctx.lang,
        ctx.palette,
    ))
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
        hermes_badges(hermes, ctx),
        ctx.lang,
        ctx.palette,
    ))
}

/// The overview or the status below the title bar, with the status bar of
/// filing under it.
pub fn ablage_area<'a>(hermes: &'a Hermes, ctx: &ViewCtx<'a>) -> Option<Element<'a, Message>> {
    let lang = ctx.lang;
    let badges = || hermes_badges(hermes, ctx);
    let space = lang.text(Msg::HermesKitKeySpace);
    let (page, hints): (_, Vec<(&str, Msg)>) = if let Some(page) = overview_page(hermes, ctx) {
        (
            page,
            vec![
                ("↑↓", Msg::HermesKitHintSelect),
                ("Tab", Msg::HermesKitHintNextField),
                ("Esc", Msg::DialogClose),
            ],
        )
    } else if let Some(state) = hermes.rule_suggestions.as_ref() {
        (
            rule_suggestions_ui::view(state, badges(), lang, ctx.palette),
            vec![
                ("↑↓", Msg::HermesKitHintSelect),
                ("Esc", Msg::DialogClose),
                ("↵", Msg::RuleSuggestionsConfirm),
            ],
        )
    } else if let Some(state) = hermes.rules.as_ref() {
        (
            rules_ui::view(state, badges(), lang, ctx.palette),
            vec![
                ("N", Msg::RulesHintNew),
                (space, Msg::RulesHintToggle),
                ("Alt ↑↓", Msg::RulesHintOrder),
                ("Del", Msg::RulesHintDelete),
            ],
        )
    } else if let Some(state) = hermes.tags.as_ref() {
        (
            tags_ui::view(state, ctx.file_list_tags, badges(), lang, ctx.palette),
            vec![
                ("↑↓", Msg::HermesKitHintSelect),
                (space, Msg::RulesHintToggle),
                ("Esc", Msg::DialogClose),
            ],
        )
    } else if let Some(state) = hermes.saved_queries.as_ref() {
        (
            saved_queries_ui::view(state, badges(), lang, ctx.palette),
            vec![
                ("↑↓", Msg::HermesKitHintSelect),
                ("N", Msg::RulesHintNew),
                ("Esc", Msg::DialogClose),
            ],
        )
    } else if let Some(state) = hermes.reorganisation.as_ref() {
        (
            reorganisation_plan_ui::view(state, badges(), lang, ctx.palette),
            vec![
                ("↑↓", Msg::HermesKitHintSelect),
                (space, Msg::HermesKitHintCheck),
                ("Esc", Msg::DialogClose),
            ],
        )
    } else if let Some(page) = review_list_page(hermes, ctx) {
        (
            page,
            vec![
                ("↑↓", Msg::HermesKitHintSelect),
                ("Tab", Msg::HermesKitHintNextField),
                ("F3", Msg::HermesKitHintView),
                ("↵", Msg::ReviewListConfirm),
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
