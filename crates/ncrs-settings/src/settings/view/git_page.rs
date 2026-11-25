//! The Git page: where the Repos tab looks for repositories, what it skips and
//! how the bulk actions behave.

use ncrs_config::repos::{ReposSettings, CONCURRENCY_CHOICES, DEPTH_CHOICES};
use ncrs_ui_kit::format::{date_text, local_time, DateFormat};

use super::index_page::add_folder_row;
use super::*;
use crate::folders::AddAs;

pub fn git_groups<'a>(view: &SettingsView<'a>) -> Vec<Group<'a>> {
    let lang = view.lang;
    let current = view
        .state
        .config()
        .map(|config| config.repos.clone())
        .unwrap_or_default();
    vec![
        Group {
            intro: None,
            title: lang.text(Msg::SettingsGitTitleSources).to_owned(),
            rows: source_rows(view, &current),
        },
        Group {
            intro: None,
            title: lang.text(Msg::SettingsGitTitleSearch).to_owned(),
            rows: search_rows(view, &current),
        },
        Group {
            intro: None,
            title: lang.text(Msg::SettingsGitTitleSkip).to_owned(),
            rows: skip_rows(view, &current),
        },
        Group {
            intro: None,
            title: lang.text(Msg::SettingsGitTitleBulk).to_owned(),
            rows: bulk_rows(view, &current),
        },
    ]
}

fn changed(current: &ReposSettings, edit: impl FnOnce(&mut ReposSettings)) -> SettingChange {
    let mut next = current.clone();
    edit(&mut next);
    SettingChange::Repos(next)
}

fn remove_button(
    label: Msg,
    setting: SettingChange,
    view: &SettingsView<'_>,
) -> RowButton<Message> {
    RowButton::new(
        view.lang.text(label),
        Some(change(setting)),
        Tone::Secondary,
    )
}

fn source_rows<'a>(view: &SettingsView<'a>, current: &ReposSettings) -> Vec<SettingRow<'a>> {
    let mut rows: Vec<SettingRow<'a>> = current
        .sources
        .iter()
        .map(|folder| {
            let without = changed(current, |next| next.sources.retain(|known| known != folder));
            SettingRow::listed_buttons(
                shown_path(folder, view.home),
                None,
                None,
                vec![remove_button(Msg::SettingsRemove, without, view)],
                view.palette,
            )
        })
        .collect();
    rows.push(add_folder_row(
        AddAs::RepoSource,
        current
            .sources
            .is_empty()
            .then_some(Msg::SettingsGitSourcesEmpty),
        vec![RowButton::new(
            view.lang.text(Msg::SettingsGitScanNow),
            Some(SettingsMessage::ScanRepos),
            Tone::Secondary,
        )],
        view,
    ));
    rows.push(SettingRow::text(
        view.lang.text(Msg::SettingsGitRowLastScan).to_owned(),
        None,
        wrapped_value_text(
            last_scan_text(view),
            font_size::BODY,
            view.palette.dim_text,
        ),
    ));
    rows
}

fn last_scan_text(view: &SettingsView<'_>) -> String {
    let facts = view.repos_scan;
    let when = facts.last.and_then(|last| {
        Some((
            last,
            date_text(
                local_time(last.at)?,
                local_time(facts.now)?,
                DateFormat::Relative,
                view.lang,
            ),
        ))
    });
    match when {
        Some((last, text)) => fill_template(
            view.lang.text(Msg::SettingsGitLastScanValue),
            &[
                ("{when}", &text),
                ("{count}", &last.repos.to_string()),
                ("{millis}", &last.millis.to_string()),
            ],
        ),
        None => view.lang.text(Msg::SettingsGitLastScanNever).to_owned(),
    }
}

fn switch_row<'a>(
    label: Msg,
    note: Msg,
    on: bool,
    setting: SettingChange,
    view: &SettingsView<'a>,
) -> SettingRow<'a> {
    let row = SettingRow::text(
        view.lang.text(label).to_owned(),
        Some(view.lang.text(note).to_owned()),
        switch(on, setting.clone(), view),
    );
    row.keyed(RowKey::Press(change(setting)))
}

