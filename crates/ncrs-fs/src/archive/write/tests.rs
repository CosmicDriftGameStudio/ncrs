use std::ffi::OsString;

use super::super::fixtures::{scratch, write_archive, Item, ALL_FORMATS, ENCRYPTED_7Z};
use super::super::{copy_out, list, refusal_of, ArchiveLocation, SevenZipRefusal};
use super::*;
use crate::transfer::Counting;

const BIG: usize = 2_621_440;

fn big_bytes() -> Vec<u8> {
    let mut state: u32 = 0x2545_f491;
    std::iter::repeat_with(|| {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        u8::try_from(state >> 24).unwrap()
    })
    .take(BIG)
    .collect()
}

fn at(archive: &Path, inner: &[&str]) -> ArchiveLocation {
    ArchiveLocation {
        archive: archive.to_path_buf(),
        format: ArchiveFormat::of_name(archive.file_name().unwrap()).unwrap(),
        inner: inner.iter().map(|part| (*part).to_owned()).collect(),
    }
}

fn listing(archive: &Path, inner: &[&str]) -> Vec<String> {
    list(&at(archive, inner))
        .unwrap()
        .entries
        .iter()
        .map(|entry| {
            let name = entry.name.to_string_lossy().into_owned();
            if entry.is_dir {
                format!("{name}/")
            } else {
                name
            }
        })
        .collect()
}

fn bytes_of(archive: &Path, inner: &[&str]) -> Vec<u8> {
    let out = scratch("bytes-of");
    let source = inner
        .iter()
        .fold(archive.to_path_buf(), |path, part| path.join(part));
    copy_out(&source, out.path(), &mut Counting, OnConflict::Fail).unwrap();
    std::fs::read(out.path().join(inner.last().unwrap())).unwrap()
}

fn inner(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

fn leftovers(dir: &Path) -> Vec<OsString> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name.to_string_lossy().starts_with(".ncrs-partial-"))
        .collect()
}

fn names_in_file(archive: &Path) -> Vec<String> {
    let mut names: Vec<String> = match ArchiveFormat::of_name(archive.file_name().unwrap()).unwrap()
    {
        ArchiveFormat::Zip => {
            let mut zip = open_zip(archive).unwrap();
            (0..zip.len())
                .map(|position| {
                    zip.by_index_raw(position)
                        .unwrap()
                        .name()
                        .unwrap()
                        .into_owned()
                })
                .collect()
        }
        ArchiveFormat::SevenZip => sevenz::open(archive)
            .unwrap()
            .archive()
            .files
            .iter()
            .map(|entry| entry.name().to_owned())
            .collect(),
        format => open_tar(archive, format)
            .unwrap()
            .entries()
            .unwrap()
            .map(|entry| {
                entry
                    .unwrap()
                    .path()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect(),
    };
    for name in &mut names {
        name.truncate(name.trim_end_matches('/').len());
    }
    names.sort();
    names
}

fn make_tree(root: &Path) -> PathBuf {
    let tree = root.join("pack-src");
    std::fs::create_dir_all(tree.join("sub")).unwrap();
    std::fs::create_dir(tree.join("empty")).unwrap();
    std::fs::write(tree.join("a.txt"), b"alpha").unwrap();
    std::fs::write(tree.join("sub").join("b.txt"), b"beta").unwrap();
    std::fs::write(tree.join("sub").join("Gr\u{fc}\u{df}e.txt"), b"hallo").unwrap();
    std::fs::write(tree.join("empty.txt"), b"").unwrap();
    std::fs::write(tree.join("big.bin"), big_bytes()).unwrap();
    tree
}

fn sample_base(archive: &Path) {
    write_archive(
        archive,
        &[
            Item::File("top.txt", b"top"),
            Item::File("2024/jan/a.jpg", b"aaaa"),
            Item::File("2024/b.jpg", b"bb"),
            Item::Dir("empty"),
        ],
    );
}

struct CancelAfter {
    calls: usize,
    limit: usize,
}

impl CancelAfter {
    fn new(limit: usize) -> Self {
        Self { calls: 0, limit }
    }

    fn tick(&mut self) -> io::Result<()> {
        self.calls += 1;
        if self.calls >= self.limit {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "cancelled",
            ));
        }
        Ok(())
    }
}

