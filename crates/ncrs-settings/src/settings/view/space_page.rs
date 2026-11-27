//! The Platz page: when the drive counts as full, which folders ncrs may
//! delete as safe, which it never touches, and how deep it measures.

use ncrs_config::space::{Depth, SpaceSettings, DEPTH_CHOICES};
use ncrs_space::{CatalogId, Os};

use super::index_page::add_folder_row;
use super::*;
use crate::folders::{warn_from_stepped, AddAs};

pub fn space_groups<'a>(view: &SettingsView<'a>) -> Vec<Group<'a>> {
    let lang = view.lang;
    let current = view
        .state
        .config()
        .map(|config| config.space.clone())
        .unwrap_or_default();
    let catalog = listed_catalog(Os::current(), view.state.sandboxed);
    let on = catalog
        .iter()
        .filter(|id| !current.catalog_off.iter().any(|off| off == id.name()))
        .count();
    let safe_title = format!(
        "{} \u{b7} {}",
        lang.text(Msg::SettingsSpaceSafeGroup),
        fill_template(
            lang.text(Msg::SettingsSpaceSafeCount),
            &[
                ("{on}", &on.to_string()),
                ("{total}", &catalog.len().to_string()),
            ],
        )
    );
    let mut groups = vec![
        Group {
            intro: None,
            title: lang.text(Msg::SettingsSpaceWarningGroup).to_owned(),
            rows: vec![warn_row(&current, view)],
        },
        Group {
            intro: Some(lang.text(Msg::SettingsSpaceRegenerableIntro).to_owned()),
            title: safe_title,
            rows: catalog
                .iter()
                .filter_map(|id| catalog_row(*id, &current, view))
                .collect(),
        },
        Group {
            intro: None,
            title: lang.text(Msg::SettingsSpaceOwnRegenerable).to_owned(),
            rows: folder_rows(
                &current.own_regenerable,
                AddAs::SpaceRegenerable,
                |next, folder| next.own_regenerable.retain(|known| known != folder),
                &current,
                view,
            ),
        },
        Group {
            intro: Some(lang.text(Msg::SettingsSpaceProtectedIntro).to_owned()),
            title: lang.text(Msg::SettingsSpaceNeverGroup).to_owned(),
            rows: folder_rows(
                &current.never_touch,
                AddAs::SpaceNeverTouch,
                |next, folder| next.never_touch.retain(|known| known != folder),
                &current,
                view,
            ),
        },
    ];
    if !view.state.sandboxed {
        groups.push(Group {
            intro: None,
            title: lang.text(Msg::SettingsSpaceWorktreesGroup).to_owned(),
            rows: vec![switch_row(
                Msg::SettingsSpaceShowWorktrees,
                Msg::SettingsSpaceWorktreesNote,
                current.show_worktrees,
                changed(&current, |next| {
                    next.show_worktrees = !current.show_worktrees
                }),
                view,
            )],
        });
    }
    groups.push(Group {
        intro: None,
        title: lang.text(Msg::SettingsSpaceMeasureGroup).to_owned(),
        rows: vec![
            depth_row(&current, view),
            switch_row(
                Msg::SettingsSpaceOtherDrives,
                Msg::SettingsSpaceOtherDrivesNote,
                current.other_drives,
                changed(&current, |next| next.other_drives = !current.other_drives),
                view,
            ),
        ],
    });
    if view.state.sandboxed {
        groups.push(Group {
            intro: Some(lang.text(Msg::SettingsSpaceStoreNote).to_owned()),
            title: String::new(),
            rows: vec![SettingRow::adding_buttons(
                None,
                None,
                vec![RowButton::new(
                    lang.text(Msg::SettingsSpaceGrantHome),
                    Some(SettingsMessage::AddFolder(AddAs::SpaceHome)),
                    Tone::Secondary,
                )],
                view.palette,
            )],
        });
    }
    groups
}

/// The catalog entries the page switches on and off: the ones this system
/// has a place for, and that the build may touch.
pub(crate) fn listed_catalog(os: Os, sandboxed: bool) -> Vec<CatalogId> {
    let mut listed = vec![
        CatalogId::CargoTarget,
        CatalogId::Sccache,
        CatalogId::CargoRegistryCache,
        CatalogId::Yarn,
        CatalogId::Npm,
        CatalogId::Bun,
        CatalogId::Pnpm,
        CatalogId::Pip,
    ];
    if os == Os::MacOs {
        listed.extend([CatalogId::Homebrew, CatalogId::DerivedData]);
        if !sandboxed {
            listed.push(CatalogId::UnavailableSimulators);
        }
    }
    listed.push(CatalogId::Gradle);
    if os == Os::Windows {
        listed.push(CatalogId::WinTempOld);
    }
    listed
}

