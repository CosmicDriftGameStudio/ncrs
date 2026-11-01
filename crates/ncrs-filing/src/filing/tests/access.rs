use super::*;

#[test]
fn the_pipeline_is_refused_in_every_phase_but_ready() {
    let held = [
        (Phase::Starting, HeldReason::Starting),
        (
            Phase::AskEncryption {
                encrypted: true,
                busy: false,
                error: None,
            },
            HeldReason::WaitingForUser,
        ),
        (
            Phase::ShowRecoveryKey {
                key: FilingModule::fixed_recovery_key(),
                group: 0,
                typed: SecretText::default(),
                mismatch: false,
            },
            HeldReason::WaitingForUser,
        ),
        (
            ask_recovery_key(KeyProblem::Missing),
            HeldReason::WaitingForUser,
        ),
        (Phase::PlainFileWarning, HeldReason::PlainFileUnconfirmed),
        (Phase::Dismissed, HeldReason::Dismissed),
        (Phase::Failed("no".to_owned()), HeldReason::Failed),
        (Phase::Ready, HeldReason::Failed),
    ];
    for (phase, reason) in held {
        let module = FilingModule::in_phase(phase);
        put_plain_index_unless_ready_is_empty(&module);
        assert_eq!(
            module.start_pipeline().err(),
            Some(reason),
            "{:?}",
            module.phase()
        );
    }
}

/// Every phase but the last holds an index in the slot, to show that the
/// phase alone keeps the pipeline away; `Ready` gets an empty slot.
fn put_plain_index_unless_ready_is_empty(module: &FilingModule) {
    if module.phase() == &Phase::Ready {
        return;
    }
    let dir = TempDir::new().unwrap();
    put_index(
        &module.index,
        Index::create_plain(&dir.path().join("index.db")).unwrap(),
    );
}

#[test]
fn an_index_put_into_the_slot_has_its_interrupted_move_settled_first() {
    use ncrs_index::{BatchKind, JournalState, NewJournalItem};

    let dir = TempDir::new().unwrap();
    let source = dir.path().join("a.pdf");
    std::fs::write(&source, "alpha").unwrap();
    let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
    let batch = index.begin_batch(BatchKind::Reorganisation).unwrap();
    index
        .add_journal_item(
            batch,
            &NewJournalItem {
                from: source.clone(),
                to: dir.path().join("b.pdf"),
                hash: ncrs_intake::hash_path(&source).unwrap(),
                document_id: None,
                duplicate: false,
                state: JournalState::Pending,
                reason: None,
            },
        )
        .unwrap();
    let slot = IndexSlot::default();

    put_index(&slot, index);

    let guard = lock(&slot);
    let items = guard.as_ref().unwrap().journal_items(batch).unwrap();
    assert_eq!(items[0].state, JournalState::Skipped);
    assert_eq!(items[0].reason.as_deref(), Some("interrupted"));
}

#[test]
fn a_ready_module_with_an_index_starts_a_pipeline_that_stops_on_drop() {
    let dir = TempDir::new().unwrap();
    let module = FilingModule::in_phase(Phase::Ready);
    put_index(
        &module.index,
        Index::create_plain(&dir.path().join("index.db")).unwrap(),
    );

    let (runner, _pdf_library_missing) = module.start_pipeline().unwrap();

    assert_ne!(runner.snapshot().state, ncrs_pipeline::RunnerState::Off);
    drop(runner);
}

#[test]
fn a_document_is_found_by_the_beginning_of_a_word_only_its_text_has() {
    use ncrs_index::{ContentHash, DocumentSource, NewDocument};

    let dir = TempDir::new().unwrap();
    let module = FilingModule::in_phase(Phase::Ready);
    let mut index = Index::create_plain(&dir.path().join("index.db")).unwrap();
    let mut add = |seed: u8, name: &str, text: &str| {
        let id = index
            .insert_document(&NewDocument {
                hash: ContentHash([seed; 32]),
                path: PathBuf::from(name),
                source: DocumentSource::Inbox,
                received_at: chrono::Utc::now(),
                stamp: None,
            })
            .unwrap();
        index.set_document_text(id, text).unwrap();
        id
    };
    add(1, "inbox/scan-1.pdf", "Rechnung Nr. 4711");
    let gone = add(2, "inbox/scan-2.PDF", "Rechnung Nr. 12");
    add(3, "inbox/photo.JPG", "Rechnung am Strand");
    add(4, "inbox/notes.txt", "Mahnung");
    index.mark_document_missing(gone).unwrap();
    put_index(&module.index, index);

    let found = module.document_finder().unwrap().find("rechn", 10).unwrap();

    let listed: Vec<(PathBuf, TextSource)> = found
        .into_iter()
        .map(|document| (document.path, document.read_by))
        .collect();
    assert_eq!(listed.len(), 2, "{listed:?}");
    assert!(listed.contains(&(PathBuf::from("inbox/scan-1.pdf"), TextSource::Pdf)));
    assert!(listed.contains(&(PathBuf::from("inbox/photo.JPG"), TextSource::Ocr)));
}

