//! The switch to a server on another computer: asked once for each server,
//! written only after the yes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::sync::Arc;

use ncrs_ai::ConsentLedger as _;
use ncrs_config::config::{self, AiProviderName, AiSetting};
use ncrs_config::file_list_tags::FileListTags;
use ncrs_config::keymap::Keymap;
use ncrs_i18n::Language;

use super::update;
use crate::action::Action;
use crate::ctx::{Ctx, Services};
use crate::message::{Message, SettingChange, SettingField, SettingsMessage};
use crate::settings::SettingsState;
use crate::state::Settings;
use crate::write::write_setting_change;

const FAR: &str = "https://ai.example.com/v1";
const FAR_ORIGIN: &str = "https://ai.example.com";

fn services() -> Services {
    Services {
        fetch_extension: |_| panic!("no download in this test"),
        install_theme: |_, _| panic!("no download in this test"),
        download_ocr_models: |_, _| panic!("no download in this test"),
        download_ai_model: |_| panic!("no download in this test"),
        check_ai_server: |_| panic!("no request in this test"),
        store_ai_key: |_, _, _| panic!("no keychain in this test"),
    }
}

struct World {
    keymap: Arc<Keymap>,
    tags: FileListTags,
    config_path: std::path::PathBuf,
    settings: Settings,
}

impl World {
    fn saved(source: &str) -> Self {
        let folder = tempfile::tempdir().unwrap().keep();
        let config_path = folder.join("config.toml");
        std::fs::write(&config_path, source).unwrap();
        let file = config::load(&config_path).unwrap();
        Self {
            keymap: Arc::default(),
            tags: FileListTags::default(),
            config_path,
            settings: Settings {
                page: Some(SettingsState::new(false, Ok(Box::new(file)), false)),
                ..Settings::default()
            },
        }
    }

    fn send(&mut self, message: SettingsMessage) -> (usize, Vec<Action>) {
        let ctx = Ctx {
            lang: Language::English,
            keymap: &self.keymap,
            config_path: Some(&self.config_path),
            home: Path::new("/home"),
            sandbox_active: false,
            blocking_overlay_open: false,
            dialog_open: false,
            ocr_models_dir: None,
            ai_models_dir: None,
            file_list_tags: &self.tags,
            ai_is_on: true,
            usage_log: None,
            services: services(),
        };
        let (task, actions) = update(&mut self.settings, Message::Settings(message), &ctx);
        (task.units(), actions)
    }

    fn page(&self) -> &SettingsState {
        self.settings.page.as_ref().unwrap()
    }

    fn type_server(&mut self, endpoint: &str, model: &str) {
        for (field, text) in [
            (SettingField::AiEndpoint, endpoint),
            (SettingField::AiModel, model),
        ] {
            self.send(SettingsMessage::Edit {
                field,
                text: text.to_owned(),
            });
        }
    }

    /// What the write task of the app does with `change`, for the answer.
    fn written(&mut self, change: SettingChange) -> Vec<Action> {
        let result = write_setting_change(&self.config_path, change.clone());
        self.send(SettingsMessage::Written { change, result }).1
    }

    fn saved_ai(&self) -> AiSetting {
        config::load(&self.config_path).unwrap().filing.ai
    }
}

const LOCAL: &str = "[modules.filing.ai]\nenabled = true\nprovider = \"local\"\n";

fn remote_saved(origin_listed: Option<&str>) -> String {
    let listed = origin_listed.map_or_else(String::new, |origin| {
        format!("consented_origins = [\"{origin}\"]\n")
    });
    format!(
        "[modules.filing.ai]\nenabled = true\nprovider = \"http\"\nendpoint = \"{FAR}\"\nmodel = \"m\"\n{listed}"
    )
}

#[test]
fn from_this_computer_to_a_far_server_the_question_comes_first_and_nothing_is_written() {
    let mut world = World::saved(LOCAL);
    world.send(SettingsMessage::ChooseProvider(AiProviderName::Http));
    world.type_server(FAR, "gpt-5");

    let (writes, _) = world.send(SettingsMessage::SaveServer);

    assert_eq!(writes, 0);
    let ask = world.page().consent_pending.as_ref().unwrap();
    assert_eq!(
        (ask.origin.as_str(), ask.host.as_str()),
        (FAR_ORIGIN, "ai.example.com")
    );
    assert_eq!(world.saved_ai(), AiSetting::Local { threads: None });
}