impl Progress for CancelAfter {
    fn advanced(&mut self, _done: usize, _total: usize) -> io::Result<()> {
        self.tick()
    }

    fn keep_going(&mut self) -> io::Result<()> {
        self.tick()
    }
}

#[cfg(unix)]
fn link(target: &str, at: &Path) {
    std::os::unix::fs::symlink(target, at).unwrap();
}

#[test]
fn every_format_packs_a_tree_that_reads_back_equal() {
    for (file, _) in ALL_FORMATS {
        let dir = scratch("pack");
        let tree = make_tree(dir.path());
        std::fs::write(dir.path().join("loose.txt"), b"loose").unwrap();
        #[cfg(unix)]
        link("a.txt", &tree.join("a-link"));
        let archive = dir.path().join(file);

        let report = pack(
            &[tree.clone(), dir.path().join("loose.txt")],
            &archive,
            &mut Counting,
        )
        .unwrap();

        assert_eq!(report.skipped, usize::from(cfg!(unix)), "{file}");
        assert_eq!(listing(&archive, &[]), ["loose.txt", "pack-src/"], "{file}");
        assert_eq!(
            listing(&archive, &["pack-src"]),
            ["a.txt", "big.bin", "empty/", "empty.txt", "sub/"],
            "{file}"
        );
        assert_eq!(
            listing(&archive, &["pack-src", "sub"]),
            ["Gr\u{fc}\u{df}e.txt", "b.txt"],
            "{file}"
        );
        assert_eq!(
            listing(&archive, &["pack-src", "empty"]),
            Vec::<String>::new()
        );
        assert_eq!(bytes_of(&archive, &["loose.txt"]), b"loose", "{file}");
        assert_eq!(bytes_of(&archive, &["pack-src", "a.txt"]), b"alpha");
        assert_eq!(bytes_of(&archive, &["pack-src", "empty.txt"]), b"");
        assert_eq!(bytes_of(&archive, &["pack-src", "big.bin"]), big_bytes());
        assert_eq!(
            bytes_of(&archive, &["pack-src", "sub", "Gr\u{fc}\u{df}e.txt"]),
            b"hallo"
        );
        assert!(leftovers(dir.path()).is_empty(), "{file}");
    }
}

#[cfg(unix)]
#[test]
fn packing_keeps_the_permission_bits() {
    use std::os::unix::fs::PermissionsExt as _;
    for (file, _) in ALL_FORMATS {
        let dir = scratch("pack-mode");
        let script = dir.path().join("run.sh");
        std::fs::write(&script, b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o750)).unwrap();
        let archive = dir.path().join(file);
        pack(&[script], &archive, &mut Counting).unwrap();

        let out = scratch("pack-mode-out");
        copy_out(
            &archive.join("run.sh"),
            out.path(),
            &mut Counting,
            OnConflict::Fail,
        )
        .unwrap();
        let mode = std::fs::metadata(out.path().join("run.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o750, "{file}");
    }
}

#[test]
fn pack_refuses_an_existing_archive_a_bad_name_and_nested_places() {
    let dir = scratch("pack-refuse");
    let tree = make_tree(dir.path());
    let existing = dir.path().join("there.zip");
    std::fs::write(&existing, b"keep me").unwrap();
    let err = pack(std::slice::from_ref(&tree), &existing, &mut Counting).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(&existing).unwrap(), b"keep me");

    let err_2 = pack(
        std::slice::from_ref(&tree),
        &dir.path().join("x.txt"),
        &mut Counting,
    )
    .unwrap_err();
    assert_eq!(err_2.kind(), io::ErrorKind::InvalidInput);

    let outer = dir.path().join("outer.zip");
    sample_base(&outer);
    let inside = outer.join("2024").join("new.zip");
    let err_3 = pack(std::slice::from_ref(&tree), &inside, &mut Counting).unwrap_err();
    assert_eq!(err_3.kind(), io::ErrorKind::InvalidInput);

    let err_4 = pack(
        &[outer.join("top.txt")],
        &dir.path().join("n.zip"),
        &mut Counting,
    )
    .unwrap_err();
    assert_eq!(err_4.kind(), io::ErrorKind::InvalidInput);
    assert!(!dir.path().join("n.zip").exists());

    let err_5 = pack(
        &[tree.clone(), tree],
        &dir.path().join("twice.zip"),
        &mut Counting,
    )
    .unwrap_err();
    assert_eq!(err_5.kind(), io::ErrorKind::InvalidInput);
    assert!(leftovers(dir.path()).is_empty());
}