#[test]
fn the_document_finder_exists_in_ready_only_and_fails_without_an_index() {
    assert!(FilingModule::in_phase(Phase::Starting)
        .document_finder()
        .is_none());
    let finder = FilingModule::in_phase(Phase::Ready)
        .document_finder()
        .unwrap();
    assert_eq!(finder.find("x", 5), Err(INDEX_NOT_OPEN.to_owned()));
}

#[test]
fn the_figures_reader_exists_in_ready_only() {
    let others = [
        Phase::Starting,
        Phase::AskEncryption {
            encrypted: true,
            busy: false,
            error: None,
        },
        Phase::ShowRecoveryKey {
            key: FilingModule::fixed_recovery_key(),
            group: 0,
            typed: SecretText::default(),
            mismatch: false,
        },
        ask_recovery_key(KeyProblem::Missing),
        Phase::PlainFileWarning,
        Phase::Dismissed,
        Phase::Failed("no".to_owned()),
    ];
    for phase in others {
        let module = FilingModule::in_phase(phase);
        assert!(
            module.index_figures_reader().is_none(),
            "{:?}",
            module.phase()
        );
    }
    assert!(FilingModule::in_phase(Phase::Ready)
        .index_figures_reader()
        .is_some());
}

#[test]
fn the_figures_reader_reports_a_plain_index_on_disk() {
    let dir = TempDir::new().unwrap();
    let db = db_in(&dir);
    let store = Arc::new(InMemoryKeyStore::empty());
    let mut module = started(&db, shared(&store));
    send(&mut module, FilingMessage::ChooseEncrypted(false));
    send(&mut module, FilingMessage::CreateIndex);

    let figures = module.index_figures_reader().unwrap().read().unwrap();

    assert!(!figures.encrypted);
    assert_eq!(figures.path, db);
    assert!(figures.file_bytes.is_some_and(|bytes| bytes > 0));
    assert_eq!(figures.statistics.document_count, 0);
}

#[test]
fn the_module_knows_whether_the_index_it_opened_is_plain() {
    for encrypted in [false, true] {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(InMemoryKeyStore::empty());
        let mut module = started(&db_in(&dir), shared(&store));
        assert!(!module.index_is_plain(), "nothing is open yet");

        send(&mut module, FilingMessage::ChooseEncrypted(encrypted));
        send(&mut module, FilingMessage::CreateIndex);

        assert_eq!(module.index_is_plain(), !encrypted);
    }
    let dir = TempDir::new().unwrap();
    make_plain(&db_in(&dir));
    let store = Arc::new(InMemoryKeyStore::empty());
    assert!(started(&db_in(&dir), shared(&store)).index_is_plain());
}

#[test]
fn the_module_remembers_whether_the_config_listed_locations() {
    let dir = TempDir::new().unwrap();
    let store = Arc::new(InMemoryKeyStore::empty());
    for (locations, expected) in [
        (Vec::new(), false),
        (vec![location(&dir.path().join("inbox"))], true),
    ] {
        let (module, _work) = FilingModule::start_picking(
            &filing_with(locations),
            Some(db_in(&dir)),
            shared(&store),
            fixed_group,
        )
        .unwrap();
        assert_eq!(module.has_configured_locations(), expected);
    }
}

#[test]
fn only_a_module_without_work_or_a_shown_key_can_be_replaced() {
    let replaceable = [
        Phase::Ready,
        Phase::Dismissed,
        Phase::PlainFileWarning,
        Phase::Failed("broken".to_owned()),
        Phase::AskEncryption {
            encrypted: true,
            busy: false,
            error: None,
        },
    ];
    for phase in replaceable {
        assert!(FilingModule::in_phase(phase).can_be_replaced());
    }
    let held = [
        Phase::Starting,
        Phase::AskEncryption {
            encrypted: true,
            busy: true,
            error: None,
        },
        Phase::ShowRecoveryKey {
            key: FilingModule::fixed_recovery_key(),
            group: 0,
            typed: SecretText::default(),
            mismatch: false,
        },
    ];
    for phase in held {
        assert!(!FilingModule::in_phase(phase).can_be_replaced());
    }
}

#[test]
fn the_figures_reader_fails_when_the_slot_is_empty() {
    let module = FilingModule::in_phase(Phase::Ready);

    let result = module.index_figures_reader().unwrap().read();

    assert_eq!(result, Err(INDEX_NOT_OPEN.to_owned()));
}

