//! The AI page: the switch of Hermes, the one model he uses (on this computer
//! or on a server) and, for a server off this computer, what goes to it.

use iced::alignment;
use iced::widget::row;

use super::index_page::{load_button, model_control, remove_model_button};
use super::*;
use crate::ai_consent::announced_items;
use crate::ai_server::AiCheckResult;
use crate::facts::{AiState, ModelKind};
use ncrs_ai::Secret;
use ncrs_config::config::AiProviderName;
use ncrs_ui_kit::hermes_kit::{field_dropdown, MenuItem};
use ncrs_ui_kit::layout::SETTINGS_WIDE_CONTROL_WIDTH;

const KEY_FIELD_ID: &str = "settings-ai-key";

pub fn ai_groups<'a>(view: &SettingsView<'a>) -> Vec<Group<'a>> {
    let lang = view.lang;
    let mut groups = vec![
        Group {
            intro: None,
            title: String::new(),
            rows: vec![hermes_switch_row(view)],
        },
        Group {
            intro: None,
            title: lang.text(Msg::SettingsAiSectionModel).to_owned(),
            rows: model_rows(view),
        },
    ];
    if let Some(agreed) = view.state.server_agreed() {
        let bullets = announced_items()
            .map(|item| format!("\u{2022} {}", lang.text(item)))
            .collect::<Vec<_>>()
            .join("\n");
        groups.push(Group {
            intro: Some(format!(
                "{bullets}\n\n{}",
                lang.text(Msg::SettingsAiConsentOutro)
            )),
            title: lang.text(Msg::SettingsAiSectionSent).to_owned(),
            rows: vec![consent_row(agreed, view)],
        });
    }
    groups
}

/// The one switch of Hermes: off, no read folder is watched and no model is
/// called; on, both, with the model that is set up here.
fn hermes_switch_row<'a>(view: &SettingsView<'a>) -> SettingRow<'a> {
    let on = view.filing.hermes_on();
    let options = [
        (
            view.lang.text(Msg::SettingsHermesSwitchOn),
            change(SettingChange::HermesSwitch(true)),
        ),
        (
            view.lang.text(Msg::SettingsHermesSwitchOff),
            change(SettingChange::HermesSwitch(false)),
        ),
    ];
    let chosen = usize::from(!on);
    simple_row(
        Msg::GroupHermes,
        Some(Msg::SettingsHermesRowSwitchNote),
        kit::segmented(&options, chosen, true, view.palette),
        view.lang,
    )
    .keyed_maybe(choice_key(
        options.iter().map(|(_, message)| message.clone()).collect(),
        chosen,
    ))
}

fn model_rows<'a>(view: &SettingsView<'a>) -> Vec<SettingRow<'a>> {
    let mut rows = vec![provider_row(view)];
    match view.state.provider() {
        AiProviderName::Local => rows.push(local_model_row(view)),
        AiProviderName::Http => rows.extend(server_rows(view)),
    }
    rows
}

fn provider_row<'a>(view: &SettingsView<'a>) -> SettingRow<'a> {
    let lang = view.lang;
    let providers = [
        (Msg::SettingsProviderLocal, AiProviderName::Local),
        (Msg::SettingsProviderHttp, AiProviderName::Http),
    ];
    let options: Vec<(&str, Message)> = providers
        .iter()
        .map(|&(label, provider)| (lang.text(label), SettingsMessage::ChooseProvider(provider)))
        .collect();
    let provider = view.state.provider();
    let chosen = providers
        .iter()
        .position(|&(_, listed)| listed == provider)
        .unwrap_or_default();
    simple_row(
        Msg::SettingsRowAiProvider,
        Some(Msg::SettingsRowAiProviderNote),
        kit::segmented(&options, chosen, true, view.palette),
        lang,
    )
    .keyed_maybe(choice_key(
        options.iter().map(|(_, message)| message.clone()).collect(),
        chosen,
    ))
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

/// How the AI stands, worded, with the colour of its word.
fn ai_state_text(ai: &AiState, view: &SettingsView<'_>) -> (String, iced::Color) {
    let (lang, palette) = (view.lang, view.palette);
    match ai {
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
            lang.text(Msg::SettingsAiConsentStateMissing).to_owned(),
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
    }
}