fn two_invoices(dir: &Path) -> Vec<PathBuf> {
    let one = dir.join("firma");
    let two = dir.join("privat");
    std::fs::create_dir_all(&one).unwrap();
    std::fs::create_dir_all(&two).unwrap();
    std::fs::write(one.join("invoice.pdf"), b"from firma").unwrap();
    std::fs::write(two.join("invoice.pdf"), b"from privat").unwrap();
    vec![one.join("invoice.pdf"), two.join("invoice.pdf")]
}

#[test]
fn pack_named_writes_flat_numbered_names_with_the_right_contents() {
    let dir = scratch("pack-named-flat");
    let sources = two_invoices(dir.path());
    let names = crate::flat_names::flat_names(&sources);
    let items: Vec<(PathBuf, String)> = sources
        .into_iter()
        .zip(names)
        .map(|(path, name)| (path, name.unwrap().to_string_lossy().into_owned()))
        .collect();
    assert!(items.iter().all(|(_, name)| !name.contains('/')));
    for extension in ["zip", "tar", "tar.gz", "tar.zst", "7z"] {
        let archive = dir.path().join(format!("flat.{extension}"));
        pack_named(&items, &archive, &mut Counting).unwrap();
        assert_eq!(
            names_in_file(&archive),
            ["invoice (2).pdf", "invoice.pdf"],
            "{extension}"
        );
        assert_eq!(bytes_of(&archive, &["invoice.pdf"]), b"from firma");
        assert_eq!(bytes_of(&archive, &["invoice (2).pdf"]), b"from privat");
    }
}

#[test]
fn pack_named_nested_names_read_back_as_folders() {
    let dir = scratch("pack-named-nested");
    let sources = two_invoices(dir.path());
    let items = vec![
        (sources[0].clone(), "firma/invoice.pdf".to_owned()),
        (sources[1].clone(), "privat/2024/invoice.pdf".to_owned()),
    ];
    for extension in ["zip", "tar", "7z"] {
        let archive = dir.path().join(format!("nested.{extension}"));
        pack_named(&items, &archive, &mut Counting).unwrap();
        assert_eq!(listing(&archive, &[]), ["firma/", "privat/"], "{extension}");
        assert_eq!(listing(&archive, &["privat"]), ["2024/"]);
        assert_eq!(listing(&archive, &["privat", "2024"]), ["invoice.pdf"]);
        assert_eq!(
            bytes_of(&archive, &["privat", "2024", "invoice.pdf"]),
            b"from privat"
        );
    }
}

#[test]
fn pack_named_refuses_unsafe_empty_and_clashing_names_without_an_archive() {
    let dir = scratch("pack-named-refuse");
    let sources = two_invoices(dir.path());
    let source = sources[0].clone();
    for bad in [
        "../x.pdf",
        "/abs.pdf",
        "",
        "a//b.pdf",
        "a/../b.pdf",
        "./x.pdf",
    ] {
        let archive = dir.path().join("bad.zip");
        let err =
            pack_named(&[(source.clone(), bad.to_owned())], &archive, &mut Counting).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{bad:?}");
        assert!(!archive.exists(), "{bad:?}");
    }
    let archive = dir.path().join("twice.zip");
    let clash = vec![
        (sources[0].clone(), "same.pdf".to_owned()),
        (sources[1].clone(), "same.pdf".to_owned()),
    ];
    pack_named(&clash, &archive, &mut Counting).unwrap_err();
    let file_and_folder = vec![
        (sources[0].clone(), "a".to_owned()),
        (sources[1].clone(), "a/b.pdf".to_owned()),
    ];
    pack_named(&file_and_folder, &archive, &mut Counting).unwrap_err();
    assert!(!archive.exists());
    assert!(leftovers(dir.path()).is_empty());
}

