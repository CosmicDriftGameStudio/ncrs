//! The dialogs in which Hermes proposes rules: a scheme for a target, the
//! rules a target still lacks, and the one rule for a file. Pure view over
//! [`Suggest`].

use iced::widget::{container, Column, Id, Row};
use iced::{alignment, Element, Length, Size};
use ncrs_config::palette::Palette;
use ncrs_filing::sorting::{ModelNote, RuleForFile, Suggested, Suggestions};
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_index::KindCatalog;
use ncrs_ui_kit::frame::{DialogSize, Frame, KeyHints};
use ncrs_ui_kit::hermes_kit::{self as page_kit, GAP_M, GAP_S, GAP_XS};
use ncrs_ui_kit::kit::{self, Check, Tone};

use crate::message::{Message, SuggestMessage};
use crate::rules_page::conditions_short;
use crate::suggest::{Found, Suggest, SuggestFor, SuggestPhase};
use ncrs_filing::sorting::SuggestError;

fn send(message: SuggestMessage) -> Message {
    Message::Suggest(message)
}

/// The target as the chip names it, "T1 Documents".
#[derive(Debug)]
pub struct TargetLabel {
    pub text: String,
    pub path: String,
}

/// The dialog that is up.
pub fn dialog<'a>(
    state: &'a Suggest,
    target: Option<&TargetLabel>,
    kinds: &KindCatalog,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut body = Column::new().spacing(GAP_S);
    if let Some(detail) = &state.problem {
        body = body.push(page_kit::error_line(
            fill_template(lang.text(Msg::SortingSuggestFailed), &[("{detail}", detail)]),
            palette,
        ));
    }
    let title = match (&state.subject, state.has_rules()) {
        (SuggestFor::File { .. }, _) => Msg::SortingSuggestRuleTitle,
        (SuggestFor::Target(_), true) => Msg::SortingSuggestRulesTitle,
        (SuggestFor::Target(_), false) => Msg::SortingSchemeTitle,
    };
    let frame_buttons;
    match (&state.phase, state.file_rule()) {
        (SuggestPhase::Running, _) => {
            body = body.push(page_kit::hint(lang.text(Msg::SortingSuggestWorking), palette));
            frame_buttons = vec![again_button(state, lang, palette)];
        }
        (SuggestPhase::Failed(error), _) => {
            body = body.push(failure_line(error, lang, palette));
            frame_buttons = vec![again_button(state, lang, palette)];
        }
        (SuggestPhase::Done, Some(found)) => {
            for element in file_rows(found, target, kinds, lang, palette) {
                body = body.push(element);
            }
            frame_buttons = file_buttons(state, lang, palette);
        }
        (SuggestPhase::Done, None) => {
            for element in scheme_rows(state, kinds, lang, palette) {
                body = body.push(element);
            }
            frame_buttons = scheme_buttons(state, lang, palette);
        }
    }
    let subtitle = match &state.subject {
        SuggestFor::File { path, .. } => path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned()),
        SuggestFor::Target(_) => target.map(|label| format!("{} · {}", label.text, label.path)),
    };
    let mut frame = Frame::new(lang.text(title), body, DialogSize::Wide, window, lang, palette);
    if let Some(subtitle) = subtitle {
        frame = frame.subtitle(subtitle);
    }
    frame
        .buttons(frame_buttons)
        .on_close(send(SuggestMessage::Close))
        .hints(KeyHints {
            enter: true,
            tab: false,
        })
        .into_element()
}

/// A file no rule fits is no failure; anything else says why nothing was proposed.
fn failure_line<'a>(error: &SuggestError, lang: Language, palette: &Palette) -> Element<'a, Message> {
    match error {
        SuggestError::NoPattern => {
            page_kit::hint(lang.text(Msg::SortingSuggestNoneForFile), palette)
        }
        other => page_kit::error_line(
            fill_template(
                lang.text(Msg::SortingSuggestFailed),
                &[("{detail}", &other.to_string())],
            ),
            palette,
        ),
    }
}

fn again_button<'a>(state: &Suggest, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let idle = !matches!(state.phase, SuggestPhase::Running) && !state.saving;
    kit::button(
        lang.text(Msg::SortingSchemeAgain),
        idle.then(|| send(SuggestMessage::Again)),
        Tone::Secondary,
        palette,
    )
}

