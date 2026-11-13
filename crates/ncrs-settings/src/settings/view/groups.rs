use iced::widget::{column, container, row};
use iced::{alignment, Length};

use super::*;
use crate::message::{SettingChange, SettingField, SettingsMessage, TerminalChoice};
use crate::settings::*;
use ncrs_config::config::ServerEntry;
use ncrs_config::file_list_tags::FileListTags;
use ncrs_fs::OpenKind;
use ncrs_i18n::fill_template;
use ncrs_i18n::{Language, Msg};
use ncrs_ui_kit::format::ellipsize_middle;
use ncrs_ui_kit::hermes_kit::{field_dropdown, MenuItem};
use ncrs_ui_kit::kit::{self, Check, Tone};
use ncrs_ui_kit::layout::SETTINGS_WIDE_CONTROL_WIDTH;
use ncrs_ui_kit::theme::{self, spacing};

use ncrs_ui_kit::palette::{hex, ColorRole, ThemePreset};

pub fn look_groups<'a>(view: &SettingsView<'a>) -> Vec<Group<'a>> {
    let lang = view.lang;
    let palette = view.palette;
    let options: Vec<(&str, Message)> = ThemePreset::ALL
        .iter()
        .map(|&preset| {
            (
                lang.text(preset_label(preset)),
                change(SettingChange::Preset(preset)),
            )
        })
        .collect();
    let chosen = ThemePreset::ALL
        .iter()
        .position(|&preset| preset == view.preset)
        .unwrap_or_default();
    let mut plain_preset_row = simple_row(
        Msg::SettingsRowPreset,
        Some(Msg::SettingsRowPresetNote),
        kit::segmented(&options, chosen, true, palette),
        lang,
    );
    plain_preset_row.control_width = Some(SETTINGS_WIDE_CONTROL_WIDTH);
    let preset_row = plain_preset_row.keyed_maybe(choice_key(
        options.iter().map(|(_, message)| message.clone()).collect(),
        chosen,
    ));

    let open_vsx_row = simple_row(
        Msg::SettingsRowOpenVsx,
        Some(Msg::SettingsRowOpenVsxNote),
        kit::button(
            lang.text(Msg::SettingsVscodeFetch),
            Some(SettingsMessage::OpenThemeFetch),
            Tone::Secondary,
            palette,
        ),
        lang,
    )
    .keyed(RowKey::Press(SettingsMessage::OpenThemeFetch));
    vec![
        Group {
            intro: None,
            title: String::new(),
            rows: [preset_row, vscode_row(view), open_vsx_row]
                .into_iter()
                .chain(font_rows(view))
                .collect(),
        },
        Group {
            intro: None,
            title: lang.text(Msg::SettingsGroupColors).to_owned(),
            rows: ColorRole::ALL
                .iter()
                .map(|&role| color_row(role, view))
                .collect(),
        },
        Group {
            intro: None,
            title: String::new(),
            rows: file_tags_rows(view),
        },
    ]
}

/// The switch for the tags after the file names.
pub fn file_tags_rows<'a>(view: &SettingsView<'a>) -> Vec<SettingRow<'a>> {
    let lang = view.lang;
    let current = view.state.file_list_tags();
    let switched = FileListTags {
        show: !current.show,
        ..current.clone()
    };
    let show_row = simple_row(
        Msg::SettingsRowFileTagsShow,
        None,
        switch(
            current.show,
            SettingChange::FileListTags(switched.clone()),
            view,
        ),
        lang,
    )
    .keyed(RowKey::Press(change(SettingChange::FileListTags(switched))));
    vec![show_row]
}

