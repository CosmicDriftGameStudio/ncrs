use super::*;
use crate::ui_tests::simulator;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn matches_snapshot(app: &App, name: &str) -> TestResult {
    let matches = simulator(app)
        .snapshot(&iced::Theme::Dark)?
        .matches_reference_image(format!("tests/snapshots/{name}.png"))?;
    if matches {
        Ok(())
    } else {
        Err(format!("the view does not match tests/snapshots/{name}.png").into())
    }
}

fn both_languages(build: fn() -> App, name: &str) -> TestResult {
    matches_snapshot(
        &build().in_language(Language::English),
        &format!("{name}_en"),
    )?;
    matches_snapshot(
        &build().in_language(Language::German),
        &format!("{name}_de"),
    )
}

#[test]
fn the_search_with_hit_buttons_matches_its_snapshots() -> TestResult {
    both_languages(App::with_fixed_search_hits, "search_hit_buttons")
}

#[test]
fn the_hit_list_matches_its_snapshots() -> TestResult {
    both_languages(App::with_fixed_hit_list, "hit_list")
}

#[test]
fn the_hit_list_context_menu_matches_its_snapshots() -> TestResult {
    both_languages(App::with_fixed_hit_list_menu, "hit_list_context_menu")
}

#[test]
fn the_search_context_menu_matches_its_snapshots() -> TestResult {
    both_languages(App::with_fixed_search_hits_menu, "search_context_menu")
}

#[test]
fn the_pack_prompt_of_a_hit_list_matches_its_snapshots() -> TestResult {
    both_languages(App::with_fixed_hit_list_pack, "hit_list_pack")
}