fn scheme_buttons<'a>(
    state: &Suggest,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    let mut buttons = vec![again_button(state, lang, palette)];
    if state.items().is_empty() {
        return buttons;
    }
    let count = state.ticked_count();
    buttons.push(
        kit::enter_button(
            &if count == 1 {
                lang.text(Msg::SortingTakeRule).to_owned()
            } else {
                fill_template(
                    lang.text(Msg::SortingTakeRules),
                    &[("{count}", &count.to_string())],
                )
            },
            (count > 0 && !state.saving).then(|| send(SuggestMessage::Adopt)),
            Tone::Primary,
            palette,
        ),
    );
    buttons
}

fn file_buttons<'a>(
    state: &Suggest,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    vec![
        kit::button(
            lang.text(Msg::SortingQuestionLeave),
            Some(send(SuggestMessage::Close)),
            Tone::Secondary,
            palette,
        ),
        kit::button(
            lang.text(Msg::SortingSuggestRuleEdit),
            (!state.saving).then(|| send(SuggestMessage::Edit(0))),
            Tone::Secondary,
            palette,
        ),
        kit::enter_button(
            lang.text(Msg::SortingSuggestRuleCreate),
            (!state.saving).then(|| send(SuggestMessage::Adopt)),
            Tone::Primary,
            palette,
        ),
    ]
}

/// What the model did, in a grey note; nothing when it did its part.
fn model_note<'a>(note: &ModelNote, lang: Language, palette: &Palette) -> Option<Element<'a, Message>> {
    let text = match note {
        ModelNote::Used => return None,
        ModelNote::NotAvailable => lang.text(Msg::SortingSuggestNoModel).to_owned(),
        ModelNote::Failed(detail) => fill_template(
            lang.text(Msg::SortingSuggestModelFailed),
            &[("{detail}", detail)],
        ),
    };
    Some(page_kit::hint(text, palette))
}

/// "Gelesen: 212 Dateien in 3 Quellen. …" for a target without rules, the
/// count of what the rules leave out for one with rules.
pub fn scheme_intro(found: &Suggestions, lang: Language) -> String {
    if found.rules_existing > 0 {
        return fill_template(
            lang.text(Msg::SortingSuggestRulesIntro),
            &[
                ("{rules}", &found.rules_existing.to_string()),
                ("{folders}", &found.items.len().to_string()),
            ],
        );
    }
    fill_template(
        lang.text(Msg::SortingSchemeRead),
        &[
            ("{files}", &found.files_read.to_string()),
            ("{sources}", &found.sources_read.to_string()),
            ("{fit}", &found.fit_suggested.to_string()),
            ("{rules}", &found.items.len().to_string()),
            ("{rest}", &found.fit_none.to_string()),
        ],
    )
}

fn scheme_rows<'a>(
    state: &'a Suggest,
    kinds: &KindCatalog,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    let Some(Found::Target(found)) = &state.found else {
        return Vec::new();
    };
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    if !found.items.is_empty() {
        rows.push(kit::paragraph(scheme_intro(found, lang), palette));
    }
    if let Some(note) = model_note(&found.model, lang, palette) {
        rows.push(note);
    }
    if found.items.is_empty() {
        rows.push(page_kit::hint(lang.text(Msg::SortingSuggestRulesNone), palette));
        return rows;
    }
    for (at, item) in found.items.iter().enumerate() {
        rows.push(if found.rules_existing > 0 {
            compact_row(state, at, item, lang, palette)
        } else {
            item_row(state, at, item, kinds, lang, palette)
        });
    }
    rows
}

