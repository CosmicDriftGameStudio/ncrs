//! The page for sorting the document folders again, a tab: the plan as a tree
//! of groups and entries, and right of it the current and the proposed path of
//! the selected entry with the reason in plain text. Pure view over
//! [`ReorganisationView`]; nothing here touches a file.

use std::path::{Component, Path};

use iced::widget::{container, mouse_area, Row};
use iced::{alignment, Element, Length};
use ncrs_index::{ConflictReason, Finding, NoActionReason, PlanOutcome};

use super::format;
use super::hermes::{self, Badges};
use super::hermes_kit as page_kit;
use super::kit;
use crate::app::fill_template;
use crate::filing_pieces::FilingTarget;
use crate::i18n::{FuturamaName, Language, Msg};
use crate::messages::{HermesPage, Message, ReorganisationPlanMessage};
use crate::palette::Palette;
use crate::reorganisation_plan::{target, Group, Line, ReorganisationView, VISIBLE_LINES};
use crate::reorganisation_work::{deciding_finding, PlannedItem};

/// What fits a line of the list, in characters.
const FILE_NAME_CHARS: usize = 26;
/// What fits a line of the detail, in characters.
const PATH_CHARS: usize = 80;
const REASON_CHARS: usize = 90;
/// Steps of a group the detail lists before it only counts the rest.
const GROUP_STEPS_SHOWN: usize = 20;

fn send(message: ReorganisationPlanMessage) -> Message {
    Message::ReorganisationPlan(message)
}

pub fn view<'a>(
    state: &'a ReorganisationView,
    badges: Badges,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let body: Element<'a, Message> = if state.lines().is_empty() {
        empty(state, lang, palette)
    } else {
        page_kit::list_and_detail(list(state, lang, palette), detail(state, lang, palette))
    };
    page_kit::tab_page(
        hermes::tab_nav(HermesPage::Reorganisation, badges, lang, palette),
        header(state, lang, palette),
        body,
        (!state.lines().is_empty()).then(|| action_bar(state, lang, palette)),
        palette,
    )
}

fn header<'a>(
    state: &ReorganisationView,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    page_kit::page_header(
        lang.text(Msg::ActionReorganisationPlan),
        lang.futurama_name(FuturamaName::ActionReorganisationPlan),
        Some(lang.text(Msg::ReorganisationPlanExplain).to_owned()),
        None,
        vec![page_kit::secondary(
            lang.text(Msg::ReorganisationPlanReplan),
            (!state.is_busy()).then(|| send(ReorganisationPlanMessage::Replan)),
            palette,
        )],
        palette,
    )
}

fn empty<'a>(
    state: &ReorganisationView,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut lines = iced::widget::Column::new().spacing(page_kit::GAP_S);
    lines = lines.push(page_kit::hint(
        lang.text(if state.is_busy() {
            Msg::ReorganisationPlanPlanning
        } else {
            Msg::ReorganisationPlanEmpty
        })
        .to_owned(),
        palette,
    ));
    if let Some(problem) = problem(state, lang, palette) {
        lines = lines.push(problem);
    }
    page_kit::centered_note(lines.into())
}

fn problem<'a>(
    state: &ReorganisationView,
    lang: Language,
    palette: &Palette,
) -> Option<Element<'a, Message>> {
    state.problem().map(|detail| {
        page_kit::error_line(
            fill_template(
                lang.text(Msg::ReorganisationPlanProblemFailed),
                &[("{detail}", detail)],
            ),
            palette,
        )
    })
}

fn group_name(group: Group) -> Msg {
    match group {
        Group::Duplicate => Msg::ReorganisationPlanGroupDuplicate,
        Group::Misplaced => Msg::ReorganisationPlanGroupMisplaced,
        Group::YearMismatch => Msg::ReorganisationPlanGroupYear,
        Group::NameOutlier => Msg::ReorganisationPlanGroupName,
        Group::Blocked => Msg::ReorganisationPlanGroupBlocked,
    }
}

fn group_explanation(group: Group) -> Msg {
    match group {
        Group::Duplicate => Msg::ReorganisationPlanGroupExplainDuplicate,
        Group::Misplaced => Msg::ReorganisationPlanGroupExplainMisplaced,
        Group::YearMismatch => Msg::ReorganisationPlanGroupExplainYear,
        Group::NameOutlier => Msg::ReorganisationPlanGroupExplainName,
        Group::Blocked => Msg::ReorganisationPlanGroupExplainBlocked,
    }
}

