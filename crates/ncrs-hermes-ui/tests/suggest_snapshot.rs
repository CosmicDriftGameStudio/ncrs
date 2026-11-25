//! The dialogs in which Hermes proposes rules in all their states, rendered
//! headless: a scheme with many proposals, the rules a target still lacks,
//! one rule for a file, nothing found and the model at work.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    clippy::indexing_slicing,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

// The CI runs `ui_tests` only on macOS, where the reference images were made.
mod ui_tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use iced::{Element, Size};
    use iced_test::Simulator;
    use ncrs_config::palette::Palette;
    use ncrs_filing::sorting::{ModelNote, RuleForFile, Suggested, SuggestError, Suggestions};
    use ncrs_hermes_ui::suggest::{Found, Suggest, SuggestFor};
    use ncrs_hermes_ui::ui::suggest::{dialog, TargetLabel};
    use ncrs_hermes_ui::Message;
    use ncrs_i18n::Language;
    use ncrs_intake::{LocationTransfer, OnNoText, OnSeveralTargets, SortSource};
    use ncrs_index::{
        Condition, ConditionKind, DocumentFields, KindCatalog, RuleConditions, RuleSpec, TargetId,
    };
    use ncrs_ui_kit::snapshot_support::ReferenceSnapshot as _;

    const WINDOW: Size = Size::new(1740.0, 1130.0);

    /// Name, condition, pattern, example and file count of the proposals of the
    /// board of the scheme dialog.
    const BOARD: [(&str, ConditionKind, &str, &str, &str, usize); 8] = [
        ("Schule", ConditionKind::Word, "zeugnis", "Schule/{recipient}/{year}/{year}{month}{day}_{kind}.pdf", "Schule/Anna/2026/20260703_Zeugnis.pdf", 34),
        ("Kita", ConditionKind::Sender, "Muster-Kita", "Kita/{year}/{year}{month}{day}_{kind}.pdf", "Kita/2025/20250901_Vertrag.pdf", 21),
        ("Arzt", ConditionKind::Kind, "Arztrechnung", "Gesundheit/{recipient}/{year}{month}{day}_{sender}.pdf", "Gesundheit/Anna/20260211_Praxis Beispiel.pdf", 48),
        ("Urlaub", ConditionKind::Word, "buchungsbestätigung", "Urlaub/{year}/{year}{month}{day}_{sender}.pdf", "Urlaub/2026/20260812_Reisebüro Muster.pdf", 26),
        ("Verein", ConditionKind::Sender, "e. V.", "Verein/{sender}/{year}{month}{day}_{kind}.pdf", "Verein/Turnverein Muster e. V./20260110_Beitrag.pdf", 19),
        ("Garantie", ConditionKind::Word, "garantie", "Garantie/{year}/{year}{month}{day}_{sender}.pdf", "Garantie/2026/20260503_Elektro Beispiel.pdf", 28),
        ("Urkunden", ConditionKind::Kind, "Urkunde", "Urkunden/{recipient}/{kind}_{year}.pdf", "Urkunden/Anna/Geburtsurkunde_2014.pdf", 11),
        ("Versicherung", ConditionKind::Word, "police", "Versicherung/{sender}/{year}/{year}{month}{day}_{kind}.pdf", "Versicherung/Beispiel AG/2026/20260115_Police.pdf", 14),
    ];

    fn scans() -> SortSource {
        SortSource {
            path: PathBuf::from("/home/anna/Scans"),
            include: String::new(),
            exclude: String::new(),
            targets: vec![TargetId::new("t1").unwrap()],
            watch: true,
            transfer: LocationTransfer::Move,
            no_text: OnNoText::Ask,
            several: OnSeveralTargets::Ask,
            own_volume: Some(false),
        }
    }

    fn spec(kind: ConditionKind, value: &str, pattern: &str) -> RuleSpec {
        RuleSpec {
            name: None,
            conditions: RuleConditions::all_of(vec![Condition::new(kind, value)]),
            extract_number: None,
            extract_date: None,
            extract_amount: None,
            kind: None,
            tags: BTreeSet::new(),
            filing_pattern: pattern.to_owned(),
            confirmed: false,
            enabled: true,
        }
    }

    fn proposals(count: usize) -> Vec<Suggested> {
        BOARD
            .iter()
            .take(count)
            .map(|(name, kind, value, pattern, example, files)| Suggested {
                name: (*name).to_owned(),
                spec: spec(*kind, value, pattern),
                example: Some((*example).to_owned()),
                files: *files,
                group_documents: u32::try_from(*files).unwrap(),
                elsewhere: 0,
            })
            .collect()
    }

    fn suggestions(rules_existing: usize, count: usize) -> Suggestions {
        Suggestions {
            files_read: 212,
            sources_read: 3,
            rules_existing,
            fit_suggested: 187,
            fit_none: 25,
            items: proposals(count),
            model: ModelNote::Used,
        }
    }

    fn label() -> TargetLabel {
        TargetLabel {
            text: "T3 Familie".to_owned(),
            path: "~/Familie".to_owned(),
        }
    }

    fn done(found: Found, subject: SuggestFor) -> Suggest {
        let mut state = Suggest::new(subject, 0);
        state.loaded(found);
        state
    }

    fn scheme_for(target: &str, found: Suggestions) -> Suggest {
        done(
            Found::Target(found),
            SuggestFor::Target(TargetId::new(target).unwrap()),
        )
    }

    fn file_found() -> Suggest {
        let rule = Suggested {
            name: "Stadtwerke".to_owned(),
            spec: spec(
                ConditionKind::Sender,
                "Muster Stadtwerke",
                "Wohnung/Energie/{year}/{year}{month}{day}_{sender}.pdf",
            ),
            example: Some("Wohnung/Energie/2026/20260214_Muster Stadtwerke.pdf".to_owned()),
            files: 4,
            group_documents: 1,
            elsewhere: 0,
        };
        let found = RuleForFile {
            target: TargetId::new("t1").unwrap(),
            rule,
            fields: DocumentFields {
                sender: Some("Muster Stadtwerke".to_owned()),
                kind: Some("Abrechnung".to_owned()),
                date: chrono::NaiveDate::from_ymd_opt(2026, 2, 14),
                ..DocumentFields::default()
            },
            also: ["scan-014.pdf", "scan-025.pdf", "scan-033.pdf"]
                .map(PathBuf::from)
                .to_vec(),
            model: ModelNote::Used,
        };
        done(
            Found::File(Box::new(found)),
            SuggestFor::File {
                source: scans(),
                path: PathBuf::from("/home/anna/Scans/scan-006.pdf"),
            },
        )
    }

    fn picture(name: &str, state: &Suggest, target: &TargetLabel, lang: Language) {
        let palette = Palette::default();
        let view: Element<'_, Message> = dialog(state, Some(target), &KindCatalog::default(), WINDOW, lang, &palette);
        let matches = Simulator::with_size(
            iced::Settings {
                default_font: ncrs_ui_kit::APP_FONT,
                ..iced::Settings::default()
            },
            WINDOW,
            view,
        )
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image(format!("tests/snapshots/{name}.png"))
        .unwrap();
        assert!(matches, "{name} differs from its image");
    }

    #[test]
    fn the_scheme_with_eight_proposals() {
        let state = scheme_for("t3", suggestions(0, 8));
        picture("suggest-scheme", &state, &label(), Language::German);
    }

    #[test]
    fn the_scheme_in_english() {
        let state = scheme_for("t3", suggestions(0, 8));
        picture("suggest-scheme-en", &state, &label(), Language::English);
    }

    #[test]
    fn a_target_with_rules_lists_what_is_missing() {
        let mut state = scheme_for("t1", suggestions(8, 2));
        state.toggle(1);
        picture("suggest-rules", &state, &label(), Language::German);
    }

    #[test]
    fn the_rule_for_one_file() {
        let target = TargetLabel {
            text: "T1 Dokumente".to_owned(),
            path: "~/Dokumente".to_owned(),
        };
        picture("suggest-file", &file_found(), &target, Language::German);
    }

    #[test]
    fn nothing_found() {
        let state = scheme_for("t3", suggestions(0, 0));
        picture("suggest-empty", &state, &label(), Language::German);
    }

    #[test]
    fn no_rule_fits_the_file() {
        let mut state = Suggest::new(
            SuggestFor::File {
                source: scans(),
                path: PathBuf::from("/home/anna/Scans/scan-006.pdf"),
            },
            0,
        );
        state.failed(SuggestError::NoPattern);
        picture("suggest-file-none", &state, &label(), Language::German);
    }

    #[test]
    fn the_model_is_working() {
        let state = Suggest::new(SuggestFor::Target(TargetId::new("t3").unwrap()), 0);
        picture("suggest-running", &state, &label(), Language::German);
    }
}