fn item_row<'a>(
    state: &Suggest,
    at: usize,
    item: &Suggested,
    kinds: &KindCatalog,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let ticked = state.ticks.get(at).copied().unwrap_or(false);
    let marker = if state.cursor == at { "▸ " } else { "" };
    let head = Row::new()
        .push(kit::labelled_checkbox(
            if ticked { Check::On } else { Check::Off },
            Some(send(SuggestMessage::Toggle(at))),
            Some(&format!("{marker}{}", item.name)),
            palette,
        ))
        .push(container(kit::dim(conditions_short(&item.spec, lang, kinds), palette)).width(Length::Fill))
        .push(kit::dim(files_text(item.files, lang), palette))
        .push(kit::button(
            lang.text(Msg::SortingChange),
            (!state.saving).then(|| send(SuggestMessage::Edit(at))),
            Tone::Secondary,
            palette,
        ))
        .push(reject_button(state, at, lang, palette))
        .spacing(GAP_M)
        .align_y(alignment::Vertical::Center);
    let mut column = Column::new().spacing(GAP_XS).push(head).push(pattern_field(
        at,
        &item.spec.filing_pattern,
        palette,
    ));
    if let Some(example) = &item.example {
        column = column.push(kit::dim(
            fill_template(lang.text(Msg::SortingSchemeExample), &[("{example}", example)]),
            palette,
        ));
    }
    column.into()
}

fn reject_button<'a>(
    state: &Suggest,
    at: usize,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    kit::button(
        lang.text(Msg::SortingSuggestReject),
        (!state.saving).then(|| send(SuggestMessage::Reject(at))),
        Tone::Secondary,
        palette,
    )
}

/// A proposal of a target that has rules: one line with the pattern.
fn compact_row<'a>(
    state: &Suggest,
    at: usize,
    item: &Suggested,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let ticked = state.ticks.get(at).copied().unwrap_or(false);
    let marker = if state.cursor == at { "▸ " } else { "" };
    Row::new()
        .push(kit::labelled_checkbox(
            if ticked { Check::On } else { Check::Off },
            Some(send(SuggestMessage::Toggle(at))),
            Some(&format!("{marker}{}", item.name)),
            palette,
        ))
        .push(container(kit::dim(item.spec.filing_pattern.clone(), palette)).width(Length::Fill))
        .push(kit::dim(files_text(item.files, lang), palette))
        .push(reject_button(state, at, lang, palette))
        .spacing(GAP_M)
        .align_y(alignment::Vertical::Center)
        .into()
}

fn files_text(count: usize, lang: Language) -> String {
    fill_template(
        lang.text(Msg::SortingSchemeFiles),
        &[("{count}", &count.to_string())],
    )
}

/// The pattern of a proposal; it is changed in the rule form, not here.
fn pattern_field<'a>(at: usize, pattern: &str, palette: &Palette) -> Element<'a, Message> {
    kit::mono_text_field(
        "",
        pattern,
        Id::from(format!("suggest-pattern-{at}")),
        None::<fn(String) -> Message>,
        send(SuggestMessage::Adopt),
        palette,
    )
}