fn number_row<'a>(
    label: Msg,
    note: Msg,
    choices: &[u8],
    chosen: u8,
    edit: impl Fn(&mut ReposSettings, u8),
    current: &ReposSettings,
    view: &SettingsView<'a>,
) -> SettingRow<'a> {
    let names: Vec<String> = choices.iter().map(u8::to_string).collect();
    let options: Vec<(&str, Message)> = choices
        .iter()
        .zip(&names)
        .map(|(number, name)| {
            (
                name.as_str(),
                change(changed(current, |next| edit(next, *number))),
            )
        })
        .collect();
    let at = choices
        .iter()
        .position(|number| *number == chosen)
        .unwrap_or_default();
    let keys = options.iter().map(|(_, message)| message.clone()).collect();
    SettingRow::text(
        view.lang.text(label).to_owned(),
        Some(view.lang.text(note).to_owned()),
        kit::segmented(&options, at, true, view.palette),
    )
    .keyed_maybe(choice_key(keys, at))
}

fn search_rows<'a>(view: &SettingsView<'a>, current: &ReposSettings) -> Vec<SettingRow<'a>> {
    vec![
        switch_row(
            Msg::SettingsGitRowScanHome,
            Msg::SettingsGitRowScanHomeNote,
            current.scan_home,
            changed(current, |next| next.scan_home = !current.scan_home),
            view,
        ),
        number_row(
            Msg::SettingsGitRowDepth,
            Msg::SettingsGitRowDepthNote,
            &DEPTH_CHOICES,
            current.depth,
            |next, number| next.depth = number,
            current,
            view,
        ),
        switch_row(
            Msg::SettingsGitRowLearn,
            Msg::SettingsGitRowLearnNote,
            current.learn_from_panel,
            changed(current, |next| {
                next.learn_from_panel = !current.learn_from_panel
            }),
            view,
        ),
    ]
}

fn skip_rows<'a>(view: &SettingsView<'a>, current: &ReposSettings) -> Vec<SettingRow<'a>> {
    let mut rows = vec![
        switch_row(
            Msg::SettingsGitRowSkipCloud,
            Msg::SettingsGitRowSkipCloudNote,
            current.skip_cloud,
            changed(current, |next| next.skip_cloud = !current.skip_cloud),
            view,
        ),
        switch_row(
            Msg::SettingsGitRowSkipNetwork,
            Msg::SettingsGitRowSkipNetworkNote,
            current.skip_network,
            changed(current, |next| next.skip_network = !current.skip_network),
            view,
        ),
    ];
    rows.extend(current.skip.iter().map(|name| {
        let without = changed(current, |next| next.skip.retain(|known| known != name));
        SettingRow::listed_buttons(
            name.clone(),
            None,
            None,
            vec![remove_button(Msg::SettingsRemove, without, view)],
            view.palette,
        )
    }));
    rows.push(add_folder_row(AddAs::RepoSkip, None, Vec::new(), view));
    rows
}

fn bulk_rows<'a>(view: &SettingsView<'a>, current: &ReposSettings) -> Vec<SettingRow<'a>> {
    let lang = view.lang;
    let info = |label: Msg, value: Msg| {
        SettingRow::text(
            lang.text(label).to_owned(),
            None,
            wrapped_value_text(
                lang.text(value).to_owned(),
                font_size::BODY,
                view.palette.dim_text,
            ),
        )
    };
    vec![
        info(
            Msg::SettingsGitRowDefaultBranch,
            Msg::SettingsGitDefaultBranchValue,
        ),
        number_row(
            Msg::SettingsGitRowConcurrency,
            Msg::SettingsGitRowConcurrencyNote,
            &CONCURRENCY_CHOICES,
            current.concurrency,
            |next, number| next.concurrency = number,
            current,
            view,
        ),
        info(
            Msg::SettingsGitRowLocalChanges,
            Msg::SettingsGitLocalChangesValue,
        ),
    ]
}