#[test]
fn a_ready_module_stays_ready_whatever_it_is_sent() {
    let outcomes = [
        WorkOutcome::Started {
            locations: Vec::new(),
            opened: OpenOutcome::NoIndexYet,
        },
        WorkOutcome::Started {
            locations: Vec::new(),
            opened: OpenOutcome::Opened,
        },
        WorkOutcome::Started {
            locations: Vec::new(),
            opened: OpenOutcome::PlainFileWithKeyEntry,
        },
        WorkOutcome::Started {
            locations: Vec::new(),
            opened: OpenOutcome::KeyMissing,
        },
        WorkOutcome::Started {
            locations: Vec::new(),
            opened: OpenOutcome::KeyMismatch,
        },
        WorkOutcome::Started {
            locations: Vec::new(),
            opened: OpenOutcome::Failed("broken".to_owned()),
        },
        WorkOutcome::Created(Ok(None)),
        WorkOutcome::Created(Ok(Some(FilingModule::fixed_recovery_key()))),
        WorkOutcome::Created(Err("broken".to_owned())),
        WorkOutcome::KeyApplied(Ok(OpenOutcome::Opened)),
        WorkOutcome::KeyApplied(Ok(OpenOutcome::KeyMismatch)),
        WorkOutcome::KeyApplied(Err(KeyError::DoesNotOpen)),
        WorkOutcome::Crashed("broken".to_owned()),
    ];
    let mut messages = vec![
        FilingMessage::ChooseEncrypted(true),
        FilingMessage::ChooseEncrypted(false),
        FilingMessage::CreateIndex,
        FilingMessage::CopyRecoveryKey,
        FilingMessage::GroupInput(SecretText::from("ZZZZ")),
        FilingMessage::GroupSubmit,
        FilingMessage::KeyInput(SecretText::from("A")),
        FilingMessage::ConfirmReplace(true),
        FilingMessage::KeySubmit,
        FilingMessage::ConfirmPlainFile,
        FilingMessage::Dismiss,
    ];
    let mut module = FilingModule::in_phase(Phase::Ready);
    for outcome in outcomes {
        for generation in [module.generation, module.generation.wrapping_sub(1)] {
            messages.push(FilingMessage::WorkDone {
                generation,
                outcome: outcome.clone(),
            });
        }
    }

    for message in messages {
        let label = format!("{message:?}");
        let _ = module.update(message);
        assert_eq!(module.phase(), &Phase::Ready, "{label}");
    }
}

#[test]
fn a_module_that_is_off_opens_and_creates_nothing() {
    let dir = TempDir::new().unwrap();
    let db = db_in(&dir);
    let store = Arc::new(InMemoryKeyStore::empty());
    let off = Filing {
        enabled: false,
        locations: vec![location(dir.path())],
        load: Load::default(),
        ai: ncrs_config::config::AiSetting::Off,
        default_pattern: None,
        own_names: Vec::new(),
        own_ibans: Vec::new(),
        own_tax_ids: Vec::new(),
    };

    assert!(FilingModule::start(&off, Some(db.clone()), shared(&store)).is_none());

    assert!(!db.exists());
    assert!(!data_dir(&dir).exists());
    assert_eq!(store.snapshot(), None);
}

#[test]
fn a_missing_folder_is_not_reachable_and_the_other_stays_ready() {
    let dir = TempDir::new().unwrap();
    let present = dir.path().join("inbox");
    fs::create_dir(&present).unwrap();
    let missing = dir.path().join("gone");

    let resolved = resolve_locations(&[location(&missing), location(&present)]);

    assert!(matches!(
        resolved[0].status,
        LocationStatus::NotReachable { .. }
    ));
    assert!(matches!(resolved[1].status, LocationStatus::Ready(_)));
}

fn assert_both_overlap(resolved: &[ResolvedLocation], first: &Path, second: &Path) {
    assert_eq!(resolved.len(), 2);
    assert_eq!(
        resolved[0].status,
        LocationStatus::Overlaps {
            with: second.to_path_buf()
        }
    );
    assert_eq!(
        resolved[1].status,
        LocationStatus::Overlaps {
            with: first.to_path_buf()
        }
    );
}

#[cfg(unix)]
#[test]
fn a_symlink_to_a_location_overlaps_it() {
    let dir = TempDir::new().unwrap();
    let real = dir.path().join("a");
    let link = dir.path().join("link");
    fs::create_dir(&real).unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let resolved = resolve_locations(&[location(&real), location(&link)]);

    assert_both_overlap(&resolved, &real, &link);
}

#[test]
fn two_spellings_of_one_folder_overlap() {
    let dir = TempDir::new().unwrap();
    let real = dir.path().join("a");
    fs::create_dir(&real).unwrap();
    let dotted = dir.path().join(".").join("a");

    let resolved = resolve_locations(&[location(&dotted), location(&real)]);

    assert_both_overlap(&resolved, &dotted, &real);
}

#[test]
fn nested_folders_overlap() {
    let dir = TempDir::new().unwrap();
    let outer = dir.path().join("a");
    let inner = outer.join("b");
    fs::create_dir_all(&inner).unwrap();

    let resolved = resolve_locations(&[location(&outer), location(&inner)]);

    assert_both_overlap(&resolved, &outer, &inner);
}

#[cfg(any(target_os = "macos", windows))]
#[test]
fn spellings_that_differ_in_case_overlap_on_case_insensitive_systems() {
    let dir = TempDir::new().unwrap();
    let real = dir.path().join("Inbox");
    fs::create_dir(&real).unwrap();
    let lower = dir.path().join("inbox");

    let resolved = resolve_locations(&[location(&real), location(&lower)]);

    assert_both_overlap(&resolved, &real, &lower);
}
