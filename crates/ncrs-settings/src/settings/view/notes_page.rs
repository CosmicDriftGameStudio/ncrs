//! The Notes page: where the notes are kept, the copy in a second folder,
//! the encryption and what Hermes may do with the notes.

use ncrs_core::notes_sync::{NotesSettings, NotesSyncDirection, NotesSyncWhen, NOTES_LOCK_MINUTES};

use super::*;
use crate::message::NotesEdit;

const SYNC_FOLDER_FIELD_ID: &str = "settings-notes-sync-folder";
const RESTORE_FIELD_ID: &str = "settings-notes-restore";

fn edit(notes_edit: NotesEdit) -> Message {
    SettingsMessage::Notes(notes_edit)
}

pub fn notes_groups<'a>(view: &SettingsView<'a>) -> Vec<Group<'a>> {
    let lang = view.lang;
    let current = view
        .state
        .config()
        .map(|config| config.notes.settings())
        .unwrap_or_default();
    vec![
        Group {
            intro: None,
            title: lang.text(Msg::SettingsNotesTitleFolder).to_owned(),
            rows: vec![folder_row(view)],
        },
        Group {
            intro: None,
            title: lang.text(Msg::SettingsNotesTitleSync).to_owned(),
            rows: sync_rows(view, &current),
        },
        Group {
            intro: None,
            title: lang.text(Msg::SettingsNotesTitleEncryption).to_owned(),
            rows: encryption_rows(view, &current),
        },
        Group {
            intro: None,
            title: lang.text(Msg::SettingsNotesTitleHermes).to_owned(),
            rows: vec![hermes_row(view, &current)],
        },
    ]
}

fn folder_row<'a>(view: &SettingsView<'a>) -> SettingRow<'a> {
    let lang = view.lang;
    let folder = view
        .state
        .config()
        .and_then(|config| config.notes.folder.clone())
        .unwrap_or_else(|| view.home.join("Notes"));
    let mut row = SettingRow::text(
        lang.text(Msg::SettingsRowNotesFolder).to_owned(),
        Some(lang.text(Msg::SettingsRowNotesFolderNote).to_owned()),
        status_text(shown_path(&folder, view.home), view.palette.text),
    );
    row.config_key = Some("notes.folder".to_owned());
    row
}

/// A row of choices on `current`, each one changing it as `edit_of` says.
fn choice_row<'a>(
    label: Msg,
    note: Option<Msg>,
    choices: Vec<(String, NotesSettings, bool)>,
    view: &SettingsView<'a>,
) -> SettingRow<'a> {
    let chosen = choices
        .iter()
        .position(|(_, _, is_current)| *is_current)
        .unwrap_or_default();
    let options: Vec<(&str, Message)> = choices
        .iter()
        .map(|(name, settings, _)| {
            (
                name.as_str(),
                change(SettingChange::Notes(settings.clone())),
            )
        })
        .collect();
    let keys = options.iter().map(|(_, message)| message.clone()).collect();
    simple_row(
        label,
        note,
        kit::segmented(&options, chosen, true, view.palette),
        view.lang,
    )
    .keyed_maybe(choice_key(keys, chosen))
}

