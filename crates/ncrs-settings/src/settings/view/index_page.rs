//! The Index page: what ncrs reads, how it reads it, what it knows about the
//! user, how the index is protected and how far it is.

use iced::widget::{column, row, Row};
use iced::{alignment, Element};

use super::*;
use crate::facts::{AiState, ModelKind};
use crate::folders::AddAs;
use crate::message::{SettingChange, SettingsMessage};
use ncrs_config::config::{FilingLocation, OwnDetail};
use ncrs_pipeline::Load;
use ncrs_ui_kit::layout::SETTINGS_WIDE_CONTROL_WIDTH;
use ncrs_ui_kit::list_kit::segmented_some_unavailable;

pub fn index_groups<'a>(view: &SettingsView<'a>) -> Vec<Group<'a>> {
    let lang = view.lang;
    let mut groups = vec![
        titled(Msg::SettingsIndexSectionRead, None, read_rows(view), view),
        titled(
            Msg::SettingsIndexSectionOcr,
            None,
            ocr_section_rows(view),
            view,
        ),
        titled(Msg::SettingsIndexSectionAi, None, ai_rows(view), view),
        titled(
            Msg::SettingsIndexSectionOwn,
            Some(Msg::SettingsGroupOwnDetailsIntro),
            [OwnDetail::Names, OwnDetail::Ibans, OwnDetail::TaxIds]
                .into_iter()
                .map(|detail| own_detail_row(detail, view))
                .collect(),
            view,
        ),
    ];
    groups.extend(protection_group(view).map(|group| Group {
        title: lang.text(Msg::SettingsIndexSectionProtection).to_owned(),
        ..group
    }));
    groups.push(titled(
        Msg::SettingsIndexSectionState,
        None,
        state_rows(view),
        view,
    ));
    groups
}

fn titled<'a>(
    title: Msg,
    intro: Option<Msg>,
    rows: Vec<SettingRow<'a>>,
    view: &SettingsView<'a>,
) -> Group<'a> {
    Group {
        intro: intro.map(|msg| view.lang.text(msg).to_owned()),
        title: view.lang.text(title).to_owned(),
        rows,
    }
}

/// A row of these pages whose texts are worded already. They show state and
/// the user's own data, so the command list does not take them.
pub fn text_row<'a>(
    label: String,
    description: Option<String>,
    control: Element<'a, Message>,
) -> SettingRow<'a> {
    SettingRow::text(label, description, control)
}

/// The locations as the file says them, else as the module runs them.
pub fn listed_locations<'a>(view: &SettingsView<'a>) -> &'a [FilingLocation] {
    view.state
        .config()
        .map_or(view.filing.locations.as_slice(), |config| {
            config.filing.locations.as_slice()
        })
}

fn folder_name(location: &FilingLocation) -> String {
    location
        .path
        .file_name()
        .unwrap_or(location.path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

fn folder_is_disconnected(location: &FilingLocation, view: &SettingsView<'_>) -> bool {
    view.facts.disconnected.contains(&location.path)
}

/// The line of a folder: its name, where it is with how many documents the
/// index holds from it when `files` says so, and a warning when its drive is
/// not connected. `buttons` act on it.
pub fn folder_row<'a>(
    location: &FilingLocation,
    status: Option<String>,
    files: Option<usize>,
    buttons: Vec<RowButton<Message>>,
    view: &SettingsView<'a>,
) -> SettingRow<'a> {
    let lang = view.lang;
    let path = shown_path(&location.path, view.home);
    let where_it_is = match files {
        Some(1) => fill_template(
            lang.text(Msg::SettingsIndexFolderOneFile),
            &[("{path}", &path)],
        ),
        Some(count) => fill_template(
            lang.text(Msg::SettingsIndexFolderFiles),
            &[("{path}", &path), ("{count}", &count.to_string())],
        ),
        None => path,
    };
    SettingRow {
        warning: folder_is_disconnected(location, view)
            .then(|| lang.text(Msg::SettingsIndexFolderMissing).to_owned()),
        ..SettingRow::listed_buttons(
            folder_name(location),
            Some(where_it_is),
            status,
            buttons,
            view.palette,
        )
    }
}

pub fn remove_folder_button(
    location: &FilingLocation,
    view: &SettingsView<'_>,
) -> RowButton<Message> {
    RowButton::new(
        view.lang.text(Msg::SettingsRemove),
        Some(SettingsMessage::RemoveFolder(location.path.clone())),
        Tone::Secondary,
    )
}

