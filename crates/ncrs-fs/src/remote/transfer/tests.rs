use super::super::test_server::{names_in, short_timeouts, symlink, Fixture};
use super::*;

/// Stops the transfer at the `after`th check.
struct StopAt {
    after: usize,
    calls: usize,
    saw: Option<Box<dyn FnMut(usize) + Send>>,
}

impl StopAt {
    fn never() -> Self {
        Self {
            after: usize::MAX,
            calls: 0,
            saw: None,
        }
    }

    fn at(after: usize) -> Self {
        Self {
            after,
            calls: 0,
            saw: None,
        }
    }
}

impl Progress for StopAt {
    fn advanced(&mut self, _done: usize, _total: usize) -> io::Result<()> {
        Ok(())
    }

    fn keep_going(&mut self) -> io::Result<()> {
        self.calls += 1;
        if let Some(saw) = self.saw.as_mut() {
            saw(self.calls);
        }
        if self.calls >= self.after {
            return Err(io::Error::other("stopped"));
        }
        Ok(())
    }
}

fn bytes(len: usize) -> Vec<u8> {
    (0..len).map(|i| u8::try_from(i % 251).unwrap()).collect()
}

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    tokio::task::spawn_blocking(work).await.unwrap()
}

async fn pull(fixture: &Fixture, remote: &str, target_dir: &Path) -> io::Result<usize> {
    pull_with(fixture, remote, target_dir, OnConflict::Fail).await
}

async fn pull_with(
    fixture: &Fixture,
    remote: &str,
    target_dir: &Path,
    how: OnConflict,
) -> io::Result<usize> {
    let pool = Arc::clone(&fixture.pool);
    let (into, source) = (target_dir.to_path_buf(), Fixture::at(remote));
    blocking(move || download(&pool, &source, &into, &mut Counting, how)).await
}

async fn push(
    fixture: &Fixture,
    local: &Path,
    remote_dir: &str,
    how: OnConflict,
) -> io::Result<usize> {
    let pool = Arc::clone(&fixture.pool);
    let (from, dir) = (local.to_path_buf(), Fixture::at(remote_dir));
    blocking(move || upload(&pool, &from, &dir, &mut Counting, how)).await
}

