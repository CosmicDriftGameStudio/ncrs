//! The saved queries: searches over the index by year, tax context, kind, tags
//! and text, each kept under a name, with the export of what a run found. A
//! page of the Hermes card. Pure view over [`SavedQueries`].
//!
//! Nothing is copied before the export step has shown where to and how many.

use iced::widget::{column, container, row, Column};
use iced::{alignment, Element, Size};
use ncrs_index::{QuerySpec, TaxContext};

use super::format;
use super::frame::{self, DialogSize, Frame, KeyHints};
use super::hermes::{self, Badges};
use super::kit::{self, dim, error_line, form_row, one_line, paragraph, Columns, Tone};
use super::layout::ROW_HEIGHT;
use super::theme::spacing;
use crate::app::fill_template;
use crate::i18n::{Language, Msg};
use crate::messages::{HermesPage, Message, SavedQueriesMessage};
use crate::palette::Palette;
use crate::saved_queries::{Problem, QueryField, SavedQueries};

/// Most queries of the list shown at once; the list follows the cursor.
pub const VISIBLE_ROWS: usize = 3;

/// Most hits listed when the result is expanded; the rest is counted.
pub const VISIBLE_HITS: usize = 6;

const NAME_ID: &str = "saved-query-name";
const YEAR_ID: &str = "saved-query-year";
const KIND_ID: &str = "saved-query-kind";
const TAGS_ID: &str = "saved-query-tags";
const FULL_TEXT_ID: &str = "saved-query-full-text";

const SIZE: DialogSize = DialogSize::Card;

/// How the width of a row is shared: name, criteria, hits.
const PORTIONS: [u16; 3] = [4, 8, 2];

/// The tax context choices, in the order of the segmented control.
const TAX_CHOICES: [(Msg, Option<TaxContext>); 4] = [
    (Msg::SavedQueriesTaxAny, None),
    (Msg::ReviewListTaxCompany, Some(TaxContext::Company)),
    (Msg::ReviewListTaxPrivate, Some(TaxContext::Private)),
    (Msg::ReviewListTaxNone, Some(TaxContext::None)),
];

fn send(message: SavedQueriesMessage) -> Message {
    Message::SavedQueries(message)
}

pub fn view<'a>(
    state: &'a SavedQueries,
    window: Size,
    badges: Badges,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let line_chars = frame::content_chars(SIZE, window, frame::SIDEBAR_ROOM);
    let mut body =
        column![paragraph(lang.text(Msg::SavedQueriesExplain), palette)].spacing(spacing::LINE_GAP);

    // The export step takes the room of the list and the form: the page
    // would not fit them all, and the step is what the user decides on.
    if state.export_step().is_none() {
        if state.rows().is_empty() {
            body = body.push(dim(
                lang.text(if state.is_busy() {
                    Msg::SavedQueriesLoading
                } else {
                    Msg::SavedQueriesEmpty
                }),
                palette,
            ));
        } else {
            body = body.push(list(state, line_chars, lang, palette));
        }
        body = body.push(form(state, lang, palette));
    }
    if state.is_confirming_delete() {
        body = body.push(delete_question(state, lang, palette));
    }
    if let Some(problem) = state.problem() {
        body = body.push(error_line(problem_text(problem, lang), palette));
    }
    if let Some(results) = state.results() {
        body = body.push(result(state, results, line_chars, lang, palette));
    }
    if let Some(step) = state.export_step() {
        body = body.push(export_step(state, step, line_chars, lang, palette));
    }

    let mut frame = Frame::new(
        lang.text(Msg::ActionSavedQueries),
        body,
        SIZE,
        window,
        lang,
        palette,
    )
    .title_theme(lang.text(Msg::ActionSavedQueriesTheme))
    .buttons(buttons(state, lang, palette))
    .sidebar(hermes::nav(HermesPage::SavedQueries, badges, lang, palette))
    .on_close(send(SavedQueriesMessage::Close))
    .hints(KeyHints {
        enter: true,
        tab: true,
    });
    if let Some(done) = state.exported() {
        let csv_name = done
            .csv_path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        frame = frame.footer_note(fill_template(
            lang.text(Msg::SavedQueriesExportedNote),
            &[
                ("{copied}", &done.copied.to_string()),
                ("{existing}", &done.skipped_existing.to_string()),
                ("{missing}", &done.skipped_missing.to_string()),
                ("{csv}", &format::ellipsize_middle(&csv_name, line_chars)),
            ],
        ));
    }
    frame.into_element()
}

/// What a query asks for, short enough for a column.
pub fn criteria_text(spec: &QuerySpec, lang: Language) -> String {
    let mut parts = Vec::new();
    if let Some(year) = spec.year {
        parts.push(year.to_string());
    }
    if let Some(tax_context) = spec.tax_context {
        if let Some((label, _)) = TAX_CHOICES
            .iter()
            .find(|(_, choice)| *choice == Some(tax_context))
        {
            parts.push(lang.text(*label).to_owned());
        }
    }
    if let Some(kind) = &spec.kind {
        parts.push(kind.clone());
    }
    parts.extend(spec.tags.iter().map(|tag| format!("#{tag}")));
    if let Some(full_text) = &spec.full_text {
        parts.push(format!("\"{full_text}\""));
    }
    if parts.is_empty() {
        lang.text(Msg::SavedQueriesCriteriaAll).to_owned()
    } else {
        parts.join(", ")
    }
}

