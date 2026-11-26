use super::*;

#[test]
fn reading_counts_files_photos_and_skips_hidden_files_and_links() {
    let setup = Setup::new();
    setup.write("in/a.txt", "Rechnung der Alfa AG");
    setup.write("in/sub/b.txt", "Mahnung der Beta GmbH");
    setup.write("in/.hidden.txt", "never read");
    setup.write("in/.cache/c.txt", "never read");
    setup.write("in/photo.jpg", "not a picture");
    setup.write("in/blank.dat", "");
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        setup.dir.path().join("in/a.txt"),
        setup.dir.path().join("in/link.txt"),
    )
    .unwrap();

    let read = setup.read(&setup.dir.path().join("in")).unwrap();
    let paths: Vec<&Path> = read
        .files
        .iter()
        .map(|file| file.relative_path.as_path())
        .collect();
    assert_eq!(
        paths,
        [
            Path::new("a.txt"),
            Path::new("blank.dat"),
            Path::new("photo.jpg"),
            Path::new("sub/b.txt")
        ]
    );
    assert_eq!(read.photos, 1);
    assert_eq!(read.without_text, 1);
    assert_eq!(read.hashes.len(), 4);
    assert_eq!(
        read.files[0].hash,
        hash_path(&setup.dir.path().join("in/a.txt")).unwrap()
    );
}

#[test]
fn a_pdf_whose_reader_is_missing_stays_out_of_the_picture_instead_of_being_unreadable() {
    let mut setup = Setup::new();
    let library = TextError::LibraryMissing {
        path: setup.dir.path().join("lib"),
    };
    setup.access = ChatAccess::new(
        Arc::clone(&setup.access.slot),
        Arc::new(
            TextChain::new()
                .with_source(PdfUnavailable::new(library))
                .with_source(PlainText),
        ),
        Arc::clone(&setup.access.trash),
    );
    setup.write("in/a.txt", "Rechnung der Alfa AG");
    setup.write("in/b.pdf", "%PDF-1.4 a pdf that cannot be read now");

    let read = setup.read(&setup.dir.path().join("in")).unwrap();

    let paths: Vec<&Path> = read
        .files
        .iter()
        .map(|file| file.relative_path.as_path())
        .collect();
    assert_eq!(paths, [Path::new("a.txt")]);
    assert_eq!(read.without_text, 0);
}

#[test]
fn reading_a_file_or_a_missing_folder_or_too_many_files_is_refused() {
    let setup = Setup::new();
    let file = setup.write("one.txt", "x");
    assert_eq!(setup.read(&file), Err(ReadError::NotADirectory));
    assert_eq!(
        setup.read(&setup.dir.path().join("missing")),
        Err(ReadError::Gone)
    );
    let big = setup.dir.path().join("big");
    fs::create_dir_all(&big).unwrap();
    for number in 0..=FILE_LIMIT {
        fs::write(big.join(format!("{number}.txt")), "").unwrap();
    }
    assert_eq!(
        setup.read(&big),
        Err(ReadError::TooMany { limit: FILE_LIMIT })
    );
}

fn read_date_of(setup: &Setup, name: &str) -> Option<NaiveDate> {
    let read = setup.read(&setup.dir.path().join("in")).unwrap();
    read.files
        .iter()
        .find(|file| file.relative_path == Path::new(name))?
        .fields
        .date
}

#[test]
fn a_date_in_the_name_stands_in_when_the_text_has_none_and_the_mtime_comes_last() {
    let setup = Setup::new();
    let account = "Kontoauszug_706014687832EUR_05010101215505397.txt";
    setup.write("in/Kontoauszug_706014687832EUR_2021-05-31_x.txt", "Saldo");
    setup.write("in/Kontoauszug_ohne_Datum.txt", "Saldo");
    setup.write(&format!("in/{account}"), "Saldo");
    let mtime = |name: &str| -> NaiveDate {
        let modified = fs::metadata(setup.dir.path().join("in").join(name))
            .unwrap()
            .modified()
            .unwrap();
        chrono::DateTime::<chrono::Local>::from(modified).date_naive()
    };

    assert_eq!(
        read_date_of(&setup, "Kontoauszug_706014687832EUR_2021-05-31_x.txt"),
        Some(date(2021, 5, 31))
    );
    assert_eq!(
        read_date_of(&setup, "Kontoauszug_ohne_Datum.txt"),
        Some(mtime("Kontoauszug_ohne_Datum.txt"))
    );
    assert_eq!(read_date_of(&setup, account), Some(mtime(account)));
}

#[test]
fn a_date_in_the_text_wins_over_a_date_in_the_name() {
    let setup = Setup::new();
    setup.write(
        "in/Rechnung_2021-05-31.txt",
        "Alfa AG\nRechnung\n12.03.2019\nBetrag 10,00 EUR",
    );
    assert_eq!(
        read_date_of(&setup, "Rechnung_2021-05-31.txt"),
        Some(date(2019, 3, 12))
    );
}

