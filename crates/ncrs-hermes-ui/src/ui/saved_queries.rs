//! The reports (saved queries) page of the settings: searches over the index
//! kept under a name, each with the hits it finds today. Changing one, the
//! export and the question before deleting open a small dialog. The criteria
//! are only shown; they are changed in the search. Pure view over
//! [`SavedQueries`].
//!
//! Nothing is copied before the export dialog has shown where to and how many.

use iced::widget::{Column, Row};
use iced::{alignment, Element, Size};
use ncrs_config::palette::Palette;
use ncrs_filing::review_work::amount_text;
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_index::{KindCatalog, QuerySpec};
use ncrs_ui_kit::format;
use ncrs_ui_kit::frame::{DialogSize, Frame, KeyHints};
use ncrs_ui_kit::hermes_kit::{self as page_kit, MenuItem, RowTwo, GAP_S, TEXT_BODY};
use ncrs_ui_kit::kit::{self, Tone};
use ncrs_ui_kit::setting_rows::{Group, RowKey, SettingRow};

use crate::message::{Message, SavedQueriesMessage};
use crate::rules_page::hits_text;
use crate::saved_queries::{ExportStep, Problem, Results, SavedQueries};

/// Hits listed in the dialog; the rest is counted.
pub const VISIBLE_HITS: usize = 5;

pub const NAME_ID: &str = "saved-query-name";

/// The room of the list of templates in the dialog of a new report.
const TEMPLATE_WIDTH: f32 = 280.0;

/// Characters of a file name in the list of hits, with the amount beside it,
/// and of the target folder, in the value column of the medium dialog.
const HIT_NAME_CHARS: usize = 48;
const EXPORT_FOLDER_CHARS: usize = 36;

fn send(message: SavedQueriesMessage) -> Message {
    Message::SavedQueries(message)
}

/// The sections of the page: the reports with the button under them.
pub fn groups<'a>(
    state: &'a SavedQueries,
    lang: Language,
    palette: &Palette,
) -> Vec<Group<'a, Message>> {
    let kinds = &state.kinds().catalog;
    let idle = !state.is_busy();
    let mut rows: Vec<SettingRow<'a, Message>> = state
        .rows()
        .iter()
        .enumerate()
        .map(|(index, query)| {
            SettingRow::listed(
                query.name.clone(),
                Some(criteria_text(&query.spec, lang, kinds)),
                state
                    .hit_count(query.id)
                    .map(|count| hits_text(u64::try_from(count).unwrap_or(u64::MAX), lang)),
                vec![
                    kit::button(
                        lang.text(Msg::HermesPagesChange),
                        idle.then(|| send(SavedQueriesMessage::Edit(index))),
                        Tone::Secondary,
                        palette,
                    ),
                    kit::button(
                        lang.text(Msg::SavedQueriesExport),
                        idle.then(|| send(SavedQueriesMessage::StartExport(index))),
                        Tone::Secondary,
                        palette,
                    ),
                    kit::button(
                        lang.text(Msg::SettingsRemove),
                        idle.then(|| send(SavedQueriesMessage::AskDelete(index))),
                        Tone::Secondary,
                        palette,
                    ),
                ],
                palette,
            )
            .keyed_if(idle, RowKey::Press(send(SavedQueriesMessage::Edit(index))))
        })
        .collect();
    let note = match state.exported() {
        Some(done) => {
            let csv_name = done
                .csv_path
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            Some(fill_template(
                lang.text(Msg::SavedQueriesExportedNote),
                &[
                    ("{copied}", &done.copied.to_string()),
                    ("{existing}", &done.skipped_existing.to_string()),
                    ("{missing}", &done.skipped_missing.to_string()),
                    ("{csv}", &csv_name),
                ],
            ))
        }
        None if state.rows().is_empty() => Some(
            lang.text(if state.is_busy() {
                Msg::SavedQueriesLoading
            } else {
                Msg::SavedQueriesEmpty
            })
            .to_owned(),
        ),
        None => None,
    };
    let problem = if state.dialog_is_open() {
        None
    } else {
        state.problem().map(|found| problem_text(found, lang))
    };
    rows.push(
        SettingRow::adding(
            note,
            problem,
            vec![kit::button(
                lang.text(Msg::SavedQueriesNew),
                idle.then(|| send(SavedQueriesMessage::New)),
                Tone::Secondary,
                palette,
            )],
        )
        .keyed_if(idle, RowKey::Press(send(SavedQueriesMessage::New))),
    );
    vec![Group {
        intro: None,
        title: String::new(),
        rows,
    }]
}