fn list<'a>(
    state: &'a SavedQueries,
    line_chars: usize,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let columns = Columns {
        portions: &PORTIONS,
        line_chars,
    };
    let header = columns.header(
        &[
            lang.text(Msg::ColumnName),
            lang.text(Msg::SavedQueriesColumnCriteria),
            lang.text(Msg::SavedQueriesColumnHits),
        ],
        palette,
    );
    let first = state
        .cursor()
        .map_or(0, |cursor| (cursor + 1).saturating_sub(VISIBLE_ROWS));
    let rows = state
        .rows()
        .iter()
        .enumerate()
        .skip(first)
        .take(VISIBLE_ROWS)
        .map(|(index, query)| {
            let selected = state.cursor() == Some(index);
            let color = if selected {
                palette.cursor_text
            } else {
                palette.text
            };
            let hits = state
                .hit_count(query.id)
                .map(|count| count.to_string())
                .unwrap_or_default();
            let content = row![
                columns.cell(0, &query.name, color),
                columns.cell(1, &criteria_text(&query.spec, lang), color),
                columns.cell(2, &hits, color),
            ]
            .align_y(alignment::Vertical::Center);
            kit::list_row(
                content.into(),
                send(SavedQueriesMessage::Select(index)),
                selected,
                palette,
            )
        });
    column![
        header,
        container(Column::with_children(rows))
            .height(ROW_HEIGHT * state.rows().len().min(VISIBLE_ROWS) as f32)
            .clip(true)
    ]
    .into()
}

fn form<'a>(state: &'a SavedQueries, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let values = state.form();
    let text_field = |field: QueryField, id: &'static str, value: &'a str| {
        kit::text_field(
            "",
            value,
            id,
            Some(move |typed| send(SavedQueriesMessage::FieldChanged(field, typed))),
            send(SavedQueriesMessage::Run),
            palette,
        )
    };
    let row_of = |label: Msg, hint: Option<Msg>, control: Element<'a, Message>| {
        form_row(
            lang.text(label),
            None,
            hint.map(|message| lang.text(message)),
            control,
            palette,
        )
    };
    let tax_options: Vec<(&str, Message)> = TAX_CHOICES
        .iter()
        .map(|(label, choice)| {
            (
                lang.text(*label),
                send(SavedQueriesMessage::TaxContextChosen(*choice)),
            )
        })
        .collect();
    let selected_tax = TAX_CHOICES
        .iter()
        .position(|(_, choice)| *choice == values.tax_context)
        .unwrap_or_default();
    column![
        row_of(
            Msg::ColumnName,
            None,
            text_field(QueryField::Name, NAME_ID, &values.name),
        ),
        row_of(
            Msg::SavedQueriesFieldYear,
            Some(Msg::SavedQueriesHintYear),
            text_field(QueryField::Year, YEAR_ID, &values.year),
        ),
        row_of(
            Msg::SavedQueriesFieldTaxContext,
            None,
            kit::segmented(&tax_options, selected_tax, true, palette),
        ),
        row_of(
            Msg::SavedQueriesFieldKind,
            Some(Msg::SavedQueriesHintKind),
            text_field(QueryField::Kind, KIND_ID, &values.kind),
        ),
        row_of(
            Msg::SavedQueriesFieldTags,
            Some(Msg::SavedQueriesHintTags),
            text_field(QueryField::Tags, TAGS_ID, &values.tags),
        ),
        row_of(
            Msg::SavedQueriesFieldFullText,
            Some(Msg::SavedQueriesHintFullText),
            text_field(QueryField::FullText, FULL_TEXT_ID, &values.full_text),
        ),
    ]
    .into()
}

fn delete_question<'a>(
    state: &SavedQueries,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let name = state.selected().map_or("", |query| query.name.as_str());
    row![
        container(paragraph(
            fill_template(
                lang.text(Msg::SavedQueriesDeleteQuestion),
                &[("{name}", name)]
            ),
            palette,
        ))
        .width(iced::Length::Fill),
        kit::button(
            lang.text(Msg::SavedQueriesCancel),
            Some(send(SavedQueriesMessage::CancelStep)),
            Tone::Secondary,
            palette,
        ),
        kit::button(
            lang.text(Msg::SavedQueriesDeleteConfirm),
            Some(send(SavedQueriesMessage::ConfirmDelete)),
            Tone::Danger,
            palette,
        ),
    ]
    .spacing(spacing::S)
    .align_y(alignment::Vertical::Center)
    .into()
}