/// How much of a group is chosen; `None` where nothing in it can be.
fn group_check(selected: usize, selectable: usize) -> Option<kit::Check> {
    if selectable == 0 {
        None
    } else if selected == 0 {
        Some(kit::Check::Off)
    } else if selected == selectable {
        Some(kit::Check::On)
    } else {
        Some(kit::Check::Mixed)
    }
}

/// The path an entry would get: its target, or the one that was in the way.
fn proposed_path(planned: &PlannedItem) -> Option<&Path> {
    match &planned.item.outcome {
        PlanOutcome::Move { target } | PlanOutcome::Rename { target } => Some(target),
        PlanOutcome::Conflict { target, .. } => target.as_deref(),
        PlanOutcome::NoAction(_) => None,
    }
}

/// The proposed path of an entry as text.
fn proposed_text(planned: &PlannedItem) -> String {
    proposed_path(planned)
        .map(|proposed| proposed.display().to_string())
        .unwrap_or_default()
}

fn file_name(planned: &PlannedItem) -> String {
    planned.item.source.file_name().map_or_else(
        || planned.item.source.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn action_text(planned: &PlannedItem, lang: Language) -> String {
    match &planned.item.outcome {
        PlanOutcome::Move { .. } => lang.text(Msg::ReorganisationPlanActionMove).to_owned(),
        PlanOutcome::Rename { .. } => lang.text(Msg::ReorganisationPlanActionRename).to_owned(),
        PlanOutcome::Conflict { .. } | PlanOutcome::NoAction(_) => "—".to_owned(),
    }
}

/// A path as parts for [`page_kit::path`]: the folders and the name that are
/// not the ones of `other` at the same place, counted from the start or from
/// the end, are in the accent. From the end too, so that a folder put in front
/// (`duplicates/`) does not mark the whole path.
fn path_parts(path: &Path, other: Option<&Path>) -> Vec<(String, bool)> {
    let names = |from: &Path| -> Vec<String> {
        from.components()
            .filter_map(|part| match part {
                Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
                Component::Prefix(_)
                | Component::RootDir
                | Component::CurDir
                | Component::ParentDir => None,
            })
            .collect()
    };
    let own = names(path);
    let compared = other.map(names).unwrap_or_default();
    let own_len = own.len();
    let last = own_len.saturating_sub(1);
    let mut parts: Vec<(String, bool)> = Vec::new();
    for (position, name) in own.into_iter().enumerate() {
        let same_from_end = compared
            .len()
            .checked_sub(own_len.saturating_sub(position))
            .and_then(|index| compared.get(index))
            == Some(&name);
        let differs = compared.get(position) != Some(&name) && !same_from_end;
        let piece = if position == last {
            name
        } else {
            format!("{name}/")
        };
        match parts.last_mut() {
            Some((text, accent)) if *accent == differs => text.push_str(&piece),
            _ => parts.push((piece, differs)),
        }
    }
    parts
}

/// Why the entry is in the plan, or why it cannot be run.
pub fn reason_text(planned: &PlannedItem, lang: Language) -> String {
    match &planned.item.outcome {
        PlanOutcome::Conflict { reason, .. } => lang
            .text(match reason {
                ConflictReason::TargetExists => Msg::ReorganisationPlanReasonTargetExists,
                ConflictReason::TargetTakenByOtherItem => Msg::ReorganisationPlanReasonTargetTaken,
                ConflictReason::TargetLeavesLocation => Msg::ReorganisationPlanReasonTargetLeaves,
            })
            .to_owned(),
        PlanOutcome::NoAction(_) => lang
            .text(Msg::ReorganisationPlanReasonMissingFields)
            .to_owned(),
        PlanOutcome::Move { .. } | PlanOutcome::Rename { .. } => {
            match deciding_finding(&planned.item) {
                Some(Finding::Duplicate { keeper }) => fill_template(
                    lang.text(Msg::ReorganisationPlanReasonDuplicate),
                    &[("{keeper}", &keeper.display().to_string())],
                ),
                Some(Finding::Misplaced { .. }) => lang
                    .text(if planned.unconfirmed_rule() {
                        Msg::ReorganisationPlanReasonMisplacedUnconfirmed
                    } else {
                        Msg::ReorganisationPlanReasonMisplaced
                    })
                    .to_owned(),
                Some(Finding::YearMismatch {
                    folder_year,
                    date_year,
                }) => fill_template(
                    lang.text(Msg::ReorganisationPlanReasonYear),
                    &[
                        ("{folder_year}", &folder_year.to_string()),
                        ("{date_year}", &date_year.to_string()),
                    ],
                ),
                Some(Finding::NameOutlier { pattern }) => fill_template(
                    lang.text(Msg::ReorganisationPlanReasonName),
                    &[("{pattern}", &FilingTarget::parse(pattern).in_words(lang))],
                ),
                None => String::new(),
            }
        }
    }
}

/// One sentence that explains the entry: what was found and what the plan
/// does about it, or why it cannot be run.
pub fn finding_sentence(planned: &PlannedItem, lang: Language) -> String {
    let target = proposed_text(planned);
    let target_slot = [("{target}", target.as_str())];
    match &planned.item.outcome {
        PlanOutcome::Conflict { reason, .. } => match reason {
            ConflictReason::TargetExists => fill_template(
                lang.text(Msg::ReorganisationPlanFindingTargetExists),
                &target_slot,
            ),
            ConflictReason::TargetTakenByOtherItem => fill_template(
                lang.text(Msg::ReorganisationPlanFindingTargetTaken),
                &target_slot,
            ),
            ConflictReason::TargetLeavesLocation => lang
                .text(Msg::ReorganisationPlanFindingTargetLeaves)
                .to_owned(),
        },
        PlanOutcome::NoAction(NoActionReason::MissingFields) => lang
            .text(Msg::ReorganisationPlanFindingMissingFields)
            .to_owned(),
        PlanOutcome::Move { .. } | PlanOutcome::Rename { .. } => {
            match deciding_finding(&planned.item) {
                Some(Finding::Duplicate { keeper }) => fill_template(
                    lang.text(Msg::ReorganisationPlanFindingDuplicate),
                    &[
                        ("{keeper}", &keeper.display().to_string()),
                        ("{target}", &target),
                    ],
                ),
                Some(Finding::Misplaced { .. }) => match &planned.rule {
                    Some(rule) => fill_template(
                        lang.text(if rule.confirmed {
                            Msg::ReorganisationPlanFindingMisplaced
                        } else {
                            Msg::ReorganisationPlanFindingMisplacedUnconfirmed
                        }),
                        &[("{rule}", &rule.label), ("{target}", &target)],
                    ),
                    None => fill_template(
                        lang.text(Msg::ReorganisationPlanFindingMisplacedUnknown),
                        &target_slot,
                    ),
                },
                Some(Finding::YearMismatch {
                    folder_year,
                    date_year,
                }) => match planned.date {
                    Some(date) => fill_template(
                        lang.text(Msg::ReorganisationPlanFindingYearDated),
                        &[
                            ("{date}", &date.to_string()),
                            ("{folder_year}", &folder_year.to_string()),
                        ],
                    ),
                    None => fill_template(
                        lang.text(Msg::ReorganisationPlanFindingYearUndated),
                        &[
                            ("{date_year}", &date_year.to_string()),
                            ("{folder_year}", &folder_year.to_string()),
                        ],
                    ),
                },
                Some(Finding::NameOutlier { pattern }) => fill_template(
                    lang.text(Msg::ReorganisationPlanFindingName),
                    &[
                        ("{pattern}", &FilingTarget::parse(pattern).in_words(lang)),
                        ("{target}", &target),
                    ],
                ),
                None => String::new(),
            }
        }
    }
}

fn list<'a>(
    state: &'a ReorganisationView,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let first = state.first_visible();
    let rows = state
        .lines()
        .iter()
        .enumerate()
        .skip(first)
        .take(VISIBLE_LINES)
        .filter_map(|(position, line)| {
            let on_cursor = position == state.cursor();
            let press = Some(send(ReorganisationPlanMessage::Select(position)));
            match *line {
                Line::Header(group) => {
                    let (selected, selectable) = state.counts(group);
                    let right = (selectable > 0).then(|| {
                        fill_template(
                            lang.text(Msg::ReorganisationPlanGroupCount),
                            &[
                                ("{selected}", &selected.to_string()),
                                ("{count}", &selectable.to_string()),
                            ],
                        )
                    });
                    let content = check_and(
                        group_check(selected, selectable),
                        position,
                        page_kit::value(lang.text(group_name(group)).to_owned(), palette),
                        palette,
                    );
                    Some(page_kit::list_row_one(
                        content, right, false, on_cursor, press, palette,
                    ))
                }
                Line::Item(index) => {
                    let planned = state.rows().get(index)?;
                    let own_check = target(planned).is_some().then(|| {
                        if state.is_selected(index) {
                            kit::Check::On
                        } else {
                            kit::Check::Off
                        }
                    });
                    let content = check_and(
                        own_check,
                        position,
                        page_kit::mono(
                            format::ellipsize_end(&file_name(planned), FILE_NAME_CHARS),
                            page_kit::TEXT_SMALL,
                            palette.text,
                        ),
                        palette,
                    );
                    Some(page_kit::list_row_one(
                        content,
                        Some(action_text(planned, lang)),
                        true,
                        on_cursor,
                        press,
                        palette,
                    ))
                }
            }
        })
        .collect();
    page_kit::list_pane(
        rows,
        |delta| send(ReorganisationPlanMessage::Scrolled(delta)),
        palette,
    )
}

/// The box of a line, which a press on toggles, before its content. A line
/// that cannot be chosen gets a box that takes no press.
fn check_and<'a>(
    check: Option<kit::Check>,
    position: usize,
    content: Element<'a, Message>,
    palette: &Palette,
) -> Element<'a, Message> {
    let toggled = check.is_some();
    let state = check.unwrap_or(kit::Check::Off);
    let boxed: Element<'a, Message> = if toggled {
        mouse_area(
            container(kit::checkbox(state, None, palette))
                .height(Length::Fill)
                .center_y(Length::Fill),
        )
        .on_press(send(ReorganisationPlanMessage::Toggle(position)))
        .into()
    } else {
        container(kit::checkbox(state, None, palette))
            .height(Length::Fill)
            .center_y(Length::Fill)
            .into()
    };
    Row::new()
        .push(boxed)
        .push(content)
        .spacing(page_kit::GAP_S)
        .align_y(alignment::Vertical::Center)
        .into()
}

