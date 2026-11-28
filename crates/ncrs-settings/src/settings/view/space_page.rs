//! The Platz page: when the drive counts as full, which folders ncrs may
//! delete as safe, which it never touches, and how deep it measures.

use ncrs_config::space::{Depth, SpaceSettings, DEPTH_CHOICES};
use ncrs_space::{CatalogId, Os};

use super::*;
use crate::folders::AddAs;
use crate::message::DriveRow;

const WARN_FIELD_ID: &str = "space-warn-from";

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
    let never_title = format!(
        "{} \u{b7} {}",
        lang.text(Msg::SettingsSpaceNeverGroup),
        current.never_touch.len()
    );
    let mut groups = vec![Group {
        intro: None,
        title: lang.text(Msg::SettingsSpaceWarningGroup).to_owned(),
        rows: vec![warn_row(view)],
    }];
    let drives = drive_rows(&current, view);
    if !drives.is_empty() {
        groups.push(Group {
            intro: None,
            title: lang.text(Msg::SettingsSpaceDrives).to_owned(),
            rows: drives,
        });
    }
    groups.extend([
        Group {
            intro: Some(lang.text(Msg::SettingsSpaceRegenerableIntro).to_owned()),
            title: safe_title,
            rows: catalog
                .iter()
                .filter_map(|id| catalog_row(*id, &current, view))
                .collect(),
        },
        Group {
            intro: Some(lang.text(Msg::SettingsSpaceOwnRegenerable).to_owned()),
            title: lang.text(Msg::SettingsSpaceRegenerable).to_owned(),
            rows: folder_rows(
                &current.own_regenerable,
                AddAs::SpaceRegenerable,
                "",
                |next, position| {
                    next.own_regenerable.remove(position);
                },
                &current,
                view,
            ),
        },
        Group {
            intro: Some(format!(
                "{}. {}",
                lang.text(Msg::SettingsSpaceProtected),
                lang.text(Msg::SettingsSpaceProtectedIntro)
            )),
            title: never_title,
            rows: folder_rows(
                &current.never_touch,
                AddAs::SpaceNeverTouch,
                "\u{2717} ",
                |next, position| {
                    next.never_touch.remove(position);
                },
                &current,
                view,
            ),
        },
    ]);
    if !view.state.sandboxed {
        groups.push(Group {
            intro: None,
            title: lang.text(Msg::SettingsSpaceWorktreesGroup).to_owned(),
            rows: vec![SettingRow::text(
                lang.text(Msg::SettingsSpaceWorktreesLabel).to_owned(),
                Some(format!(
                    "{} \u{b7} {}",
                    lang.text(Msg::SettingsSpaceShowWorktrees),
                    lang.text(Msg::SettingsSpaceWorktreesNote)
                )),
                switch(
                    current.show_worktrees,
                    changed(&current, |next| {
                        next.show_worktrees = !current.show_worktrees
                    }),
                    view,
                ),
            )
            .keyed(RowKey::Press(change(changed(&current, |next| {
                next.show_worktrees = !current.show_worktrees
            }))))],
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

/// One drive of the warning list: its name, whether the warning is on for it, and what is known of it.
struct DriveEntry<'d> {
    name: String,
    on: bool,
    measured: Option<&'d DriveRow>,
}

/// The drives the warning is for: the listed ones, or the drive of the home folder when none is listed.
fn warned_drives(current: &SpaceSettings, drives: &[DriveRow]) -> Vec<String> {
    if current.warn_drives.is_empty() {
        drives
            .iter()
            .filter(|drive| drive.is_home)
            .map(|drive| drive.name.clone())
            .collect()
    } else {
        current.warn_drives.clone()
    }
}

/// The connected drives, then the listed ones that are not connected.
fn drive_entries<'d>(current: &SpaceSettings, drives: &'d [DriveRow]) -> Vec<DriveEntry<'d>> {
    let warned = warned_drives(current, drives);
    let mut entries: Vec<DriveEntry<'d>> = drives
        .iter()
        .map(|drive| DriveEntry {
            on: warned.contains(&drive.name),
            name: drive.name.clone(),
            measured: Some(drive),
        })
        .collect();
    entries.extend(
        current
            .warn_drives
            .iter()
            .filter(|name| !drives.iter().any(|drive| &drive.name == *name))
            .map(|name| DriveEntry {
                name: name.clone(),
                on: true,
                measured: None,
            }),
    );
    entries
}

fn with_drive_toggled(current: &SpaceSettings, drives: &[DriveRow], name: &str) -> SpaceSettings {
    let mut listed = warned_drives(current, drives);
    if listed.iter().any(|known| known == name) {
        listed.retain(|known| known != name);
    } else {
        listed.push(name.to_owned());
    }
    let mut next = current.clone();
    next.warn_drives = listed;
    next
}

fn drive_rows<'a>(current: &SpaceSettings, view: &SettingsView<'a>) -> Vec<SettingRow<'a>> {
    let lang = view.lang;
    drive_entries(current, &view.state.space_drives)
        .into_iter()
        .map(|entry| {
            let (label, note) = match entry.measured {
                Some(drive) => (
                    drive.name.clone(),
                    Some(fill_template(
                        lang.text(Msg::SettingsSpaceDriveRow),
                        &[
                            (
                                "{total}",
                                &ncrs_ui_kit::format::size_shown(drive.total, lang),
                            ),
                            ("{used}", &ncrs_ui_kit::format::size_shown(drive.used, lang)),
                        ],
                    )),
                ),
                None => (
                    entry.name.clone(),
                    Some(lang.text(Msg::SettingsSpaceDriveAbsent).to_owned()),
                ),
            };
            let setting = SettingChange::Space(with_drive_toggled(
                current,
                &view.state.space_drives,
                &entry.name,
            ));
            SettingRow::text(label, note, switch(entry.on, setting.clone(), view))
                .keyed(RowKey::Press(change(setting)))
        })
        .collect()
}

