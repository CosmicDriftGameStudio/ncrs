//! The reports (saved queries): searches over the index kept under a name,
//! each with the hits it finds today and the export of them. A page of the
//! Ablage tab. The criteria are only shown; they are changed in the search.
//! Pure view over [`SavedQueries`].
//!
//! Nothing is copied before the export step has shown where to and how many.

use iced::widget::{container, row, Row};
use iced::{alignment, Element};
use ncrs_index::{KindCatalog, QuerySpec};

use super::hermes::{self, Badges};
use crate::message::{HermesPage, Message, SavedQueriesMessage};
use crate::rules_page::hits_text;
use crate::saved_queries::{Draft, ExportStep, Problem, Results, SavedQueries, VISIBLE_ROWS};
use ncrs_config::palette::Palette;
use ncrs_filing::review_work::amount_text;
use ncrs_i18n::fill_template;
use ncrs_i18n::{FuturamaName, Language, Msg};
use ncrs_ui_kit::format;
use ncrs_ui_kit::hermes_kit::{
    self as page_kit, MenuItem, MenuLook, RowTwo, GAP_S, GAP_XS, TEXT_BODY,
};
use ncrs_ui_kit::kit;

/// Hits listed in the detail; the rest is counted.
pub const VISIBLE_HITS: usize = 5;

pub const NAME_ID: &str = "saved-query-name";

/// Characters of the name and of the criteria that fit a row of the list.
const NAME_CHARS: usize = 32;
const CRITERIA_CHARS: usize = 44;

/// Widths of the date and the sender in a line of a hit.
const DATE_WIDTH: f32 = 88.0;
const SENDER_WIDTH: f32 = 120.0;

fn send(message: SavedQueriesMessage) -> Message {
    Message::SavedQueries(message)
}

pub fn view<'a>(
    state: &'a SavedQueries,
    badges: Badges,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let body: Element<'a, Message> = if state.rows().is_empty() && state.unsaved().is_none() {
        let sentence = if state.is_busy() {
            Msg::SavedQueriesLoading
        } else {
            Msg::SavedQueriesEmpty
        };
        page_kit::centered_note(page_kit::hint(lang.text(sentence), palette))
    } else {
        page_kit::list_and_detail(list(state, lang, palette), detail(state, lang, palette))
    };
    page_kit::tab_page(
        hermes::tab_nav(HermesPage::SavedQueries, badges, lang, palette),
        header(state, lang, palette),
        body,
        Some(action_bar(state, lang, palette)),
        palette,
    )
}

fn header<'a>(state: &SavedQueries, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let mut new_items = vec![MenuItem::new(
        lang.text(Msg::SavedQueriesEmptySearch),
        send(SavedQueriesMessage::NewSearch(QuerySpec::default())),
    )];
    new_items.extend(state.templates().iter().map(|template| {
        MenuItem::new(
            template.name(lang),
            send(SavedQueriesMessage::NewSearch(template.spec(lang))),
        )
    }));
    let mut actions = vec![page_kit::menu_button(
        &format!("{} ▾", lang.text(Msg::SavedQueriesNew)),
        &new_items,
        MenuLook::Button,
        palette,
    )];
    if !state.is_busy() && state.selected().is_some() {
        let more = [MenuItem {
            key_hint: Some("Del".to_owned()),
            ..MenuItem::new(
                lang.text(Msg::SavedQueriesDelete),
                send(SavedQueriesMessage::AskDelete),
            )
        }];
        actions.push(page_kit::menu_button("", &more, MenuLook::Icon, palette));
    }
    page_kit::page_header(
        lang.text(Msg::ActionSavedQueries),
        lang.futurama_name(FuturamaName::ActionSavedQueries),
        Some(lang.text(Msg::SavedQueriesExplain).to_owned()),
        None,
        actions,
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

fn list<'a>(state: &SavedQueries, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let kinds = &state.kinds().catalog;
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    if let Some(draft) = state.unsaved() {
        rows.push(page_kit::list_row_two(
            unsaved_row(draft, lang, kinds),
            true,
            None,
            palette,
        ));
    }
    let room = VISIBLE_ROWS.saturating_sub(rows.len());
    let first = state.first_visible();
    for (index, query) in state.rows().iter().enumerate().skip(first).take(room) {
        rows.push(page_kit::list_row_two(
            RowTwo {
                primary: format::ellipsize_end(&query.name, NAME_CHARS),
                secondary: format::ellipsize_end(
                    &criteria_text(&query.spec, lang, kinds),
                    CRITERIA_CHARS,
                ),
                right: state
                    .hit_count(query.id)
                    .map(|count| hits_text(u64::try_from(count).unwrap_or(u64::MAX), lang)),
                lead: None,
                muted: false,
            },
            state.cursor() == Some(index),
            Some(send(SavedQueriesMessage::Select(index))),
            palette,
        ));
    }
    page_kit::list_pane(
        rows,
        |delta| send(SavedQueriesMessage::Scrolled(delta)),
        palette,
    )
}