fn detail<'a>(
    state: &'a ReorganisationView,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut children: Vec<Element<'a, Message>> = Vec::new();
    match state.lines().get(state.cursor()) {
        Some(&Line::Item(index)) => {
            if let Some(planned) = state.rows().get(index) {
                children.extend(entry_rows(planned, lang, palette));
            }
        }
        Some(&Line::Header(group)) => {
            children.push(page_kit::value(
                lang.text(group_name(group)).to_owned(),
                palette,
            ));
            children.push(page_kit::hint(
                lang.text(group_explanation(group)).to_owned(),
                palette,
            ));
            if group == Group::Blocked {
                children.extend(blocked_rows(state, lang, palette));
            } else {
                children.extend(group_step_rows(state, group, lang, palette));
            }
        }
        None => {}
    }
    if let Some(line) = problem(state, lang, palette) {
        children.push(line);
    }
    page_kit::detail(children)
}

fn entry_rows<'a>(
    planned: &PlannedItem,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    let mut rows = vec![page_kit::mono(
        format::ellipsize_end(&file_name(planned), PATH_CHARS),
        page_kit::TEXT_BODY,
        palette.text,
    )];
    rows.extend(now_and_proposed_rows(planned, lang, palette));
    rows.push(page_kit::form_row(
        lang.text(Msg::ReorganisationPlanColumnReason),
        page_kit::sentence(reason_text(planned, lang), palette),
        None,
        palette,
    ));
    let sentence = finding_sentence(planned, lang);
    let finding = if Group::of(planned) == Group::Blocked {
        page_kit::error_line(sentence, palette)
    } else {
        page_kit::hint(sentence, palette)
    };
    let mut why = Row::new()
        .push(container(finding).width(Length::Fill))
        .spacing(page_kit::GAP_S)
        .align_y(alignment::Vertical::Center);
    if let Some(rule) = &planned.rule {
        why = why.push(page_kit::link(
            lang.text(if rule.confirmed {
                Msg::ReorganisationPlanShowRule
            } else {
                Msg::ReorganisationPlanShowSuggestion
            }),
            Some(send(ReorganisationPlanMessage::OpenRule(rule.id))),
            palette,
        ));
    }
    rows.push(page_kit::form_row("", why.into(), None, palette));
    rows
}

