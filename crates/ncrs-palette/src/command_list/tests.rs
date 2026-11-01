use super::*;
use ncrs_config::keymap::binding_label;

fn palette(scope: KeyScope, lang: Language) -> CommandPalette {
    CommandPalette::new(scope, lang, &Keymap::default(), &[])
}

fn asked(lang: Language, query: &str) -> CommandPalette {
    let mut palette = palette(KeyScope::Panel, lang);
    palette.set_query(query.to_owned());
    palette
}

fn labels(palette: &CommandPalette) -> Vec<&str> {
    palette
        .hits()
        .map(|hit| palette.entry(hit).unwrap().label.as_str())
        .collect()
}

fn found(lang: Language, query: &str) -> Vec<KeyAction> {
    let palette = asked(lang, query);
    palette
        .hits()
        .map(|hit| palette.entry(hit).unwrap().command().unwrap())
        .collect()
}

fn actions_of(palette: &CommandPalette) -> Vec<KeyAction> {
    palette
        .hits()
        .map(|hit| palette.entry(hit).unwrap().command().unwrap())
        .collect()
}

fn headers(palette: &CommandPalette) -> Vec<&'static str> {
    palette
        .rows()
        .iter()
        .filter_map(|row| match row {
            Row::Header(header) => Some(header.label),
            Row::Hit(_) => None,
        })
        .collect()
}

#[test]
fn se_lists_the_search_first_and_not_sort_by_size() {
    let palette = asked(Language::English, "se");
    let labels = labels(&palette);
    assert!(labels[..2].contains(&"Search in files"), "{labels:?}");
    assert!(!labels.contains(&"Sort by size"), "{labels:?}");
    assert!(!labels.contains(&"Switch panel"), "{labels:?}");
}

#[test]
fn a_theme_name_finds_the_command() {
    assert_eq!(
        found(Language::EnglishFuturama, "smell"),
        [KeyAction::Search]
    );
    assert!(found(Language::EnglishFuturama, "dark matter").contains(&KeyAction::QuickOpen));
}

#[test]
fn the_standard_language_has_no_theme_name_to_find_or_show() {
    assert!(found(Language::English, "smell").is_empty());
    assert!(found(Language::German, "dark matter").is_empty());
    for lang in [Language::English, Language::German] {
        let palette = palette(KeyScope::Panel, lang);
        for row in palette.rows() {
            match row {
                Row::Header(header) => assert_eq!(header.theme_name, None),
                Row::Hit(hit) => assert_eq!(palette.entry(hit).unwrap().theme_name, None),
            }
        }
    }
}

#[test]
fn the_theme_name_is_marked_where_it_matched() {
    let palette = asked(Language::EnglishFuturama, "smell");
    let hit = palette.hits().next().unwrap();
    let theme_name = palette.entry(hit).unwrap().theme_name.clone().unwrap();
    let marked: Vec<&str> = hit
        .theme_highlight
        .iter()
        .map(|range| &theme_name[range.clone()])
        .collect();
    assert_eq!(marked, ["Smell"]);
    assert!(hit.label_highlight.is_empty());
}

#[test]
fn the_config_name_finds_the_dark_matter_drive() {
    assert!(found(Language::English, "quick open").contains(&KeyAction::QuickOpen));
    assert!(found(Language::English, "quick op").contains(&KeyAction::QuickOpen));
}

#[test]
fn a_hit_through_the_config_name_marks_nothing() {
    let palette = asked(Language::English, "quick open");
    let hit = palette
        .hits()
        .find(|hit| palette.entry(hit).unwrap().command().unwrap() == KeyAction::QuickOpen)
        .unwrap();
    assert!(hit.label_highlight.is_empty());
    assert!(hit.theme_highlight.is_empty());
}

#[test]
fn a_word_only_matches_at_its_start() {
    let actions = found(Language::English, "ize");
    assert!(actions.is_empty(), "{actions:?}");
    assert_eq!(
        found(Language::EnglishFuturama, "insp"),
        [KeyAction::FilingStatus, KeyAction::HermesInspect]
    );
}

#[test]
fn german_labels_match_in_german() {
    let palette = asked(Language::German, "such");
    assert!(labels(&palette).contains(&"In Dateien suchen"));
    assert_eq!(
        labels(&asked(Language::German, "zu datei")),
        ["Zu Datei springen"]
    );
}