/// The list of the templates over the form of a new report; the first entry
/// is no template.
fn template_row<'a>(
    state: &SavedQueries,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut items = vec![MenuItem::new(
        lang.text(Msg::SavedQueriesTemplateNone),
        send(SavedQueriesMessage::Template(0)),
    )];
    items.extend(
        state
            .templates()
            .iter()
            .enumerate()
            .map(|(position, template)| {
                MenuItem::new(
                    template.name(lang),
                    send(SavedQueriesMessage::Template(position + 1)),
                )
            }),
    );
    page_kit::form_row(
        lang.text(Msg::SavedQueriesTemplate),
        page_kit::dropdown(TEMPLATE_WIDTH, &items, state.template_choice(), palette),
        None,
        palette,
    )
}

/// What a query asks for, as name and value, in the order of the filters.
fn criteria(spec: &QuerySpec, lang: Language, kinds: &KindCatalog) -> Vec<(&'static str, String)> {
    let mut found = Vec::new();
    if let Some(kind) = &spec.kind {
        found.push((
            lang.text(Msg::SavedQueriesFieldKind),
            lang.kind_label(kinds, kind).into_owned(),
        ));
    }
    if let Some(year) = spec.year {
        found.push((lang.text(Msg::SavedQueriesFieldYear), year.to_string()));
    }
    if let Some(sender) = &spec.sender {
        found.push((lang.text(Msg::SavedQueriesFieldSender), sender.clone()));
    }
    found.extend(
        spec.tags
            .iter()
            .map(|tag| (lang.text(Msg::SavedQueriesFieldTags), tag.clone())),
    );
    if let Some(full_text) = &spec.full_text {
        found.push((lang.text(Msg::SavedQueriesFieldFullText), full_text.clone()));
    }
    found
}

/// What a query asks for in one line, short enough for a row.
pub fn criteria_text(spec: &QuerySpec, lang: Language, kinds: &KindCatalog) -> String {
    let parts: Vec<String> = criteria(spec, lang, kinds)
        .into_iter()
        .map(|(label, value)| format!("{label}: {value}"))
        .collect();
    if parts.is_empty() {
        lang.text(Msg::SavedQueriesCriteriaAll).to_owned()
    } else {
        parts.join(" · ")
    }
}

/// The dialog that is up: the question before a report is deleted, the
/// export, or the name and criteria of a report.
pub fn dialog<'a>(
    state: &'a SavedQueries,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Option<Element<'a, Message>> {
    if state.is_confirming_delete() {
        return Some(delete_dialog(state, window, lang, palette));
    }
    if let Some(step) = state.export_step() {
        return Some(export_dialog(state, step, window, lang, palette));
    }
    state
        .edit_is_open()
        .then(|| edit_dialog(state, window, lang, palette))
}

fn cancel_button<'a>(lang: Language, palette: &Palette) -> Element<'a, Message> {
    kit::button(
        lang.text(Msg::SavedQueriesCancel),
        Some(send(SavedQueriesMessage::CancelStep)),
        Tone::Secondary,
        palette,
    )
}

fn delete_dialog<'a>(
    state: &SavedQueries,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let name = state.selected().map_or("", |query| query.name.as_str());
    let question = fill_template(
        lang.text(Msg::SavedQueriesDeleteQuestion),
        &[("{name}", name)],
    );
    Frame::new(
        lang.text(Msg::SavedQueriesDeleteConfirm),
        kit::paragraph(question, palette),
        DialogSize::Small,
        window,
        lang,
        palette,
    )
    .buttons(vec![
        cancel_button(lang, palette),
        kit::button(
            lang.text(Msg::SavedQueriesDeleteConfirm),
            (!state.is_busy()).then(|| send(SavedQueriesMessage::ConfirmDelete)),
            Tone::Danger,
            palette,
        ),
    ])
    .on_close(send(SavedQueriesMessage::CancelStep))
    .hints(KeyHints {
        enter: false,
        tab: false,
    })
    .into_element()
}

