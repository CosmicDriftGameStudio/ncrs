use super::writing::lines_of;
use super::*;
use crate::panel_columns::{Appearance, DateFormat, FontChoice, FontSizeChoice, SizeFormat};
use ncrs_core::PanelSide;

#[test]
fn without_a_section_the_panels_look_as_before() {
    let config = from_text("version = 1\n").unwrap();

    assert_eq!(config.columns, PanelColumns::default());
    assert!(config.columns.same_in_both);
    assert_eq!(config.columns.left, Column::DEFAULT);
    assert_eq!(config.columns.date_format, DateFormat::Iso);
    assert_eq!(config.columns.size_format, SizeFormat::Readable);
    assert_eq!(config.appearance, Appearance::default());
}

#[test]
fn an_unknown_column_is_dropped_and_the_name_is_always_there() {
    let text = "[columns]\nleft = [\"size\", \"bogus\", \"owner\"]\nright = []\n";
    let columns = from_text(text).unwrap().columns;

    assert_eq!(columns.left, [Column::Name, Column::Size, Column::Owner]);
    assert_eq!(columns.right, [Column::Name]);
}

#[test]
fn an_unknown_width_key_and_a_bad_width_are_dropped() {
    let text = "[columns.widths]\nname = 300\nbogus = 10\nsize = -5\nowner = 70000\n";
    let columns = from_text(text).unwrap().columns;

    assert_eq!(columns.widths, [(Column::Name, 300)]);
    assert_eq!(columns.width_of(Column::Name), Some(300));
    assert_eq!(columns.width_of(Column::Size), None);
}

#[test]
fn the_right_panel_has_its_own_columns_only_when_they_are_not_the_same() {
    let text = "[columns]\nleft = [\"name\"]\nright = [\"name\", \"size\"]\n";
    let same = from_text(text).unwrap().columns;
    let own = from_text(&format!("{text}same_in_both = false\n"))
        .unwrap()
        .columns;

    assert_eq!(same.of(PanelSide::Right), [Column::Name]);
    assert_eq!(own.of(PanelSide::Left), [Column::Name]);
    assert_eq!(own.of(PanelSide::Right), [Column::Name, Column::Size]);
}

#[test]
fn an_unknown_format_is_an_error() {
    assert!(from_text("[columns]\ndate_format = \"long\"\n").is_err());
    assert!(from_text("[columns]\nsize_format = \"huge\"\n").is_err());
}

#[test]
fn the_columns_are_written_and_read_back() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");
    std::fs::write(&path, "# mine\n[columns]\nsame_in_both = true # keep\n").unwrap();
    let columns = PanelColumns {
        same_in_both: false,
        left: vec![Column::Icon, Column::Name, Column::Permissions],
        right: vec![Column::Name, Column::Modified],
        widths: vec![(Column::Name, 280), (Column::Size, 90)],
        date_format: DateFormat::Relative,
        size_format: SizeFormat::Bytes,
    };

    let written = write_columns(&path, &columns).unwrap();

    assert_eq!(written.columns.same_in_both, columns.same_in_both);
    assert_eq!(written.columns.left, columns.left);
    assert_eq!(written.columns.right, columns.right);
    assert_eq!(written.columns.date_format, columns.date_format);
    assert_eq!(written.columns.size_format, columns.size_format);
    assert_eq!(written.columns.width_of(Column::Name), Some(280));
    assert_eq!(written.columns.width_of(Column::Size), Some(90));
    let text = lines_of(&path);
    assert!(text.starts_with("# mine\n"), "{text}");
    assert!(text.contains("same_in_both = false # keep"), "{text}");
    assert_eq!(load(&path).unwrap().columns, written.columns);
}

#[test]
fn writing_no_widths_drops_the_line() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");
    std::fs::write(&path, "[columns]\nwidths = { name = 300 }\n").unwrap();

    let written = write_columns(&path, &PanelColumns::default()).unwrap();

    assert!(written.columns.widths.is_empty());
    assert!(!lines_of(&path).contains("widths"));
}

#[test]
fn the_font_is_written_and_read_back_and_the_system_drops_it() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");
    let chosen = Appearance {
        font: FontChoice::Named("Menlo".to_owned()),
        font_size: FontSizeChoice::Points(14),
    };

    let written = write_appearance(&path, &chosen).unwrap();
    assert_eq!(written.appearance, chosen);
    assert_eq!(load(&path).unwrap().appearance, chosen);

    let back = write_appearance(&path, &Appearance::default()).unwrap();
    assert_eq!(back.appearance, Appearance::default());
    let text = lines_of(&path);
    assert!(!text.contains("font"), "{text}");
}

#[test]
fn a_font_size_outside_ten_to_twenty_is_the_systems() {
    for points in [9, 21, 0, -3] {
        let text = format!("[appearance]\nfont_size = {points}\n");
        let config = from_text(&text).unwrap();
        assert_eq!(
            config.appearance.font_size,
            FontSizeChoice::System,
            "{points}"
        );
    }
    let config = from_text("[appearance]\nfont_size = 16\n").unwrap();
    assert_eq!(config.appearance.font_size, FontSizeChoice::Points(16));
}