#[test]
fn losch_finds_loeschen_without_breaking_the_marks() {
    for typed in ["losch", "lösch"] {
        let palette = asked(Language::German, typed);
        let labels = labels(&palette);
        assert!(labels.contains(&"Löschen"), "{typed}: {labels:?}");
        for hit in palette.hits() {
            let entry = palette.entry(hit).unwrap();
            for range in &hit.label_highlight {
                assert!(entry.label.get(range.clone()).is_some(), "{range:?}");
            }
        }
        let delete = palette
            .hits()
            .find(|hit| palette.entry(hit).unwrap().label == "Löschen")
            .unwrap();
        assert_eq!(delete.label_highlight, vec![0.."Lösch".len()]);
    }
}

#[test]
fn the_tiers_are_the_whole_label_then_words_in_order_then_another_order() {
    let palette = palette(KeyScope::Panel, Language::English);
    let entry = palette
        .hits()
        .map(|hit| palette.entry(hit).unwrap())
        .find(|entry| entry.command().unwrap() == KeyAction::Move)
        .unwrap();
    let tier_of = |query: &str| match_words(entry, &query_words(query), query).map(|m| m.tier);
    assert_eq!(tier_of("move to"), Some(Tier::Start));
    assert_eq!(tier_of("to other"), Some(Tier::InOrder));
    assert_eq!(tier_of("other to"), Some(Tier::AnyOrder));
    assert_eq!(tier_of("move"), Some(Tier::Start));
    assert_eq!(tier_of("quick open"), None);
    assert!(Tier::Start < Tier::InOrder);
    assert!(Tier::InOrder < Tier::AnyOrder);
    assert!(Tier::AnyOrder < Tier::ConfigName);
}

#[test]
fn a_better_tier_is_listed_before_a_worse_one() {
    // "Fetch color theme…" has a word that starts with "co" but does not
    // start with it, so it comes after the labels of its group that do.
    // Across groups the order is by group, so only its group counts.
    let asked_co = asked(Language::English, "co");
    let entries: Vec<_> = asked_co
        .hits()
        .map(|hit| asked_co.entry(hit).unwrap())
        .collect();
    let color_group = entries
        .iter()
        .find(|entry| entry.label.contains("color"))
        .unwrap()
        .group;
    let labels: Vec<&str> = entries
        .iter()
        .filter(|entry| entry.group == color_group)
        .map(|entry| entry.label.as_str())
        .collect();
    let color = labels
        .iter()
        .position(|label| label.contains("color"))
        .unwrap();
    let last_start = labels
        .iter()
        .rposition(|label| label.to_lowercase().starts_with("co"))
        .unwrap();
    assert!(last_start < color, "{labels:?}");
}

#[test]
fn hits_are_grouped_under_a_header() {
    let palette = asked(Language::English, "s");
    let mut group_of_hit = None;
    let mut headers_seen = 0;
    for row in palette.rows() {
        match row {
            Row::Header(_) => {
                headers_seen += 1;
                group_of_hit = None;
            }
            Row::Hit(hit) => {
                let group = palette.entry(hit).unwrap().group;
                assert!(
                    group_of_hit.is_none_or(|before| before == group),
                    "a hit of another group under the same header"
                );
                group_of_hit = Some(group);
            }
        }
    }
    assert!(headers_seen > 1);
    assert!(matches!(palette.rows().first(), Some(Row::Header(_))));
}

#[test]
fn no_group_header_repeats() {
    for (lang, query) in [
        (Language::German, "st"),
        (Language::English, "st"),
        (Language::English, "s"),
    ] {
        let palette = asked(lang, query);
        let seen = headers(&palette);
        for (index, label) in seen.iter().enumerate() {
            assert!(!seen[..index].contains(label), "{query}: {seen:?}");
        }
    }
}