fn unsaved_row(draft: &Draft, lang: Language, kinds: &KindCatalog) -> RowTwo<Message> {
    let name = draft.name.trim();
    RowTwo {
        primary: if name.is_empty() {
            lang.text(Msg::SavedQueriesUnsaved).to_owned()
        } else {
            format::ellipsize_end(name, NAME_CHARS)
        },
        secondary: format::ellipsize_end(&criteria_text(&draft.spec, lang, kinds), CRITERIA_CHARS),
        right: None,
        lead: None,
        muted: false,
    }
}

fn detail<'a>(state: &SavedQueries, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let mut children: Vec<Element<'a, Message>> = Vec::new();
    if state.is_confirming_delete() {
        let name = state.selected().map_or("", |query| query.name.as_str());
        children.push(page_kit::error_line(
            fill_template(
                lang.text(Msg::SavedQueriesDeleteQuestion),
                &[("{name}", name)],
            ),
            palette,
        ));
    }
    if let Some(problem) = state.problem() {
        children.push(page_kit::error_line(problem_text(problem, lang), palette));
    }
    let Some(spec) = state.selected_spec() else {
        return page_kit::detail(children);
    };
    if let Some(step) = state.export_step() {
        children.extend(export_step(state, step, lang, palette));
    }
    children.push(page_kit::form_row(
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
    children.push(page_kit::form_row(
        lang.text(Msg::SavedQueriesColumnCriteria),
        criteria_control(spec, lang, state, palette),
        None,
        palette,
    ));
    children.push(page_kit::section(
        lang.text(Msg::SavedQueriesResult),
        palette,
    ));
    match state.results() {
        Some(results) => {
            children.extend(result_rows(results, lang, palette));
            children.extend(hit_rows(results, lang, palette));
        }
        None => children.push(page_kit::hint(lang.text(Msg::SavedQueriesLoading), palette)),
    }
    page_kit::detail(children)
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
    items.push(page_kit::link(
        lang.text(Msg::SavedQueriesEditInSearch),
        Some(send(SavedQueriesMessage::EditInSearch)),
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
                page_kit::link(
                    lang.text(if results.missing_shown {
                        Msg::SavedQueriesHideList
                    } else {
                        Msg::SavedQueriesShowList
                    }),
                    Some(send(SavedQueriesMessage::ToggleMissing)),
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
    let mut rows = vec![page_kit::section(
        lang.text(Msg::SavedQueriesSummaryHits),
        palette,
    )];
    for hit in shown.iter().take(VISIBLE_HITS) {
        let date = hit
            .fields
            .date
            .map(|date| date.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        let file_name = hit
            .path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let line: Element<'a, Message> = row![
            container(page_kit::value_dim(date, palette)).width(DATE_WIDTH),
            container(page_kit::value(
                hit.fields.sender.clone().unwrap_or_default(),
                palette
            ))
            .width(SENDER_WIDTH),
            page_kit::mono(file_name, TEXT_BODY, palette.text),
        ]
        .spacing(GAP_XS)
        .into();
        rows.push(page_kit::list_row_one(
            line,
            hit.fields.amount.as_ref().map(amount_text),
            false,
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

fn export_step<'a>(
    state: &SavedQueries,
    step: &ExportStep,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    vec![
        page_kit::section(lang.text(Msg::SavedQueriesExportTitle), palette),
        page_kit::form_row(
            lang.text(Msg::SavedQueriesExportFolder),
            page_kit::cell(vec![
                page_kit::mono(step.folder.display().to_string(), TEXT_BODY, palette.text),
                page_kit::link(
                    lang.text(Msg::SavedQueriesChooseFolder),
                    Some(send(SavedQueriesMessage::PickFolder)),
                    palette,
                ),
            ]),
            None,
            palette,
        ),
        page_kit::form_row(
            lang.text(Msg::SavedQueriesExportCount),
            page_kit::value(state.copies().to_string(), palette),
            None,
            palette,
        ),
        page_kit::form_row(
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
        ),
        page_kit::hint(lang.text(Msg::SavedQueriesExportPromise), palette),
    ]
}

fn action_bar<'a>(state: &SavedQueries, lang: Language, palette: &Palette) -> Element<'a, Message> {
    if state.is_confirming_delete() {
        return page_kit::action_bar(
            None,
            vec![
                page_kit::secondary(
                    lang.text(Msg::SavedQueriesCancel),
                    Some(send(SavedQueriesMessage::CancelStep)),
                    palette,
                ),
                page_kit::primary(
                    lang.text(Msg::SavedQueriesDeleteConfirm),
                    Some(kit::ENTER_KEY),
                    Some(send(SavedQueriesMessage::ConfirmDelete)),
                    palette,
                ),
            ],
            palette,
        );
    }
    if state.export_step().is_some() {
        return page_kit::action_bar(
            None,
            vec![
                page_kit::secondary(
                    lang.text(Msg::SavedQueriesCancel),
                    Some(send(SavedQueriesMessage::CancelStep)),
                    palette,
                ),
                page_kit::primary(
                    lang.text(Msg::SavedQueriesExportConfirm),
                    Some(kit::ENTER_KEY),
                    (!state.is_busy()).then(|| send(SavedQueriesMessage::Export)),
                    palette,
                ),
            ],
            palette,
        );
    }
    if state.selected_spec().is_none() {
        return page_kit::action_bar(None, Vec::new(), palette);
    }
    let note = match state.exported() {
        Some(done) => {
            let csv_name = done
                .csv_path
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            fill_template(
                lang.text(Msg::SavedQueriesExportedNote),
                &[
                    ("{copied}", &done.copied.to_string()),
                    ("{existing}", &done.skipped_existing.to_string()),
                    ("{missing}", &done.skipped_missing.to_string()),
                    ("{csv}", &csv_name),
                ],
            )
        }
        None => lang.text(Msg::SavedQueriesExportNote).to_owned(),
    };
    let idle = !state.is_busy();
    let open = page_kit::secondary(
        lang.text(Msg::SavedQueriesOpenInSearch),
        Some(send(SavedQueriesMessage::OpenInSearch)),
        palette,
    );
    let last = if state.unsaved().is_some() {
        page_kit::primary(
            lang.text(Msg::SavedQueriesSave),
            Some(kit::ENTER_KEY),
            idle.then(|| send(SavedQueriesMessage::Save)),
            palette,
        )
    } else {
        page_kit::primary(
            lang.text(Msg::SavedQueriesExport),
            Some(kit::ENTER_KEY),
            (idle && state.copies() > 0).then(|| send(SavedQueriesMessage::StartExport)),
            palette,
        )
    };
    page_kit::action_bar(Some(note), vec![open, last], palette)
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
