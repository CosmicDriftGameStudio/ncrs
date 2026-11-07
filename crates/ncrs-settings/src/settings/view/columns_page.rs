//! The Columns page: which columns the panels show, in which order, and how
//! dates and sizes are written.

use iced::widget::Row;
use iced::Element;

use super::*;
use crate::message::ColumnsEdit;
use crate::settings::columns::{rows, ColumnRow};
use ncrs_config::panel_columns::PanelColumns;
use ncrs_core::{Column, DateFormat, PanelSide, SizeFormat};
use ncrs_ui_kit::choice_rows::{list_entry_row, ListEntry};
use ncrs_ui_kit::layout::SETTINGS_WIDE_CONTROL_WIDTH;

fn edit(columns_edit: ColumnsEdit) -> Message {
    SettingsMessage::Columns(columns_edit)
}

pub fn columns_groups<'a>(view: &SettingsView<'a>) -> Vec<Group<'a>> {
    let lang = view.lang;
    let columns = view.state.panel_columns();
    let side = view.state.edited_columns_side();
    vec![
        Group {
            intro: None,
            title: String::new(),
            rows: vec![panels_row(&columns, side, view)],
        },
        Group {
            intro: Some(lang.text(Msg::SettingsRowColumnsListNote).to_owned()),
            title: lang.text(Msg::SettingsRowColumnsList).to_owned(),
            rows: list_rows(&columns, side, view),
        },
        Group {
            intro: None,
            title: lang.text(Msg::SettingsColumnsFormats).to_owned(),
            rows: vec![
                format_row(
                    (Msg::SettingsRowDateFormat, Msg::SettingsRowDateFormatNote),
                    DateFormat::ALL.map(|format| (date_label(format), format)),
                    columns.date_format,
                    ColumnsEdit::Date,
                    view,
                ),
                format_row(
                    (Msg::SettingsRowSizeFormat, Msg::SettingsRowSizeFormatNote),
                    SizeFormat::ALL.map(|format| (size_label(format), format)),
                    columns.size_format,
                    ColumnsEdit::Size,
                    view,
                ),
            ],
        },
    ]
}

fn panels_row<'a>(
    columns: &PanelColumns,
    side: PanelSide,
    view: &SettingsView<'a>,
) -> SettingRow<'a> {
    let lang = view.lang;
    let palette = view.palette;
    let panels = [
        (
            lang.text(Msg::SettingsColumnsBothSame),
            edit(ColumnsEdit::SameInBoth(true)),
        ),
        (
            lang.text(Msg::SettingsColumnsPerPanel),
            edit(ColumnsEdit::SameInBoth(false)),
        ),
    ];
    let mut control = Row::new()
        .spacing(spacing::S)
        .align_y(iced::alignment::Vertical::Center)
        .push(kit::segmented(
            &panels,
            usize::from(!columns.same_in_both),
            true,
            palette,
        ));
    if !columns.same_in_both {
        let sides = [
            (
                lang.text(Msg::SettingsColumnsSideLeft),
                edit(ColumnsEdit::Side(PanelSide::Left)),
            ),
            (
                lang.text(Msg::SettingsColumnsSideRight),
                edit(ColumnsEdit::Side(PanelSide::Right)),
            ),
        ];
        control = control.push(kit::segmented(
            &sides,
            usize::from(side == PanelSide::Right),
            true,
            palette,
        ));
    }
    let panels_key = choice_key(
        vec![
            edit(ColumnsEdit::SameInBoth(true)),
            edit(ColumnsEdit::SameInBoth(false)),
        ],
        usize::from(!columns.same_in_both),
    );
    let mut row = simple_row(
        Msg::SettingsRowColumnsPanels,
        Some(Msg::SettingsRowColumnsPanelsNote),
        control.into(),
        lang,
    );
    row.control_width = Some(SETTINGS_WIDE_CONTROL_WIDTH);
    row.keyed(panels_key)
}