#[test]
fn the_group_with_the_best_hit_comes_first_then_the_group_order() {
    for lang in [Language::English, Language::German] {
        for query in ["s", "st", "pa", "in", "e"] {
            let palette = asked(lang, query);
            let words = query_words(query);
            // The group of each hit with the best tier any hit of it has.
            let mut listed: Vec<(Tier, PaletteGroup)> = Vec::new();
            for hit in palette.hits() {
                let entry = palette.entry(hit).unwrap();
                let tier = match_words(entry, &words, query).unwrap().tier;
                match listed.iter_mut().find(|(_, group)| *group == entry.group) {
                    Some(known) => known.0 = known.0.min(tier),
                    None => listed.push((tier, entry.group)),
                }
            }
            let mut sorted = listed.clone();
            sorted.sort();
            assert_eq!(listed, sorted, "{lang:?} {query:?}");
        }
    }
}

#[test]
fn a_later_group_with_a_better_hit_is_listed_first() {
    // "in" is a word of "Search in files" (Search group) and starts the
    // theme names "Inspection" and "Inventory" (Hermes group, after Search
    // in the fixed order).
    let palette = asked(Language::EnglishFuturama, "in");
    let position_of = |group| {
        palette
            .hits()
            .position(|hit| palette.entry(hit).unwrap().group == group)
    };
    let hermes = position_of(PaletteGroup::Command(CommandGroup::Hermes));
    let search = position_of(PaletteGroup::Command(CommandGroup::Search));
    assert!(hermes.is_some());
    assert!(hermes < search, "{hermes:?} {search:?}");
    assert!(palette
        .hits()
        .any(|hit| palette.entry(hit).unwrap().command().unwrap() == KeyAction::Search));
}

#[test]
fn the_cursor_and_select_skip_headers() {
    let mut palette = asked(Language::English, "s");
    assert!(matches!(
        palette.rows().get(palette.cursor()),
        Some(Row::Hit(_))
    ));
    palette.select(0);
    assert!(matches!(
        palette.rows().get(palette.cursor()),
        Some(Row::Hit(_))
    ));
    for _ in 0..palette.hits().count() {
        palette.move_cursor(1);
        assert!(matches!(
            palette.rows().get(palette.cursor()),
            Some(Row::Hit(_))
        ));
    }
    assert_eq!(palette.cursor(), 1, "a full round ends on the first hit");
    assert_eq!(palette.visible().start, 0, "its header is in view");
}

#[test]
fn an_empty_query_lists_every_command_by_group_in_a_fixed_order() {
    let palette = palette(KeyScope::Panel, Language::EnglishFuturama);
    assert_eq!(
        headers(&palette),
        ["File", "Edit", "View", "Go", "Search", "Filing", "Help"]
    );
    let Some(Row::Header(first)) = palette.rows().first() else {
        panic!("a header comes first");
    };
    assert_eq!(first.label, "File");
    let Some(Row::Header(hermes)) = palette
        .rows()
        .iter()
        .find(|row| matches!(row, Row::Header(header) if header.label == "Filing"))
    else {
        panic!("the Hermes group");
    };
    assert_eq!(hermes.theme_name, Some("Hermes"));
    assert_eq!(hermes.beta_mark, Some("Beta"));
    let marked = palette
        .rows()
        .iter()
        .filter(|row| matches!(row, Row::Header(header) if header.beta_mark.is_some()))
        .count();
    assert_eq!(marked, 1, "only the Hermes group carries the mark");
    let mut again = super::tests::palette(KeyScope::Panel, Language::EnglishFuturama);
    again.set_query(String::new());
    assert_eq!(palette.rows(), again.rows());
}

#[test]
fn recent_commands_head_the_empty_list_most_recent_first() {
    let keymap = Keymap::default();
    let recent = [KeyAction::Copy, KeyAction::Mkdir, KeyAction::Search];
    let palette = CommandPalette::new(KeyScope::Panel, Language::English, &keymap, &recent);
    let Some(Row::Header(first)) = palette.rows().first() else {
        panic!("a header comes first");
    };
    assert_eq!(first.label, "Recent");
    let after_header: Vec<KeyAction> = palette
        .rows()
        .iter()
        .skip(1)
        .take(3)
        .map(|row| match row {
            Row::Hit(hit) => palette.entry(hit).unwrap().command().unwrap(),
            Row::Header(_) => panic!("three hits come first"),
        })
        .collect();
    assert_eq!(after_header, recent);
    assert_eq!(headers(&palette).get(1), Some(&"File"));
    let again = palette
        .hits()
        .filter(|hit| palette.entry(hit).unwrap().command().unwrap() == KeyAction::Copy)
        .count();
    assert_eq!(again, 2, "a recent command is also in its group");
    assert_eq!(
        palette.selected_target(),
        Some(Target::Command(KeyAction::Copy)),
        "the most recent one is highlighted"
    );
}