pub fn vscode_row<'a>(view: &SettingsView<'a>) -> SettingRow<'a> {
    let lang = view.lang;
    let palette = view.palette;
    let file = match view.vscode_theme {
        Some(path) => mono(ellipsize_middle(path, THEME_FILE_CHARS), palette.text),
        None => status_text(
            lang.text(Msg::SettingsVscodeNone).to_owned(),
            palette.dim_text,
        ),
    };
    let mut control = row![
        container(file).width(Length::Fill),
        kit::button(
            lang.text(Msg::SettingsChoose),
            Some(SettingsMessage::PickThemeFile),
            Tone::Secondary,
            palette,
        ),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    if view.vscode_theme.is_some() {
        control = control.push(kit::button(
            lang.text(Msg::SettingsRemove),
            Some(change(SettingChange::VscodeTheme(None))),
            Tone::Secondary,
            palette,
        ));
    }
    let warning = view
        .state
        .config()
        .and_then(|config| config.vscode_warning.as_ref())
        .map(|warning| {
            fill_template(
                lang.text(Msg::SettingsVscodeProblem),
                &[("{reason}", &warning.reason)],
            )
        });
    SettingRow {
        key: Some(RowKey::Press(SettingsMessage::PickThemeFile)),
        problem: warning,
        control_width: Some(SETTINGS_WIDE_CONTROL_WIDTH),
        in_command_list: true,
        list_indent: None,
        marked_controls: Vec::new(),
        leading: None,
        ..simple_row(
            Msg::SettingsRowVscode,
            Some(Msg::SettingsRowVscodeNote),
            control.into(),
            lang,
        )
    }
}

pub fn color_label(role: ColorRole) -> Msg {
    match role {
        ColorRole::Background => Msg::SettingsColorBackground,
        ColorRole::Panel => Msg::SettingsColorPanel,
        ColorRole::Text => Msg::SettingsColorText,
        ColorRole::DimText => Msg::SettingsColorDimText,
        ColorRole::Directory => Msg::SettingsColorDirectory,
        ColorRole::Cursor => Msg::SettingsColorCursor,
        ColorRole::CursorText => Msg::SettingsColorCursorText,
        ColorRole::Tagged => Msg::SettingsColorTagged,
        ColorRole::Border => Msg::SettingsColorBorder,
        ColorRole::Bar => Msg::SettingsColorBar,
        ColorRole::BarInactive => Msg::SettingsColorBarInactive,
        ColorRole::StatusBar => Msg::SettingsColorStatusBar,
        ColorRole::Dialog => Msg::SettingsColorDialog,
        ColorRole::Menu => Msg::SettingsColorMenu,
        ColorRole::Accent => Msg::SettingsColorAccent,
        ColorRole::Success => Msg::SettingsColorSuccess,
        ColorRole::Error => Msg::SettingsColorError,
    }
}

/// The contrast warning of the file, at the row of its text color.
pub fn contrast_problem(role: ColorRole, view: &SettingsView<'_>) -> Option<String> {
    let warning = view.state.config()?.contrast_warning.as_ref()?;
    (warning.foreground == role).then(|| {
        fill_template(
            view.lang.text(Msg::SettingsContrastLow),
            &[
                (
                    "{background}",
                    view.lang.text(color_label(warning.background)),
                ),
                ("{ratio}", &format!("{:.1}", warning.ratio)),
            ],
        )
    })
}

pub fn color_row<'a>(role: ColorRole, view: &SettingsView<'a>) -> SettingRow<'a> {
    let palette = view.palette;
    let field = SettingField::Color(role);
    let shown = palette.color(role);
    let swatch = container("")
        .width(SWATCH_SIZE)
        .height(SWATCH_SIZE)
        .style(theme::swatch(shown, palette.border_inactive));
    let control = row![
        swatch,
        setting_field(field, role.field_id(), &hex(shown), true, view)
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    SettingRow {
        key: Some(RowKey::Text(role.field_id())),
        label: view.lang.text(color_label(role)).to_owned(),
        theme_name: None,
        config_key: Some(role.name().to_owned()),
        description: None,
        problem: field_problem(field, view).or_else(|| contrast_problem(role, view)),
        warning: None,
        control: control.into(),
        control_width: None,
        in_command_list: true,
        list_indent: None,
        marked_controls: Vec::new(),
        leading: None,
    }
}

