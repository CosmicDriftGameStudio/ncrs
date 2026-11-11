//! The body of the dialog of the own server, top to bottom: provider, address,
//! key, test, model, and what leaving the computer means.

use iced::widget::{column, row, Column};
use iced::Element;

use super::*;
use ncrs_ai::Secret;
use ncrs_config::config::AiProviderName;
use ncrs_ui_kit::hermes_kit::{field_dropdown, MenuItem};
use ncrs_ui_kit::kit::{self, Tone};
use ncrs_ui_kit::setting_rows::{simple_row, status_text};
use ncrs_ui_kit::theme::spacing;

use crate::ai_server::AiCheckResult;

const KEY_FIELD_ID: &str = "settings-ai-key";

/// The text fields of the wide dialog: its 640 less the padding on both sides.
const FIELD_WIDTH: f32 = 608.0;

fn push_row<'a>(
    body: Column<'a, Message>,
    setting: SettingRow<'a>,
    palette: &Palette,
) -> Column<'a, Message> {
    let described = match setting.description {
        Some(description) => body
            .push(kit::paragraph(setting.label, palette))
            .push(kit::dim(description, palette)),
        None => body.push(kit::paragraph(setting.label, palette)),
    };
    let with_control = described.push(setting.control);
    match setting.problem {
        Some(problem) => with_control.push(kit::error_line(problem, palette)),
        None => with_control,
    }
}

fn key_row<'a>(view: &SettingsView<'a>) -> SettingRow<'a> {
    let typed = view
        .state
        .ai_key
        .as_ref()
        .map_or("", |key| key.expose())
        .to_owned();
    simple_row(
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

fn test_row<'a>(view: &SettingsView<'a>) -> Element<'a, Message> {
    let (lang, palette) = (view.lang, view.palette);
    let asking = view.state.ai_check == AiCheck::Asking;
    let button = kit::button(
        lang.text(Msg::SettingsAiTest),
        (!asking).then_some(SettingsMessage::TestAiServer),
        Tone::Secondary,
        palette,
    );
    let status = match &view.state.ai_check {
        AiCheck::Idle => None,
        AiCheck::Asking => Some((
            lang.text(Msg::SettingsAiTestAsking).to_owned(),
            palette.dim_text,
        )),
        AiCheck::Done(AiCheckResult::Listed(models)) => Some((
            fill_template(
                lang.text(Msg::SettingsAiTestListed),
                &[("{count}", &models.len().to_string())],
            ),
            palette.success,
        )),
        AiCheck::Done(AiCheckResult::NotListed) => Some((
            lang.text(Msg::SettingsAiTestNotListed).to_owned(),
            palette.text,
        )),
        AiCheck::Done(AiCheckResult::KeyRefused) => Some((
            lang.text(Msg::SettingsAiTestKeyRefused).to_owned(),
            palette.error,
        )),
        AiCheck::Done(AiCheckResult::ServerError(code)) => Some((
            fill_template(
                lang.text(Msg::SettingsAiTestServerError),
                &[("{status}", &code.to_string())],
            ),
            palette.error,
        )),
        AiCheck::Done(AiCheckResult::NoAnswer) => Some((
            lang.text(Msg::SettingsAiTestNoAnswer).to_owned(),
            palette.error,
        )),
    };
    let mut line = row![button]
        .spacing(spacing::M)
        .align_y(iced::alignment::Vertical::Center);
    if let Some((content, color)) = status {
        line = line.push(status_text(content, color));
    }
    line.into()
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
    model.control = field_dropdown(FIELD_WIDTH, &items, selected, view.palette);
    model
}

fn leaving_notice<'a>(view: &SettingsView<'a>) -> Option<Element<'a, Message>> {
    let (lang, palette) = (view.lang, view.palette);
    let host = view.state.host_documents_go_to()?;
    Some(kit::warning_box(
        fill_template(
            lang.text(Msg::SettingsAiLeavingWarning),
            &[("{host}", &host)],
        ),
        palette,
    ))
}

/// The switch back to the local model, and while the own server is chosen
/// everything it needs.
pub fn server_dialog_body<'a>(view: &SettingsView<'a>) -> Element<'a, Message> {
    let palette = view.palette;
    let mut rows = provider_rows(view);
    let model = if rows.len() == 3 { rows.pop() } else { None };
    let mut body = column![].spacing(spacing::LINE_GAP);
    for setting in rows {
        body = push_row(body, setting, palette);
    }
    if view.state.provider() == AiProviderName::Http {
        body = push_row(body, key_row(view), palette).push(test_row(view));
        if let Some(model_row) = model {
            body = push_row(body, with_model_menu(model_row, view), palette);
        }
        if let Some(notice) = leaving_notice(view) {
            body = body.push(notice);
        }
    }
    body.into()
}