fn sync_rows<'a>(view: &SettingsView<'a>, current: &NotesSettings) -> Vec<SettingRow<'a>> {
    let lang = view.lang;
    let with = |edit: &dyn Fn(&mut NotesSettings)| {
        let mut next = current.clone();
        edit(&mut next);
        next
    };
    let typed = view
        .state
        .notes
        .sync_folder_draft
        .clone()
        .unwrap_or_else(|| {
            current
                .sync_folder
                .as_ref()
                .map(|folder| folder.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    let folder = SettingRow::text(
        lang.text(Msg::SettingsRowNotesSyncFolder).to_owned(),
        Some(lang.text(Msg::SettingsRowNotesSyncFolderNote).to_owned()),
        kit::mono_text_field(
            lang.text(Msg::SettingsNotesSyncFolderPlaceholder),
            &typed,
            SYNC_FOLDER_FIELD_ID,
            Some(|text| edit(NotesEdit::SyncFolderText(text))),
            edit(NotesEdit::SyncFolderSubmit),
            view.palette,
        ),
    );
    let direction = choice_row(
        Msg::SettingsRowNotesDirection,
        Some(Msg::SettingsRowNotesDirectionNote),
        [
            (NotesSyncDirection::OneWay, Msg::SettingsNotesOneWay),
            (NotesSyncDirection::BothWays, Msg::SettingsNotesBothWays),
        ]
        .into_iter()
        .map(|(direction, name)| {
            (
                lang.text(name).to_owned(),
                with(&|settings| settings.direction = direction),
                current.direction == direction,
            )
        })
        .collect(),
        view,
    );
    let mut times = vec![
        (
            NotesSyncWhen::AfterSave,
            lang.text(Msg::SettingsNotesAfterSave).to_owned(),
        ),
        (
            NotesSyncWhen::AtStart,
            lang.text(Msg::SettingsNotesAtStart).to_owned(),
        ),
    ];
    times.extend(NotesSyncWhen::INTERVALS.map(|minutes| {
        (
            NotesSyncWhen::EveryMinutes(minutes),
            fill_template(
                lang.text(Msg::SettingsNotesEveryMinutes),
                &[("{minutes}", &minutes.to_string())],
            ),
        )
    }));
    let when = choice_row(
        Msg::SettingsRowNotesWhen,
        None,
        times
            .into_iter()
            .map(|(when, name)| {
                (
                    name,
                    with(&|settings| settings.when = when),
                    current.when == when,
                )
            })
            .collect(),
        view,
    );
    let notes = &view.state.notes;
    let last = match (&notes.last_sync, notes.syncing) {
        (_, true) => lang.text(Msg::SettingsNotesSyncing).to_owned(),
        (Some(line), false) => line.clone(),
        (None, false) => lang.text(Msg::SettingsNotesNeverSynced).to_owned(),
    };
    let can_sync = current.sync_folder.is_some() && !notes.syncing;
    let sync_now = kit::button(
        lang.text(Msg::SettingsNotesSyncNow),
        can_sync.then(|| edit(NotesEdit::SyncNow)),
        Tone::Secondary,
        view.palette,
    );
    let last_row = SettingRow::text(
        lang.text(Msg::SettingsRowNotesLastSync).to_owned(),
        None,
        row![status_text(last, view.palette.dim_text), sync_now]
            .spacing(spacing::M)
            .align_y(iced::alignment::Vertical::Center)
            .into(),
    );
    vec![folder, direction, when, last_row]
}

fn encryption_rows<'a>(view: &SettingsView<'a>, current: &NotesSettings) -> Vec<SettingRow<'a>> {
    let lang = view.lang;
    let palette = view.palette;
    let all = !current.encrypt_all;
    let mut toggled = current.clone();
    toggled.encrypt_all = all;
    let encrypt_all = SettingRow::text(
        lang.text(Msg::SettingsRowNotesEncryptAll).to_owned(),
        Some(lang.text(Msg::SettingsRowNotesEncryptAllNote).to_owned()),
        switch(
            current.encrypt_all,
            SettingChange::Notes(toggled.clone()),
            view,
        ),
    )
    .keyed(RowKey::Press(change(SettingChange::Notes(toggled))));

    let lock_choices = NOTES_LOCK_MINUTES
        .into_iter()
        .map(|minutes| {
            let name = if minutes == 0 {
                lang.text(Msg::SettingsNotesLockNever).to_owned()
            } else {
                fill_template(
                    lang.text(Msg::SettingsNotesLockMinutes),
                    &[("{minutes}", &minutes.to_string())],
                )
            };
            let mut next = current.clone();
            next.lock_minutes = minutes;
            (name, next, current.lock_minutes == minutes)
        })
        .collect();
    let lock = choice_row(Msg::SettingsRowNotesLock, None, lock_choices, view);

    let notes = &view.state.notes;
    let (shown, label, message) = match &notes.recovery {
        Some(key) => (
            key.clone(),
            Msg::SettingsNotesHideKey,
            edit(NotesEdit::HideRecovery),
        ),
        None => (
            lang.text(Msg::SettingsNotesKeyHidden).to_owned(),
            Msg::SettingsNotesShowKey,
            edit(NotesEdit::ShowRecovery),
        ),
    };
    let recovery = SettingRow::text(
        lang.text(Msg::SettingsRowNotesRecovery).to_owned(),
        Some(lang.text(Msg::SettingsRowNotesRecoveryNote).to_owned()),
        with_button(
            mono(shown, palette.text),
            lang.text(label),
            message,
            palette,
        ),
    );
    let restore = SettingRow::text(
        lang.text(Msg::SettingsRowNotesRestore).to_owned(),
        Some(lang.text(Msg::SettingsRowNotesRestoreNote).to_owned()),
        kit::mono_text_field(
            lang.text(Msg::SettingsNotesRestorePlaceholder),
            &notes.restore_draft,
            RESTORE_FIELD_ID,
            Some(|text| edit(NotesEdit::RestoreText(text))),
            edit(NotesEdit::RestoreSubmit),
            palette,
        ),
    );
    vec![encrypt_all, lock, recovery, restore]
}

fn hermes_row<'a>(view: &SettingsView<'a>, current: &NotesSettings) -> SettingRow<'a> {
    let lang = view.lang;
    let mut toggled = current.clone();
    toggled.hermes_access = !current.hermes_access;
    SettingRow::text(
        lang.text(Msg::SettingsRowNotesHermes).to_owned(),
        Some(lang.text(Msg::SettingsRowNotesHermesNote).to_owned()),
        switch(
            current.hermes_access,
            SettingChange::Notes(toggled.clone()),
            view,
        ),
    )
    .keyed(RowKey::Press(change(SettingChange::Notes(toggled))))
}
