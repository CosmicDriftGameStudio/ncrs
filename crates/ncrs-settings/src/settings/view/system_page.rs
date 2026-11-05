use super::*;
use ncrs_config::system_monitor::{
    DriveColumn, NetworkUnit, RefreshEvery, StatusStyle, StatusValue, SystemSettings, TaskSort,
    Visibility,
};

pub fn system_groups<'a>(view: &SettingsView<'a>) -> Vec<Group<'a>> {
    let lang = view.lang;
    let current = view.state.system_settings();
    let segmented_row =
        |label: Msg, choices: &[(Msg, SystemSettings)], chosen: usize| -> SettingRow<'a> {
            let options: Vec<(&str, Message)> = choices
                .iter()
                .map(|(msg, settings)| {
                    (
                        lang.text(*msg),
                        change(SettingChange::System(settings.clone())),
                    )
                })
                .collect();
            simple_row(
                label,
                None,
                kit::segmented(&options, chosen, true, view.palette),
                lang,
            )
        };
    let choice_row = |label: Msg,
                      choices: &[(Msg, SystemSettings)],
                      is_current: fn(&SystemSettings, &SystemSettings) -> bool|
     -> SettingRow<'a> {
        let chosen = choices
            .iter()
            .position(|(_, settings)| is_current(&current, settings))
            .unwrap_or_default();
        segmented_row(label, choices, chosen)
    };
    let with = |edit: &dyn Fn(&mut SystemSettings)| {
        let mut next = current.clone();
        edit(&mut next);
        next
    };

    let refresh = choice_row(
        Msg::SettingsRowSystemRefresh,
        &RefreshEvery::ALL
            .iter()
            .map(|&every| (refresh_label(every), with(&|s| s.refresh = every)))
            .collect::<Vec<_>>(),
        |a, b| a.refresh == b.refresh,
    );
    let sort = choice_row(
        Msg::SettingsRowSystemSort,
        &TaskSort::ALL
            .iter()
            .map(|&sort| (sort_label(sort), with(&|s| s.sort = sort)))
            .collect::<Vec<_>>(),
        |a, b| a.sort == b.sort,
    );
    let unit = choice_row(
        Msg::SettingsRowSystemNetworkUnit,
        &NetworkUnit::ALL
            .iter()
            .map(|&unit| (unit_label(unit), with(&|s| s.network_unit = unit)))
            .collect::<Vec<_>>(),
        |a, b| a.network_unit == b.network_unit,
    );
    let drive = choice_row(
        Msg::SettingsRowSystemDriveColumn,
        &DriveColumn::ALL
            .iter()
            .map(|&column| (drive_label(column), with(&|s| s.drive_column = column)))
            .collect::<Vec<_>>(),
        |a, b| a.drive_column == b.drive_column,
    );
    let mut rows = vec![refresh, sort, unit, drive];
    rows.extend(
        StatusValue::ALL
            .iter()
            .filter(|value| value.available())
            .map(|&value| {
                let on = current.status_values.contains(&value);
                simple_row(
                    status_value_label(value),
                    None,
                    switch(on, SettingChange::System(current.toggled(value, !on)), view),
                    lang,
                )
            }),
    );
    rows.push(choice_row(
        Msg::SettingsRowSystemStatusStyle,
        &StatusStyle::ALL
            .iter()
            .map(|&style| (style_label(style), with(&|s| s.status_style = style)))
            .collect::<Vec<_>>(),
        |a, b| a.status_style == b.status_style,
    ));
    rows.push(choice_row(
        Msg::SettingsRowSystemIdleInterfaces,
        &Visibility::ALL
            .iter()
            .map(|&shown| {
                (
                    visibility_label(shown),
                    with(&|s| s.idle_interfaces = shown),
                )
            })
            .collect::<Vec<_>>(),
        |a, b| a.idle_interfaces == b.idle_interfaces,
    ));
    rows.push(choice_row(
        Msg::SettingsRowSystemSystemVolumes,
        &Visibility::ALL
            .iter()
            .map(|&shown| (visibility_label(shown), with(&|s| s.system_volumes = shown)))
            .collect::<Vec<_>>(),
        |a, b| a.system_volumes == b.system_volumes,
    ));
    vec![Group {
        intro: None,
        title: String::new(),
        rows,
    }]
}

fn refresh_label(every: RefreshEvery) -> Msg {
    match every {
        RefreshEvery::OneSecond => Msg::SettingsSystemOneSecond,
        RefreshEvery::TwoSeconds => Msg::SettingsSystemTwoSeconds,
        RefreshEvery::FiveSeconds => Msg::SettingsSystemFiveSeconds,
    }
}

fn sort_label(sort: TaskSort) -> Msg {
    match sort {
        TaskSort::Cpu => Msg::SettingsSystemSortCpu,
        TaskSort::Memory => Msg::SettingsSystemSortMemory,
        TaskSort::Time => Msg::SettingsSystemSortTime,
    }
}

fn unit_label(unit: NetworkUnit) -> Msg {
    match unit {
        NetworkUnit::Bytes => Msg::SettingsSystemUnitBytes,
        NetworkUnit::Bits => Msg::SettingsSystemUnitBits,
    }
}

fn drive_label(column: DriveColumn) -> Msg {
    match column {
        DriveColumn::Used => Msg::SettingsSystemDriveUsed,
        DriveColumn::Free => Msg::SettingsSystemDriveFree,
    }
}

fn style_label(style: StatusStyle) -> Msg {
    match style {
        StatusStyle::Number => Msg::SettingsSystemStyleNumber,
        StatusStyle::Symbol => Msg::SettingsSystemStyleSymbol,
    }
}

fn visibility_label(shown: Visibility) -> Msg {
    match shown {
        Visibility::Hide => Msg::SettingsSystemHide,
        Visibility::Show => Msg::SettingsSystemShow,
    }
}

fn status_value_label(value: StatusValue) -> Msg {
    match value {
        StatusValue::Cpu => Msg::SettingsRowSystemStatusCpu,
        StatusValue::Memory => Msg::SettingsRowSystemStatusMemory,
        StatusValue::Swap => Msg::SettingsRowSystemStatusSwap,
        StatusValue::Load => Msg::SettingsRowSystemStatusLoad,
        StatusValue::Network => Msg::SettingsRowSystemStatusNetwork,
    }
}