/// The folder list a key of the Platz page works on, and the folder under the cursor in it.
#[derive(Debug, PartialEq, Eq)]
pub struct ListTarget {
    pub kind: AddAs,
    pub folder: Option<usize>,
}

/// Which list the row `row` (counted among the rows with a key) belongs to. The rows are keyed in
/// the order `space_groups` builds them: warning field, drives, safe places, own folders (each,
/// then the add button), protected folders (the same).
pub fn space_list_target(
    row: usize,
    current: &SpaceSettings,
    drives: &[DriveRow],
    sandboxed: bool,
) -> Option<ListTarget> {
    let own_start =
        1 + drive_entries(current, drives).len() + listed_catalog(Os::current(), sandboxed).len();
    let never_start = own_start + current.own_regenerable.len() + 1;
    let inside = |start: usize, len: usize, kind: AddAs| {
        (start..=start + len).contains(&row).then(|| ListTarget {
            kind,
            folder: (row < start + len).then(|| row - start),
        })
    };
    inside(
        own_start,
        current.own_regenerable.len(),
        AddAs::SpaceRegenerable,
    )
    .or_else(|| {
        inside(
            never_start,
            current.never_touch.len(),
            AddAs::SpaceNeverTouch,
        )
    })
}

/// The catalog entries the page switches on and off: the ones this system
/// has a place for, and that the build may touch.
pub fn listed_catalog(os: Os, sandboxed: bool) -> Vec<CatalogId> {
    let mut listed = vec![
        CatalogId::CargoTarget,
        CatalogId::CargoTargetHome,
        CatalogId::Sccache,
        CatalogId::CargoRegistryCache,
        CatalogId::Yarn,
        CatalogId::Npm,
        CatalogId::Bun,
        CatalogId::Pnpm,
        CatalogId::Pip,
        CatalogId::NodeModulesStale,
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
pub fn with_catalog_toggled(current: &SpaceSettings, id: CatalogId) -> SpaceSettings {
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
            fill_template(view.lang.text(name), &[("{name}", "target")]),
            Some(view.lang.text(why).to_owned()),
            switch(on, setting.clone(), view),
        )
        .keyed(RowKey::Press(change(setting))),
    )
}

fn warn_row<'a>(view: &SettingsView<'a>) -> SettingRow<'a> {
    let lang = view.lang;
    let field = SettingField::SpaceWarnFrom;
    SettingRow {
        key: Some(RowKey::Text(WARN_FIELD_ID)),
        problem: field_problem(field, view),
        ..SettingRow::text(
            lang.text(Msg::SettingsSpaceWarnFrom).to_owned(),
            Some(lang.text(Msg::SettingsSpaceWarnSuffix).to_owned()),
            kit::number_field(
                lang.text(Msg::SettingsSpaceWarnUnit),
                &view.state.field_text(field),
                WARN_FIELD_ID,
                Some(move |text| SettingsMessage::Edit { field, text }),
                SettingsMessage::Submit(field),
                view.palette,
            ),
        )
    }
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

/// The add key as the platform writes it.
const ADD_KEY: &str = if cfg!(target_os = "macos") {
    "\u{2318}N"
} else {
    "Ctrl+N"
};

fn folder_rows<'a>(
    folders: &[std::path::PathBuf],
    add_as: AddAs,
    mark: &str,
    remove: impl Fn(&mut SpaceSettings, usize),
    current: &SpaceSettings,
    view: &SettingsView<'a>,
) -> Vec<SettingRow<'a>> {
    let mut rows: Vec<SettingRow<'a>> = folders
        .iter()
        .enumerate()
        .map(|(position, folder)| {
            let without = changed(current, |next| remove(next, position));
            SettingRow::listed_buttons(
                format!("{mark}{}", shown_path(folder, view.home)),
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
    rows.push(SettingRow::adding_buttons(
        None,
        view.state.folder_note.clone(),
        vec![RowButton::new(
            format!("{}  {ADD_KEY}", view.lang.text(Msg::SettingsSpaceAddFolder)),
            Some(SettingsMessage::AddFolder(add_as)),
            Tone::Secondary,
        )],
        view.palette,
    ));
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
    fn the_keys_of_the_lists_find_their_folder() {
        let current = SpaceSettings {
            own_regenerable: vec!["/a".into(), "/b".into()],
            never_touch: vec!["/c".into()],
            ..SpaceSettings::default()
        };
        let drives = vec![DriveRow {
            name: "Macintosh HD".into(),
            total: 10,
            used: 5,
            is_home: true,
        }];
        let own_start = 1 + 1 + listed_catalog(Os::current(), false).len();
        let target = |row| space_list_target(row, &current, &drives, false);
        assert_eq!(target(own_start - 1), None);
        assert_eq!(
            target(own_start + 1),
            Some(ListTarget {
                kind: AddAs::SpaceRegenerable,
                folder: Some(1)
            })
        );
        assert_eq!(
            target(own_start + 2),
            Some(ListTarget {
                kind: AddAs::SpaceRegenerable,
                folder: None
            })
        );
        assert_eq!(
            target(own_start + 3),
            Some(ListTarget {
                kind: AddAs::SpaceNeverTouch,
                folder: Some(0)
            })
        );
        assert_eq!(target(own_start + 5), None);
    }

    #[test]
    fn a_drive_that_is_not_connected_stays_listed() {
        let current = SpaceSettings {
            warn_drives: vec!["Archiv".into()],
            ..SpaceSettings::default()
        };
        let entries = drive_entries(&current, &[]);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].on && entries[0].measured.is_none());
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