/// The model on this computer: its state and the button that loads, turns on
/// or removes it.
fn local_model_row<'a>(view: &SettingsView<'a>) -> SettingRow<'a> {
    let (lang, palette) = (view.lang, view.palette);
    // Without a running module the file on disk tells whether the model is there.
    let ai = match (&view.facts.ai, view.state.ai_model_present) {
        (AiState::Missing, true) => &AiState::Ready,
        (other, _) => other,
    };
    let (state, color) = ai_state_text(ai, view);
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
    let mut model = simple_row(
        Msg::SettingsAiRowLocal,
        Some(Msg::SettingsIndexRowAiNote),
        model_control(state, color, button.into_iter().collect()),
        lang,
    );
    model.key = ai_press.map(RowKey::Press);
    if let Some(detail) = &view.facts.ai_detail {
        let note = lang.text(Msg::SettingsIndexRowAiNote);
        model.description = Some(format!("{note}\n{detail}"));
    }
    model
}

/// The own server: address, model, key and the buttons that test and save.
fn server_rows<'a>(view: &SettingsView<'a>) -> Vec<SettingRow<'a>> {
    let wide = |row: SettingRow<'a>| SettingRow {
        control_width: Some(SETTINGS_WIDE_CONTROL_WIDTH),
        ..row
    };
    vec![
        wide(field_row(
            SettingField::AiEndpoint,
            (
                Msg::SettingsRowAiEndpoint,
                Msg::SettingsRowAiEndpointNote,
                ENDPOINT_FIELD_ID,
            ),
            view,
        )),
        wide(with_model_menu(
            field_row(
                SettingField::AiModel,
                (
                    Msg::SettingsRowAiModelName,
                    Msg::SettingsRowAiModelNameNote,
                    MODEL_FIELD_ID,
                ),
                view,
            ),
            view,
        )),
        wide(key_row(view)),
        connection_row(view),
    ]
}

fn field_row<'a>(
    field: SettingField,
    (label, note, id): (Msg, Msg, &'static str),
    view: &SettingsView<'a>,
) -> SettingRow<'a> {
    SettingRow {
        key: Some(RowKey::Text(id)),
        problem: field_problem(field, view),
        ..simple_row(
            label,
            Some(note),
            setting_field(field, id, "", true, view),
            view.lang,
        )
    }
}

fn key_row<'a>(view: &SettingsView<'a>) -> SettingRow<'a> {
    let typed = view
        .state
        .ai_key
        .as_ref()
        .map_or("", |key| key.expose())
        .to_owned();
    SettingRow {
        key: Some(RowKey::Text(KEY_FIELD_ID)),
        ..simple_row(
            Msg::SettingsRowAiKey,
            Some(Msg::SettingsRowAiKeyNote),
            kit::secret_field(
                "",
                &typed,
                KEY_FIELD_ID,
                Some(|text: String| SettingsMessage::AiKey(Secret::new(text))),
                SettingsMessage::TestAiServer,
                view.palette,
            ),
            view.lang,
        )
    }
}

/// After a test that returned a list the model is picked from it; otherwise
/// it stays the text field.
fn with_model_menu<'a>(mut model: SettingRow<'a>, view: &SettingsView<'a>) -> SettingRow<'a> {
    let Some(listed) = view.state.listed_models() else {
        return model;
    };
    let current = view.state.field_text(SettingField::AiModel);
    let pick = |name: &str| SettingsMessage::Edit {
        field: SettingField::AiModel,
        text: name.to_owned(),
    };
    let mut items: Vec<MenuItem<Message>> = Vec::new();
    if current.is_empty() {
        items.push(MenuItem::new(
            view.lang.text(Msg::SettingsAiChooseModel),
            pick(""),
        ));
    } else if !listed.contains(&current) {
        items.push(MenuItem::new(current.clone(), pick(&current)));
    }
    items.extend(
        listed
            .iter()
            .map(|name| MenuItem::new(name.clone(), pick(name))),
    );
    let selected = items
        .iter()
        .position(|item| item.label == current)
        .unwrap_or_default();
    model.control = field_dropdown(SETTINGS_WIDE_CONTROL_WIDTH, &items, selected, view.palette);
    model.key = Some(dropdown_key(&items, selected));
    model
}