#[test]
fn pack_named_keeps_folder_sources_below_their_entry_name() {
    let dir = scratch("pack-named-folder");
    let tree = make_tree(dir.path());
    let archive = dir.path().join("tree.zip");
    pack_named(
        &[(tree.clone(), "docs/kept".to_owned())],
        &archive,
        &mut Counting,
    )
    .unwrap();
    let names = names_in_file(&archive);
    assert!(names.contains(&"docs".to_owned()) && names.contains(&"docs/kept".to_owned()));
    assert!(names.iter().all(|name| name.starts_with("docs")));
}

#[test]
fn an_archive_can_be_packed_into_an_archive() {
    let dir = scratch("pack-archive");
    let inner_archive = dir.path().join("inner.zip");
    sample_base(&inner_archive);
    let outer = dir.path().join("outer.tar");
    pack(std::slice::from_ref(&inner_archive), &outer, &mut Counting).unwrap();
    assert_eq!(listing(&outer, &[]), ["inner.zip"]);
}

#[test]
fn add_puts_sources_into_the_root_and_into_a_folder_in_every_format() {
    for (file, _) in ALL_FORMATS {
        let dir = scratch("add");
        let archive = dir.path().join(file);
        sample_base(&archive);
        let photos = dir.path().join("photos");
        std::fs::create_dir(&photos).unwrap();
        std::fs::write(photos.join("x.jpg"), b"xx").unwrap();
        let note = dir.path().join("note.txt");
        std::fs::write(&note, b"note").unwrap();

        add(
            &archive,
            &[],
            &[photos.clone(), note.clone()],
            OnConflict::Fail,
            &mut Counting,
        )
        .unwrap();
        add(
            &archive,
            &inner(&["2024", "jan"]),
            &[note],
            OnConflict::Fail,
            &mut Counting,
        )
        .unwrap();

        assert_eq!(
            listing(&archive, &[]),
            ["2024/", "empty/", "note.txt", "photos/", "top.txt"],
            "{file}"
        );
        assert_eq!(bytes_of(&archive, &["photos", "x.jpg"]), b"xx", "{file}");
        assert_eq!(bytes_of(&archive, &["2024", "jan", "note.txt"]), b"note");
        assert_eq!(bytes_of(&archive, &["2024", "jan", "a.jpg"]), b"aaaa");
        assert_eq!(bytes_of(&archive, &["top.txt"]), b"top", "{file}");
        assert_eq!(listing(&archive, &["empty"]), Vec::<String>::new());
        assert!(leftovers(dir.path()).is_empty(), "{file}");
    }
}

#[test]
fn add_follows_the_conflict_policy() {
    for (file, _) in ALL_FORMATS {
        let dir = scratch("add-conflict");
        let archive = dir.path().join(file);
        sample_base(&archive);
        let clash = dir.path().join("2024");
        std::fs::create_dir(&clash).unwrap();
        std::fs::write(clash.join("new.txt"), b"fresh").unwrap();
        let top = dir.path().join("top.txt");
        std::fs::write(&top, b"replacement").unwrap();
        let before = std::fs::read(&archive).unwrap();

        let err = add(
            &archive,
            &[],
            &[top.clone(), clash.clone()],
            OnConflict::Fail,
            &mut Counting,
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists, "{file}");
        assert_eq!(std::fs::read(&archive).unwrap(), before, "{file}");
        assert!(leftovers(dir.path()).is_empty(), "{file}");

        let report = add(
            &archive,
            &[],
            &[top.clone(), clash.clone()],
            OnConflict::Skip,
            &mut Counting,
        )
        .unwrap();
        assert_eq!(report.kept_existing, 2, "{file}");
        assert_eq!(bytes_of(&archive, &["top.txt"]), b"top", "{file}");
        assert_eq!(listing(&archive, &["2024"]), ["b.jpg", "jan/"], "{file}");

        add(
            &archive,
            &[],
            &[top, clash],
            OnConflict::Overwrite,
            &mut Counting,
        )
        .unwrap();
        assert_eq!(bytes_of(&archive, &["top.txt"]), b"replacement", "{file}");
        assert_eq!(listing(&archive, &["2024"]), ["new.txt"], "{file}");
        assert_eq!(listing(&archive, &[]).len(), 3, "{file}");
    }
}

