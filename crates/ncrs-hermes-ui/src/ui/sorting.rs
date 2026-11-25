//! The groups of the page Einsortieren: the sources, the targets, the history.

use std::path::Path;

use iced::widget::container;
use ncrs_config::config::Filing;
use ncrs_config::palette::Palette;
use ncrs_core::facts::{RunDay, RunFacts};
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_ui_kit::kit::Tone;
use ncrs_ui_kit::setting_rows::{Group, RowButton, SettingRow};

use super::{sources, targets};
use crate::message::Message;
use crate::rules_page::RulesPage;
use crate::sorting_page::{SortingMessage, SortingPage, RUNS_SHOWN};

pub fn groups<'a>(
    state: &SortingPage,
    rules: Option<&RulesPage>,
    filing: &Filing,
    home: &Path,
    lang: Language,
    palette: &Palette,
) -> Vec<Group<'a, Message>> {
    vec![
        sources::group(state, filing, home, lang, palette),
        targets::group(rules, filing, home, lang, palette),
        history_group(state, lang, palette),
    ]
}

fn history_group<'a>(state: &SortingPage, lang: Language, palette: &Palette) -> Group<'a, Message> {
    let runs = &state.runs;
    let shown = if state.all_runs { runs.len() } else { RUNS_SHOWN };
    let mut rows: Vec<SettingRow<'a, Message>> = runs
        .iter()
        .take(shown)
        .map(|run| run_row(run, lang, palette))
        .collect();
    if runs.is_empty() {
        rows.push(SettingRow::text(
            lang.text(Msg::SettingsHermesHistoryEmpty).to_owned(),
            None,
            container("").into(),
        ));
    }
    if runs.len() > RUNS_SHOWN {
        let label = if state.all_runs {
            Msg::SettingsHermesAreasShowFewer
        } else {
            Msg::SettingsHermesHistoryShowAll
        };
        rows.push(SettingRow::adding_buttons(
            None,
            None,
            vec![RowButton::new(
                lang.text(label),
                Some(Message::Sorting(SortingMessage::ToggleAllRuns)),
                Tone::Secondary,
            )],
            palette,
        ));
    }
    Group::headed(
        lang.text(Msg::SortingHistoryTitle),
        None,
        Vec::new(),
        rows,
        palette,
    )
}

fn run_row<'a>(run: &RunFacts, lang: Language, palette: &Palette) -> SettingRow<'a, Message> {
    let examples = (!run.examples.is_empty()).then(|| {
        fill_template(
            lang.text(Msg::SettingsHermesHistoryExamples),
            &[("{folders}", &run.examples.join(", "))],
        )
    });
    let undo = run.undoable.then(|| {
        RowButton::new(
            lang.text(Msg::SettingsHermesHistoryUndo),
            Some(Message::Sorting(SortingMessage::Undo(run.batch))),
            Tone::Secondary,
        )
    });
    SettingRow::listed_buttons(
        run_line(run, lang),
        examples,
        None,
        undo.into_iter().collect(),
        palette,
    )
}

fn run_line(run: &RunFacts, lang: Language) -> String {
    let day = match &run.day {
        RunDay::Today => lang.text(Msg::SettingsHermesHistoryToday).to_owned(),
        RunDay::Yesterday => lang.text(Msg::SettingsHermesHistoryYesterday).to_owned(),
        RunDay::On(date) => date.clone(),
    };
    let when = format!("{day} {}", run.time);
    let line = match run.files {
        1 => fill_template(
            lang.text(Msg::SettingsHermesHistoryRunOne),
            &[("{when}", &when), ("{from}", &run.from)],
        ),
        count => fill_template(
            lang.text(Msg::SettingsHermesHistoryRun),
            &[
                ("{when}", &when),
                ("{count}", &count.to_string()),
                ("{from}", &run.from),
            ],
        ),
    };
    if run.to.is_empty() {
        line
    } else {
        fill_template(
            lang.text(Msg::SettingsHermesHistoryTo),
            &[("{line}", &line), ("{to}", &run.to)],
        )
    }
}