/// The other files the rule takes, by name, the first few.
fn also_names(found: &RuleForFile) -> String {
    const NAMED: usize = 5;
    let mut names: Vec<String> = found
        .also
        .iter()
        .take(NAMED)
        .filter_map(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .collect();
    if found.also.len() > NAMED {
        names.push("…".to_owned());
    }
    names.join(", ")
}

/// "Erkannt: Stadtwerke · Rechnung · 14.07.2026".
pub fn facts_text(found: &RuleForFile) -> String {
    let fields = &found.fields;
    [
        fields.sender.clone(),
        fields.kind.clone(),
        fields.date.map(|date| date.format("%d.%m.%Y").to_string()),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ")
}

fn file_rows<'a>(
    found: &'a RuleForFile,
    target: Option<&TargetLabel>,
    kinds: &KindCatalog,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    let mut rows: Vec<Element<'a, Message>> = vec![kit::paragraph(
        fill_template(
            lang.text(Msg::SortingSuggestRuleFound),
            &[("{facts}", &facts_text(found))],
        ),
        palette,
    )];
    if let Some(note) = model_note(&found.model, lang, palette) {
        rows.push(note);
    }
    let heading = fill_template(
        lang.text(Msg::SortingSuggestRuleSection),
        &[(
            "{target}",
            &target.map(|label| label.text.to_uppercase()).unwrap_or_default(),
        )],
    );
    rows.push(page_kit::section(&heading, palette));
    let spec = &found.rule.spec;
    rows.push(page_kit::form_row(
        lang.text(Msg::RulesFormName),
        kit::one_line(found.rule.name.clone(), palette),
        None,
        palette,
    ));
    rows.push(page_kit::form_row(
        lang.text(Msg::SortingSuggestRuleIf),
        kit::paragraph(conditions_short(spec, lang, kinds), palette),
        None,
        palette,
    ));
    rows.push(page_kit::form_row(
        lang.text(Msg::SortingRulePattern),
        pattern_field(0, &spec.filing_pattern, palette),
        found
            .rule
            .example
            .as_ref()
            .map(|example| {
                page_kit::hint(
                    fill_template(
                        lang.text(Msg::SortingSchemeExample),
                        &[("{example}", example)],
                    ),
                    palette,
                )
            }),
        palette,
    ));
    if !found.also.is_empty() {
        rows.push(page_kit::hint(
            fill_template(
                lang.text(Msg::SortingSuggestRuleAlso),
                &[("{count}", &found.also.len().to_string())],
            ),
            palette,
        ));
        rows.push(kit::dim(also_names(found), palette));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use ncrs_index::{
        Condition, ConditionKind, DocumentFields, RuleConditions, RuleSpec, TargetId,
    };

    fn spec(word: &str) -> RuleSpec {
        RuleSpec {
            name: None,
            conditions: RuleConditions::all_of(vec![Condition::new(ConditionKind::Word, word)]),
            extract_number: None,
            extract_date: None,
            extract_amount: None,
            kind: None,
            tags: std::collections::BTreeSet::new(),
            filing_pattern: "Schule/{year}/".to_owned(),
            confirmed: false,
            enabled: true,
        }
    }

    fn board(rules_existing: usize) -> Suggestions {
        Suggestions {
            files_read: 212,
            sources_read: 3,
            rules_existing,
            fit_suggested: 187,
            fit_none: 25,
            items: (0..7)
                .map(|at| Suggested {
                    name: format!("Regel {at}"),
                    spec: spec("elternbrief"),
                    example: Some("Schule/2026/".to_owned()),
                    files: 12,
                    group_documents: 12,
                    elsewhere: 0,
                })
                .collect(),
            model: ModelNote::Used,
        }
    }

    #[test]
    fn the_scheme_reads_what_was_looked_at_in_both_languages() {
        assert_eq!(
            scheme_intro(&board(0), Language::German),
            "Gelesen: 212 Dateien in 3 Quellen. Davon passen 187 in diese 7 Regeln, 25 in keine."
        );
        assert_eq!(
            scheme_intro(&board(0), Language::English),
            "Read: 212 files in 3 sources. 187 match these 7 rules, 25 match none."
        );
    }

    #[test]
    fn a_target_with_rules_names_what_they_leave_out() {
        assert_eq!(
            scheme_intro(&board(8), Language::German),
            "8 Regeln sind da. 7 Ordner im Ziel deckt keine Regel ab:"
        );
    }

    #[test]
    fn the_facts_of_a_file_are_listed_without_the_missing_ones() {
        let found = RuleForFile {
            target: TargetId::main(),
            rule: board(0).items.remove(0),
            fields: DocumentFields {
                sender: Some("Stadtwerke".to_owned()),
                kind: None,
                date: chrono::NaiveDate::from_ymd_opt(2026, 7, 14),
                ..DocumentFields::default()
            },
            also: Vec::new(),
            model: ModelNote::NotAvailable,
        };
        assert_eq!(facts_text(&found), "Stadtwerke · 14.07.2026");
    }

    #[test]
    fn every_phase_builds_a_dialog() {
        let palette = Palette::default();
        let window = Size::new(1200.0, 760.0);
        let mut state = Suggest::default();
        for lang in [Language::German, Language::English] {
            drop(dialog(&state, None, &KindCatalog::default(), window, lang, &palette));
            state.failed(ncrs_filing::sorting::SuggestError::Index("index missing".to_owned()));
            drop(dialog(&state, None, &KindCatalog::default(), window, lang, &palette));
            state.loaded(crate::suggest::Found::Target(board(0)));
            drop(dialog(&state, None, &KindCatalog::default(), window, lang, &palette));
            state.loaded(crate::suggest::Found::Target(Suggestions {
                items: Vec::new(),
                ..board(0)
            }));
            drop(dialog(&state, None, &KindCatalog::default(), window, lang, &palette));
            state = Suggest::default();
        }
    }
}
