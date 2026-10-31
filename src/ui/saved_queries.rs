//! The reports (saved queries): searches over the index by year, tax context,
//! sender, kind, tags and text, each kept under a name, with the export of what a run found. A
//! page of the Hermes card. Pure view over [`SavedQueries`].
//!
//! Nothing is copied before the export step has shown where to and how many.

use iced::widget::{column, container, row};
use iced::{alignment, Element, Size};
use ncrs_index::{QuerySpec, TaxContext, TaxContextEntry};

use super::format;
use super::frame::{self, DialogSize, Frame, KeyHints};
use super::hermes::{self, Badges};
use super::kit::{self, dim, error_line, form_row, one_line, paragraph, Columns, Tone};
use super::theme::spacing;
use crate::app::fill_template;
use crate::i18n::{FuturamaName, Language, Msg};
use iced::advanced::widget::operation::{Focusable, Outcome};
use iced::advanced::widget::{self, Operation};
use iced::Rectangle;

use crate::messages::{HermesFocus, HermesPage, Message, SavedQueriesMessage};
use crate::palette::Palette;
use crate::review_work::amount_text;
use crate::saved_queries::{Problem, QueryField, ReportTemplate, SavedQueries, VISIBLE_ROWS};

/// Most hits listed when the result is expanded; the rest is counted.
pub const VISIBLE_HITS: usize = 6;

/// The form field that has the keyboard focus, or `None` while none has.
pub fn focused_field() -> impl Operation<Option<QueryField>> {
    struct FocusedField {
        found: Option<QueryField>,
    }

    impl Operation<Option<QueryField>> for FocusedField {
        fn focusable(
            &mut self,
            id: Option<&widget::Id>,
            _bounds: Rectangle,
            state: &mut dyn Focusable,
        ) {
            if state.is_focused() {
                self.found = id.and_then(field_of);
            }
        }

        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Option<QueryField>>)) {
            operate(self);
        }

        fn finish(&self) -> Outcome<Option<QueryField>> {
            Outcome::Some(self.found)
        }
    }

    FocusedField { found: None }
}

fn field_of(id: &widget::Id) -> Option<QueryField> {
    [
        (NAME_ID, QueryField::Name),
        (YEAR_ID, QueryField::Year),
        (SENDER_ID, QueryField::Sender),
        (KIND_ID, QueryField::Kind),
        (TAGS_ID, QueryField::Tags),
        (FULL_TEXT_ID, QueryField::FullText),
    ]
    .into_iter()
    .find(|(known, _)| *id == widget::Id::new(known))
    .map(|(_, field)| field)
}

pub const NAME_ID: &str = "saved-query-name";
const YEAR_ID: &str = "saved-query-year";
const SENDER_ID: &str = "saved-query-sender";
const KIND_ID: &str = "saved-query-kind";
const TAGS_ID: &str = "saved-query-tags";
const FULL_TEXT_ID: &str = "saved-query-full-text";

const SIZE: DialogSize = DialogSize::Card;

/// How the width of a row is shared: name, criteria, hits.
const PORTIONS: [u16; 3] = [4, 8, 2];

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
        if state.cursor().is_none() {
            body = body.push(templates(state, lang, palette));
        }
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
            body = body.push(list(state, line_chars, badges.focus, lang, palette));
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
    .title_theme(lang.futurama_name(FuturamaName::ActionSavedQueries))
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

/// The buttons that start a report from a template, and what open means.
fn templates<'a>(state: &SavedQueries, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let buttons: Vec<Element<'a, Message>> = state
        .templates()
        .iter()
        .map(|template: &ReportTemplate| {
            kit::button(
                &template.name(lang),
                (!state.is_busy())
                    .then(|| send(SavedQueriesMessage::UseTemplate(template.form(lang)))),
                Tone::Secondary,
                palette,
            )
        })
        .collect();
    column![
        dim(lang.text(Msg::SavedQueriesTemplateLabel), palette),
        row(buttons).spacing(spacing::S).wrap(),
        dim(
            fill_template(
                lang.text(Msg::SavedQueriesOpenInvoicesNote),
                &[("{tag}", lang.text(Msg::SavedQueriesTagOpen))],
            ),
            palette,
        ),
    ]
    .spacing(spacing::XS)
    .into()
}