pub fn program_row<'a>(kind: OpenKind, view: &SettingsView<'a>) -> SettingRow<'a> {
    let lang = view.lang;
    let palette = view.palette;
    let state = view.state;
    let field = SettingField::Program(kind);
    let (label, note, id) = match kind {
        OpenKind::View => (
            Msg::SettingsRowViewer,
            Msg::SettingsRowViewerNote,
            VIEWER_FIELD_ID,
        ),
        OpenKind::Edit => (
            Msg::SettingsRowEditor,
            Msg::SettingsRowEditorNote,
            EDITOR_FIELD_ID,
        ),
        OpenKind::Terminal => (
            Msg::SettingsRowTerminal,
            Msg::SettingsRowTerminalNote,
            TERMINAL_FIELD_ID,
        ),
    };
    let mut buttons = vec![RowButton::new(
        lang.text(Msg::SettingsChoose),
        Some(SettingsMessage::PickProgram(kind)),
        Tone::Secondary,
    )];
    if state.can_reset_program(kind) {
        buttons.push(RowButton::new(
            lang.text(Msg::SettingsDefault),
            Some(change(SettingChange::Program { kind, line: None })),
            Tone::Secondary,
        ));
    }
    let build = |marked| {
        let program_field = setting_field(
            field,
            id,
            lang.text(Msg::SettingsProgramPlaceholder),
            !state.sandboxed,
            view,
        );
        button_row(Some(program_field), &buttons, marked, palette)
    };
    let mut description = lang.text(note).to_owned();
    if state.sandboxed {
        description.push(' ');
        description.push_str(lang.text(Msg::SettingsProgramsSandboxNote));
    }
    SettingRow {
        key: None,
        label: lang.text(label).to_owned(),
        theme_name: None,
        config_key: None,
        description: Some(description),
        problem: field_problem(field, view),
        warning: None,
        control: build(None),
        control_width: Some(SETTINGS_WIDE_CONTROL_WIDTH),
        in_command_list: true,
        list_indent: None,
        marked_controls: Vec::new(),
        leading: None,
    }
    .with_buttons(pressable(&buttons), build)
}

/// The terminal of "Open terminal here": a list of Automatic, the terminals
/// found on this computer and, outside the App Store build, an own command.
pub fn terminal_row<'a>(view: &SettingsView<'a>) -> SettingRow<'a> {
    let lang = view.lang;
    let state = view.state;
    let field = SettingField::Program(OpenKind::Terminal);
    let automatic = match state.terminals.first() {
        Some(first) => fill_template(
            lang.text(Msg::SettingsTerminalAutomatic),
            &[("{name}", &first.name)],
        ),
        None => lang.text(Msg::SettingsTerminalAutomaticNone).to_owned(),
    };
    let mut items = vec![MenuItem::new(
        automatic,
        SettingsMessage::ChooseTerminal(TerminalChoice::Automatic),
    )];
    items.extend(state.terminals.iter().enumerate().map(|(index, terminal)| {
        MenuItem::new(
            terminal.name.clone(),
            SettingsMessage::ChooseTerminal(TerminalChoice::Detected(index)),
        )
    }));
    let choice = state.terminal_choice();
    if !state.sandboxed || choice == TerminalChoice::Own {
        items.push(MenuItem::new(
            lang.text(Msg::SettingsTerminalOwn),
            SettingsMessage::ChooseTerminal(TerminalChoice::Own),
        ));
    }
    let selected = match choice {
        TerminalChoice::Automatic => 0,
        TerminalChoice::Detected(index) => index + 1,
        TerminalChoice::Own => items.len() - 1,
    };
    let mut control = column![field_dropdown(
        SETTINGS_WIDE_CONTROL_WIDTH,
        &items,
        selected,
        view.palette
    )]
    .spacing(spacing::M);
    if choice == TerminalChoice::Own {
        let mut own = row![setting_field(
            field,
            TERMINAL_FIELD_ID,
            lang.text(Msg::SettingsTerminalCommandPlaceholder),
            !state.sandboxed,
            view,
        )]
        .spacing(spacing::M)
        .align_y(alignment::Vertical::Center);
        if state.can_reset_program(OpenKind::Terminal) {
            own = own.push(kit::button(
                lang.text(Msg::SettingsDefault),
                Some(change(SettingChange::Program {
                    kind: OpenKind::Terminal,
                    line: None,
                })),
                Tone::Secondary,
                view.palette,
            ));
        }
        control = control.push(own);
    }
    SettingRow {
        key: Some(dropdown_key(&items, selected)),
        label: lang.text(Msg::SettingsRowTerminal).to_owned(),
        theme_name: None,
        config_key: None,
        description: Some(lang.text(Msg::SettingsRowTerminalNote).to_owned()),
        problem: field_problem(field, view),
        warning: None,
        control: control.into(),
        control_width: Some(SETTINGS_WIDE_CONTROL_WIDTH),
        in_command_list: true,
        list_indent: None,
        marked_controls: Vec::new(),
        leading: None,
    }
}