/// The button under a folder list, and the note when a change of the list was
/// refused.
pub fn add_folder_row<'a>(
    kind: AddAs,
    empty: Option<Msg>,
    mut buttons: Vec<RowButton<Message>>,
    view: &SettingsView<'a>,
) -> SettingRow<'a> {
    let lang = view.lang;
    buttons.push(RowButton::new(
        lang.text(Msg::SettingsFolderAdd),
        Some(SettingsMessage::AddFolder(kind)),
        Tone::Secondary,
    ));
    SettingRow::adding_buttons(
        empty.map(|msg| lang.text(msg).to_owned()),
        view.state.folder_note.clone(),
        buttons,
        view.palette,
    )
}

fn read_rows<'a>(view: &SettingsView<'a>) -> Vec<SettingRow<'a>> {
    let lang = view.lang;
    let scope = segmented_some_unavailable(
        &[
            (lang.text(Msg::SettingsIndexScopeAll), None),
            (
                lang.text(Msg::SettingsIndexScopeOnly),
                Some(SettingsMessage::Select(SettingsFeature::Index)),
            ),
        ],
        1,
        view.palette,
    );
    let locations = listed_locations(view);
    let mut rows = vec![text_row(
        lang.text(Msg::SettingsIndexRowScope).to_owned(),
        Some(lang.text(Msg::SettingsIndexScopeOnlyHint).to_owned()),
        scope,
    )
    .keyed(RowKey::Press(SettingsMessage::Select(
        SettingsFeature::Index,
    )))];
    rows.extend(locations.iter().map(|location| {
        folder_row(
            location,
            None,
            view.facts.folder_files.get(&location.path).copied(),
            vec![remove_folder_button(location, view)],
            view,
        )
    }));
    rows.push(add_folder_row(
        AddAs::Indexed,
        locations
            .is_empty()
            .then_some(Msg::SettingsIndexFoldersEmpty),
        Vec::new(),
        view,
    ));
    rows
}

/// A state word with a model's buttons beside it.
fn model_control<'a>(
    state: String,
    color: iced::Color,
    buttons: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    buttons
        .into_iter()
        .fold(
            Row::new()
                .push(status_text(state, color))
                .spacing(spacing::M)
                .align_y(alignment::Vertical::Center),
            |controls, button| controls.push(button),
        )
        .into()
}

fn load_button<'a>(on_press: Message, view: &SettingsView<'a>) -> Element<'a, Message> {
    kit::button(
        view.lang.text(Msg::SettingsModelsDownload),
        Some(on_press),
        Tone::Secondary,
        view.palette,
    )
}

fn remove_model_button<'a>(kind: ModelKind, view: &SettingsView<'a>) -> Element<'a, Message> {
    kit::button(
        view.lang.text(Msg::SettingsRemove),
        Some(SettingsMessage::AskRemoveModel(kind)),
        Tone::Secondary,
        view.palette,
    )
}

fn ocr_section_rows<'a>(view: &SettingsView<'a>) -> Vec<SettingRow<'a>> {
    let lang = view.lang;
    let palette = view.palette;
    let mut rows = Vec::new();
    if view.facts.system_reads_scans {
        rows.push(simple_row(
            Msg::SettingsIndexRowOcrSystem,
            Some(Msg::SettingsIndexRowOcrSystemNote),
            status_text(
                lang.text(Msg::SettingsIndexStateReady).to_owned(),
                palette.success,
            ),
            lang,
        ));
    }
    let (state, color, button) = if view.state.ocr_models_present {
        (
            Msg::SettingsIndexStateReady,
            palette.success,
            remove_model_button(ModelKind::Ocr, view),
        )
    } else {
        (
            Msg::SettingsModelsMissing,
            palette.text,
            load_button(SettingsMessage::OpenOcrModels, view),
        )
    };
    rows.push(
        simple_row(
            Msg::SettingsIndexRowOcrFallback,
            Some(Msg::SettingsIndexRowOcrFallbackNote),
            model_control(lang.text(state).to_owned(), color, vec![button]),
            lang,
        )
        .keyed(RowKey::Press(if view.state.ocr_models_present {
            SettingsMessage::AskRemoveModel(ModelKind::Ocr)
        } else {
            SettingsMessage::OpenOcrModels
        })),
    );
    rows
}

#[derive(Debug, PartialEq, Eq)]
pub enum AiButton {
    Remove,
    /// The model is on disk and only the switch is off.
    TurnOn,
    Load,
}