#[test]
fn no_more_than_five_recent_commands_are_listed() {
    let recent = [
        KeyAction::Copy,
        KeyAction::Move,
        KeyAction::Mkdir,
        KeyAction::Open,
        KeyAction::Edit,
        KeyAction::View,
        KeyAction::Quit,
    ];
    let palette = CommandPalette::new(
        KeyScope::Panel,
        Language::English,
        &Keymap::default(),
        &recent,
    );
    let before_groups = palette
        .rows()
        .iter()
        .skip(1)
        .take_while(|row| matches!(row, Row::Hit(_)))
        .count();
    assert_eq!(before_groups, RECENT_LIMIT);
}

#[test]
fn a_query_has_no_recent_header() {
    let mut palette = CommandPalette::new(
        KeyScope::Panel,
        Language::English,
        &Keymap::default(),
        &[KeyAction::Copy],
    );
    palette.set_query("copy".to_owned());
    assert!(!headers(&palette).contains(&"Recent"));
}

#[test]
fn the_lab_does_not_list_itself() {
    assert!(!actions_of(&palette(KeyScope::Panel, Language::English))
        .contains(&KeyAction::CommandPalette));
}

#[test]
fn the_search_lists_its_own_actions_and_not_the_panel_ones() {
    let search = palette(KeyScope::Search, Language::English);
    let actions = actions_of(&search);
    assert!(actions.contains(&KeyAction::SearchRegex));
    assert!(actions.contains(&KeyAction::Quit));
    assert!(!actions.contains(&KeyAction::Mkdir));
    let panel = palette(KeyScope::Panel, Language::English);
    assert!(!actions_of(&panel).contains(&KeyAction::SearchRegex));
}

fn shortcuts_match(keymap: &Keymap) {
    let mut seen: Vec<KeyAction> = Vec::new();
    for scope in [KeyScope::Panel, KeyScope::Search] {
        let palette = CommandPalette::new(scope, Language::English, keymap, &[]);
        let actions = actions_of(&palette);
        for (index, action) in actions.iter().enumerate() {
            assert!(!actions[..index].contains(action), "{action:?} twice");
        }
        for hit in palette.hits() {
            let entry = palette.entry(hit).unwrap();
            assert_eq!(
                entry.shortcut,
                keymap
                    .bindings_of(entry.command().unwrap())
                    .next()
                    .and_then(binding_label),
                "{}",
                entry.command().unwrap().name()
            );
            if !seen.contains(&entry.command().unwrap()) {
                seen.push(entry.command().unwrap());
            }
        }
    }
    for command in commands::all() {
        assert!(
            seen.contains(&command.action) || command.action == KeyAction::CommandPalette,
            "{} is in no list",
            command.action.name()
        );
    }
}

#[test]
fn every_command_is_listed_once_with_the_shortcut_of_the_keymap() {
    shortcuts_match(&Keymap::default());
}

#[test]
fn an_override_shows_in_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[keys]\nfiling_status = \"F11\"\ncopy = \"F12\"\n").unwrap();
    let keymap = (*ncrs_config::config::load(&path).unwrap().keymap).clone();
    shortcuts_match(&keymap);
    let palette = CommandPalette::new(KeyScope::Panel, Language::English, &keymap, &[]);
    let shortcut_of = |action| {
        palette
            .hits()
            .map(|hit| palette.entry(hit).unwrap())
            .find(|entry| entry.command().unwrap() == action)
            .unwrap()
            .shortcut
            .clone()
    };
    assert_eq!(shortcut_of(KeyAction::FilingStatus).as_deref(), Some("F11"));
    assert_eq!(shortcut_of(KeyAction::Copy).as_deref(), Some("F12"));
}