fn export_dialog<'a>(
    state: &SavedQueries,
    step: &ExportStep,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut body = Column::new().spacing(GAP_S);
    if let Some(problem) = state.problem() {
        body = body.push(page_kit::error_line(problem_text(problem, lang), palette));
    }
    body = body
        .push(page_kit::form_row(
            lang.text(Msg::SavedQueriesExportFolder),
            Column::new()
                .spacing(GAP_S)
                .push(page_kit::mono(
                    format::ellipsize_middle(
                        &step.folder.display().to_string(),
                        EXPORT_FOLDER_CHARS,
                    ),
                    TEXT_BODY,
                    palette.text,
                ))
                .push(kit::button(
                    lang.text(Msg::SavedQueriesChooseFolder),
                    Some(send(SavedQueriesMessage::PickFolder)),
                    Tone::Secondary,
                    palette,
                ))
                .into(),
            None,
            palette,
        ))
        .push(page_kit::form_row(
            lang.text(Msg::SavedQueriesExportCount),
            page_kit::value(state.copies().to_string(), palette),
            None,
            palette,
        ))
        .push(page_kit::form_row(
            lang.text(Msg::SavedQueriesExportCsv),
            page_kit::mono(
                lang.text(Msg::SavedQueriesExportCsvName).to_owned(),
                TEXT_BODY,
                palette.text,
            ),
            Some(page_kit::hint(
                lang.text(Msg::SavedQueriesExportCsvExplain),
                palette,
            )),
            palette,
        ))
        .push(page_kit::hint(
            lang.text(Msg::SavedQueriesExportPromise),
            palette,
        ));
    Frame::new(
        lang.text(Msg::SavedQueriesExportTitle),
        body,
        DialogSize::Medium,
        window,
        lang,
        palette,
    )
    .buttons(vec![
        cancel_button(lang, palette),
        kit::button(
            lang.text(Msg::SavedQueriesExportConfirm),
            (!state.is_busy() && state.copies() > 0).then(|| send(SavedQueriesMessage::Export)),
            Tone::Primary,
            palette,
        ),
    ])
    .on_close(send(SavedQueriesMessage::CancelStep))
    .hints(KeyHints {
        enter: false,
        tab: false,
    })
    .into_element()
}

fn edit_dialog<'a>(
    state: &SavedQueries,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let creating = state.unsaved().is_some();
    let mut body = Column::new().spacing(GAP_S);
    if let Some(problem) = state.problem() {
        body = body.push(page_kit::error_line(problem_text(problem, lang), palette));
    }
    if creating {
        body = body.push(template_row(state, lang, palette));
    }
    body = body.push(page_kit::form_row(
        lang.text(Msg::ColumnName),
        page_kit::text_field(
            "",
            state.name(),
            NAME_ID,
            Some(|typed| send(SavedQueriesMessage::NameChanged(typed))),
            send(SavedQueriesMessage::Save),
            palette,
        ),
        None,
        palette,
    ));
    if let Some(spec) = state.selected_spec() {
        body = body.push(page_kit::form_row(
            lang.text(Msg::SavedQueriesColumnCriteria),
            criteria_control(spec, lang, state, palette),
            None,
            palette,
        ));
    }
    body = body.push(page_kit::section(
        lang.text(Msg::SavedQueriesResult),
        palette,
    ));
    match state.results() {
        Some(results) => {
            for element in result_rows(results, lang, palette) {
                body = body.push(element);
            }
            for element in hit_rows(results, lang, palette) {
                body = body.push(element);
            }
        }
        None if state.problem().is_none() => {
            body = body.push(page_kit::hint(lang.text(Msg::SavedQueriesLoading), palette));
        }
        None => {}
    }
    let can_save = !state.is_busy() && (state.unsaved().is_some() || state.name_is_changed());
    Frame::new(
        lang.text(if creating {
            Msg::SavedQueriesDialogNew
        } else {
            Msg::SavedQueriesDialogEdit
        }),
        body,
        DialogSize::Medium,
        window,
        lang,
        palette,
    )
    .buttons(vec![
        cancel_button(lang, palette),
        kit::button(
            lang.text(Msg::SavedQueriesSave),
            can_save.then(|| send(SavedQueriesMessage::Save)),
            Tone::Primary,
            palette,
        ),
    ])
    .on_close(send(SavedQueriesMessage::CancelStep))
    .hints(KeyHints {
        enter: false,
        tab: false,
    })
    .into_element()
}