pub fn ai_button(ai: &AiState, model_present: bool) -> Option<AiButton> {
    match ai {
        AiState::Ready | AiState::NextStart => Some(AiButton::Remove),
        AiState::Off if model_present => Some(AiButton::TurnOn),
        AiState::Off | AiState::Missing | AiState::Failed => Some(AiButton::Load),
        AiState::Downloading | AiState::NotAllowed | AiState::OwnServer { .. } => None,
    }
}

fn ai_rows<'a>(view: &SettingsView<'a>) -> Vec<SettingRow<'a>> {
    let lang = view.lang;
    let palette = view.palette;
    // Without a running module the file on disk tells whether the model is there.
    let ai = match (&view.facts.ai, view.state.ai_model_present) {
        (AiState::Missing, true) => &AiState::Ready,
        (other, _) => other,
    };
    let (state, color) = match ai {
        AiState::Off => (
            lang.text(Msg::SettingsIndexAiOff).to_owned(),
            palette.dim_text,
        ),
        AiState::Missing => (
            lang.text(Msg::SettingsIndexAiMissing).to_owned(),
            palette.text,
        ),
        AiState::Downloading => (
            lang.text(Msg::SettingsIndexAiDownloading).to_owned(),
            palette.dim_text,
        ),
        AiState::Ready => (
            lang.text(Msg::SettingsIndexAiReady).to_owned(),
            palette.success,
        ),
        AiState::OwnServer {
            failure: Some(failure),
            ..
        } => (failure.clone(), palette.error),
        AiState::OwnServer {
            model,
            failure: None,
        } => (
            fill_template(lang.text(Msg::SettingsIndexAiServer), &[("{model}", model)]),
            palette.success,
        ),
        AiState::NotAllowed => (
            lang.text(Msg::SettingsIndexAiNotAllowed).to_owned(),
            palette.text,
        ),
        AiState::Failed => (
            lang.text(Msg::SettingsIndexAiFailed).to_owned(),
            palette.error,
        ),
        AiState::NextStart => (
            lang.text(Msg::SettingsIndexAiNextStart).to_owned(),
            palette.dim_text,
        ),
    };
    let ai_kind = ai_button(ai, view.state.ai_model_present);
    let ai_press = ai_kind.as_ref().map(|kind| match kind {
        AiButton::Remove => SettingsMessage::AskRemoveModel(ModelKind::Ai),
        AiButton::TurnOn => SettingsMessage::Change(SettingChange::HermesSwitch(true)),
        AiButton::Load => SettingsMessage::OpenAiModel,
    });
    let button = ai_kind.map(|kind| match kind {
        AiButton::Remove => remove_model_button(ModelKind::Ai, view),
        AiButton::TurnOn => kit::button(
            lang.text(Msg::AiModelTurnOn),
            Some(SettingsMessage::Change(SettingChange::HermesSwitch(true))),
            Tone::Secondary,
            palette,
        ),
        AiButton::Load => load_button(SettingsMessage::OpenAiModel, view),
    });
    // The local model's name would be wrong while the own server answers.
    let note_msg = if matches!(ai, AiState::OwnServer { .. } | AiState::NotAllowed) {
        Msg::SettingsIndexRowAiNoteServer
    } else {
        Msg::SettingsIndexRowAiNote
    };
    let mut model = simple_row(
        Msg::SettingsIndexRowAi,
        Some(note_msg),
        model_control(state, color, button.into_iter().collect()),
        lang,
    );
    model.key = ai_press.map(RowKey::Press);
    if let Some(detail) = &view.facts.ai_detail {
        let note = lang.text(note_msg);
        model.description = Some(format!("{note}\n{detail}"));
    }
    let server = simple_row(
        Msg::SettingsIndexRowServer,
        Some(Msg::SettingsIndexRowServerNote),
        kit::button(
            lang.text(Msg::SettingsIndexServerSetup),
            Some(SettingsMessage::ServerDialog(true)),
            Tone::Secondary,
            palette,
        ),
        lang,
    )
    .keyed(RowKey::Press(SettingsMessage::ServerDialog(true)));
    vec![model, server]
}