#[test]
fn a_new_query_puts_the_cursor_back_on_the_first_hit() {
    let mut palette = palette(KeyScope::Panel, Language::English);
    palette.move_cursor(5);
    assert_ne!(palette.cursor(), 1);
    palette.set_query("e".to_owned());
    assert!(matches!(
        palette.rows().get(palette.cursor()),
        Some(Row::Hit(_))
    ));
    assert_eq!(palette.cursor(), 1);
}

#[test]
fn the_cursor_stays_in_view_and_inside_the_list() {
    let mut palette = palette(KeyScope::Panel, Language::English);
    assert_eq!(palette.cursor(), 1);
    palette.move_cursor(-1);
    let last_hit = palette.rows().len() - 1;
    assert_eq!(palette.cursor(), last_hit, "up from the first goes round");
    let visible = palette.visible();
    assert!(visible.contains(&palette.cursor()), "{visible:?}");
    assert_eq!(visible.len(), MIN_VISIBLE_ROWS);
    palette.move_cursor(1);
    assert_eq!(palette.cursor(), 1, "down from the last goes round");
    assert_eq!(palette.visible().start, 0);
}

#[test]
fn the_cursor_goes_round_at_both_ends_and_survives_an_empty_list() {
    let mut palette = asked(Language::English, "s");
    let hits = palette.hits().count();
    for _ in 0..hits {
        palette.move_cursor(1);
    }
    assert_eq!(palette.cursor(), 1, "a full round ends where it began");
    let mut nothing = asked(Language::English, "zzzzzzzzqq");
    assert_eq!(nothing.hits().count(), 0);
    nothing.move_cursor(1);
    nothing.move_cursor(-1);
}

#[test]
fn selecting_a_header_or_past_the_end_changes_nothing() {
    let mut palette = palette(KeyScope::Panel, Language::English);
    palette.select(3);
    let at = palette.cursor();
    palette.select(10_000);
    assert_eq!(palette.cursor(), at);
    let header = palette
        .rows()
        .iter()
        .position(|row| matches!(row, Row::Header(_)))
        .unwrap();
    assert_eq!(header, 0);
    palette.select(header);
    assert_eq!(palette.cursor(), at);
}

#[test]
fn the_entry_carries_the_first_key_of_its_action() {
    let palette = palette(KeyScope::Panel, Language::English);
    let key_of = |action| {
        palette
            .hits()
            .map(|hit| palette.entry(hit).unwrap())
            .find(|entry| entry.command().unwrap() == action)
            .unwrap()
            .shortcut
            .clone()
    };
    assert_eq!(key_of(KeyAction::Copy).as_deref(), Some("F5"));
    assert_eq!(key_of(KeyAction::Open).as_deref(), Some("Enter"));
}

#[test]
fn the_hermes_file_commands_are_listed_only_while_the_column_is_up() {
    let listed = |palette: &CommandPalette| {
        palette.hits().any(|hit| {
            palette.entry(hit).unwrap().command().unwrap() == KeyAction::HermesProcessAgain
        })
    };
    let with = palette(KeyScope::Panel, Language::English);
    assert!(listed(&with));
    let without = with.without_hermes_file_commands();
    assert!(!listed(&without));
    assert!(without
        .hits()
        .any(|hit| without.entry(hit).unwrap().command().unwrap() == KeyAction::FilingStatus));
}

#[test]
fn without_locations_only_the_role_commands_stay_among_the_hermes_file_commands() {
    let palette = palette(KeyScope::Panel, Language::English).without_hermes_location_commands();
    let actions: Vec<KeyAction> = palette
        .hits()
        .map(|hit| palette.entry(hit).unwrap().command().unwrap())
        .collect();
    assert!(actions.contains(&KeyAction::HermesSetInbox));
    assert!(actions.contains(&KeyAction::HermesSetFilingLocation));
    assert!(actions.contains(&KeyAction::HermesSetArchive));
    assert!(!actions.contains(&KeyAction::HermesProcessAgain));
    assert!(!actions.contains(&KeyAction::HermesExclude));
}

#[test]
fn one_command_can_be_dropped_for_the_row_under_the_cursor() {
    let palette =
        palette(KeyScope::Panel, Language::English).without_command(KeyAction::HermesProcessAgain);
    let actions: Vec<KeyAction> = palette
        .hits()
        .map(|hit| palette.entry(hit).unwrap().command().unwrap())
        .collect();
    assert!(!actions.contains(&KeyAction::HermesProcessAgain));
    assert!(actions.contains(&KeyAction::HermesExclude));
}

