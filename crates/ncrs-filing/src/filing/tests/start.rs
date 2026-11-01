use super::*;

#[test]
fn first_start_with_encryption_asks_shows_the_key_and_opens_after_confirmation() {
    let dir = TempDir::new().unwrap();
    let db = db_in(&dir);
    let store = Arc::new(InMemoryKeyStore::empty());

    let (mut module, work) = FilingModule::start_picking(
        &filing_with(Vec::new()),
        Some(db.clone()),
        shared(&store),
        fixed_group,
    )
    .unwrap();
    assert_eq!(module.phase(), &Phase::Starting);
    assert_eq!(module.start_pipeline().err(), Some(HeldReason::Starting));
    settle(&mut module, work);

    assert!(matches!(
        module.phase(),
        Phase::AskEncryption {
            encrypted: true,
            busy: false,
            error: None
        }
    ));
    assert!(!db.exists());
    assert!(!data_dir(&dir).exists());
    assert_eq!(
        module.start_pipeline().err(),
        Some(HeldReason::WaitingForUser)
    );

    send(&mut module, FilingMessage::CreateIndex);
    assert!(matches!(
        module.phase(),
        Phase::ShowRecoveryKey { group: 4, .. }
    ));
    assert_eq!(
        module.start_pipeline().err(),
        Some(HeldReason::WaitingForUser)
    );

    send(
        &mut module,
        FilingMessage::GroupInput(SecretText::from("ZZZZ")),
    );
    send(&mut module, FilingMessage::GroupSubmit);
    assert!(matches!(
        module.phase(),
        Phase::ShowRecoveryKey { mismatch: true, .. }
    ));
    assert!(!is_open(&module));

    let shown = module.recovery_key_text().unwrap();
    let group = shown.split('-').nth(4).unwrap().to_ascii_lowercase();
    send(
        &mut module,
        FilingMessage::GroupInput(SecretText::from(format!(" {group} "))),
    );
    send(&mut module, FilingMessage::GroupSubmit);

    assert_eq!(module.phase(), &Phase::Ready);
    assert_eq!(module.recovery_key_text(), None);
    module.start_pipeline().unwrap();
    assert_eq!(
        lock(&module.index).as_ref().map(Index::is_encrypted),
        Some(true)
    );
    assert_eq!(store.snapshot().unwrap().len(), 32);
    assert!(Index::open(&db, store.as_ref()).unwrap().is_encrypted());
}

#[test]
fn the_typed_block_is_taken_in_any_case_and_with_spaces_around_it() {
    let phase = |typed: &str| Phase::ShowRecoveryKey {
        key: FilingModule::varied_recovery_key(),
        group: 1,
        typed: SecretText::from(typed),
        mismatch: false,
    };
    for typed in ["EFGH", "efgh", "eFgH", "  efgh", "EFGH \t", " Efgh "] {
        let mut module = FilingModule::in_phase(phase(typed));
        send(&mut module, FilingMessage::GroupSubmit);
        assert_eq!(module.phase(), &Phase::Ready, "{typed:?}");
    }
    for typed in ["", "ABCD", "EFG", "EFGHI", "EF GH"] {
        let mut module = FilingModule::in_phase(phase(typed));
        send(&mut module, FilingMessage::GroupSubmit);
        assert!(
            matches!(
                module.phase(),
                Phase::ShowRecoveryKey { mismatch: true, .. }
            ),
            "{typed:?}"
        );
    }
}

#[test]
fn first_start_without_encryption_opens_a_plain_index() {
    let dir = TempDir::new().unwrap();
    let db = db_in(&dir);
    let store = Arc::new(InMemoryKeyStore::empty());
    let mut module = started(&db, shared(&store));

    send(&mut module, FilingMessage::ChooseEncrypted(false));
    send(&mut module, FilingMessage::CreateIndex);

    assert_eq!(module.phase(), &Phase::Ready);
    assert!(is_open(&module));
    assert_eq!(store.snapshot(), None);
    assert!(fs::read(&db).unwrap().starts_with(b"SQLite format 3\0"));
}

#[test]
fn a_later_start_with_key_and_database_opens_directly() {
    let dir = TempDir::new().unwrap();
    let db = db_in(&dir);
    let store = Arc::new(InMemoryKeyStore::empty());
    make_encrypted(&db, &store);

    let module = started(&db, shared(&store));

    assert_eq!(module.phase(), &Phase::Ready);
    assert!(is_open(&module));
}

