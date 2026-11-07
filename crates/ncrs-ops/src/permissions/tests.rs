// A failing assertion in a test is the signal, so `unwrap` belongs here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;

const LANG: Language = Language::English;

fn target(name: &str, is_dir: bool, mode: u32) -> PermissionTarget {
    PermissionTarget {
        path: format!("/home/anna/{name}").into(),
        is_dir,
        mode: Some(mode),
        owner: Some("anna".into()),
        group: Some("staff".into()),
        read_only: None,
    }
}

fn dialog_of(targets: Vec<PermissionTarget>) -> PermissionsDialog {
    PermissionsDialog::new(targets, false)
}

fn one(mode: u32) -> PermissionsDialog {
    dialog_of(vec![target("a.txt", false, mode)])
}

#[test]
fn the_boxes_show_the_mode_and_nothing_is_touched() {
    let dialog = one(0o644);
    assert_eq!(dialog.state(0), Check::On);
    assert_eq!(dialog.state(2), Check::Off);
    assert_eq!(dialog.state(6), Check::On);
    assert_eq!(dialog.octal_text(LANG), "644");
    assert_eq!(dialog.symbolic().as_deref(), Some("rw-r--r--"));
    assert_eq!(dialog.plan(), None);
}

#[test]
fn a_bit_that_differs_between_entries_is_mixed() {
    let dialog = dialog_of(vec![target("a", false, 0o644), target("b", false, 0o664)]);
    assert_eq!(dialog.state(4), Check::Mixed);
    assert_eq!(dialog.state(0), Check::On);
    assert_eq!(dialog.octal_text(LANG), "mixed");
    assert_eq!(dialog.symbolic(), None);
}

#[test]
fn a_plain_box_toggles_and_a_mixed_box_goes_round() {
    let mut dialog = dialog_of(vec![target("a", false, 0o644), target("b", false, 0o664)]);
    dialog.click(4);
    assert_eq!(dialog.state(4), Check::On);
    dialog.click(4);
    assert_eq!(dialog.state(4), Check::Off);
    dialog.click(4);
    assert_eq!(dialog.state(4), Check::Mixed);
    assert_eq!(dialog.plan(), None, "back on mixed it is untouched");

    dialog.click(2);
    assert_eq!(dialog.state(2), Check::On);
    dialog.click(2);
    assert_eq!(dialog.state(2), Check::Off);
}

#[test]
fn the_plan_sets_and_clears_only_touched_bits() {
    let mut dialog = one(0o644);
    dialog.click(2);
    dialog.click(1);
    let plan = dialog.plan().unwrap();
    assert_eq!(plan.edit.set, 0o100);
    assert_eq!(plan.edit.clear, 0o200);
    assert_eq!(plan.edit.read_only, None);
    assert_eq!(plan.recurse, None);
}

#[test]
fn the_octal_field_follows_the_boxes_and_the_boxes_follow_the_field() {
    let mut dialog = one(0o644);
    dialog.click(2);
    assert_eq!(dialog.octal_text(LANG), "744");

    dialog.type_octal("600".into());
    assert_eq!(dialog.state(0), Check::On);
    assert_eq!(dialog.state(1), Check::On);
    assert_eq!(dialog.state(3), Check::Off);
    assert_eq!(dialog.octal_text(LANG), "600");
    let plan = dialog.plan().unwrap();
    assert_eq!(plan.edit.set, 0o600);
    assert_eq!(plan.edit.clear, 0o7777 & !0o600);
}

#[test]
fn an_invalid_number_stays_in_the_field_only() {
    let mut dialog = one(0o644);
    dialog.type_octal("89".into());
    assert_eq!(dialog.octal_text(LANG), "89");
    assert_eq!(dialog.state(0), Check::On);
    assert_eq!(dialog.plan(), None);
    dialog.type_octal("12".into());
    dialog.type_octal("7777x".into());
    assert_eq!(dialog.plan(), None);
}

#[test]
fn a_special_bit_opens_the_advanced_part_and_makes_four_digits() {
    assert!(!one(0o755).advanced_open());
    let dialog = one(0o4755);
    assert!(dialog.advanced_open());
    assert_eq!(dialog.octal_text(LANG), "4755");
    assert_eq!(dialog.symbolic().as_deref(), Some("rwsr-xr-x"));

    let mut typed = one(0o644);
    typed.type_octal("1777".into());
    assert!(typed.advanced_open());
    assert_eq!(typed.symbolic().as_deref(), Some("rwxrwxrwt"));
}

#[test]
fn the_contents_only_count_with_a_folder_and_the_switch() {
    let mut files = one(0o644);
    files.toggle_contents();
    assert!(!files.has_folder());
    assert_eq!(files.plan(), None);

    let mut folder = dialog_of(vec![target("d", true, 0o755)]);
    assert_eq!(folder.plan(), None);
    folder.toggle_contents();
    folder.set_for_files(ForFiles::WithoutExecute);
    folder.set_for_folders(ForFolders::Unchanged);
    let plan = folder.plan().unwrap();
    assert_eq!(
        plan.recurse,
        Some((ForFiles::WithoutExecute, ForFolders::Unchanged))
    );
}

#[test]
fn owner_and_group_are_shown_or_mixed() {
    let same = one(0o644);
    assert_eq!(
        same.owner_and_group(LANG).as_deref(),
        Some("Owner anna · Group staff")
    );
    let mut other = target("b", false, 0o644);
    other.owner = Some("bob".into());
    let mixed = dialog_of(vec![target("a", false, 0o644), other]);
    assert_eq!(
        mixed.owner_and_group(LANG).as_deref(),
        Some("Owner mixed · Group staff")
    );
}

#[test]
fn the_title_names_one_entry_or_counts_several() {
    assert_eq!(one(0o644).title(LANG), "Permissions – a.txt");
    let several = dialog_of(vec![
        target("a", false, 0o644),
        target("b", false, 0o644),
        target("d", true, 0o755),
    ]);
    assert_eq!(several.title(LANG), "Permissions – 2 files, 1 folders");
}

#[test]
fn on_windows_only_read_only_goes_into_the_plan() {
    let mut read_only = target("a", false, 0);
    read_only.mode = None;
    read_only.read_only = Some(false);
    let mut dialog = PermissionsDialog::new(vec![read_only], true);
    assert_eq!(dialog.state(READ_ONLY), Check::Off);
    assert_eq!(dialog.plan(), None);
    dialog.click(READ_ONLY);
    let plan = dialog.plan().unwrap();
    assert_eq!(plan.edit.read_only, Some(true));
    assert_eq!((plan.edit.set, plan.edit.clear), (0, 0));
}

#[test]
fn the_parent_row_is_never_a_target() {
    let parent = FileEntry {
        name: "..".into(),
        path: "/home".into(),
        is_dir: true,
        is_symlink: false,
        is_parent: true,
        size: 0,
        modified: None,
        folder_size: None,
        details: Default::default(),
    };
    assert!(targets_of(&[parent]).is_empty());
}
