use crate::app::*;
use ncrs_index::InMemoryKeyStore;

#[test]
fn a_module_that_is_off_leaves_no_module_and_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("data").join("index.db");
    let store = Arc::new(InMemoryKeyStore::empty());
    let off = Filing {
        enabled: false,
        locations: Vec::new(),
        load: ncrs_pipeline::Load::default(),
        ai: crate::config::AiSetting::Off,
        default_pattern: None,
        own_names: Vec::new(),
        own_ibans: Vec::new(),
        own_tax_ids: Vec::new(),
    };

    let (app, _task) = App::with_fixed_panels().with_filing(&off, Some(db.clone()), store);

    assert!(app.hermes.filing.is_none());
    assert!(app.hermes.pipeline.is_none());
    assert!(!db.exists());
    assert!(!dir.path().join("data").exists());
}

#[test]
fn copying_the_recovery_key_needs_a_shown_key() {
    let mut without_card = App::with_fixed_panels();
    let _nothing = without_card.update(Message::Filing(FilingMessage::CopyRecoveryKey));
    assert!(without_card.notice.is_none());

    let mut app = App::with_fixed_filing(Phase::ShowRecoveryKey {
        key: FilingModule::fixed_recovery_key(),
        group: 2,
        typed: crate::filing::SecretText::default(),
        mismatch: false,
    });
    let _task = app.update(Message::Filing(FilingMessage::CopyRecoveryKey));
    let notice = app.notice.clone().unwrap();
    assert!(!notice.contains("AAAA"), "{notice}");
}