fn catalog_texts(id: CatalogId) -> Option<(Msg, Msg)> {
    Some(match id {
        CatalogId::CargoTarget => (
            Msg::SpaceCatalogCargoTarget,
            Msg::SpaceCatalogCargoTargetWhy,
        ),
        CatalogId::CargoTargetHome => (
            Msg::SpaceCatalogCargoTargetHome,
            Msg::SpaceCatalogCargoTargetHomeWhy,
        ),
        CatalogId::NodeModulesStale => (
            Msg::SpaceCatalogNodeModulesStale,
            Msg::SpaceCatalogNodeModulesStaleWhy,
        ),
        CatalogId::Sccache => (Msg::SpaceCatalogSccache, Msg::SpaceCatalogSccacheWhy),
        CatalogId::CargoRegistryCache => (
            Msg::SpaceCatalogCargoRegistryCache,
            Msg::SpaceCatalogCargoRegistryCacheWhy,
        ),
        CatalogId::Yarn => (Msg::SpaceCatalogYarn, Msg::SpaceCatalogYarnWhy),
        CatalogId::Npm => (Msg::SpaceCatalogNpm, Msg::SpaceCatalogNpmWhy),
        CatalogId::Bun => (Msg::SpaceCatalogBun, Msg::SpaceCatalogBunWhy),
        CatalogId::Pnpm => (Msg::SpaceCatalogPnpm, Msg::SpaceCatalogPnpmWhy),
        CatalogId::Pip => (Msg::SpaceCatalogPip, Msg::SpaceCatalogPipWhy),
        CatalogId::Homebrew => (Msg::SpaceCatalogHomebrew, Msg::SpaceCatalogHomebrewWhy),
        CatalogId::DerivedData => (
            Msg::SpaceCatalogDerivedData,
            Msg::SpaceCatalogDerivedDataWhy,
        ),
        CatalogId::UnavailableSimulators => (
            Msg::SpaceCatalogUnavailableSimulators,
            Msg::SpaceCatalogUnavailableSimulatorsWhy,
        ),
        CatalogId::Gradle => (Msg::SpaceCatalogGradle, Msg::SpaceCatalogGradleWhy),
        CatalogId::WinTempOld => (Msg::SpaceCatalogWinTempOld, Msg::SpaceCatalogWinTempOldWhy),
        CatalogId::AppCache(_) | CatalogId::OwnRegenerable => return None,
    })
}

/// `current` with the entry `id` switched: off when it was on, on when it was off.
pub(crate) fn with_catalog_toggled(current: &SpaceSettings, id: CatalogId) -> SpaceSettings {
    let mut next = current.clone();
    let name = id.name();
    if next.catalog_off.iter().any(|off| off == name) {
        next.catalog_off.retain(|off| off != name);
    } else {
        next.catalog_off.push(name.to_owned());
    }
    next
}

fn changed(current: &SpaceSettings, edit: impl FnOnce(&mut SpaceSettings)) -> SettingChange {
    let mut next = current.clone();
    edit(&mut next);
    SettingChange::Space(next)
}

fn switch_row<'a>(
    label: Msg,
    note: Msg,
    on: bool,
    setting: SettingChange,
    view: &SettingsView<'a>,
) -> SettingRow<'a> {
    SettingRow::text(
        view.lang.text(label).to_owned(),
        Some(view.lang.text(note).to_owned()),
        switch(on, setting.clone(), view),
    )
    .keyed(RowKey::Press(change(setting)))
}

fn catalog_row<'a>(
    id: CatalogId,
    current: &SpaceSettings,
    view: &SettingsView<'a>,
) -> Option<SettingRow<'a>> {
    let (name, why) = catalog_texts(id)?;
    let on = !current.catalog_off.iter().any(|off| off == id.name());
    let setting = SettingChange::Space(with_catalog_toggled(current, id));
    Some(
        SettingRow::text(
            view.lang.text(name).to_owned(),
            Some(view.lang.text(why).to_owned()),
            switch(on, setting.clone(), view),
        )
        .keyed(RowKey::Press(change(setting))),
    )
}