/// The current path of an entry and, where it has one, the proposed path.
fn now_and_proposed_rows<'a>(
    planned: &PlannedItem,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    let proposed_target = proposed_path(planned);
    let mut rows = vec![page_kit::form_row(
        lang.text(Msg::ReorganisationPlanColumnCurrent),
        page_kit::path(path_parts(&planned.item.source, proposed_target), palette),
        None,
        palette,
    )];
    if let Some(proposed) = proposed_target {
        rows.push(page_kit::form_row(
            lang.text(Msg::ReorganisationPlanColumnProposed),
            page_kit::path(path_parts(proposed, Some(&planned.item.source)), palette),
            None,
            palette,
        ));
    }
    rows
}

/// The steps of a group in the order of the list, as many as fit.
fn group_step_rows<'a>(
    state: &ReorganisationView,
    group: Group,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    let steps: Vec<&PlannedItem> = state
        .rows()
        .iter()
        .filter(|planned| Group::of(planned) == group)
        .collect();
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    for planned in steps.iter().take(GROUP_STEPS_SHOWN) {
        rows.push(page_kit::mono(
            format::ellipsize_end(&file_name(planned), PATH_CHARS),
            page_kit::TEXT_SMALL,
            palette.text,
        ));
        rows.extend(now_and_proposed_rows(planned, lang, palette));
    }
    if steps.len() > GROUP_STEPS_SHOWN {
        rows.push(page_kit::hint(
            fill_template(
                lang.text(Msg::ReorganisationPlanMoreSteps),
                &[("{count}", &(steps.len() - GROUP_STEPS_SHOWN).to_string())],
            ),
            palette,
        ));
    }
    rows
}