#[test]
fn a_missing_key_is_restored_from_the_recovery_key() {
    let dir = TempDir::new().unwrap();
    let db = db_in(&dir);
    let original = Arc::new(InMemoryKeyStore::empty());
    let recovery = make_encrypted(&db, &original);
    let empty = Arc::new(InMemoryKeyStore::empty());

    let mut module = started(&db, shared(&empty));
    assert!(matches!(
        module.phase(),
        Phase::AskRecoveryKey {
            reason: KeyProblem::Missing,
            ..
        }
    ));
    assert!(!is_open(&module));

    type_key(&mut module, "not a key");
    assert!(module.update(FilingMessage::KeySubmit).is_none());
    assert!(matches!(
        module.phase(),
        Phase::AskRecoveryKey {
            error: Some(KeyError::InvalidFormat),
            ..
        }
    ));

    type_key(&mut module, &wrong_key_text());
    send(&mut module, FilingMessage::KeySubmit);
    assert!(matches!(
        module.phase(),
        Phase::AskRecoveryKey {
            busy: false,
            error: Some(KeyError::DoesNotOpen),
            ..
        }
    ));
    assert_eq!(empty.snapshot(), None);

    type_key(&mut module, &recovery);
    send(&mut module, FilingMessage::KeySubmit);
    assert_eq!(module.phase(), &Phase::Ready);
    assert!(is_open(&module));
    assert_eq!(empty.snapshot(), original.snapshot());
}

#[test]
fn a_wrong_stored_key_is_replaced_only_after_confirmation_and_proof() {
    let dir = TempDir::new().unwrap();
    let db = db_in(&dir);
    let original = Arc::new(InMemoryKeyStore::empty());
    let recovery = make_encrypted(&db, &original);
    let other = Arc::new(InMemoryKeyStore::with_entry(vec![7; 32]));

    let mut module = started(&db, shared(&other));
    assert!(matches!(
        module.phase(),
        Phase::AskRecoveryKey {
            reason: KeyProblem::Mismatch,
            ..
        }
    ));

    type_key(&mut module, &recovery);
    assert!(module.update(FilingMessage::KeySubmit).is_none());
    assert!(matches!(
        module.phase(),
        Phase::AskRecoveryKey {
            error: Some(KeyError::ConfirmFirst),
            ..
        }
    ));
    assert_eq!(other.snapshot().unwrap(), vec![7; 32]);

    send(&mut module, FilingMessage::ConfirmReplace(true));
    type_key(&mut module, &wrong_key_text());
    send(&mut module, FilingMessage::KeySubmit);
    assert!(matches!(
        module.phase(),
        Phase::AskRecoveryKey {
            error: Some(KeyError::DoesNotOpen),
            ..
        }
    ));
    assert_eq!(other.snapshot().unwrap(), vec![7; 32]);

    type_key(&mut module, &recovery);
    send(&mut module, FilingMessage::KeySubmit);
    assert_eq!(module.phase(), &Phase::Ready);
    assert_eq!(other.snapshot(), original.snapshot());
}

#[test]
fn a_plain_file_next_to_a_stored_key_waits_for_confirmation() {
    let dir = TempDir::new().unwrap();
    let db = db_in(&dir);
    make_plain(&db);
    let store = Arc::new(InMemoryKeyStore::with_entry(vec![5; 32]));

    let mut module = started(&db, shared(&store));

    assert_eq!(module.phase(), &Phase::PlainFileWarning);
    assert_eq!(
        module.start_pipeline().err(),
        Some(HeldReason::PlainFileUnconfirmed)
    );
    send(&mut module, FilingMessage::ConfirmPlainFile);
    assert!(is_open(&module));
    assert_eq!(store.snapshot().unwrap(), vec![5; 32]);
}

#[test]
fn a_plain_file_opens_where_there_is_no_credential_store() {
    let dir = TempDir::new().unwrap();
    let db = db_in(&dir);
    make_plain(&db);

    let module = started(&db, Arc::new(UnavailableStore));

    assert_eq!(module.phase(), &Phase::Ready);
    assert!(is_open(&module));
}

#[test]
fn creating_an_encrypted_index_without_a_credential_store_creates_no_file() {
    let dir = TempDir::new().unwrap();
    let db = db_in(&dir);
    let mut module = started(&db, Arc::new(UnavailableStore));

    send(&mut module, FilingMessage::CreateIndex);

    assert!(matches!(
        module.phase(),
        Phase::AskEncryption {
            busy: false,
            error: Some(_),
            ..
        }
    ));
    assert!(!db.exists());
    assert!(!is_open(&module));
}

#[test]
fn a_result_of_an_older_generation_is_ignored() {
    let dir = TempDir::new().unwrap();
    let db = db_in(&dir);
    let store = Arc::new(InMemoryKeyStore::empty());
    let (mut module, work) = FilingModule::start_picking(
        &filing_with(Vec::new()),
        Some(db),
        shared(&store),
        fixed_group,
    )
    .unwrap();

    send(&mut module, FilingMessage::Dismiss);
    settle(&mut module, work);

    assert_eq!(module.phase(), &Phase::Dismissed);
    assert_eq!(module.start_pipeline().err(), Some(HeldReason::Dismissed));
}