/// What a query asks for, short enough for a column.
pub fn criteria_text(spec: &QuerySpec, tax_contexts: &[TaxContextEntry], lang: Language) -> String {
    let mut parts = Vec::new();
    if let Some(year) = spec.year {
        parts.push(year.to_string());
    }
    match &spec.tax_context {
        None => {}
        Some(TaxContext::None) => parts.push(lang.text(Msg::ReviewListTaxNone).to_owned()),
        Some(TaxContext::Named(short_name)) => parts.push(
            tax_contexts
                .iter()
                .find(|entry| entry.short_name.eq_ignore_ascii_case(short_name))
                .map_or_else(|| short_name.clone(), |entry| entry.name.clone()),
        ),
    }
    if let Some(sender) = &spec.sender {
        parts.push(sender.clone());
    }
    if let Some(kind) = &spec.kind {
        parts.push(lang.kind_label(kind).into_owned());
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
    focus: HermesFocus,
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
    let first = state.first_visible();
    let rows = state
        .rows()
        .iter()
        .enumerate()
        .skip(first)
        .take(VISIBLE_ROWS)
        .map(|(index, query)| {
            let selected = state.cursor() == Some(index);
            let color = if selected && focus == HermesFocus::Content {
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
                columns.cell(
                    1,
                    &criteria_text(&query.spec, state.tax_contexts(), lang),
                    color
                ),
                columns.cell(2, &hits, color),
            ]
            .align_y(alignment::Vertical::Center);
            kit::list_row(
                content.into(),
                send(SavedQueriesMessage::Select(index)),
                selected,
                focus == HermesFocus::Content,
                palette,
            )
        })
        .collect();
    kit::scroll_list(
        header,
        rows,
        kit::ListView {
            len: state.rows().len(),
            visible: VISIBLE_ROWS,
            offset: first,
        },
        |delta| send(SavedQueriesMessage::Scrolled(delta)),
        palette,
    )
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
    let tax_choices = kit::tax_context_choices(
        lang.text(Msg::SavedQueriesTaxAny),
        lang.text(Msg::ReviewListTaxNone),
        state.tax_contexts(),
    );
    let tax_options: Vec<(&str, Message)> = tax_choices
        .iter()
        .map(|(label, choice)| {
            (
                *label,
                send(SavedQueriesMessage::TaxContextChosen(choice.clone())),
            )
        })
        .collect();
    let selected_tax = tax_choices
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
            kit::dropdown(&tax_options, selected_tax, true, palette),
        ),
        row_of(
            Msg::SavedQueriesFieldSender,
            Some(Msg::SavedQueriesHintSender),
            text_field(QueryField::Sender, SENDER_ID, &values.sender),
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
    let mut figures = vec![
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
    let totals = results.totals();
    if !totals.is_empty() {
        figures.push((
            lang.text(Msg::SavedQueriesSummarySum),
            totals
                .iter()
                .map(amount_text)
                .collect::<Vec<_>>()
                .join(" · "),
        ));
    }
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
                paragraph(lang.text(Msg::SavedQueriesExportCsvExplain), palette),
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

#[cfg(test)]
mod tests {
    use super::*;

    struct Field(bool);

    impl Focusable for Field {
        fn is_focused(&self) -> bool {
            self.0
        }
        fn focus(&mut self) {}
        fn unfocus(&mut self) {}
    }

    #[test]
    fn the_focused_form_field_is_found_by_its_id() {
        let mut operation = focused_field();
        let bounds = Rectangle::default();
        operation.focusable(Some(&widget::Id::new(NAME_ID)), bounds, &mut Field(false));
        operation.focusable(Some(&widget::Id::new(KIND_ID)), bounds, &mut Field(true));
        operation.focusable(
            Some(&widget::Id::new("elsewhere")),
            bounds,
            &mut Field(false),
        );
        assert!(matches!(
            operation.finish(),
            Outcome::Some(Some(QueryField::Kind))
        ));
    }
}