#[test]
fn add_refuses_a_missing_archive_folder_and_a_file_as_folder() {
    let dir = scratch("add-refuse");
    let archive = dir.path().join("a.zip");
    sample_base(&archive);
    let note = dir.path().join("note.txt");
    std::fs::write(&note, b"n").unwrap();
    let before = std::fs::read(&archive).unwrap();

    for folder in [inner(&["nope"]), inner(&["top.txt"])] {
        let err = add(
            &archive,
            &folder,
            std::slice::from_ref(&note),
            OnConflict::Fail,
            &mut Counting,
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound, "{folder:?}");
    }
    let err_2 = add(
        &dir.path().join("gone.zip"),
        &[],
        std::slice::from_ref(&note),
        OnConflict::Fail,
        &mut Counting,
    )
    .unwrap_err();
    assert_eq!(err_2.kind(), io::ErrorKind::InvalidInput);
    let err_3 = add(
        &archive.join("2024").join("x.zip"),
        &[],
        std::slice::from_ref(&note),
        OnConflict::Fail,
        &mut Counting,
    )
    .unwrap_err();
    assert_eq!(err_3.kind(), io::ErrorKind::InvalidInput);
    let err_4 = add(
        &archive,
        &[],
        &[archive.join("top.txt")],
        OnConflict::Fail,
        &mut Counting,
    )
    .unwrap_err();
    assert_eq!(err_4.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(std::fs::read(&archive).unwrap(), before);
    assert!(leftovers(dir.path()).is_empty());
}

#[test]
fn delete_removes_a_file_a_folder_with_children_and_everything() {
    for (file, _) in ALL_FORMATS {
        let dir = scratch("delete");
        let archive = dir.path().join(file);
        sample_base(&archive);

        delete(&archive, &[inner(&["top.txt"])], &mut Counting).unwrap();
        assert_eq!(listing(&archive, &[]), ["2024/", "empty/"], "{file}");

        delete(&archive, &[inner(&["2024"])], &mut Counting).unwrap();
        assert_eq!(listing(&archive, &[]), ["empty/"], "{file}");
        assert_eq!(names_in_file(&archive), ["empty"], "{file}");

        let err = delete(&archive, &[inner(&["2024"])], &mut Counting).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound, "{file}");
        let err_2 = delete(&archive, &[Vec::new()], &mut Counting).unwrap_err();
        assert_eq!(err_2.kind(), io::ErrorKind::InvalidInput, "{file}");

        delete(&archive, &[inner(&["empty"])], &mut Counting).unwrap();
        assert_eq!(listing(&archive, &[]), Vec::<String>::new(), "{file}");
        assert!(leftovers(dir.path()).is_empty(), "{file}");
    }
}