fn setting_targets(lang: Language) -> Vec<SettingsTarget> {
    let (page, viewer) = match lang.base() {
        Language::English | Language::EnglishFuturama => ("Settings: Programs", "Viewer (F3)"),
        Language::German | Language::GermanFuturama => {
            ("Einstellungen: Programme", "Betrachter (F3)")
        }
    };
    vec![
        SettingsTarget {
            feature: SettingsFeature::Programs,
            row: None,
            label: page.to_owned(),
            theme_name: None,
        },
        SettingsTarget {
            feature: SettingsFeature::Programs,
            row: Some(viewer.to_owned()),
            label: viewer.to_owned(),
            theme_name: None,
        },
    ]
}

fn with_settings(lang: Language, query: &str) -> CommandPalette {
    let mut palette = palette(KeyScope::Panel, lang).with_settings(setting_targets(lang));
    palette.set_query(query.to_owned());
    palette
}

fn setting_hits(palette: &CommandPalette) -> Vec<(SettingsFeature, Option<String>)> {
    palette
        .hits()
        .filter_map(|hit| match &palette.entry(hit).unwrap().target {
            Target::Setting { feature, row } => Some((*feature, row.clone())),
            Target::Command(_) => None,
        })
        .collect()
}

#[test]
fn a_settings_page_is_found_by_its_name_in_both_languages() {
    for (lang, query) in [
        (Language::English, "settings prog"),
        (Language::German, "einstellungen prog"),
    ] {
        let palette = with_settings(lang, query);
        assert!(
            setting_hits(&palette).contains(&(SettingsFeature::Programs, None)),
            "{lang:?}"
        );
    }
}

#[test]
fn one_setting_is_found_by_its_label_in_both_languages() {
    for (lang, query, label) in [
        (Language::English, "viewer", "Viewer (F3)"),
        (Language::German, "betrachter", "Betrachter (F3)"),
    ] {
        let palette = with_settings(lang, query);
        assert_eq!(
            setting_hits(&palette),
            [(SettingsFeature::Programs, Some(label.to_owned()))],
            "{lang:?}"
        );
        assert_eq!(headers(&palette), [lang.text(Msg::ActionSettings)]);
    }
}

#[test]
fn a_command_comes_before_a_setting_of_the_same_tier() {
    let targets = vec![SettingsTarget {
        feature: SettingsFeature::Programs,
        row: Some("Search depth".to_owned()),
        label: "Search depth".to_owned(),
        theme_name: None,
    }];
    let mut palette = palette(KeyScope::Panel, Language::English).with_settings(targets);
    palette.set_query("search".to_owned());
    let first = palette.hits().next().unwrap();
    assert!(palette.entry(first).unwrap().command().is_some());
    let last = palette.hits().last().unwrap();
    assert_eq!(palette.entry(last).unwrap().group, PaletteGroup::Settings);
}

#[test]
fn settings_are_not_in_the_overview_and_recent_is_untouched() {
    let recent = [KeyAction::Copy];
    let with = CommandPalette::new(
        KeyScope::Panel,
        Language::English,
        &Keymap::default(),
        &recent,
    )
    .with_settings(setting_targets(Language::English));
    let without = CommandPalette::new(
        KeyScope::Panel,
        Language::English,
        &Keymap::default(),
        &recent,
    );
    assert_eq!(with.rows(), without.rows());
    assert!(setting_hits(&with).is_empty());
    assert_eq!(headers(&with)[0], "Recent");
}

#[test]
fn a_setting_is_never_found_through_a_config_name() {
    let mut palette = with_settings(Language::English, "viewer");
    palette.set_query("quick open".to_owned());
    assert!(setting_hits(&palette).is_empty());
}

#[test]
fn running_a_setting_gives_its_target() {
    let palette = with_settings(Language::English, "viewer");
    assert_eq!(
        palette.selected_target(),
        Some(Target::Setting {
            feature: SettingsFeature::Programs,
            row: Some("Viewer (F3)".to_owned()),
        })
    );
}
