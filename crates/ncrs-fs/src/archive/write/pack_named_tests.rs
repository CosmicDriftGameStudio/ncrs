use super::tests::{bytes_of, leftovers, listing, make_tree, names_in_file};
use super::*;
use crate::archive::fixtures::scratch;
use crate::transfer::Counting;

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