#[test]
fn cancelling_keeps_the_old_model_and_forgets_what_was_typed_for_the_other() {
    let mut world = World::saved(LOCAL);
    world.send(SettingsMessage::ChooseProvider(AiProviderName::Http));
    world.type_server(FAR, "gpt-5");
    world.send(SettingsMessage::SaveServer);

    let (writes, _) = world.send(SettingsMessage::CancelAiConsent);

    assert_eq!(writes, 0);
    let page = world.page();
    assert_eq!(page.consent_pending, None);
    assert_eq!(page.provider(), AiProviderName::Local);
    assert!(page.drafts.is_empty());
    assert_eq!(world.saved_ai(), AiSetting::Local { threads: None });
}

#[test]
fn confirming_writes_the_switch_with_the_consent_for_that_server_only() {
    let mut world = World::saved(LOCAL);
    world.send(SettingsMessage::ChooseProvider(AiProviderName::Http));
    world.type_server(FAR, "gpt-5");
    world.send(SettingsMessage::SaveServer);
    let ask = world.page().consent_pending.clone().unwrap();

    let (writes, _) = world.send(SettingsMessage::ConfirmAiConsent);

    assert_eq!(writes, 1);
    assert_eq!(world.page().consent_pending, None);
    let actions = world.written(SettingChange::AiProvider(ask.agreed()));
    assert!(actions
        .iter()
        .any(|action| matches!(action, Action::Apply { .. })));
    let AiSetting::Http {
        model,
        consented_origins,
        ..
    } = world.saved_ai()
    else {
        panic!("the switch was not written");
    };
    assert_eq!(model, "gpt-5");
    assert_eq!(consented_origins, [FAR_ORIGIN]);
    let ledger = ncrs_ai::ConsentGivenInSettings::new(&consented_origins);
    assert!(ledger.granted(FAR_ORIGIN, Path::new("")));
    assert!(!ledger.granted("https://other.example.org", Path::new("")));
}

#[test]
fn the_same_server_is_not_asked_again_but_another_one_is() {
    let mut world = World::saved(&remote_saved(Some(FAR_ORIGIN)));

    world.type_server("https://ai.example.com/v2", "other-model");
    let (writes, _) = world.send(SettingsMessage::SaveServer);
    assert_eq!((writes, world.page().consent_pending.is_none()), (1, true));

    world.type_server("https://other.example.org/v1", "m");
    let (writes_for_other, _) = world.send(SettingsMessage::SaveServer);
    assert_eq!(writes_for_other, 0);
    assert_eq!(
        world
            .page()
            .consent_pending
            .as_ref()
            .map(|ask| ask.origin.as_str()),
        Some("https://other.example.org")
    );
}

#[test]
fn going_back_to_a_server_agreed_to_earlier_asks_nothing() {
    let mut world = World::saved(&format!("{LOCAL}consented_origins = [\"{FAR_ORIGIN}\"]\n"));
    world.send(SettingsMessage::ChooseProvider(AiProviderName::Http));
    world.type_server(FAR, "gpt-5");

    let (writes, _) = world.send(SettingsMessage::SaveServer);

    assert_eq!(writes, 1);
    assert_eq!(world.page().consent_pending, None);
}

#[test]
fn a_file_from_before_the_question_keeps_its_saved_server_without_asking() {
    let mut world = World::saved(&remote_saved(None));

    world.type_server(FAR, "a-new-model");
    let (writes, _) = world.send(SettingsMessage::SaveServer);

    assert_eq!((writes, world.page().consent_pending.is_none()), (1, true));
}

#[test]
fn the_model_on_this_computer_is_never_asked_about() {
    let mut from_far = World::saved(&remote_saved(Some(FAR_ORIGIN)));
    let (switch_writes, _) = from_far.send(SettingsMessage::ChooseProvider(AiProviderName::Local));
    assert_eq!(
        (switch_writes, from_far.page().consent_pending.is_none()),
        (1, true)
    );

    let mut from_local = World::saved(LOCAL);
    from_local.send(SettingsMessage::ChooseProvider(AiProviderName::Http));
    from_local.type_server("http://localhost:11434", "qwen");
    let (save_writes, _) = from_local.send(SettingsMessage::SaveServer);
    assert_eq!(
        (save_writes, from_local.page().consent_pending.is_none()),
        (1, true)
    );
}

#[test]
fn escape_on_the_question_cancels_it() {
    let mut world = World::saved(LOCAL);
    world.send(SettingsMessage::ChooseProvider(AiProviderName::Http));
    world.type_server(FAR, "gpt-5");
    world.send(SettingsMessage::SaveServer);
    assert!(world.page().dialog_is_open());

    world.send(SettingsMessage::Close);

    assert_eq!(world.page().consent_pending, None);
    assert_eq!(world.saved_ai(), AiSetting::Local { threads: None });
}
