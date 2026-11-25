//! The group "Targets and their rules" of the page Einsortieren: a folding
//! head per target and, unfolded, the rules of the target in the order they
//! apply.

use std::path::Path;

use iced::widget::container;
use ncrs_config::config::{Filing, TargetSetting};
use ncrs_config::palette::Palette;
use ncrs_filing::rules_work::ListedRule;
use ncrs_filing::sorting::{TargetState, WaitReason};
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_ui_kit::format::shown_path;
use ncrs_ui_kit::hermes_kit as page_kit;
use ncrs_ui_kit::kit::{Check, Tone};
use ncrs_ui_kit::setting_rows::{Group, RowButton, RuleCells, SettingRow};

use crate::message::{Message, RulesMessage};
use crate::rules_page::{conditions_short, hits_text, rule_title, RulesPage};
use crate::target_dialog::{target_name, TargetsMessage};

fn send(message: TargetsMessage) -> Message {
    Message::Targets(message)
}

fn send_rule(message: RulesMessage) -> Message {
    Message::Rules(message)
}

pub fn group<'a>(
    rules: Option<&RulesPage>,
    filing: &Filing,
    home: &Path,
    lang: Language,
    palette: &Palette,
) -> Group<'a, Message> {
    let mut rows: Vec<SettingRow<'a, Message>> = Vec::new();
    for (index, setting) in filing.targets.iter().enumerate() {
        rows.extend(target_rows(
            index, setting, rules, filing, home, lang, palette,
        ));
    }
    if rows.is_empty() {
        rows.push(SettingRow::text(
            lang.text(Msg::SortingTargetsEmpty).to_owned(),
            None,
            container("").into(),
        ));
    }
    Group::headed(
        lang.text(Msg::SortingTargetsTitle),
        Some(lang.text(Msg::SortingTargetsNote)),
        vec![RowButton::new(
            lang.text(Msg::SortingAddTarget),
            Some(send(TargetsMessage::Add)),
            Tone::Secondary,
        )],
        rows,
        palette,
    )
}

fn target_rows<'a>(
    index: usize,
    setting: &TargetSetting,
    rules: Option<&RulesPage>,
    filing: &Filing,
    home: &Path,
    lang: Language,
    palette: &Palette,
) -> Vec<SettingRow<'a, Message>> {
    let target = &setting.target;
    let own: Vec<(usize, &ListedRule)> = rules
        .map(|page| page.rows_of(&target.id).collect())
        .unwrap_or_default();
    let open = rules.is_some_and(|page| page.is_unfolded(&target.id));
    let loaded = rules.is_some_and(|page| !page.is_busy());
    let state = rules.and_then(|page| page.target_state(&target.id));
    let mut buttons = vec![
        RowButton::new(
            lang.text(Msg::SortingAddRule),
            rules.map(|_| send_rule(RulesMessage::NewIn(target.id.clone()))),
            Tone::Secondary,
        ),
        RowButton::new(
            lang.text(if own.is_empty() {
                Msg::SortingSuggestScheme
            } else {
                Msg::SortingSuggestRules
            }),
            Some(send(TargetsMessage::Suggest(target.id.clone()))),
            if own.is_empty() {
                Tone::Primary
            } else {
                Tone::Secondary
            },
        ),
    ];
    match state {
        Some(TargetState::Waiting(WaitReason::Unconfirmed)) => buttons.push(RowButton::new(
            lang.text(Msg::SortingConfirmTarget),
            Some(send(TargetsMessage::Confirm(index))),
            Tone::Primary,
        )),
        Some(TargetState::Waiting(
            WaitReason::Missing | WaitReason::NotMounted | WaitReason::NotAccessible,
        )) => buttons.push(RowButton::new(
            lang.text(Msg::SortingRepickFolder),
            Some(send(TargetsMessage::Change(index))),
            Tone::Secondary,
        )),
        _ => {}
    }
    buttons.push(RowButton::new(
        lang.text(Msg::SortingChange),
        Some(send(TargetsMessage::Change(index))),
        Tone::Secondary,
    ));
    buttons.push(RowButton::new(
        lang.text(Msg::SortingRemove),
        Some(send(TargetsMessage::AskRemove(index))),
        Tone::Secondary,
    ));
    let mut head = SettingRow::folding_head(
        open,
        page_kit::target_chip(index, &format!("T{}", index + 1), palette),
        target_name(target),
        Some(head_description(setting, filing, state, home, lang)),
        send(TargetsMessage::ToggleFold(target.id.clone())),
        buttons,
        palette,
    );
    if loaded && own.is_empty() {
        head.warning = Some(lang.text(Msg::SortingNoScheme).to_owned());
    }
    let mut rows = vec![head];
    if !open {
        return rows;
    }
    let last = own.len().saturating_sub(1);
    for (place, (at, listed)) in own.into_iter().enumerate() {
        rows.push(rule_row(
            at,
            (place > 0, place < last),
            listed,
            rules,
            lang,
            palette,
        ));
    }
    rows
}

