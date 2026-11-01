use super::*;

/// Loads fine and has no entry, but cannot keep a new one.
#[derive(Debug)]
struct RefusesToSave;

impl KeyStore for RefusesToSave {
    fn load(&self) -> Result<Option<Vec<u8>>, KeyStoreError> {
        Ok(None)
    }

    fn store_new(&self, _key: &[u8]) -> Result<(), KeyStoreError> {
        Err(KeyStoreError::Unavailable("locked".to_owned()))
    }

    fn replace(&self, _key: &[u8]) -> Result<(), KeyStoreError> {
        Err(KeyStoreError::Unavailable("locked".to_owned()))
    }

    fn delete(&self) -> Result<(), KeyStoreError> {
        Ok(())
    }
}

fn begin(module: &mut FilingModule, begin: fn(&mut FilingModule) -> Option<Work>) {
    let work = begin(module);
    assert!(
        work.is_some(),
        "the change was refused: {:?}",
        module.phase()
    );
    settle(module, work);
}

#[test]
fn a_plain_index_is_encrypted_and_its_recovery_key_shown_once() {
    let dir = TempDir::new().unwrap();
    let db = db_in(&dir);
    make_plain(&db);
    let store = Arc::new(InMemoryKeyStore::empty());
    let mut module = started(&db, shared(&store));
    assert!(module.can_encrypt());

    begin(&mut module, FilingModule::begin_encrypting);

    assert!(matches!(module.phase(), Phase::ShowRecoveryKey { .. }));
    assert!(module.recovery_key_text().is_some());
    assert!(!Index::file_is_plain(&db).unwrap());
    assert!(store.snapshot().is_some());
    assert_eq!(module.index_encryption(), Some(true));
    assert!(!module.can_encrypt());
    let Phase::ShowRecoveryKey { group, .. } = module.phase() else {
        panic!("the key is shown");
    };
    let asked = module
        .recovery_key_text()
        .unwrap()
        .split('-')
        .nth(*group)
        .unwrap()
        .to_owned();
    send(
        &mut module,
        FilingMessage::GroupInput(SecretText::from(asked)),
    );
    send(&mut module, FilingMessage::GroupSubmit);
    assert_eq!(module.phase(), &Phase::Ready);
    assert!(is_open(&module));
}

#[test]
fn a_failed_encryption_leaves_the_plain_index_open_and_says_why() {
    let dir = TempDir::new().unwrap();
    let db = db_in(&dir);
    make_plain(&db);
    let mut module = started(&db, Arc::new(RefusesToSave));

    begin(&mut module, FilingModule::begin_encrypting);

    assert_eq!(module.phase(), &Phase::Ready);
    assert!(matches!(
        module.protection_note(),
        Some(ProtectionNote::EncryptFailed(_))
    ));
    assert!(Index::file_is_plain(&db).unwrap());
    assert!(is_open(&module));
    assert_eq!(module.index_encryption(), Some(false));
    assert!(module.can_encrypt());
}

#[test]
fn no_encryption_is_offered_without_a_credential_store() {
    let dir = TempDir::new().unwrap();
    let db = db_in(&dir);
    make_plain(&db);
    let mut module = started(&db, Arc::new(UnavailableStore));

    assert_eq!(module.phase(), &Phase::Ready);
    assert!(!module.can_encrypt());
    assert!(module.begin_encrypting().is_none());
}

#[test]
fn an_encrypted_index_is_not_encrypted_again() {
    let dir = TempDir::new().unwrap();
    let db = db_in(&dir);
    let store = Arc::new(InMemoryKeyStore::empty());
    make_encrypted(&db, &store);
    let module = started(&db, shared(&store));

    assert!(!module.can_encrypt());
}

#[test]
fn starting_over_removes_the_index_files_and_the_key_and_asks_again() {
    let dir = TempDir::new().unwrap();
    let db = db_in(&dir);
    let store = Arc::new(InMemoryKeyStore::empty());
    make_encrypted(&db, &store);
    let neighbour = data_dir(&dir).join("index.db.bak");
    fs::write(&neighbour, b"mine").unwrap();
    fs::write(data_dir(&dir).join("index.db.encrypting"), b"half").unwrap();
    let mut module = started(&db, shared(&store));
    assert_eq!(module.phase(), &Phase::Ready);

    begin(&mut module, FilingModule::begin_starting_over);

    assert!(module.asks_encryption());
    assert!(!db.exists());
    assert!(!data_dir(&dir).join("index.db.encrypting").exists());
    assert!(neighbour.exists());
    assert_eq!(store.snapshot(), None);
    assert!(module.protection_note().is_none());
}

#[test]
fn starting_over_a_plain_index_works_without_a_key_entry() {
    let dir = TempDir::new().unwrap();
    let db = db_in(&dir);
    make_plain(&db);
    let mut module = started(&db, Arc::new(UnavailableStore));

    begin(&mut module, FilingModule::begin_starting_over);

    assert!(module.asks_encryption());
    assert!(!db.exists());
}

#[test]
fn the_files_to_remove_are_only_the_index_files_in_the_data_folder() {
    let dir = TempDir::new().unwrap();
    let data = data_dir(&dir);
    fs::create_dir_all(&data).unwrap();
    let other = dir.path().join("other");
    fs::create_dir_all(&other).unwrap();

    let right = index_files(&data.join("index.db"), &data).unwrap();
    let names: Vec<_> = right
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        [
            "index.db.encrypting",
            "index.db-journal",
            "index.db-wal",
            "index.db-shm",
            "index.db",
        ]
    );
    index_files(&data.join("index.db.bak"), &data).unwrap_err();
    index_files(&data.join("notes.txt"), &data).unwrap_err();
    index_files(&other.join("index.db"), &data).unwrap_err();
    index_files(&data.join("index.db"), &data.join("missing")).unwrap_err();
    index_files(Path::new("index.db"), &data).unwrap_err();
}

#[test]
fn a_start_over_of_a_file_not_named_index_db_removes_nothing() {
    let dir = TempDir::new().unwrap();
    let db = db_in(&dir);
    make_plain(&db);
    let mut module = started(&db, Arc::new(UnavailableStore));
    module.db_path = data_dir(&dir).join("notes.db");

    begin(&mut module, FilingModule::begin_starting_over);

    assert!(matches!(
        module.protection_note(),
        Some(ProtectionNote::StartOverFailed(_))
    ));
    assert!(db.exists());
}

#[test]
fn a_dismissed_key_card_leaves_the_problem_to_the_overview() {
    let dir = TempDir::new().unwrap();
    let db = db_in(&dir);
    make_encrypted(&db, &InMemoryKeyStore::empty());
    let mut module = started(&db, Arc::new(InMemoryKeyStore::empty()));
    assert_eq!(module.key_problem(), Some(KeyProblem::Missing));

    send(&mut module, FilingMessage::Dismiss);

    assert_eq!(module.phase(), &Phase::Dismissed);
    assert_eq!(module.key_problem(), Some(KeyProblem::Missing));
    assert!(module.can_start_over());
    send(&mut module, FilingMessage::AskKeyAgain);
    assert!(matches!(module.phase(), Phase::AskRecoveryKey { .. }));
}

#[test]
fn a_dismissed_module_without_a_key_problem_cannot_start_over() {
    let mut module = FilingModule::in_phase(Phase::Dismissed);

    assert!(module.key_problem().is_none());
    assert!(!module.can_start_over());
    assert!(module.begin_starting_over().is_none());
}