fn criteria_control<'a>(
    spec: &QuerySpec,
    lang: Language,
    state: &SavedQueries,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut items: Vec<Element<'a, Message>> = criteria(spec, lang, &state.kinds().catalog)
        .into_iter()
        .map(|(label, value)| page_kit::criterion(label, &value, palette))
        .collect();
    if items.is_empty() {
        items.push(page_kit::value_dim(
            lang.text(Msg::SavedQueriesCriteriaAll),
            palette,
        ));
    }
    items.push(kit::button(
        lang.text(Msg::SavedQueriesEditInSearch),
        Some(send(SavedQueriesMessage::EditInSearch)),
        Tone::Secondary,
        palette,
    ));
    Row::with_children(items)
        .spacing(GAP_S)
        .align_y(alignment::Vertical::Center)
        .wrap()
        .into()
}

/// The figures: hits, their sum and the files that are gone.
fn result_rows<'a>(
    results: &Results,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    let mut rows = vec![page_kit::form_row(
        lang.text(Msg::SavedQueriesSummaryHits),
        page_kit::value(results.hits.len().to_string(), palette),
        None,
        palette,
    )];
    let totals = results.totals();
    if !totals.is_empty() {
        rows.push(page_kit::form_row(
            lang.text(Msg::SavedQueriesSummarySum),
            page_kit::value(
                totals
                    .iter()
                    .map(amount_text)
                    .collect::<Vec<_>>()
                    .join(" · "),
                palette,
            ),
            None,
            palette,
        ));
    }
    let missing = results.missing();
    if missing > 0 {
        rows.push(page_kit::form_row(
            lang.text(if missing == 1 {
                Msg::SavedQueriesSummaryMissingOne
            } else {
                Msg::SavedQueriesSummaryMissingOther
            }),
            page_kit::cell(vec![
                page_kit::value_in(missing.to_string(), palette.error),
                kit::button(
                    lang.text(if results.missing_shown {
                        Msg::SavedQueriesHideList
                    } else {
                        Msg::SavedQueriesShowList
                    }),
                    Some(send(SavedQueriesMessage::ToggleMissing)),
                    Tone::Secondary,
                    palette,
                ),
            ]),
            None,
            palette,
        ));
    }
    rows
}

/// The first hits, or the missing ones while those are asked for.
fn hit_rows<'a>(results: &Results, lang: Language, palette: &Palette) -> Vec<Element<'a, Message>> {
    let shown: Vec<_> = results
        .hits
        .iter()
        .filter(|hit| hit.missing || !results.missing_shown)
        .collect();
    if shown.is_empty() {
        return Vec::new();
    }
    let mut rows = Vec::new();
    for hit in shown.iter().take(VISIBLE_HITS) {
        let file_name = hit
            .path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let date_and_sender = [
            hit.fields
                .date
                .map(|date| date.format("%Y-%m-%d").to_string()),
            hit.fields.sender.clone(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
        rows.push(page_kit::list_row_two(
            RowTwo {
                primary: format::ellipsize_middle(&file_name, HIT_NAME_CHARS),
                secondary: date_and_sender,
                right: hit.fields.amount.as_ref().map(amount_text),
                lead: None,
                muted: false,
            },
            false,
            None,
            palette,
        ));
    }
    let left_out = shown.len().saturating_sub(VISIBLE_HITS);
    if left_out > 0 {
        rows.push(page_kit::hint(
            fill_template(
                lang.text(Msg::SavedQueriesHitsMore),
                &[("{count}", &left_out.to_string())],
            ),
            palette,
        ));
    }
    rows
}

fn problem_text(problem: &Problem, lang: Language) -> String {
    match problem {
        Problem::NameMissing => lang.text(Msg::SavedQueriesProblemName).to_owned(),
        Problem::Failed(detail) => fill_template(
            lang.text(Msg::SavedQueriesProblemFailed),
            &[("{detail}", detail)],
        ),
    }
}