fn state_text(state: Option<TargetState>, lang: Language) -> Option<&'static str> {
    let Some(TargetState::Waiting(reason)) = state else {
        return None;
    };
    Some(lang.text(match reason {
        WaitReason::Missing => Msg::SortingStateFolderMissing,
        WaitReason::NotMounted => Msg::SortingStateDriveMissing,
        WaitReason::NotAccessible => Msg::SortingStateUnreadable,
        WaitReason::Unconfirmed => Msg::SortingStateUnconfirmed,
        WaitReason::Paused => Msg::SortingStatePaused,
    }))
}

/// "~/Documents · from Downloads, Scans · state" or "… · no source yet".
fn head_description(
    setting: &TargetSetting,
    filing: &Filing,
    state: Option<TargetState>,
    home: &Path,
    lang: Language,
) -> String {
    let target = &setting.target;
    let path = shown_path(&target.path, home);
    let sources: Vec<String> = filing
        .sources
        .iter()
        .filter(|source| source.source.targets.contains(&target.id))
        .map(|source| {
            source
                .source
                .path
                .file_name()
                .map_or_else(String::new, |last| last.to_string_lossy().into_owned())
        })
        .collect();
    let line = if sources.is_empty() {
        fill_template(lang.text(Msg::SortingTargetNoSource), &[("{path}", &path)])
    } else {
        fill_template(
            lang.text(Msg::SortingTargetFrom),
            &[("{path}", &path), ("{sources}", &sources.join(", "))],
        )
    };
    match state_text(state, lang) {
        Some(waiting) => format!("{line} · {waiting}"),
        None => line,
    }
}

/// `at` is the place of the rule in the whole list; `(can_up, can_down)` is
/// whether it can move within its target.
fn rule_row<'a>(
    at: usize,
    (can_up, can_down): (bool, bool),
    listed: &ListedRule,
    rules: Option<&RulesPage>,
    lang: Language,
    palette: &Palette,
) -> SettingRow<'a, Message> {
    let spec = &listed.rule.spec;
    let idle = rules.is_some_and(|page| !page.is_busy());
    let cells = RuleCells {
        name: rules.map_or_else(String::new, |page| rule_title(spec, lang, page.kinds())),
        condition: rules.map_or_else(String::new, |page| {
            conditions_short(spec, lang, page.kinds())
        }),
        pattern: spec.filing_pattern.clone(),
        hits: if spec.enabled {
            hits_text(listed.hits, lang)
        } else {
            lang.text(Msg::RulesOff).to_owned()
        },
    };
    SettingRow::listed_rule(
        RowButton::check(
            if spec.enabled { Check::On } else { Check::Off },
            idle.then(|| send_rule(RulesMessage::ToggleAt(at))),
        ),
        &cells,
        vec![
            RowButton::new(
                "↑",
                (idle && can_up).then(|| {
                    send_rule(RulesMessage::ReorderAt {
                        index: at,
                        delta: -1,
                    })
                }),
                Tone::Secondary,
            ),
            RowButton::new(
                "↓",
                (idle && can_down).then(|| {
                    send_rule(RulesMessage::ReorderAt {
                        index: at,
                        delta: 1,
                    })
                }),
                Tone::Secondary,
            ),
            RowButton::new(
                lang.text(Msg::SortingChange),
                idle.then(|| send_rule(RulesMessage::Edit(at))),
                Tone::Secondary,
            ),
        ],
        palette,
    )
}

#[cfg(test)]
// A failing assertion in a test is the signal, so `expect` belongs here; the
// lint is meant for the production paths.
mod tests {
    use std::path::PathBuf;

    use ncrs_index::TargetId;
    use ncrs_intake::SortTarget;
    use ncrs_ui_kit::setting_rows::{keyed_rows, RowKey};

    use super::*;
    use crate::rules_page::fixtures::page;

    #[test]
    fn a_rule_row_switches_moves_and_changes_the_rule_and_does_not_remove_it() {
        let rules = page();
        let filing = Filing {
            targets: vec![TargetSetting {
                target: SortTarget {
                    id: TargetId::main(),
                    name: "Documents".to_owned(),
                    path: PathBuf::from("/home/me/Documents"),
                    own_volume: None,
                },
                paused: None,
            }],
            ..Filing::default()
        };
        let keys = keyed_rows(
            vec![group(
                Some(&rules),
                &filing,
                Path::new("/home/me"),
                Language::English,
                &Palette::default(),
            )],
            |_| true,
        );

        let middle = keys
            .iter()
            .find_map(|key| match key {
                RowKey::Buttons(presses)
                    if presses.contains(&send_rule(RulesMessage::ToggleAt(1))) =>
                {
                    Some(presses)
                }
                _ => None,
            })
            .expect("the second rule has a row");

        assert_eq!(
            middle,
            &[
                send_rule(RulesMessage::ToggleAt(1)),
                send_rule(RulesMessage::ReorderAt {
                    index: 1,
                    delta: -1
                }),
                send_rule(RulesMessage::ReorderAt { index: 1, delta: 1 }),
                send_rule(RulesMessage::Edit(1)),
            ]
        );
    }
}
