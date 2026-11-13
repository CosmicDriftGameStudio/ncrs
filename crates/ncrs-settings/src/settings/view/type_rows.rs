//! "F3 by file type" on the programs page: one row for each file type, and the
//! button that adds one.

use iced::widget::{column, container, row};
use iced::{alignment, Length};

use super::*;
use crate::settings::{TypeRow, TypeRuleEdit, WithKind};
use ncrs_i18n::Msg;
use ncrs_ui_kit::kit::{self, Tone};
use ncrs_ui_kit::theme::spacing;

/// The width of the field that takes the extensions.
const EXTENSIONS_FIELD_WIDTH: f32 = 130.0;

fn rule_message(edit: TypeRuleEdit) -> Message {
    SettingsMessage::TypeRule(edit)
}

pub fn by_type_group<'a>(view: &SettingsView<'a>) -> Group<'a> {
    let lang = view.lang;
    let mut rows: Vec<SettingRow<'a>> = view
        .state
        .type_rows
        .iter()
        .enumerate()
        .map(|(index, each)| type_row(index, each, view))
        .collect();
    rows.push(SettingRow {
        in_command_list: true,
        ..SettingRow::text(
            String::new(),
            None,
            kit::button(
                lang.text(Msg::SettingsByTypeAdd),
                Some(rule_message(TypeRuleEdit::Add)),
                Tone::Secondary,
                view.palette,
            ),
        )
    });
    Group {
        intro: Some(lang.text(Msg::SettingsByTypeIntro).to_owned()),
        title: lang.text(Msg::SettingsByTypeTitle).to_owned(),
        rows,
    }
}

fn type_row<'a>(index: usize, each: &TypeRow, view: &SettingsView<'a>) -> SettingRow<'a> {
    let lang = view.lang;
    let palette = view.palette;
    let typing = |on_input: fn(usize, String) -> Message| move |text| on_input(index, text);
    let extensions = kit::mono_text_field(
        lang.text(Msg::SettingsByTypeExtensionsPlaceholder),
        &each.extensions,
        format!("settings-type-extensions-{index}"),
        Some(typing(|row, text| {
            rule_message(TypeRuleEdit::Extensions { row, text })
        })),
        rule_message(TypeRuleEdit::Commit(index)),
        palette,
    );
    let ways = [
        (Msg::SettingsByTypeInternal, WithKind::Internal),
        (Msg::SettingsByTypeSystem, WithKind::System),
        (Msg::SettingsByTypeProgram, WithKind::Program),
    ];
    let options: Vec<(&str, Message)> = ways
        .iter()
        .map(|&(label, with)| {
            (
                lang.text(label),
                rule_message(TypeRuleEdit::With { row: index, with }),
            )
        })
        .collect();
    let chosen = ways
        .iter()
        .position(|&(_, with)| with == each.with)
        .unwrap_or_default();
    let first_line = row![
        container(extensions).width(Length::Fixed(EXTENSIONS_FIELD_WIDTH)),
        kit::segmented(&options, chosen, true, palette),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);

    let mut second_line = row![]
        .spacing(spacing::M)
        .align_y(alignment::Vertical::Center);
    if each.with == WithKind::Program {
        second_line = second_line
            .push(kit::mono_text_field(
                lang.text(Msg::SettingsProgramPlaceholder),
                &each.program,
                format!("settings-type-program-{index}"),
                (!view.state.sandboxed).then_some(typing(|row, text| {
                    rule_message(TypeRuleEdit::Program { row, text })
                })),
                rule_message(TypeRuleEdit::Commit(index)),
                palette,
            ))
            .push(kit::button(
                lang.text(Msg::SettingsByTypeProgramPick),
                Some(rule_message(TypeRuleEdit::Pick(index))),
                Tone::Secondary,
                palette,
            ));
    } else {
        let hint = match each.with {
            WithKind::Internal => Msg::SettingsByTypeProgramHintInternal,
            _ => Msg::SettingsByTypeProgramHintSystem,
        };
        second_line =
            second_line.push(container(kit::dim(lang.text(hint), palette)).width(Length::Fill));
    }
    second_line = second_line.push(kit::button(
        lang.text(Msg::SettingsByTypeRemove),
        Some(rule_message(TypeRuleEdit::Remove(index))),
        Tone::Secondary,
        palette,
    ));

    SettingRow {
        problem: each
            .problem
            .as_ref()
            .map(|problem| problem_text(problem, lang)),
        in_command_list: false,
        ..SettingRow::text(
            lang.text(Msg::SettingsByTypeExtensions).to_owned(),
            None,
            column![first_line, second_line]
                .spacing(spacing::XS)
                .width(Length::Fill)
                .into(),
        )
    }
}
