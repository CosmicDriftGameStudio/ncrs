use super::*;

fn marker_of(db: &Path) -> PathBuf {
    PathBuf::from(format!("{}.recovery-key-unconfirmed", db.display()))
}

fn confirm_shown_key(module: &mut FilingModule) {
    let shown = module.recovery_key_text().unwrap();
    let Phase::ShowRecoveryKey { group, .. } = module.phase() else {
        panic!("the key is shown");
    };
    let asked = shown.split('-').nth(*group).unwrap().to_owned();
    send(module, FilingMessage::GroupInput(SecretText::from(asked)));
    send(module, FilingMessage::GroupSubmit);
}

#[test]
fn a_new_index_shows_its_key_again_after_a_quit_until_the_user_confirmed_it() {
    let dir = TempDir::new().unwrap();
    let db = db_in(&dir);
    let store = Arc::new(InMemoryKeyStore::empty());
    let mut module = started(&db, shared(&store));
    send(&mut module, FilingMessage::CreateIndex);
    let first_key = module.recovery_key_text().unwrap();
    assert!(marker_of(&db).exists());
    assert_eq!(fs::metadata(marker_of(&db)).unwrap().len(), 0);

    drop(module);
    let mut module = started(&db, shared(&store));

    assert!(matches!(module.phase(), Phase::ShowRecoveryKey { .. }));
    assert_eq!(module.recovery_key_text().unwrap(), first_key);
    assert_eq!(
        module.start_pipeline().err(),
        Some(HeldReason::WaitingForUser)
    );

    confirm_shown_key(&mut module);
    assert!(matches!(module.phase(), Phase::Ready));
    assert!(!marker_of(&db).exists());

    drop(module);
    let module = started(&db, shared(&store));
    assert!(matches!(module.phase(), Phase::Ready));
}

#[test]
fn an_index_encrypted_in_place_shows_its_key_again_after_a_quit() {
    let dir = TempDir::new().unwrap();
    let db = db_in(&dir);
    make_plain(&db);
    let store = Arc::new(InMemoryKeyStore::empty());
    let mut module = started(&db, shared(&store));
    begin_encrypting(&mut module);
    let first_key = module.recovery_key_text().unwrap();

    drop(module);
    let mut module = started(&db, shared(&store));

    assert!(matches!(module.phase(), Phase::ShowRecoveryKey { .. }));
    assert_eq!(module.recovery_key_text().unwrap(), first_key);
    confirm_shown_key(&mut module);
    assert!(!marker_of(&db).exists());
}

fn begin_encrypting(module: &mut FilingModule) {
    let work = module.begin_encrypting();
    assert!(work.is_some(), "refused: {:?}", module.phase());
    settle(module, work);
}

#[test]
fn a_plain_index_leaves_no_marker() {
    let dir = TempDir::new().unwrap();
    let db = db_in(&dir);
    let store = Arc::new(InMemoryKeyStore::empty());
    let mut module = started(&db, shared(&store));
    send(&mut module, FilingMessage::ChooseEncrypted(false));
    send(&mut module, FilingMessage::CreateIndex);

    assert!(matches!(module.phase(), Phase::Ready));
    assert!(!marker_of(&db).exists());
}

#[test]
fn starting_over_drops_the_marker_with_the_index() {
    let dir = TempDir::new().unwrap();
    let db = db_in(&dir);
    let store = Arc::new(InMemoryKeyStore::empty());
    let mut module = started(&db, shared(&store));
    send(&mut module, FilingMessage::CreateIndex);
    confirm_shown_key(&mut module);
    fs::write(marker_of(&db), b"").unwrap();

    let work = module.begin_starting_over();
    settle(&mut module, work);

    assert!(!marker_of(&db).exists());
}