fn warn_row<'a>(current: &SpaceSettings, view: &SettingsView<'a>) -> SettingRow<'a> {
    let lang = view.lang;
    let palette = view.palette;
    let stepped = |step: i8| {
        changed(current, |next| {
            next.warn_from_percent = warn_from_stepped(current.warn_from_percent, step);
        })
    };
    let control: Element<'a, Message> = row![
        kit::button(
            "\u{2212}",
            Some(change(stepped(-1))),
            Tone::Secondary,
            palette
        ),
        text(current.warn_from_percent.to_string())
            .size(font_size::BODY)
            .color(palette.text),
        kit::button("+", Some(change(stepped(1))), Tone::Secondary, palette),
    ]
    .spacing(spacing::M)
    .align_y(iced::alignment::Vertical::Center)
    .into();
    SettingRow::text(
        lang.text(Msg::SettingsSpaceWarnFrom).to_owned(),
        Some(lang.text(Msg::SettingsSpaceWarnSuffix).to_owned()),
        control,
    )
    .keyed(RowKey::Choice {
        previous: change(stepped(-1)),
        next: change(stepped(1)),
    })
}

fn depth_row<'a>(current: &SpaceSettings, view: &SettingsView<'a>) -> SettingRow<'a> {
    let lang = view.lang;
    let names: Vec<String> = DEPTH_CHOICES.iter().map(u8::to_string).collect();
    let depths: Vec<Depth> = std::iter::once(Depth::Auto)
        .chain(DEPTH_CHOICES.iter().copied().map(Depth::Levels))
        .collect();
    let labels: Vec<&str> = std::iter::once(lang.text(Msg::SettingsSpaceDepthAuto))
        .chain(names.iter().map(String::as_str))
        .collect();
    let options: Vec<(&str, Message)> = labels
        .into_iter()
        .zip(&depths)
        .map(|(label, depth)| (label, change(changed(current, |next| next.depth = *depth))))
        .collect();
    let at = depths
        .iter()
        .position(|depth| *depth == current.depth)
        .unwrap_or_default();
    let keys = options.iter().map(|(_, message)| message.clone()).collect();
    SettingRow::text(
        lang.text(Msg::SettingsSpaceDepth).to_owned(),
        Some(lang.text(Msg::SettingsSpaceDepthNote).to_owned()),
        kit::segmented(&options, at, true, view.palette),
    )
    .keyed_maybe(choice_key(keys, at))
}

fn folder_rows<'a>(
    folders: &[std::path::PathBuf],
    add_as: AddAs,
    remove: impl Fn(&mut SpaceSettings, &std::path::Path),
    current: &SpaceSettings,
    view: &SettingsView<'a>,
) -> Vec<SettingRow<'a>> {
    let mut rows: Vec<SettingRow<'a>> = folders
        .iter()
        .map(|folder| {
            let without = changed(current, |next| remove(next, folder));
            SettingRow::listed_buttons(
                shown_path(folder, view.home),
                None,
                None,
                vec![RowButton::new(
                    view.lang.text(Msg::SettingsSpaceRemove),
                    Some(change(without)),
                    Tone::Secondary,
                )],
                view.palette,
            )
        })
        .collect();
    rows.push(add_folder_row(add_as, None, Vec::new(), view));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_an_entry_twice_gives_the_settings_back() {
        let on = SpaceSettings::default();
        let off = with_catalog_toggled(&on, CatalogId::Yarn);
        assert_eq!(
            off.catalog_off,
            vec!["node-modules-stale".to_owned(), "yarn".to_owned()]
        );
        assert_eq!(with_catalog_toggled(&off, CatalogId::Yarn), on);
    }

    #[test]
    fn only_entries_of_this_system_and_build_are_listed() {
        let mac = listed_catalog(Os::MacOs, false);
        assert!(mac.contains(&CatalogId::UnavailableSimulators));
        assert!(!listed_catalog(Os::MacOs, true).contains(&CatalogId::UnavailableSimulators));
        assert!(!listed_catalog(Os::Linux, false).contains(&CatalogId::DerivedData));
        assert!(listed_catalog(Os::Windows, false).contains(&CatalogId::WinTempOld));
        assert!(!mac.contains(&CatalogId::WinTempOld));
    }

    #[test]
    fn every_listed_entry_has_its_words() {
        for os in [Os::MacOs, Os::Linux, Os::Windows] {
            for id in listed_catalog(os, false) {
                assert!(catalog_texts(id).is_some(), "{}", id.name());
            }
        }
    }
}