/// The list of one own detail as chips with a cross, the field for the next
/// entry and the button that adds what is typed.
pub fn own_detail_row<'a>(detail: OwnDetail, view: &SettingsView<'a>) -> SettingRow<'a> {
    let lang = view.lang;
    let field = SettingField::Own(detail);
    let (label, placeholder, id) = match detail {
        OwnDetail::Names => (
            Msg::SettingsRowOwnNames,
            Msg::SettingsOwnNamesPlaceholder,
            OWN_NAMES_FIELD_ID,
        ),
        OwnDetail::Ibans => (
            Msg::SettingsRowOwnIbans,
            Msg::SettingsOwnIbansPlaceholder,
            OWN_IBANS_FIELD_ID,
        ),
        OwnDetail::TaxIds => (
            Msg::SettingsRowOwnTaxIds,
            Msg::SettingsOwnTaxIdsPlaceholder,
            OWN_TAX_IDS_FIELD_ID,
        ),
    };
    let entries = view.state.own_entries(detail);
    let chips: Vec<Element<'a, Message>> = entries
        .iter()
        .map(|entry| {
            let remaining = entries
                .iter()
                .filter(|other| *other != entry)
                .cloned()
                .collect();
            kit::removable_chip(
                entry,
                change(SettingChange::OwnDetails {
                    detail,
                    entries: remaining,
                }),
                view.palette,
            )
        })
        .collect();
    let input = row![
        setting_field(field, id, lang.text(placeholder), true, view),
        kit::button(
            lang.text(Msg::SettingsIndexOwnAdd),
            Some(SettingsMessage::AddOwnDetail(detail)),
            Tone::Secondary,
            view.palette,
        ),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    let control: Element<'a, Message> = if chips.is_empty() {
        input.into()
    } else {
        column![Row::with_children(chips).spacing(spacing::XS).wrap(), input]
            .spacing(spacing::XS)
            .into()
    };
    SettingRow {
        key: Some(RowKey::Text(id)),
        label: lang.text(label).to_owned(),
        theme_name: None,
        config_key: None,
        description: matches!(detail, OwnDetail::Names)
            .then(|| lang.text(Msg::SettingsIndexOwnNote).to_owned()),
        problem: field_problem(field, view),
        warning: None,
        control,
        control_width: Some(SETTINGS_WIDE_CONTROL_WIDTH),
        in_command_list: true,
        list_indent: None,
        marked_controls: Vec::new(),
        leading: None,
    }
}

fn state_rows<'a>(view: &SettingsView<'a>) -> Vec<SettingRow<'a>> {
    let lang = view.lang;
    let palette = view.palette;
    let facts = &view.facts;
    let counts = fill_template(
        lang.text(Msg::SettingsIndexStateCounts),
        &[
            ("{documents}", &facts.documents.to_string()),
            ("{understood}", &facts.understood.to_string()),
        ],
    );
    let pause = kit::button(
        lang.text(if facts.paused {
            Msg::SettingsIndexResume
        } else {
            Msg::SettingsIndexPause
        }),
        Some(SettingsMessage::TogglePause),
        Tone::Secondary,
        palette,
    );
    let loads = [
        (Msg::SettingsIndexLoadGentle, Load::Gentle),
        (Msg::SettingsIndexLoadBalanced, Load::Balanced),
        (Msg::SettingsIndexLoadFull, Load::Full),
    ];
    let options: Vec<(&str, Message)> = loads
        .iter()
        .map(|&(label, load)| (lang.text(label), change(SettingChange::Load(load))))
        .collect();
    let chosen = loads
        .iter()
        .position(|&(_, load)| load == view.filing.load)
        .unwrap_or(loads.len());
    let mut rows = vec![
        text_row(
            counts,
            Some(lang.text(Msg::SettingsIndexStateNote).to_owned()),
            pause,
        )
        .keyed(RowKey::Press(SettingsMessage::TogglePause)),
        simple_row(
            Msg::SettingsIndexRowLoad,
            Some(Msg::SettingsIndexRowLoadNote),
            kit::segmented(&options, chosen, true, palette),
            lang,
        )
        .keyed_maybe(choice_key(
            options.iter().map(|(_, message)| message.clone()).collect(),
            chosen,
        )),
    ];
    if facts.unreadable > 0 {
        let text = match facts.unreadable {
            1 => lang.text(Msg::SettingsIndexErrorsOne).to_owned(),
            count => fill_template(
                lang.text(Msg::SettingsIndexErrors),
                &[("{count}", &count.to_string())],
            ),
        };
        rows.push(
            text_row(
                text,
                None,
                kit::button(
                    lang.text(Msg::SettingsIndexErrorsShow),
                    Some(SettingsMessage::Select(SettingsFeature::ReadErrors)),
                    Tone::Secondary,
                    palette,
                ),
            )
            .keyed(RowKey::Press(SettingsMessage::Select(
                SettingsFeature::ReadErrors,
            ))),
        );
    }
    rows
}
