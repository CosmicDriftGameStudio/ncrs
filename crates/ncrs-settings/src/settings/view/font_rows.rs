//! The font rows of the Appearance page: the family, the size and a preview
//! of a file panel in them.

use std::time::{Duration, UNIX_EPOCH};

use iced::widget::{container, Column as Rows, Row};
use iced::Element;

use super::*;
use ncrs_config::panel_columns::{Appearance, FontChoice, FontSizeChoice, FONT_POINTS};
use ncrs_core::{EntryDetails, FileEntry, Selection};
use ncrs_ui_kit::column_header::ColumnLayout;
use ncrs_ui_kit::file_row::{file_row, tag_star, RowExtras};
use ncrs_ui_kit::font;
use ncrs_ui_kit::hermes_kit::{field_dropdown, MenuItem};
use ncrs_ui_kit::layout::{ROW_HEIGHT, SETTINGS_WIDE_CONTROL_WIDTH};

const DROPDOWN_WIDTH: f32 = 300.0;
/// 5 November 2026, 16:12 UTC: a fixed time, so the preview looks the same on every day.
const SAMPLE_TIME: Duration = Duration::from_secs(1_793_895_120);

fn appearance_change(font: FontChoice, font_size: FontSizeChoice) -> Message {
    change(SettingChange::Appearance(Appearance { font, font_size }))
}

pub fn font_rows<'a>(view: &SettingsView<'a>) -> Vec<SettingRow<'a>> {
    let lang = view.lang;
    let palette = view.palette;
    let appearance = view.state.appearance();
    let system = view.state.system_font.as_ref();

    let family_choices = font_choices(view);
    let family_items: Vec<MenuItem<Message>> = family_choices
        .iter()
        .map(|(choice, label, hint)| {
            let mut item = MenuItem::new(
                label.clone(),
                appearance_change(choice.clone(), appearance.font_size),
            );
            item.key_hint = hint.clone();
            item
        })
        .collect();
    let family_selected = family_choices
        .iter()
        .position(|(choice, _, _)| *choice == appearance.font)
        .unwrap_or_default();

    let size_choices: Vec<FontSizeChoice> = std::iter::once(FontSizeChoice::System)
        .chain(FONT_POINTS.map(FontSizeChoice::Points))
        .collect();
    let size_items: Vec<MenuItem<Message>> = size_choices
        .iter()
        .map(|&choice| {
            let (template, points) = match choice {
                FontSizeChoice::System => (
                    Msg::SettingsFontSizeSystem,
                    format!("{:.0}", font::system_points(system)),
                ),
                FontSizeChoice::Points(points) => (Msg::SettingsFontSizePoints, points.to_string()),
            };
            MenuItem::new(
                fill_template(lang.text(template), &[("{size}", &points)]),
                appearance_change(appearance.font.clone(), choice),
            )
        })
        .collect();
    let size_selected = size_choices
        .iter()
        .position(|&choice| choice == appearance.font_size)
        .unwrap_or_default();

    let mut preview = simple_row(
        Msg::SettingsRowFontPreview,
        Some(Msg::SettingsRowFontPreviewNote),
        preview_panel(view),
        lang,
    );
    preview.control_width = Some(SETTINGS_WIDE_CONTROL_WIDTH);
    let family_key = choice_key(
        family_choices
            .iter()
            .map(|(choice, _, _)| appearance_change(choice.clone(), appearance.font_size))
            .collect(),
        family_selected,
    );
    let size_key = choice_key(
        size_choices
            .iter()
            .map(|&choice| appearance_change(appearance.font.clone(), choice))
            .collect(),
        size_selected,
    );
    vec![
        simple_row(
            Msg::SettingsRowFont,
            Some(Msg::SettingsRowFontNote),
            field_dropdown(DROPDOWN_WIDTH, &family_items, family_selected, palette),
            lang,
        )
        .keyed(family_key),
        simple_row(
            Msg::SettingsRowFontSize,
            Some(Msg::SettingsRowFontSizeNote),
            field_dropdown(DROPDOWN_WIDTH, &size_items, size_selected, palette),
            lang,
        )
        .keyed(size_key),
        preview,
    ]
}

/// The entries of the font list: the system's font first, the font that ships
/// with the app (where it does) next, then every monospace family found.
fn font_choices(view: &SettingsView<'_>) -> Vec<(FontChoice, String, Option<String>)> {
    let lang = view.lang;
    let bundled = font::bundled_family();
    let system_family = font::system_family_name(view.state.system_font.as_ref());
    let mut choices = vec![(
        FontChoice::System,
        fill_template(
            lang.text(Msg::SettingsFontSystem),
            &[("{name}", &system_family)],
        ),
        None,
    )];
    if let Some(family) = bundled {
        choices.push((
            FontChoice::Named(family.to_owned()),
            family.to_owned(),
            Some(lang.text(Msg::SettingsFontBundled).to_owned()),
        ));
    }
    choices.extend(
        view.state
            .installed_fonts
            .iter()
            .filter(|family| Some(family.as_str()) != bundled)
            .map(|family| (FontChoice::Named(family.clone()), family.clone(), None)),
    );
    choices
}

/// A few rows of a panel, drawn by the same row the panels use. The clicks
/// lead nowhere: they select the page that is open anyway.
fn preview_panel<'a>(view: &SettingsView<'a>) -> Element<'a, Message> {
    let palette = view.palette;
    let nothing = SettingsMessage::Select(view.state.feature);
    let rows = preview_entries()
        .into_iter()
        .enumerate()
        .map(|(position, entry)| {
            let mark = Selection {
                cursor: position == 2,
                tagged: position == 3,
            };
            let prefix =
                Row::new().push(tag_star(mark, true, palette, ROW_HEIGHT, nothing.clone()));
            let on_click = nothing.clone();
            file_row(
                &entry,
                mark,
                true,
                palette,
                move |_| on_click.clone(),
                prefix,
                RowExtras {
                    columns: ColumnLayout::standard(view.lang),
                    special: None,
                    hermes: None,
                    hit_scope: None,
                    third_line: None,
                },
            )
        });
    container(Rows::with_children(rows))
        .style(theme::outlined(palette))
        .into()
}

fn preview_entries() -> Vec<FileEntry> {
    let sample = |name: &str, is_dir: bool, size: u64| FileEntry {
        name: name.into(),
        path: name.into(),
        is_dir,
        is_symlink: false,
        is_parent: false,
        size,
        modified: Some(UNIX_EPOCH + SAMPLE_TIME),
        folder_size: None,
        details: EntryDetails::default(),
    };
    vec![
        sample("Documents", true, 0),
        sample("Invoices", true, 0),
        sample("Invoice-2026-11.pdf", false, 1_433_600),
        sample("notes.txt", false, 2_048),
        sample("photo.jpg", false, 3_407_872),
    ]
}