fn list_rows<'a>(
    columns: &PanelColumns,
    side: PanelSide,
    view: &SettingsView<'a>,
) -> Vec<SettingRow<'a>> {
    let lang = view.lang;
    let palette = view.palette;
    let listed = rows(columns, side);
    let shown_count = listed.iter().filter(|row| row.shown).count();
    let mut shown_before = 0;
    let mut entries: Vec<SettingRow<'a>> = Vec::with_capacity(listed.len() + 1);
    for ColumnRow { column, shown } in listed {
        let position = shown.then(|| {
            shown_before += 1;
            shown_before - 1
        });
        let entry = ListEntry {
            label: lang.text(column_label(column)).to_owned(),
            description: lang.text(column_description(column)).to_owned(),
            on: shown,
            always: (column == Column::Name).then(|| lang.text(Msg::ColumnMenuAlways).to_owned()),
            on_toggle: edit(ColumnsEdit::Toggle(column)),
            on_up: position
                .filter(|&at| at > 0)
                .map(|_| edit(ColumnsEdit::Up(column))),
            on_down: position
                .filter(|&at| at + 1 < shown_count)
                .map(|_| edit(ColumnsEdit::Down(column))),
        };
        entries.push(
            list_entry_row(entry, lang, palette)
                .keyed(RowKey::Press(edit(ColumnsEdit::Toggle(column)))),
        );
    }
    entries.push(
        SettingRow::adding(
            None,
            None,
            vec![kit::button(
                lang.text(Msg::ColumnMenuRestore),
                Some(edit(ColumnsEdit::Restore)),
                Tone::Secondary,
                palette,
            )],
        )
        .keyed(RowKey::Press(edit(ColumnsEdit::Restore))),
    );
    entries
}

/// A row with a segment of the three `formats`; `make` turns the chosen one
/// into the edit.
fn format_row<'a, F: Copy + PartialEq>(
    (label, note): (Msg, Msg),
    formats: [(Msg, F); 3],
    current: F,
    make: fn(F) -> ColumnsEdit,
    view: &SettingsView<'a>,
) -> SettingRow<'a> {
    let lang = view.lang;
    let options: Vec<(&str, Message)> = formats
        .iter()
        .map(|&(name, format)| (lang.text(name), edit(make(format))))
        .collect();
    let chosen = formats
        .iter()
        .position(|&(_, format)| format == current)
        .unwrap_or_default();
    let control: Element<'a, Message> = kit::segmented(&options, chosen, true, view.palette);
    let mut row = simple_row(label, Some(note), control, lang);
    row.control_width = Some(SETTINGS_WIDE_CONTROL_WIDTH);
    row.keyed(choice_key(
        formats
            .iter()
            .map(|&(_, format)| edit(make(format)))
            .collect(),
        chosen,
    ))
}

fn date_label(format: DateFormat) -> Msg {
    match format {
        DateFormat::Relative => Msg::SettingsDateRelative,
        DateFormat::Short => Msg::SettingsDateShort,
        DateFormat::Iso => Msg::SettingsDateIso,
    }
}

fn size_label(format: SizeFormat) -> Msg {
    match format {
        SizeFormat::Bytes => Msg::SettingsSizeBytes,
        SizeFormat::Kilobytes => Msg::SettingsSizeKilobytes,
        SizeFormat::Readable => Msg::SettingsSizeReadable,
    }
}

fn column_label(column: Column) -> Msg {
    match column {
        Column::Icon => Msg::ColumnIcon,
        Column::Name => Msg::ColumnName,
        Column::Size => Msg::ColumnSize,
        Column::Modified => Msg::ColumnModified,
        Column::Created => Msg::ColumnCreated,
        Column::Type => Msg::ColumnType,
        Column::Extension => Msg::ColumnExtension,
        Column::Permissions => Msg::ColumnPermissions,
        Column::Owner => Msg::ColumnOwner,
        Column::Group => Msg::ColumnGroup,
        Column::Accessed => Msg::ColumnAccessed,
        Column::LinkTarget => Msg::ColumnLinkTarget,
    }
}

fn column_description(column: Column) -> Msg {
    match column {
        Column::Icon => Msg::SettingsColumnDescIcon,
        Column::Name => Msg::SettingsColumnDescName,
        Column::Size => Msg::SettingsColumnDescSize,
        Column::Modified => Msg::SettingsColumnDescModified,
        Column::Created => Msg::SettingsColumnDescCreated,
        Column::Type => Msg::SettingsColumnDescType,
        Column::Extension => Msg::SettingsColumnDescExtension,
        Column::Permissions => Msg::SettingsColumnDescPermissions,
        Column::Owner | Column::Group => Msg::SettingsColumnDescOwnerGroup,
        Column::Accessed => Msg::SettingsColumnDescAccessed,
        Column::LinkTarget => Msg::SettingsColumnDescLinkTarget,
    }
}