/// What the test found, else how the saved server answers: a line under the
/// label, and a problem in red.
fn connection_status(view: &SettingsView<'_>) -> (Option<String>, Option<String>) {
    let lang = view.lang;
    match &view.state.ai_check {
        AiCheck::Asking => (Some(lang.text(Msg::SettingsAiTestAsking).to_owned()), None),
        AiCheck::Done(AiCheckResult::Listed(models)) => (
            Some(fill_template(
                lang.text(Msg::SettingsAiTestListed),
                &[("{count}", &models.len().to_string())],
            )),
            None,
        ),
        AiCheck::Done(AiCheckResult::NotListed) => (
            Some(lang.text(Msg::SettingsAiTestNotListed).to_owned()),
            None,
        ),
        AiCheck::Done(AiCheckResult::KeyRefused) => (
            None,
            Some(lang.text(Msg::SettingsAiTestKeyRefused).to_owned()),
        ),
        AiCheck::Done(AiCheckResult::ServerError(code)) => (
            None,
            Some(fill_template(
                lang.text(Msg::SettingsAiTestServerError),
                &[("{status}", &code.to_string())],
            )),
        ),
        AiCheck::Done(AiCheckResult::NoAnswer) => (
            None,
            Some(lang.text(Msg::SettingsAiTestNoAnswer).to_owned()),
        ),
        AiCheck::Idle => match &view.facts.ai {
            AiState::OwnServer {
                failure: Some(failure),
                ..
            } if view.state.saved_provider() == AiProviderName::Http => {
                (None, Some(failure.clone()))
            }
            AiState::OwnServer { .. } if view.state.saved_provider() == AiProviderName::Http => {
                (Some(ai_state_text(&view.facts.ai, view).0), None)
            }
            _ => (None, None),
        },
    }
}

fn connection_row<'a>(view: &SettingsView<'a>) -> SettingRow<'a> {
    let (lang, palette) = (view.lang, view.palette);
    let asking = view.state.ai_check == AiCheck::Asking;
    let test = kit::button(
        lang.text(Msg::SettingsAiTest),
        (!asking).then_some(SettingsMessage::TestAiServer),
        Tone::Secondary,
        palette,
    );
    let save = kit::button(
        lang.text(Msg::SettingsServerDialogSave),
        Some(SettingsMessage::SaveServer),
        Tone::Primary,
        palette,
    );
    let (description, problem) = connection_status(view);
    SettingRow {
        description,
        problem,
        key: Some(RowKey::Buttons(vec![
            SettingsMessage::TestAiServer,
            SettingsMessage::SaveServer,
        ])),
        control_width: Some(SETTINGS_WIDE_CONTROL_WIDTH),
        ..simple_row(
            Msg::SettingsAiRowConnection,
            None,
            row![test, save]
                .spacing(spacing::M)
                .align_y(alignment::Vertical::Center)
                .into(),
            lang,
        )
    }
}

/// The agreement to send to a server off this computer.
fn consent_row<'a>(agreed: bool, view: &SettingsView<'a>) -> SettingRow<'a> {
    let (lang, palette) = (view.lang, view.palette);
    let (state, color, buttons) = if agreed {
        (
            Msg::SettingsAiConsentStateGiven,
            palette.dim_text,
            Vec::new(),
        )
    } else {
        let allow = kit::button(
            lang.text(Msg::SettingsAiConsentConfirm),
            Some(SettingsMessage::ConfirmAiConsent),
            Tone::Secondary,
            palette,
        );
        (
            Msg::SettingsAiConsentStateMissing,
            palette.dim_text,
            vec![allow],
        )
    };
    SettingRow {
        key: (!agreed).then_some(RowKey::Press(SettingsMessage::ConfirmAiConsent)),
        ..simple_row(
            Msg::SettingsAiConsentRow,
            None,
            model_control(lang.text(state).to_owned(), color, buttons),
            lang,
        )
    }
}