pub fn programs_groups<'a>(view: &SettingsView<'a>) -> Vec<Group<'a>> {
    let plain = |rows| Group {
        intro: None,
        title: String::new(),
        rows,
    };
    vec![
        plain(vec![program_row(OpenKind::View, view)]),
        by_type_group(view),
        plain(vec![program_row(OpenKind::Edit, view), terminal_row(view)]),
    ]
}

pub fn keys_groups<'a>(view: &SettingsView<'a>) -> Vec<Group<'a>> {
    let lang = view.lang;
    let palette = view.palette;
    let state = if view.keys_changed == 0 {
        lang.text(Msg::SettingsKeysBuiltIn).to_owned()
    } else {
        fill_template(
            lang.text(Msg::SettingsKeysChanged),
            &[("{count}", &view.keys_changed.to_string())],
        )
    };
    vec![Group {
        intro: None,
        title: String::new(),
        rows: vec![simple_row(
            Msg::SettingsRowKeys,
            Some(Msg::SettingsRowKeysNote),
            with_button(
                status_text(state, palette.dim_text),
                lang.text(Msg::SettingsKeysEdit),
                SettingsMessage::OpenKeyBindings,
                palette,
            ),
            lang,
        )
        .keyed(RowKey::Press(SettingsMessage::OpenKeyBindings))],
    }]
}

pub fn servers_groups<'a>(view: &SettingsView<'a>) -> Vec<Group<'a>> {
    let lang = view.lang;
    let palette = view.palette;
    let add = |label: Msg, button: Msg| SettingRow {
        key: Some(RowKey::Press(SettingsMessage::OpenServerAdd)),
        control_width: None,
        in_command_list: true,
        ..SettingRow::text(
            lang.text(label).to_owned(),
            None,
            kit::button(
                lang.text(button),
                Some(SettingsMessage::OpenServerAdd),
                Tone::Secondary,
                palette,
            ),
        )
    };
    if view.servers.is_empty() {
        return vec![Group {
            intro: None,
            title: String::new(),
            rows: vec![add(
                Msg::SettingsServersEmpty,
                Msg::SettingsServersAddButton,
            )],
        }];
    }
    let mut rows: Vec<SettingRow<'a>> = view
        .servers
        .iter()
        .enumerate()
        .map(|(index, server)| {
            let remaining: Vec<ServerEntry> = view
                .servers
                .iter()
                .enumerate()
                .filter(|(other, _)| *other != index)
                .map(|(_, kept)| kept.clone())
                .collect();
            let mut detail = server.server_id().to_string();
            if let Some(folder) = &server.path {
                detail.push(' ');
                detail.push_str(folder);
            }
            SettingRow {
                in_command_list: true,
                list_indent: None,
                ..SettingRow::listed_buttons(
                    server.label(),
                    Some(detail),
                    None,
                    vec![
                        RowButton::new(
                            lang.text(Msg::RemoteConnect),
                            Some(SettingsMessage::OpenServer(server.url())),
                            Tone::Secondary,
                        ),
                        RowButton::new(
                            lang.text(Msg::SettingsServersRemove),
                            Some(change(SettingChange::Servers(remaining))),
                            Tone::Secondary,
                        ),
                    ],
                    palette,
                )
            }
        })
        .collect();
    rows.push(add(Msg::SettingsServersAdd, Msg::SettingsServersAddButton));
    vec![Group {
        intro: None,
        title: String::new(),
        rows,
    }]
}