#[test]
fn a_failed_open_offers_no_way_to_create_a_new_index_over_the_file() {
    let mut module = FilingModule::in_phase(Phase::Failed("broken".to_owned()));

    assert!(module.update(FilingMessage::CreateIndex).is_none());

    assert_eq!(module.phase(), &Phase::Failed("broken".to_owned()));
}

#[test]
fn the_encryption_question_is_the_first_run_card_and_not_the_module_s() {
    let module = FilingModule::in_phase(Phase::AskEncryption {
        encrypted: true,
        busy: false,
        error: None,
    });
    assert!(!module.has_card());
    assert!(module.asks_encryption());
}

#[test]
fn dismiss_does_not_discard_the_shown_recovery_key() {
    let mut module = FilingModule::in_phase(Phase::ShowRecoveryKey {
        key: FilingModule::fixed_recovery_key(),
        group: 2,
        typed: SecretText::default(),
        mismatch: false,
    });

    send(&mut module, FilingMessage::Dismiss);

    assert!(module.recovery_key_text().is_some());
    assert!(module.has_card());
}

#[test]
fn only_ready_locations_are_reachable() {
    let dir = TempDir::new().unwrap();
    let present = dir.path().join("inbox");
    fs::create_dir(&present).unwrap();
    let missing = dir.path().join("gone");
    let store = Arc::new(InMemoryKeyStore::empty());
    let filing = filing_with(vec![location(&missing), location(&present)]);
    let (mut module, work) =
        FilingModule::start_picking(&filing, Some(db_in(&dir)), shared(&store), fixed_group)
            .unwrap();
    settle(&mut module, work);

    assert_eq!(module.locations().len(), 2);
    let reachable: Vec<&Location> = module.reachable_locations().collect();
    assert_eq!(reachable.len(), 1);
    assert!(reachable[0].path.ends_with("inbox"));
}

#[test]
fn no_data_directory_fails_without_work() {
    let store = Arc::new(InMemoryKeyStore::empty());

    let (module, work) =
        FilingModule::start(&filing_with(Vec::new()), None, shared(&store)).unwrap();

    assert!(work.is_none());
    assert_eq!(module.phase(), &Phase::Failed(NO_DATA_DIRECTORY.to_owned()));
    assert!(module.has_card());
}

#[test]
fn typed_text_and_the_key_stay_out_of_debug_output() {
    let typed = "ABCD-EFGH-SECRET";
    let shown = format!("{:?}", FilingMessage::KeyInput(SecretText::from(typed)));
    assert!(!shown.contains("SECRET"), "{shown}");

    let dir = TempDir::new().unwrap();
    let db = db_in(&dir);
    let store = Arc::new(InMemoryKeyStore::empty());
    let mut module = started(&db, shared(&store));
    send(&mut module, FilingMessage::CreateIndex);
    let key_text = module.recovery_key_text().unwrap();
    send(
        &mut module,
        FilingMessage::GroupInput(SecretText::from("TYPEDSECRET")),
    );

    let module_debug = format!("{module:?}");
    let phase_debug = format!("{:?}", module.phase());
    for debug_text in [&module_debug, &phase_debug] {
        assert!(!debug_text.contains("TYPEDSECRET"), "{debug_text}");
        assert!(!debug_text.contains(&key_text), "{debug_text}");
    }
    for group in key_text.split('-') {
        assert!(!phase_debug.contains(group), "{phase_debug}");
    }
}

#[test]
fn nothing_that_carries_a_recovery_key_prints_it() {
    let key_text = FilingModule::varied_recovery_key().display_text();
    let key = || key_text.parse::<RecoveryKey>().unwrap();
    let report = || EncryptReport {
        result: Ok(ShownRecoveryKey::new(key())),
        opened: OpenOutcome::Opened,
    };
    let shown = [
        format!("{:?}", report()),
        format!("{:?}", WorkOutcome::Encrypted(report())),
        format!(
            "{:?}",
            FilingMessage::WorkDone {
                generation: 1,
                outcome: WorkOutcome::Encrypted(report()),
            }
        ),
        format!("{:?}", Job::Restore { key: key() }),
        format!("{:?}", Job::Replace { key: key() }),
        format!(
            "{:?}",
            Phase::ShowRecoveryKey {
                key: ShownRecoveryKey::new(key()),
                group: 0,
                typed: SecretText::default(),
                mismatch: false,
            }
        ),
    ];
    for debug_text in &shown {
        for group in key_text.split('-') {
            assert!(!debug_text.contains(group), "{debug_text}");
        }
    }
}