fn out_dir(fixture: &Fixture) -> PathBuf {
    let out = fixture.scratch.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn download_file_streams_in_blocks_and_lands_under_its_name() {
    let fixture = Fixture::connected().await;
    let content = bytes(CHUNK * 2 + 1000);
    std::fs::write(fixture.server.local("/home/tester/big.bin"), &content).unwrap();
    let out = out_dir(&fixture);
    let (pool, target) = (Arc::clone(&fixture.pool), out.clone());
    let source = Fixture::at("/home/tester/big.bin");

    let (written, calls) = blocking(move || {
        let mut progress = StopAt::never();
        let written = download(&pool, &source, &target, &mut progress, OnConflict::Fail).unwrap();
        (written, progress.calls)
    })
    .await;

    assert_eq!(written, 1);
    assert_eq!(std::fs::read(out.join("big.bin")).unwrap(), content);
    assert!(
        calls >= 4,
        "asked to go on once per block, not once per file: {calls}"
    );
    assert_eq!(names_in(&out), ["big.bin"], "no staging folder is left");
}

#[tokio::test(flavor = "multi_thread")]
async fn download_folder_skips_server_symlinks() {
    let fixture = Fixture::connected().await;
    let docs = fixture.server.local("/home/tester/docs");
    std::fs::create_dir_all(docs.join("sub")).unwrap();
    std::fs::write(docs.join("a.txt"), "a").unwrap();
    std::fs::write(docs.join("sub/b.txt"), "b").unwrap();
    std::fs::write(fixture.server.local("/secret.txt"), "secret").unwrap();
    symlink(&fixture.server.local("/secret.txt"), &docs.join("link"));
    symlink(&fixture.server.local("/home"), &docs.join("dirlink"));
    let out = out_dir(&fixture);

    let written = pull(&fixture, "/home/tester/docs", &out).await.unwrap();

    assert_eq!(written, 2);
    assert_eq!(names_in(&out.join("docs")), ["a.txt", "sub"]);
    assert_eq!(std::fs::read(out.join("docs/sub/b.txt")).unwrap(), b"b");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_link_asked_for_directly_is_not_followed() {
    let fixture = Fixture::connected().await;
    std::fs::write(fixture.server.local("/secret.txt"), "secret").unwrap();
    symlink(
        &fixture.server.local("/secret.txt"),
        &fixture.server.local("/home/tester/link"),
    );
    let out = out_dir(&fixture);

    let error = pull(&fixture, "/home/tester/link", &out).await.unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    assert!(names_in(&out).is_empty());
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn download_unsafe_name_is_refused_and_leaves_nothing() {
    for unsafe_one in ["a\\b", "C:", "x\u{202E}y", "bell\u{7}"] {
        let fixture = Fixture::connected().await;
        let docs = fixture.server.local("/home/tester/docs");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(docs.join("fine.txt"), "ok").unwrap();
        std::fs::write(docs.join(unsafe_one), "bad").unwrap();
        let out = out_dir(&fixture);

        let error = pull(&fixture, "/home/tester/docs", &out).await.unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{unsafe_one}");
        let named = error
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<UnsafeName>());
        assert_eq!(named.map(|name| name.0.as_str()), Some(unsafe_one));
        assert!(
            names_in(&out).is_empty(),
            "{unsafe_one}: {:?}",
            names_in(&out)
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn download_abort_leaves_no_partial_file() {
    let fixture = Fixture::connected().await;
    std::fs::write(
        fixture.server.local("/home/tester/big.bin"),
        bytes(CHUNK * 3),
    )
    .unwrap();
    let out = out_dir(&fixture);
    let (pool, target) = (Arc::clone(&fixture.pool), out.clone());
    let source = Fixture::at("/home/tester/big.bin");

    let result = blocking(move || {
        download(
            &pool,
            &source,
            &target,
            &mut StopAt::at(3),
            OnConflict::Fail,
        )
    })
    .await;

    assert_eq!(result.unwrap_err().to_string(), "stopped");
    assert!(names_in(&out).is_empty(), "{:?}", names_in(&out));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_download_over_an_existing_name_follows_the_conflict_rule() {
    let fixture = Fixture::connected().await;
    std::fs::write(fixture.server.local("/home/tester/x.txt"), "new").unwrap();
    let out = out_dir(&fixture);
    std::fs::write(out.join("x.txt"), "old").unwrap();

    let error = pull(&fixture, "/home/tester/x.txt", &out)
        .await
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(out.join("x.txt")).unwrap(), b"old");

    let skipped = pull_with(&fixture, "/home/tester/x.txt", &out, OnConflict::Skip)
        .await
        .unwrap();
    assert_eq!(skipped, 0);
    assert_eq!(std::fs::read(out.join("x.txt")).unwrap(), b"old");

    pull_with(&fixture, "/home/tester/x.txt", &out, OnConflict::Overwrite)
        .await
        .unwrap();
    assert_eq!(std::fs::read(out.join("x.txt")).unwrap(), b"new");
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_file_goes_through_partial_and_rename() {
    let fixture = Fixture::connected().await;
    let content = bytes(CHUNK + 500);
    let local = fixture.scratch.path().join("up.bin");
    std::fs::write(&local, &content).unwrap();
    let home = fixture.server.local("/home/tester");
    let seen = Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let (pool, dir) = (Arc::clone(&fixture.pool), Fixture::at("/home/tester"));
    let (watched, folder) = (Arc::clone(&seen), home.clone());

    blocking(move || {
        let mut progress = StopAt::never();
        progress.saw = Some(Box::new(move |_| {
            watched.lock().unwrap().push(names_in(&folder));
        }));
        upload(&pool, &local, &dir, &mut progress, OnConflict::Fail).unwrap()
    })
    .await;

    assert_eq!(std::fs::read(home.join("up.bin")).unwrap(), content);
    assert_eq!(names_in(&home), ["up.bin"], "no hidden file is left");
    let snapshots = seen.lock().unwrap().clone();
    assert!(
        snapshots.iter().any(|names| {
            names.len() == 1
                && names[0].starts_with(".ncrs-partial-")
                && names[0].ends_with("-up.bin")
        }),
        "while it was written it had a hidden name: {snapshots:?}"
    );
    assert!(snapshots
        .iter()
        .all(|names| !names.contains(&"up.bin".to_owned())));
    let events = fixture.server.control.events();
    let open = events.iter().position(|event| *event == "open").unwrap();
    let rename = events.iter().position(|event| *event == "rename").unwrap();
    assert!(open < rename);
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_conflict_fail_skip_overwrite() {
    let fixture = Fixture::connected().await;
    let home = fixture.server.local("/home/tester");
    std::fs::write(home.join("same.txt"), "old").unwrap();
    let local = fixture.scratch.path().join("same.txt");
    std::fs::write(&local, "new").unwrap();

    let failed = push(&fixture, &local, "/home/tester", OnConflict::Fail)
        .await
        .unwrap_err();
    assert_eq!(failed.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(home.join("same.txt")).unwrap(), b"old");

    let skipped = push(&fixture, &local, "/home/tester", OnConflict::Skip)
        .await
        .unwrap();
    assert_eq!(skipped, 0);
    assert_eq!(std::fs::read(home.join("same.txt")).unwrap(), b"old");

    let written = push(&fixture, &local, "/home/tester", OnConflict::Overwrite)
        .await
        .unwrap();
    assert_eq!(written, 1);
    assert_eq!(std::fs::read(home.join("same.txt")).unwrap(), b"new");
    assert_eq!(names_in(&home), ["same.txt"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_abort_removes_partial_on_server() {
    let fixture = Fixture::connected().await;
    let local = fixture.scratch.path().join("big.bin");
    std::fs::write(&local, bytes(CHUNK * 3)).unwrap();
    let home = fixture.server.local("/home/tester");
    let (pool, dir) = (Arc::clone(&fixture.pool), Fixture::at("/home/tester"));

    let result =
        blocking(move || upload(&pool, &local, &dir, &mut StopAt::at(3), OnConflict::Fail)).await;

    assert_eq!(result.unwrap_err().to_string(), "stopped");
    assert!(names_in(&home).is_empty(), "{:?}", names_in(&home));
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_folder_is_built_hidden_and_renamed_once() {
    let fixture = Fixture::connected().await;
    let tree = fixture.scratch.path().join("tree");
    std::fs::create_dir_all(tree.join("inner")).unwrap();
    std::fs::write(tree.join("a.txt"), "a").unwrap();
    std::fs::write(tree.join("inner/b.txt"), "b").unwrap();
    let home = fixture.server.local("/home/tester");

    let written = push(&fixture, &tree, "/home/tester", OnConflict::Fail)
        .await
        .unwrap();

    assert_eq!(written, 2);
    assert_eq!(names_in(&home), ["tree"]);
    assert_eq!(std::fs::read(home.join("tree/inner/b.txt")).unwrap(), b"b");
    let renames = fixture
        .server
        .control
        .events()
        .iter()
        .filter(|e| **e == "rename")
        .count();
    assert!(renames >= 3, "two files and the folder: {renames}");
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_folder_over_a_folder_merges_when_overwriting() {
    let fixture = Fixture::connected().await;
    let home = fixture.server.local("/home/tester");
    std::fs::create_dir_all(home.join("tree")).unwrap();
    std::fs::write(home.join("tree/keep.txt"), "keep").unwrap();
    std::fs::write(home.join("tree/a.txt"), "old").unwrap();
    let tree = fixture.scratch.path().join("tree");
    std::fs::create_dir_all(tree.join("new")).unwrap();
    std::fs::write(tree.join("a.txt"), "new").unwrap();
    std::fs::write(tree.join("new/c.txt"), "c").unwrap();

    let refused = push(&fixture, &tree, "/home/tester", OnConflict::Fail)
        .await
        .unwrap_err();
    assert_eq!(refused.kind(), io::ErrorKind::AlreadyExists);
    push(&fixture, &tree, "/home/tester", OnConflict::Overwrite)
        .await
        .unwrap();

    assert_eq!(names_in(&home.join("tree")), ["a.txt", "keep.txt", "new"]);
    assert_eq!(std::fs::read(home.join("tree/a.txt")).unwrap(), b"new");
    assert_eq!(std::fs::read(home.join("tree/new/c.txt")).unwrap(), b"c");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn upload_folder_does_not_follow_local_symlinks() {
    let fixture = Fixture::connected().await;
    let outside = fixture.scratch.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), "secret").unwrap();
    let tree = fixture.scratch.path().join("tree");
    std::fs::create_dir(&tree).unwrap();
    std::fs::write(tree.join("a.txt"), "a").unwrap();
    std::os::unix::fs::symlink(&outside, tree.join("dirlink")).unwrap();
    std::os::unix::fs::symlink(outside.join("secret.txt"), tree.join("filelink")).unwrap();

    push(&fixture, &tree, "/home/tester", OnConflict::Fail)
        .await
        .unwrap();

    let home = fixture.server.local("/home/tester");
    assert_eq!(names_in(&home.join("tree")), ["a.txt"]);

    let alone = push(
        &fixture,
        &tree.join("filelink"),
        "/home/tester",
        OnConflict::Fail,
    )
    .await
    .unwrap_err();
    assert_eq!(alone.kind(), io::ErrorKind::Unsupported);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn view_copy_is_owner_only() {
    use std::os::unix::fs::PermissionsExt as _;
    let fixture = Fixture::connected().await;
    std::fs::write(fixture.server.local("/home/tester/doc.txt"), "text").unwrap();
    let slot = fixture.scratch.path().join("slot");
    std::fs::create_dir(&slot).unwrap();
    let (pool, to) = (Arc::clone(&fixture.pool), slot.clone());
    let source = Fixture::at("/home/tester/doc.txt");

    let copy = blocking(move || download_for_view(&pool, &source, &to, &mut Counting))
        .await
        .unwrap();

    assert_eq!(copy, slot.join("doc.txt"));
    assert_eq!(std::fs::read(&copy).unwrap(), b"text");
    assert_eq!(
        std::fs::metadata(&copy).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_view_copy_removes_its_slot() {
    let fixture = Fixture::connected().await;
    let slot = fixture.scratch.path().join("slot");
    std::fs::create_dir(&slot).unwrap();
    let (pool, to) = (Arc::clone(&fixture.pool), slot.clone());
    let source = Fixture::at("/home/tester/missing.txt");

    let error = blocking(move || download_for_view(&pool, &source, &to, &mut Counting))
        .await
        .unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert!(!slot.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stalled_block_times_out_and_the_connection_stays() {
    let fixture = Fixture::connected_with(short_timeouts()).await;
    std::fs::write(
        fixture.server.local("/home/tester/big.bin"),
        bytes(CHUNK * 2),
    )
    .unwrap();
    fixture.server.control.stall_after(0);
    let out = out_dir(&fixture);

    let error = pull(&fixture, "/home/tester/big.bin", &out)
        .await
        .unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(fixture.pool.is_connected(&Fixture::id()));
    assert!(names_in(&out).is_empty());
}