#[test]
fn a_known_document_without_a_date_gets_the_date_of_its_name() {
    let setup = Setup::new();
    let path = setup.write("in/Beleg_2021-05-31.txt", "no letter date in here");
    let hash = hash_path(&path).unwrap();
    let mut guard = setup.access.slot.lock().unwrap();
    let index = guard.as_mut().unwrap();
    index
        .insert_document(&ncrs_index::NewDocument {
            hash,
            path,
            source: ncrs_index::DocumentSource::Archive,
            received_at: chrono::Utc::now(),
            stamp: None,
        })
        .unwrap();
    drop(guard);
    assert_eq!(
        read_date_of(&setup, "Beleg_2021-05-31.txt"),
        Some(date(2021, 5, 31))
    );
}

#[test]
fn a_known_document_brings_its_fields_and_text_from_the_index() {
    let setup = Setup::new();
    let path = setup.write("in/known.txt", "no letter date in here");
    let hash = hash_path(&path).unwrap();
    let mut guard = setup.access.slot.lock().unwrap();
    let index = guard.as_mut().unwrap();
    let id = index
        .insert_document(&ncrs_index::NewDocument {
            hash,
            path: path.clone(),
            source: ncrs_index::DocumentSource::Archive,
            received_at: chrono::Utc::now(),
            stamp: None,
        })
        .unwrap();
    index
        .set_document_fields(
            id,
            &DocumentFields {
                sender: Some("Alfa AG".to_owned()),
                ..DocumentFields::default()
            },
            |_| ncrs_index::FieldOrigin::Unknown,
        )
        .unwrap();
    index.set_document_text(id, "stored text").unwrap();
    drop(guard);
    let read = setup.read(&setup.dir.path().join("in")).unwrap();
    assert_eq!(read.files[0].fields.sender.as_deref(), Some("Alfa AG"));
    assert_eq!(read.files[0].text.as_deref(), Some("stored text"));
}

fn index_with_stored_hash(setup: &Setup, path: &Path, stamp: ncrs_index::FileStamp) {
    let mut guard = setup.access.slot.lock().unwrap();
    let index = guard.as_mut().unwrap();
    let id = index
        .insert_document(&ncrs_index::NewDocument {
            hash: ContentHash([7; 32]),
            path: path.to_path_buf(),
            source: ncrs_index::DocumentSource::Archive,
            received_at: chrono::Utc::now(),
            stamp: Some(stamp),
        })
        .unwrap();
    index
        .set_document_fields(
            id,
            &DocumentFields {
                sender: Some("Alfa AG".to_owned()),
                ..DocumentFields::default()
            },
            |_| ncrs_index::FieldOrigin::Unknown,
        )
        .unwrap();
    index.set_document_text(id, "stored text").unwrap();
}

fn stamp_of_file(path: &Path) -> ncrs_index::FileStamp {
    let metadata = fs::metadata(path).unwrap();
    ncrs_index::FileStamp {
        size: metadata.len(),
        mtime: metadata.modified().unwrap(),
    }
}

#[test]
fn a_file_with_the_stamp_of_the_index_is_not_opened() {
    let setup = Setup::new();
    let path = setup.write(
        "in/known.txt",
        "content that does not match the stored hash",
    );
    index_with_stored_hash(&setup, &path, stamp_of_file(&path));
    let read = setup.read(&setup.dir.path().join("in")).unwrap();
    assert_eq!(read.files[0].hash, ContentHash([7; 32]));
    assert_eq!(read.files[0].fields.sender.as_deref(), Some("Alfa AG"));
    assert_eq!(read.files[0].text.as_deref(), Some("stored text"));
}

#[test]
fn a_file_with_another_stamp_than_the_index_is_hashed_again() {
    let setup = Setup::new();
    let path = setup.write(
        "in/known.txt",
        "content that does not match the stored hash",
    );
    let mut stamp = stamp_of_file(&path);
    stamp.size += 1;
    index_with_stored_hash(&setup, &path, stamp);
    let read = setup.read(&setup.dir.path().join("in")).unwrap();
    assert_eq!(read.files[0].hash, hash_path(&path).unwrap());
    assert_eq!(read.files[0].fields.sender, None);
}

#[test]
fn a_finished_read_has_counted_every_file_found() {
    let setup = Setup::new();
    for number in 0..5 {
        setup.write(&format!("in/deep/{number}.txt"), &format!("file {number}"));
    }
    let progress = Arc::new(ReadProgress::default());
    setup
        .read_with_progress(&setup.dir.path().join("in"), Arc::clone(&progress))
        .unwrap();
    assert_eq!(
        (progress.found(), progress.total(), progress.done()),
        (5, 5, 5)
    );
}
