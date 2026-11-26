//! The switch to a server on another computer: asked once for each server,
//! written only after the yes.

use std::path::Path;
use std::sync::Arc;

use ncrs_ai::{ConsentLedger as _, ConsentPurpose, ConsentedOrigins};
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
    _folder: tempfile::TempDir,
    keymap: Arc<Keymap>,
    tags: FileListTags,
    config_path: std::path::PathBuf,
    settings: Settings,
}

impl World {
    fn saved(source: &str) -> Self {
        let folder = tempfile::tempdir().unwrap();
        let config_path = folder.path().join("config.toml");
        std::fs::write(&config_path, source).unwrap();
        let file = config::load(&config_path).unwrap();
        Self {
            _folder: folder,
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
            hermes_on: true,
            usage_log: None,
            git_shortcuts: Vec::new(),
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

fn remote_with(lines: &str) -> String {
    format!(
        "[modules.filing.ai]\nenabled = true\nprovider = \"http\"\nendpoint = \"{FAR}\"\nmodel = \"m\"\n{lines}"
    )
}

/// A file with an agreement to every kind.
fn remote_agreed() -> String {
    remote_with(&format!(
        "consented_inbox = [\"{FAR_ORIGIN}\"]\nconsented_archive = [\"{FAR_ORIGIN}\"]\n\
         consented_folders = [\"{FAR_ORIGIN}\"]\n"
    ))
}

/// A file with the single agreement of before the split.
fn remote_legacy() -> String {
    remote_with(&format!("consented_origins = [\"{FAR_ORIGIN}\"]\n"))
}

#[test]
fn saving_a_far_server_writes_it_without_the_agreement_and_asks_for_it_on_the_page() {
    let mut world = World::saved(LOCAL);
    world.send(SettingsMessage::ChooseProvider(AiProviderName::Http));
    world.type_server(FAR, "gpt-5");

    let (writes, _) = world.send(SettingsMessage::SaveServer);
    assert_eq!(writes, 1);
    let change = world.page().clone().save_server().unwrap();
    world.written(change);

    let AiSetting::Http {
        endpoint,
        consented_origins,
        ..
    } = world.saved_ai()
    else {
        panic!("the server was not written");
    };
    assert_eq!(endpoint, FAR);
    assert_eq!(consented_origins, ConsentedOrigins::default());
    assert_eq!(
        world.page().server_agreed(ConsentPurpose::Inbox),
        Some(false)
    );
    assert!(!world.page().dialog_is_open());
}

#[test]
fn allowing_writes_the_server_with_the_agreement_for_that_server_only() {
    let mut world = World::saved(LOCAL);
    world.send(SettingsMessage::ChooseProvider(AiProviderName::Http));
    world.type_server(FAR, "gpt-5");
    assert_eq!(
        world.page().server_agreed(ConsentPurpose::Inbox),
        Some(false)
    );
    let change = world
        .page()
        .clone()
        .agree_to_server(ConsentPurpose::Inbox)
        .unwrap();

    let (writes, _) = world.send(SettingsMessage::ConfirmAiConsent(ConsentPurpose::Inbox));

    assert_eq!(writes, 1);
    let actions = world.written(change);
    assert!(actions
        .iter()
        .any(|action| matches!(action, Action::Apply { .. })));
    let AiSetting::Http {
        model,
        consented_origins,
        ..
    } = world.saved_ai()
    else {
        panic!("the server was not written");
    };
    assert_eq!(model, "gpt-5");
    assert_eq!(consented_origins.inbox, [FAR_ORIGIN]);
    assert!(consented_origins.archive.is_empty() && consented_origins.folders.is_empty());
    assert_eq!(
        world.page().server_agreed(ConsentPurpose::Inbox),
        Some(true)
    );
    let ledger = ncrs_ai::ConsentGivenInSettings::new(&consented_origins);
    assert!(ledger.granted(FAR_ORIGIN, Path::new(""), ConsentPurpose::Inbox));
    assert!(!ledger.granted(
        "https://other.example.org",
        Path::new(""),
        ConsentPurpose::Inbox
    ));
}

#[test]
fn allowing_one_kind_of_data_leaves_the_other_two_unagreed() {
    let mut world = World::saved(LOCAL);
    world.send(SettingsMessage::ChooseProvider(AiProviderName::Http));
    world.type_server(FAR, "gpt-5");
    let change = world
        .page()
        .clone()
        .agree_to_server(ConsentPurpose::Folders)
        .unwrap();
    world.send(SettingsMessage::ConfirmAiConsent(ConsentPurpose::Folders));
    world.written(change);

    let AiSetting::Http {
        consented_origins, ..
    } = world.saved_ai()
    else {
        panic!("the server was not written");
    };
    assert_eq!(consented_origins.folders, [FAR_ORIGIN]);
    assert!(consented_origins.inbox.is_empty());
    assert!(consented_origins.archive.is_empty());
    let page = world.page();
    assert_eq!(page.server_agreed(ConsentPurpose::Folders), Some(true));
    assert_eq!(page.server_agreed(ConsentPurpose::Inbox), Some(false));
    assert_eq!(page.server_agreed(ConsentPurpose::Archive), Some(false));
}

#[test]
fn the_single_agreement_of_before_grants_nothing_and_the_page_says_it_was_dropped() {
    let mut world = World::saved(&remote_legacy());
    for purpose in ConsentPurpose::ALL {
        assert_eq!(
            world.page().server_agreed(purpose),
            Some(false),
            "{purpose:?}"
        );
    }
    assert!(world.page().legacy_agreement_dropped());
    let AiSetting::Http {
        consented_origins, ..
    } = world.saved_ai()
    else {
        panic!("not the http provider");
    };
    let ledger = ncrs_ai::ConsentGivenInSettings::new(&consented_origins);
    for purpose in ConsentPurpose::ALL {
        assert!(!ledger.granted(FAR_ORIGIN, Path::new(""), purpose));
    }

    let change = world
        .page()
        .clone()
        .agree_to_server(ConsentPurpose::Inbox)
        .unwrap();
    world.written(change);
    assert!(!world.page().legacy_agreement_dropped());
    assert_eq!(
        world.page().server_agreed(ConsentPurpose::Archive),
        Some(false)
    );
}

#[test]
fn allowing_a_server_with_a_wrong_address_writes_nothing_and_shows_the_problem() {
    let mut world = World::saved(LOCAL);
    world.send(SettingsMessage::ChooseProvider(AiProviderName::Http));
    world.type_server("not an address", "gpt-5");

    let (writes, _) = world.send(SettingsMessage::ConfirmAiConsent(ConsentPurpose::Inbox));

    assert_eq!(writes, 0);
    assert!(world
        .page()
        .problems
        .contains_key(&SettingField::AiEndpoint));
    assert_eq!(world.saved_ai(), AiSetting::Local { threads: None });
}

#[test]
fn the_same_server_is_agreed_to_already_but_another_one_is_not() {
    let mut world = World::saved(&remote_agreed());
    assert_eq!(
        world.page().server_agreed(ConsentPurpose::Inbox),
        Some(true)
    );

    world.type_server("https://ai.example.com/v2", "other-model");
    assert_eq!(
        world.page().server_agreed(ConsentPurpose::Inbox),
        Some(true)
    );

    world.type_server("https://other.example.org/v1", "m");
    assert_eq!(
        world.page().server_agreed(ConsentPurpose::Inbox),
        Some(false)
    );
}

#[test]
fn a_file_from_before_the_agreement_does_not_keep_its_saved_server_agreed() {
    let world = World::saved(&remote_with(""));
    assert_eq!(
        world.page().server_agreed(ConsentPurpose::Inbox),
        Some(false)
    );
    assert!(world.page().legacy_agreement_dropped());
}

#[test]
fn the_model_on_this_computer_needs_no_agreement() {
    let mut from_far = World::saved(&remote_agreed());
    let (switch_writes, _) = from_far.send(SettingsMessage::ChooseProvider(AiProviderName::Local));
    assert_eq!(switch_writes, 1);
    assert_eq!(from_far.page().server_agreed(ConsentPurpose::Inbox), None);

    let mut from_local = World::saved(LOCAL);
    from_local.send(SettingsMessage::ChooseProvider(AiProviderName::Http));
    from_local.type_server("http://localhost:11434", "qwen");
    assert_eq!(from_local.page().server_agreed(ConsentPurpose::Inbox), None);
    let (save_writes, _) = from_local.send(SettingsMessage::SaveServer);
    assert_eq!(save_writes, 1);
}

#[test]
fn withdrawing_one_kind_keeps_the_other_two() {
    let mut world = World::saved(&remote_agreed());
    let change = world
        .page()
        .clone()
        .withdraw_from_server(ConsentPurpose::Archive)
        .unwrap();

    let (writes, _) = world.send(SettingsMessage::WithdrawAiConsent(ConsentPurpose::Archive));
    assert_eq!(writes, 1);
    world.written(change);

    let AiSetting::Http {
        consented_origins, ..
    } = world.saved_ai()
    else {
        panic!("the server was not written");
    };
    assert_eq!(consented_origins.inbox, [FAR_ORIGIN]);
    assert!(consented_origins.archive.is_empty());
    assert_eq!(consented_origins.folders, [FAR_ORIGIN]);
    let page = world.page();
    assert_eq!(page.server_agreed(ConsentPurpose::Archive), Some(false));
    assert_eq!(page.server_agreed(ConsentPurpose::Inbox), Some(true));
    assert_eq!(page.server_agreed(ConsentPurpose::Folders), Some(true));
    let ledger = ncrs_ai::ConsentGivenInSettings::new(&consented_origins);
    assert!(!ledger.granted(FAR_ORIGIN, Path::new(""), ConsentPurpose::Archive));
    assert!(ledger.granted(FAR_ORIGIN, Path::new(""), ConsentPurpose::Inbox));
}

#[test]
fn a_withdrawn_kind_can_be_allowed_again() {
    let mut world = World::saved(&remote_agreed());
    let change = world
        .page()
        .clone()
        .withdraw_from_server(ConsentPurpose::Inbox)
        .unwrap();
    world.written(change);
    assert_eq!(
        world.page().server_agreed(ConsentPurpose::Inbox),
        Some(false)
    );

    let allow_again = world
        .page()
        .clone()
        .agree_to_server(ConsentPurpose::Inbox)
        .unwrap();
    world.written(allow_again);
    assert_eq!(
        world.page().server_agreed(ConsentPurpose::Inbox),
        Some(true)
    );
}

#[test]
fn a_new_page_scrolls_the_sidebar_to_its_entry_and_the_same_page_does_not() {
    use ncrs_config::SettingsFeature;
    let mut world = World::saved(LOCAL);

    let (opened, _) = world.send(SettingsMessage::Select(SettingsFeature::Activity));
    assert_eq!(world.page().feature, SettingsFeature::Activity);
    assert_eq!(opened, 1);

    let (again, _) = world.send(SettingsMessage::Select(SettingsFeature::Activity));
    assert_eq!(again, 0);

    let (searched, _) = world.send(SettingsMessage::SearchChanged("rules".to_owned()));
    assert_eq!(searched, 1);
}
