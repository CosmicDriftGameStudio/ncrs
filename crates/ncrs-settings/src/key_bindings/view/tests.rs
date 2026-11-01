use super::*;

#[test]
fn a_theme_name_with_room_shows_whole() {
    assert_eq!(
        fitted_theme_name("Copy", "Teleporter", None, 400.0).as_deref(),
        Some("Teleporter")
    );
}

#[test]
fn a_theme_name_without_room_is_cut_with_an_ellipsis() {
    let label = "Unmark all";
    let theme_name = "Remove every stamp from the crates";
    let width = format::estimated_width(label, font_size::ROW)
        + spacing::S
        + format::estimated_width("Remove every", font_size::ROW);
    let fitted = fitted_theme_name(label, theme_name, None, width).unwrap();
    assert!(fitted.ends_with('…'), "{fitted}");
    assert!(theme_name.starts_with(fitted.trim_end_matches('…')));
}

#[test]
fn a_theme_name_that_would_show_only_a_stub_is_left_out() {
    let label = "Mark the row or remove its mark";
    let width = format::estimated_width(label, font_size::ROW) + 20.0;
    assert_eq!(fitted_theme_name(label, "Stamp", None, width), None);
}

#[test]
fn the_changed_mark_takes_its_room_before_the_theme_name() {
    let label = "Copy";
    let width = format::estimated_width(label, font_size::ROW)
        + spacing::S
        + format::estimated_width("Teleporter", font_size::ROW)
        + 1.0;
    assert_eq!(
        fitted_theme_name(label, "Teleporter", None, width).as_deref(),
        Some("Teleporter")
    );
    assert_ne!(
        fitted_theme_name(label, "Teleporter", Some("changed"), width).as_deref(),
        Some("Teleporter")
    );
}