pub fn language_groups<'a>(view: &SettingsView<'a>) -> Vec<Group<'a>> {
    let lang = view.lang;
    let futurama = lang.shows_futurama_names();
    let options: Vec<(&str, Message)> = [Language::English, Language::German]
        .into_iter()
        .map(|language| {
            (
                language.name(),
                change(SettingChange::Language(language.with_futurama(futurama))),
            )
        })
        .collect();
    let chosen = [Language::English, Language::German]
        .iter()
        .position(|&language| language == lang.base())
        .unwrap_or_default();
    let language_key = choice_key(
        options.iter().map(|(_, message)| message.clone()).collect(),
        chosen,
    );
    vec![Group {
        intro: None,
        title: String::new(),
        rows: vec![
            simple_row(
                Msg::SettingsRowLanguage,
                Some(Msg::SettingsRowLanguageNote),
                kit::segmented(&options, chosen, true, view.palette),
                lang,
            )
            .keyed_maybe(language_key),
            simple_row(
                Msg::SettingsRowFuturamaNames,
                Some(Msg::SettingsRowFuturamaNamesNote),
                kit::checkbox(
                    if futurama { Check::On } else { Check::Off },
                    Some(change(SettingChange::Language(
                        lang.base().with_futurama(!futurama),
                    ))),
                    view.palette,
                ),
                lang,
            )
            .keyed(RowKey::Press(change(SettingChange::Language(
                lang.base().with_futurama(!futurama),
            )))),
        ],
    }]
}

/// The protection of the index: its state and the buttons that change it. The
/// question before the index is removed is asked by the app.
pub fn protection_group<'a>(view: &SettingsView<'a>) -> Option<Group<'a>> {
    let panel = view.protection.as_ref()?;
    let lang = view.lang;
    let palette = view.palette;
    let color = if panel.good {
        palette.success
    } else {
        palette.text
    };
    let build = |marked: Option<usize>| {
        let mut control = column![status_text(panel.state.clone(), color)]
            .spacing(spacing::M)
            .align_x(alignment::Horizontal::Right);
        let mut on = 0;
        for (position, button) in panel.buttons.iter().enumerate() {
            let is_marked = button.enabled && marked == Some(on);
            on += usize::from(button.enabled);
            let tone = if is_marked || (button.primary && marked.is_none()) {
                Tone::Primary
            } else {
                Tone::Secondary
            };
            control = control.push(kit::button(
                lang.text(button.label),
                button
                    .enabled
                    .then_some(SettingsMessage::Protection(position)),
                tone,
                palette,
            ));
        }
        control.into()
    };
    let enabled_buttons: Vec<SettingsMessage> = panel
        .buttons
        .iter()
        .enumerate()
        .filter(|(_, button)| button.enabled)
        .map(|(position, _)| SettingsMessage::Protection(position))
        .collect();
    let protection = SettingRow {
        problem: panel.failure.clone(),
        description: Some(panel.sentence.clone()),
        in_command_list: false,
        list_indent: None,
        marked_controls: Vec::new(),
        leading: None,
        ..simple_row(Msg::SettingsIndexRowEncryption, None, build(None), lang)
    }
    .with_buttons(enabled_buttons, build);
    Some(Group {
        intro: None,
        title: String::new(),
        rows: vec![protection],
    })
}
