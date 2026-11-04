//! The Hermes page: whether he works, which folders he reads, the order he
//! learned from the user's files, and what he did last.

use iced::widget::container;
use iced::Element;

use super::index_page::{
    add_folder_row, folder_row, listed_locations, remove_folder_button, text_row,
};
use super::*;
use crate::facts::{RunDay, RunFacts};
use crate::folders::AddAs;
use crate::message::SettingsMessage;
use ncrs_config::config::{FilingLocation, LocationTransfer};
use ncrs_intake::LocationMode;

/// How many areas the Order section lists before "Show all".
const AREAS_SHOWN: usize = 3;

/// How many runs the History section lists before "Show all".
const RUNS_SHOWN: usize = 3;

pub fn hermes_groups<'a>(view: &SettingsView<'a>) -> Vec<Group<'a>> {
    let lang = view.lang;
    let group = |title: Msg, intro: Option<Msg>, rows| Group {
        intro: intro.map(|msg| lang.text(msg).to_owned()),
        title: lang.text(title).to_owned(),
        rows,
    };
    vec![
        Group {
            intro: None,
            title: String::new(),
            rows: vec![simple_row(
                Msg::SettingsHermesRowOn,
                Some(Msg::SettingsHermesRowOnNote),
                switch(
                    view.filing.enabled,
                    SettingChange::HermesEnabled(!view.filing.enabled),
                    view,
                ),
                lang,
            )],
        },
        group(
            Msg::SettingsHermesSectionFolders,
            Some(Msg::SettingsHermesSectionFoldersIntro),
            read_folder_rows(view),
        ),
        group(
            Msg::SettingsHermesSectionOrder,
            Some(Msg::SettingsHermesSectionOrderIntro),
            order_rows(view),
        ),
        group(Msg::SettingsHermesSectionHistory, None, history_rows(view)),
        group(Msg::SettingsGroupAdvanced, None, advanced_rows(view)),
    ]
}

/// "always · move · also images", in the words of the page.
fn folder_status(location: &FilingLocation, lang: Language) -> String {
    let mut parts = vec![
        lang.text(match location.mode {
            LocationMode::Inbox => Msg::SettingsHermesStatusAlways,
            LocationMode::Archive => Msg::SettingsHermesStatusOnRequest,
        }),
        lang.text(match location.transfer {
            LocationTransfer::Move => Msg::SettingsHermesStatusMove,
            LocationTransfer::Copy => Msg::SettingsHermesStatusCopy,
        }),
    ];
    if location.images {
        parts.push(lang.text(Msg::SettingsHermesStatusImages));
    }
    parts.join(" \u{b7} ")
}

fn read_folder_rows<'a>(view: &SettingsView<'a>) -> Vec<SettingRow<'a>> {
    let lang = view.lang;
    let read_folders: Vec<&FilingLocation> = listed_locations(view)
        .iter()
        .filter(|location| !location.is_target)
        .collect();
    let mut rows: Vec<SettingRow<'a>> = read_folders
        .iter()
        .map(|location| {
            let change = kit::button(
                lang.text(Msg::SettingsHermesFolderChange),
                Some(SettingsMessage::EditFolder(Some(location.path.clone()))),
                Tone::Secondary,
                view.palette,
            );
            folder_row(
                location,
                Some(folder_status(location, lang)),
                None,
                vec![change, remove_folder_button(location, view)],
                view,
            )
        })
        .collect();
    let chat = kit::button(
        lang.text(Msg::SettingsHermesSetupInChat),
        Some(SettingsMessage::OpenChat(None)),
        Tone::Secondary,
        view.palette,
    );
    rows.push(add_folder_row(
        AddAs::ReadFolder,
        read_folders
            .is_empty()
            .then_some(Msg::SettingsHermesFoldersEmpty),
        vec![chat],
        view,
    ));
    rows
}

fn note_row<'a>(text: String) -> SettingRow<'a> {
    text_row(text, None, container("").into())
}