#[cfg(unix)]
#[test]
fn an_archive_behind_a_symlink_is_changed_and_the_link_stays() {
    let dir = scratch("behind-link");
    let real = dir.path().join("real.zip");
    sample_base(&real);
    let through = dir.path().join("link.zip");
    link("real.zip", &through);

    delete(&through, &[inner(&["top.txt"])], &mut Counting).unwrap();

    assert!(std::fs::symlink_metadata(&through)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(listing(&real, &[]), ["2024/", "empty/"]);
    assert_eq!(listing(&through, &[]), ["2024/", "empty/"]);
}

#[test]
fn a_folder_that_exists_only_through_its_children_is_deleted_with_them() {
    for (file, _) in ALL_FORMATS {
        let dir = scratch("delete-implicit");
        let archive = dir.path().join(file);
        write_archive(
            &archive,
            &[
                Item::File("deep/er/f.txt", b"f"),
                Item::File("keep.txt", b"k"),
            ],
        );
        delete(&archive, &[inner(&["deep"])], &mut Counting).unwrap();
        assert_eq!(listing(&archive, &[]), ["keep.txt"], "{file}");
        assert_eq!(names_in_file(&archive), ["keep.txt"], "{file}");
    }
}

#[test]
fn links_the_index_leaves_out_survive_an_add_and_a_delete() {
    for (file, _) in ALL_FORMATS {
        let dir = scratch("hidden");
        let archive = dir.path().join(file);
        write_archive(
            &archive,
            &[
                Item::File("a.txt", b"a"),
                Item::File("b.txt", b"b"),
                Item::Link("hidden-link", "a.txt"),
            ],
        );
        let note = dir.path().join("note.txt");
        std::fs::write(&note, b"n").unwrap();

        add(&archive, &[], &[note], OnConflict::Fail, &mut Counting).unwrap();
        let names = names_in_file(&archive);
        assert!(
            names.contains(&"hidden-link".to_owned()),
            "{file}: {names:?}"
        );
        assert_eq!(list(&at(&archive, &[])).unwrap().left_out, 1, "{file}");

        delete(&archive, &[inner(&["b.txt"])], &mut Counting).unwrap();
        let names_after_delete = names_in_file(&archive);
        assert_eq!(
            names_after_delete,
            ["a.txt", "hidden-link", "note.txt"],
            "{file}"
        );
    }
}

#[test]
fn apple_double_entries_survive_a_delete() {
    for (file, _) in ALL_FORMATS {
        let dir = scratch("apple");
        let archive = dir.path().join(file);
        write_archive(
            &archive,
            &[
                Item::File("a.txt", b"a"),
                Item::File("b.txt", b"b"),
                Item::File("__MACOSX/._a.txt", b"x"),
                Item::File("._b.txt", b"x"),
            ],
        );

        delete(&archive, &[inner(&["a.txt"])], &mut Counting).unwrap();
        assert_eq!(
            names_in_file(&archive),
            ["._b.txt", "__MACOSX/._a.txt", "b.txt"],
            "{file}"
        );
        assert_eq!(listing(&archive, &[]), ["b.txt"], "{file}");
    }
}

#[test]
fn a_link_does_not_block_a_name_it_shares_with_a_source() {
    for (file, _) in ALL_FORMATS {
        let dir = scratch("hidden-conflict");
        let archive = dir.path().join(file);
        write_archive(
            &archive,
            &[Item::File("a.txt", b"a"), Item::Link("l", "a.txt")],
        );
        let source = dir.path().join("l");
        std::fs::write(&source, b"file l").unwrap();
        let before = std::fs::read(&archive).unwrap();

        let err = add(
            &archive,
            &[],
            std::slice::from_ref(&source),
            OnConflict::Fail,
            &mut Counting,
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists, "{file}");
        assert_eq!(std::fs::read(&archive).unwrap(), before, "{file}");

        add(
            &archive,
            &[],
            &[source],
            OnConflict::Overwrite,
            &mut Counting,
        )
        .unwrap();
        assert_eq!(names_in_file(&archive), ["a.txt", "l"], "{file}");
        assert_eq!(bytes_of(&archive, &["l"]), b"file l", "{file}");
    }
}

#[test]
fn an_entry_with_an_unsafe_name_is_never_written_by_a_tar_rewrite() {
    let dir = scratch("unsafe-tar");
    let archive = dir.path().join("evil.tar");
    write_archive(
        &archive,
        &[Item::File("ok.txt", b"ok"), Item::File("../evil.txt", b"x")],
    );
    let note = dir.path().join("note.txt");
    std::fs::write(&note, b"n").unwrap();
    let before = std::fs::read(&archive).unwrap();

    let err = add(
        &archive,
        &[],
        std::slice::from_ref(&note),
        OnConflict::Fail,
        &mut Counting,
    )
    .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert_eq!(std::fs::read(&archive).unwrap(), before);
    assert!(leftovers(dir.path()).is_empty());

    let zipped = dir.path().join("evil.zip");
    write_archive(
        &zipped,
        &[Item::File("ok.txt", b"ok"), Item::File("../evil.txt", b"x")],
    );
    add(&zipped, &[], &[note], OnConflict::Fail, &mut Counting).unwrap();
    assert_eq!(listing(&zipped, &[]), ["note.txt", "ok.txt"]);
    assert!(names_in_file(&zipped).contains(&"../evil.txt".to_owned()));
    assert!(!dir.path().parent().unwrap().join("evil.txt").exists());
}

#[test]
fn a_cancelled_write_changes_nothing_and_leaves_no_temporary_file() {
    for (file, _) in ALL_FORMATS {
        let dir = scratch("cancel");
        let archive = dir.path().join(file);
        sample_base(&archive);
        let note = dir.path().join("note.txt");
        std::fs::write(&note, b"n").unwrap();
        let before = std::fs::read(&archive).unwrap();
        let listed_before: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();

        let err = add(
            &archive,
            &[],
            &[note],
            OnConflict::Fail,
            &mut CancelAfter::new(3),
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::ConnectionAborted, "{file}");
        let err_2 = delete(&archive, &[inner(&["2024"])], &mut CancelAfter::new(3)).unwrap_err();
        assert_eq!(err_2.kind(), io::ErrorKind::ConnectionAborted, "{file}");

        assert_eq!(std::fs::read(&archive).unwrap(), before, "{file}");
        let mut listed_after: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        let mut expected = listed_before;
        expected.sort();
        listed_after.sort();
        assert_eq!(listed_after, expected, "{file}");
    }
}

#[test]
fn a_cancelled_pack_creates_no_archive_also_inside_a_large_file() {
    for (file, _) in ALL_FORMATS {
        let dir = scratch("cancel-pack");
        let tree = make_tree(dir.path());
        let archive = dir.path().join(file);
        for limit in [3, 8] {
            let err = pack(
                std::slice::from_ref(&tree),
                &archive,
                &mut CancelAfter::new(limit),
            )
            .unwrap_err();
            assert_eq!(
                err.kind(),
                io::ErrorKind::ConnectionAborted,
                "{file} {limit}"
            );
            assert!(!archive.exists(), "{file} {limit}");
            assert!(leftovers(dir.path()).is_empty(), "{file} {limit}");
        }

        let big = dir.path().join("big-only.bin");
        std::fs::write(&big, big_bytes()).unwrap();
        let err_2 = pack(&[big], &archive, &mut CancelAfter::new(4)).unwrap_err();
        assert_eq!(err_2.kind(), io::ErrorKind::ConnectionAborted, "{file}");
        assert!(!archive.exists(), "{file}");
        assert!(leftovers(dir.path()).is_empty(), "{file}");
    }
}

#[test]
fn a_7z_rewrite_keeps_every_entry() {
    let dir = scratch("sevenz-rewrite");
    let archive = dir.path().join("keep.7z");
    write_archive(
        &archive,
        &[
            Item::File("a.txt", b"alpha"),
            Item::File("sub/b.txt", b"beta"),
            Item::File("empty.txt", b""),
            Item::Dir("void"),
            Item::Link("lnk", "a.txt"),
        ],
    );
    let note = dir.path().join("note.txt");
    std::fs::write(&note, b"n").unwrap();

    add(&archive, &[], &[note], OnConflict::Fail, &mut Counting).unwrap();

    assert_eq!(
        names_in_file(&archive),
        ["a.txt", "empty.txt", "lnk", "note.txt", "sub/b.txt", "void"]
    );
    assert_eq!(bytes_of(&archive, &["a.txt"]), b"alpha");
    assert_eq!(bytes_of(&archive, &["sub", "b.txt"]), b"beta");
    assert_eq!(bytes_of(&archive, &["empty.txt"]), b"");
    assert_eq!(
        listing(&archive, &[]),
        ["a.txt", "empty.txt", "note.txt", "sub/", "void/"]
    );
}

#[test]
fn an_encrypted_7z_is_refused_and_left_unchanged() {
    for (file, bytes) in ENCRYPTED_7Z {
        let dir = scratch("sevenz-encrypted");
        let archive = dir.path().join(file);
        std::fs::write(&archive, bytes).unwrap();
        let note = dir.path().join("note.txt");
        std::fs::write(&note, b"n").unwrap();

        let added = add(&archive, &[], &[note], OnConflict::Fail, &mut Counting).unwrap_err();
        assert_eq!(
            refusal_of(&added),
            Some(SevenZipRefusal::Encrypted),
            "{file}"
        );
        let deleted = delete(&archive, &[inner(&["secret.txt"])], &mut Counting).unwrap_err();
        assert_eq!(
            refusal_of(&deleted),
            Some(SevenZipRefusal::Encrypted),
            "{file}"
        );

        assert_eq!(std::fs::read(&archive).unwrap(), bytes, "{file}");
        assert!(leftovers(dir.path()).is_empty(), "{file}");
    }
}

#[test]
fn a_rewrite_keeps_the_permissions_of_the_archive_file() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = scratch("keep-mode");
        let archive = dir.path().join("m.zip");
        sample_base(&archive);
        std::fs::set_permissions(&archive, std::fs::Permissions::from_mode(0o640)).unwrap();
        delete(&archive, &[inner(&["top.txt"])], &mut Counting).unwrap();
        let mode = std::fs::metadata(&archive).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o640);
    }
}