/// What cannot be run, one line each with the reason.
fn blocked_rows<'a>(
    state: &ReorganisationView,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    state
        .rows()
        .iter()
        .filter(|planned| Group::of(planned) == Group::Blocked)
        .map(|planned| {
            page_kit::error_line(
                format::ellipsize_end(
                    &format!("{}: {}", file_name(planned), reason_text(planned, lang)),
                    REASON_CHARS,
                ),
                palette,
            )
        })
        .collect()
}

fn action_bar<'a>(
    state: &ReorganisationView,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut summary = fill_template(
        lang.text(Msg::ReorganisationPlanGroupCount),
        &[
            ("{selected}", &state.selected_count().to_string()),
            ("{count}", &state.runnable_count().to_string()),
        ],
    );
    let blocked = state.blocked_count();
    if blocked > 0 {
        summary.push_str(" · ");
        summary.push_str(&fill_template(
            lang.text(Msg::ReorganisationPlanCannotRun),
            &[("{count}", &blocked.to_string())],
        ));
    }
    let runnable = state.has_selection() && !state.is_busy();
    page_kit::action_bar(
        Some(summary),
        vec![page_kit::primary(
            lang.text(Msg::ReorganisationPlanExecute),
            Some(kit::ENTER_KEY),
            runnable.then(|| Message::ExecuteReorganisation(state.selection())),
            palette,
        )],
        palette,
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::reorganisation_work::tests::{item, sample_items};

    fn sentence(source: &str) -> String {
        finding_sentence(item(&sample_items(), source), Language::English)
    }

    #[test]
    fn a_confirmed_rule_is_named_with_its_target() {
        let sentence = sentence("Insurance/2024/2024-07-01_invoice.pdf");

        assert!(sentence.starts_with("Rule ‘tax notice’ files documents like this one under "));
        let folder = Path::new("Taxes").join("2024").display().to_string();
        assert!(sentence.contains(&folder), "{sentence}");
    }

    #[test]
    fn a_suggested_rule_is_named_and_said_to_be_unconfirmed() {
        let sentence = sentence("Insurance/2024/2024-08-01_invoice.pdf");

        assert!(sentence.starts_with("The suggested rule ‘late fee’ would file this under "));
        let folder = Path::new("Fees").join("2024").display().to_string();
        assert!(sentence.contains(&folder), "{sentence}");
        assert!(sentence.ends_with("it is not confirmed yet."), "{sentence}");
    }

    #[test]
    fn a_rule_that_is_gone_is_described_without_its_name() {
        let mut planned = item(&sample_items(), "Insurance/2024/2024-07-01_invoice.pdf").clone();
        planned.rule = None;

        let sentence = finding_sentence(&planned, Language::English);

        assert!(sentence.starts_with("A rule that is no longer there would file this under "));
    }

    #[test]
    fn a_wrong_year_names_the_date_and_the_folder() {
        assert_eq!(
            sentence("Insurance/2023/2024-05-01_invoice.pdf"),
            "Dated 2024-05-01, but it lies in the folder 2023."
        );
    }

    #[test]
    fn a_wrong_year_without_a_known_date_names_the_year() {
        let mut planned = item(&sample_items(), "Insurance/2023/2024-05-01_invoice.pdf").clone();
        planned.date = None;

        assert_eq!(
            finding_sentence(&planned, Language::English),
            "Dated in 2024, but it lies in the folder 2023."
        );
    }

    #[test]
    fn a_copy_names_the_file_that_stays_and_where_the_copy_goes() {
        let planned = item(&sample_items(), "Misc/2024-04-01_invoice.pdf").clone();
        let target = proposed_text(&planned);

        let sentence = finding_sentence(&planned, Language::English);

        assert!(
            sentence
                .starts_with("Same content as Insurance/2024/2024-04-01_invoice.pdf, which stays;"),
            "{sentence}"
        );
        assert!(
            sentence.ends_with(&format!("this copy moves to {target}.")),
            "{sentence}"
        );
    }

    #[test]
    fn an_odd_name_names_the_pattern_and_the_new_name() {
        let planned = item(&sample_items(), "Insurance/2024/scan 0042.pdf").clone();
        let target = proposed_text(&planned);

        let sentence = finding_sentence(&planned, Language::English);

        assert!(
            sentence.starts_with("The name does not follow the pattern "),
            "{sentence}"
        );
        assert!(
            sentence.ends_with(&format!("it becomes {target}.")),
            "{sentence}"
        );
    }

    #[test]
    fn a_target_that_exists_says_which() {
        let planned = item(&sample_items(), "Insurance/2023/2024-06-01_invoice.pdf").clone();
        let target = proposed_text(&planned);

        assert!(!target.is_empty());
        assert_eq!(
            finding_sentence(&planned, Language::English),
            format!("{target} exists already, so this entry cannot be run.")
        );
    }

    #[test]
    fn a_target_wanted_twice_and_one_outside_the_folder_are_explained() {
        let mut planned = item(&sample_items(), "Insurance/2023/2024-06-01_invoice.pdf").clone();
        planned.item.outcome = PlanOutcome::Conflict {
            target: Some(PathBuf::from("Taxes/2024/x.pdf")),
            reason: ConflictReason::TargetTakenByOtherItem,
        };
        assert_eq!(
            finding_sentence(&planned, Language::English),
            "Another entry of this plan wants Taxes/2024/x.pdf as well, so this one cannot be run."
        );

        planned.item.outcome = PlanOutcome::Conflict {
            target: None,
            reason: ConflictReason::TargetLeavesLocation,
        };
        assert_eq!(
            finding_sentence(&planned, Language::English),
            "The proposed path would lie outside the document folder, so this entry cannot be run."
        );
    }

    #[test]
    fn missing_fields_are_explained_with_the_date_as_example() {
        let items = sample_items();
        let planned = item(&items, "Insurance/2024/IMG_7.pdf");
        assert!(matches!(planned.item.outcome, PlanOutcome::NoAction(_)));

        let sentence = finding_sentence(planned, Language::English);

        assert!(
            sentence.contains("unknown for this file, such as its date"),
            "{sentence}"
        );
    }

    #[test]
    fn the_sentences_come_in_german_too() {
        let items = sample_items();

        let rule = finding_sentence(
            item(&items, "Insurance/2024/2024-07-01_invoice.pdf"),
            Language::German,
        );
        let year = finding_sentence(
            item(&items, "Insurance/2023/2024-05-01_invoice.pdf"),
            Language::German,
        );

        assert!(
            rule.starts_with("Die Regel ‘tax notice’ legt solche Dokumente unter "),
            "{rule}"
        );
        assert_eq!(year, "Datiert auf 2024-05-01, liegt aber im Ordner 2023.");
        assert!(
            Path::new(&proposed_text(item(&items, "Misc/2024-04-01_invoice.pdf"))).is_relative()
        );
    }

    #[test]
    fn only_the_parts_that_differ_are_marked() {
        let current = Path::new("Misc").join("a.pdf");
        let copy = Path::new("duplicates").join("Misc").join("a.pdf");
        let moved = Path::new("Taxes").join("2024").join("a.pdf");
        let filed = Path::new("Insurance").join("2024").join("a.pdf");

        assert_eq!(
            path_parts(&copy, Some(&current)),
            vec![
                ("duplicates/".to_owned(), true),
                ("Misc/a.pdf".to_owned(), false)
            ]
        );
        assert_eq!(
            path_parts(&current, Some(&copy)),
            vec![("Misc/a.pdf".to_owned(), false)]
        );
        assert_eq!(
            path_parts(&moved, Some(&filed)),
            vec![
                ("Taxes/".to_owned(), true),
                ("2024/a.pdf".to_owned(), false)
            ]
        );
    }
}
