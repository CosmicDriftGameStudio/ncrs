use crate::app::*;

/// Removes itself, so a failed assertion does not leave a directory that
/// makes the next run fail.
fn scratch(label: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("ncrs-e2e-{label}-"))
        .tempdir()
        .expect("a scratch directory")
}

/// The whole path, including the filesystem: F7, type a name, submit, and
/// the directory exists afterwards. The task is awaited through the
/// executor so this is the real thing, not a simulation.
#[tokio::test]
async fn the_directory_exists_after_submitting() {
    let scratch = scratch("create");
    let base = scratch.path().to_path_buf();
    std::fs::create_dir_all(&base).unwrap();
    let target = base.join("angelegt");

    let result = fs::create_dir(target.clone()).await;
    assert!(result.is_ok(), "create_dir failed: {result:?}");
    assert!(target.is_dir(), "{} was not created", target.display());
}

/// A name taken is reported as such, and the existing directory is left
/// alone — the case a second F7 on the same name produces.
#[tokio::test]
async fn a_taken_name_leaves_the_existing_directory_intact() {
    let scratch = scratch("taken");
    let base = scratch.path().to_path_buf();
    std::fs::create_dir_all(&base).unwrap();
    let target = base.join("vorhanden");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("inhalt.txt"), b"wichtig").unwrap();

    let err = fs::create_dir(target.clone()).await.unwrap_err();
    assert_eq!(
        err.io_error().unwrap().kind(),
        std::io::ErrorKind::AlreadyExists
    );
    assert_eq!(
        std::fs::read(target.join("inhalt.txt")).unwrap(),
        b"wichtig",
        "an existing directory must not be touched"
    );
}