fn result<'a>(
    state: &SavedQueries,
    results: &crate::saved_queries::Results,
    line_chars: usize,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let figures = [
        (
            lang.text(Msg::SavedQueriesSummaryHits),
            results.hits.len().to_string(),
        ),
        (
            lang.text(if results.missing() == 1 {
                Msg::SavedQueriesSummaryMissingOne
            } else {
                Msg::SavedQueriesSummaryMissingOther
            }),
            results.missing().to_string(),
        ),
    ];
    let mut content = column![kit::count_summary(
        &figures,
        None,
        results.expanded,
        send(SavedQueriesMessage::ToggleHits),
        lang.text(Msg::SavedQueriesShowList),
        lang.text(Msg::SavedQueriesHideList),
        palette,
    )]
    .spacing(spacing::S);
    if results.expanded {
        let missing_mark = lang.text(Msg::SavedQueriesHitMissing);
        for hit in results.hits.iter().take(VISIBLE_HITS) {
            let path = hit.path.display().to_string();
            let line = if hit.missing {
                let room = line_chars.saturating_sub(missing_mark.chars().count() + 3);
                format!("{} ({missing_mark})", format::ellipsize_middle(&path, room))
            } else {
                format::ellipsize_middle(&path, line_chars)
            };
            content = content.push(one_line(line, palette));
        }
        let left_out = results.hits.len().saturating_sub(VISIBLE_HITS);
        if left_out > 0 {
            content = content.push(dim(
                fill_template(
                    lang.text(Msg::SavedQueriesHitsMore),
                    &[("{count}", &left_out.to_string())],
                ),
                palette,
            ));
        }
    }
    // The export step has its own buttons: the one that opens it has no place
    // beside it.
    if state.export_step().is_none() {
        content = content.push(
            row![kit::button(
                lang.text(Msg::SavedQueriesExport),
                (state.copies() > 0 && !state.is_busy())
                    .then(|| send(SavedQueriesMessage::StartExport)),
                Tone::Secondary,
                palette,
            )]
            .align_y(alignment::Vertical::Center),
        );
    }
    content.into()
}

fn export_step<'a>(
    state: &SavedQueries,
    step: &crate::saved_queries::ExportStep,
    line_chars: usize,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let folder = format::ellipsize_middle(
        &step.folder.display().to_string(),
        line_chars.saturating_sub(24),
    );
    column![
        kit::section(
            lang.text(Msg::SavedQueriesExportTitle),
            column![
                paragraph(
                    fill_template(
                        lang.text(Msg::SavedQueriesExportFolder),
                        &[("{folder}", &folder)]
                    ),
                    palette,
                ),
                paragraph(
                    fill_template(
                        lang.text(Msg::SavedQueriesExportCount),
                        &[("{count}", &state.copies().to_string())],
                    ),
                    palette,
                ),
                paragraph(
                    fill_template(
                        lang.text(Msg::SavedQueriesExportCsv),
                        &[("{name}", lang.text(Msg::SavedQueriesExportCsvName))],
                    ),
                    palette,
                ),
                dim(lang.text(Msg::SavedQueriesExportPromise), palette),
            ]
            .spacing(spacing::XS),
            palette,
        ),
        row![kit::button(
            lang.text(Msg::SavedQueriesChooseFolder),
            Some(send(SavedQueriesMessage::PickFolder)),
            Tone::Secondary,
            palette,
        )],
    ]
    .spacing(spacing::S)
    .into()
}

fn buttons<'a>(
    state: &SavedQueries,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    if state.export_step().is_some() {
        return vec![
            kit::button(
                lang.text(Msg::SavedQueriesCancel),
                Some(send(SavedQueriesMessage::CancelStep)),
                Tone::Secondary,
                palette,
            ),
            kit::enter_button(
                lang.text(Msg::SavedQueriesExportConfirm),
                (!state.is_busy()).then(|| send(SavedQueriesMessage::Export)),
                Tone::Primary,
                palette,
            ),
        ];
    }
    let idle = !state.is_busy();
    let secondary = |label: Msg, message: SavedQueriesMessage, enabled: bool| {
        kit::button(
            lang.text(label),
            enabled.then(|| send(message)),
            Tone::Secondary,
            palette,
        )
    };
    vec![
        secondary(Msg::SavedQueriesClose, SavedQueriesMessage::Close, true),
        secondary(Msg::SavedQueriesNew, SavedQueriesMessage::New, idle),
        secondary(Msg::SavedQueriesSave, SavedQueriesMessage::Save, idle),
        secondary(
            Msg::SavedQueriesDelete,
            SavedQueriesMessage::AskDelete,
            idle && state.selected().is_some(),
        ),
        kit::enter_button(
            lang.text(Msg::SavedQueriesRun),
            idle.then(|| send(SavedQueriesMessage::Run)),
            Tone::Primary,
            palette,
        ),
    ]
}

fn problem_text(problem: &Problem, lang: Language) -> String {
    match problem {
        Problem::InvalidYear => lang.text(Msg::SavedQueriesProblemYear).to_owned(),
        Problem::NameMissing => lang.text(Msg::SavedQueriesProblemName).to_owned(),
        Problem::Failed(detail) => fill_template(
            lang.text(Msg::SavedQueriesProblemFailed),
            &[("{detail}", detail)],
        ),
    }
}