#[test]
fn the_cached_listing_is_dropped_after_a_write() {
    let dir = scratch("forget");
    let archive = dir.path().join("f.tar");
    sample_base(&archive);
    assert_eq!(listing(&archive, &[]).len(), 3);
    delete(&archive, &[inner(&["top.txt"])], &mut Counting).unwrap();
    assert_eq!(listing(&archive, &[]).len(), 2);
}

#[test]
fn a_tar_rewrite_keeps_long_names_the_root_entry_and_link_targets() {
    let dir = scratch("tar-long");
    let archive = dir.path().join("long.tar");
    let long_folder = "d".repeat(120);
    let long_file = format!("{long_folder}/{}.txt", "f".repeat(120));
    let mut builder = tar::Builder::new(File::create(&archive).unwrap());
    let mut root = tar::Header::new_gnu();
    root.set_entry_type(tar::EntryType::Directory);
    root.set_size(0);
    root.set_mode(0o755);
    builder.append_data(&mut root, "./", io::empty()).unwrap();
    let mut file = tar::Header::new_gnu();
    file.set_size(4);
    file.set_mode(0o644);
    builder
        .append_data(&mut file, &long_file, &b"long"[..])
        .unwrap();
    let mut link = tar::Header::new_gnu();
    link.set_entry_type(tar::EntryType::Symlink);
    link.set_size(0);
    let target = "t".repeat(130);
    builder.append_link(&mut link, "ln", &target).unwrap();
    let mut gone = tar::Header::new_gnu();
    gone.set_size(1);
    builder
        .append_data(&mut gone, "gone.txt", &b"g"[..])
        .unwrap();
    builder.finish().unwrap();
    drop(builder);

    delete(&archive, &[inner(&["gone.txt"])], &mut Counting).unwrap();

    let mut tar = open_tar(&archive, ArchiveFormat::Tar).unwrap();
    let kept: Vec<(String, Option<String>)> = tar
        .entries()
        .unwrap()
        .map(|result| {
            let entry = result.unwrap();
            (
                entry.path().unwrap().to_string_lossy().into_owned(),
                entry
                    .link_name()
                    .unwrap()
                    .map(|linked| linked.to_string_lossy().into_owned()),
            )
        })
        .collect();
    assert_eq!(kept.len(), 3, "{kept:?}");
    assert!(kept.iter().any(|(name, _)| *name == long_file));
    assert!(kept
        .iter()
        .any(|(name, link_name)| name == "ln" && link_name.as_deref() == Some(target.as_str())));
    assert_eq!(
        bytes_of(&archive, &long_file.split('/').collect::<Vec<_>>()),
        b"long"
    );
}

#[cfg(unix)]
#[test]
fn a_link_to_a_folder_is_skipped_not_followed() {
    let dir = scratch("link-dir");
    let outside = dir.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), b"s").unwrap();
    let tree = dir.path().join("tree");
    std::fs::create_dir(&tree).unwrap();
    link("../outside", &tree.join("way-out"));
    let archive = dir.path().join("t.zip");

    let report = pack(
        &[tree.clone(), tree.join("way-out")],
        &archive,
        &mut Counting,
    )
    .unwrap();
    assert_eq!(report.skipped, 2);
    assert_eq!(listing(&archive, &["tree"]), Vec::<String>::new());
}