fn order_rows<'a>(view: &SettingsView<'a>) -> Vec<SettingRow<'a>> {
    let lang = view.lang;
    let palette = view.palette;
    let Some(areas) = &view.facts.areas else {
        return vec![note_row(
            lang.text(Msg::SettingsHermesOrderReading).to_owned(),
        )];
    };
    if areas.is_empty() {
        return vec![note_row(
            lang.text(Msg::SettingsHermesOrderEmpty).to_owned(),
        )];
    }
    let shown = if view.state.all_areas {
        areas.len()
    } else {
        AREAS_SHOWN
    };
    let mut rows: Vec<SettingRow<'a>> = areas
        .iter()
        .take(shown)
        .map(|area| {
            let root = shown_path(&area.root, view.home);
            let where_and_how = if area.order.is_empty() {
                root
            } else {
                format!("{root} \u{203a} {}", area.order)
            };
            let name = match area.files {
                1 => fill_template(
                    lang.text(Msg::SettingsHermesAreaOneFile),
                    &[("{name}", &area.name)],
                ),
                count => fill_template(
                    lang.text(Msg::SettingsHermesAreaFiles),
                    &[("{name}", &area.name), ("{count}", &count.to_string())],
                ),
            };
            text_row(
                name,
                Some(where_and_how),
                kit::button(
                    lang.text(Msg::SettingsHermesAreaChange),
                    Some(SettingsMessage::OpenChat(Some(area.clone()))),
                    Tone::Secondary,
                    palette,
                ),
            )
        })
        .collect();
    let hidden = areas.len().saturating_sub(AREAS_SHOWN);
    if hidden > 0 {
        let (label, button) = if view.state.all_areas {
            (String::new(), Msg::SettingsHermesAreasShowFewer)
        } else {
            let label = match hidden {
                1 => lang.text(Msg::SettingsHermesAreasMoreOne).to_owned(),
                count => fill_template(
                    lang.text(Msg::SettingsHermesAreasMore),
                    &[("{count}", &count.to_string())],
                ),
            };
            (label, Msg::SettingsHermesAreasShowAll)
        };
        rows.push(text_row(
            label,
            None,
            kit::button(
                lang.text(button),
                Some(SettingsMessage::ToggleAllAreas),
                Tone::Secondary,
                palette,
            ),
        ));
    }
    rows
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

fn history_rows<'a>(view: &SettingsView<'a>) -> Vec<SettingRow<'a>> {
    let lang = view.lang;
    let runs = &view.facts.runs;
    if runs.is_empty() {
        return vec![note_row(
            lang.text(Msg::SettingsHermesHistoryEmpty).to_owned(),
        )];
    }
    let shown = if view.state.all_runs {
        runs.len()
    } else {
        RUNS_SHOWN
    };
    let mut rows: Vec<SettingRow<'a>> = runs
        .iter()
        .take(shown)
        .map(|run| {
            let control: Element<'a, Message> = if run.undoable {
                kit::button(
                    lang.text(Msg::SettingsHermesHistoryUndo),
                    Some(SettingsMessage::UndoRun(run.batch)),
                    Tone::Secondary,
                    view.palette,
                )
            } else {
                container("").into()
            };
            let examples = (!run.examples.is_empty()).then(|| {
                fill_template(
                    lang.text(Msg::SettingsHermesHistoryExamples),
                    &[("{folders}", &run.examples.join(", "))],
                )
            });
            text_row(run_line(run, lang), examples, control)
        })
        .collect();
    if runs.len() > RUNS_SHOWN {
        let button = if view.state.all_runs {
            Msg::SettingsHermesAreasShowFewer
        } else {
            Msg::SettingsHermesHistoryShowAll
        };
        rows.push(text_row(
            String::new(),
            None,
            kit::button(
                lang.text(button),
                Some(SettingsMessage::ToggleAllRuns),
                Tone::Secondary,
                view.palette,
            ),
        ));
    }
    rows
}

fn advanced_rows<'a>(view: &SettingsView<'a>) -> Vec<SettingRow<'a>> {
    let lang = view.lang;
    let open = |message: SettingsMessage| -> Element<'a, Message> {
        kit::button(
            lang.text(Msg::SettingsOpenEllipsis),
            Some(message),
            Tone::Secondary,
            view.palette,
        )
    };
    vec![
        text_row(
            lang.text(Msg::SettingsHermesRowRules).to_owned(),
            Some(fill_template(
                lang.text(Msg::SettingsHermesRowRulesNote),
                &[("{count}", &view.facts.rules.to_string())],
            )),
            open(SettingsMessage::OpenRules),
        ),
        text_row(
            lang.text(Msg::SettingsHermesRowTags).to_owned(),
            Some(lang.text(Msg::SettingsHermesRowTagsNote).to_owned()),
            open(SettingsMessage::ChooseFileListTags),
        ),
    ]
}
