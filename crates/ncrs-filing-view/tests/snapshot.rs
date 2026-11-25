//! The filing tab in all its states, rendered headless.

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
    use std::path::{Path, PathBuf};
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    use iced::Size;
    use iced_test::Simulator;
    use ncrs_filing::sorting::{ReadProblem, RowState, Taken, WaitReason};
    use ncrs_filing_view::question::Question;
    use ncrs_filing_view::run::RunProgress;
    use ncrs_filing_view::{view, Change, Message, Outcome, Row, Session, State, ViewCtx};
    use ncrs_i18n::Language;
    use ncrs_index::{
        Condition, ConditionKind, ContentHash, DocumentFields, DocumentId, Field, Index,
        RuleConditions, RuleId, RuleOrigin, RuleSpec, TargetId,
    };
    use ncrs_intake::{
        LocationTransfer, OnNoText, OnSeveralTargets, Place, SortSource, SortTarget,
    };
    use ncrs_ui_kit::palette::ThemePreset;
    use ncrs_ui_kit::snapshot_support::ReferenceSnapshot as _;

    const WINDOW: Size = Size::new(1740.0, 1130.0);
    const FILES: usize = 64;
    const RULES: [&str; 5] = [
        "Bank",
        "Versicherung",
        "Eingangsrechnungen",
        "Gehalt",
        "Bescheide",
    ];

    fn id(name: &str) -> TargetId {
        TargetId::new(name).unwrap()
    }

    /// Only an index gives out the numbers of rules.
    fn rules() -> Vec<(RuleId, String)> {
        let dir = tempfile::tempdir().unwrap();
        let mut index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        RULES
            .iter()
            .map(|name| {
                let spec = RuleSpec {
                    name: Some((*name).to_owned()),
                    conditions: RuleConditions::all_of(vec![Condition::new(
                        ConditionKind::Substring,
                        name.to_lowercase(),
                    )]),
                    extract_number: None,
                    extract_date: None,
                    extract_amount: None,
                    kind: None,
                    tags: BTreeSet::new(),
                    filing_pattern: (*name).to_owned(),
                    confirmed: true,
                    enabled: true,
                };
                let rule = index
                    .create_rule_in(&spec, RuleOrigin::default(), &id("documents"))
                    .unwrap();
                (rule, (*name).to_owned())
            })
            .collect()
    }

    fn target(name: &str, shown: &str, path: &str) -> SortTarget {
        SortTarget {
            id: id(name),
            name: shown.to_owned(),
            path: PathBuf::from(path),
            own_volume: Some(false),
        }
    }

    fn source(once: bool) -> SortSource {
        SortSource {
            path: PathBuf::from("/home/anna/Scans"),
            include: "**/*.pdf".to_owned(),
            exclude: String::new(),
            targets: vec![id("documents"), id("company")],
            watch: !once,
            transfer: LocationTransfer::Copy,
            no_text: OnNoText::Ask,
            several: OnSeveralTargets::Ask,
            own_volume: None,
        }
    }

    fn place(rule: RuleId, target: &str, relative: String) -> Place {
        Place {
            target: id(target),
            rule,
            relative: PathBuf::from(relative),
            fields: DocumentFields::default(),
        }
    }

    fn by_rule(index: usize, rules: &[(RuleId, String)]) -> Place {
        let month = index % 12 + 1;
        match index % 4 {
            0 => place(
                rules[0].0,
                "documents",
                format!("Bank/Beispielbank/2026/2026{month:02}28_Kontoauszug_{index:03}.pdf"),
            ),
            1 => place(
                rules[1].0,
                "documents",
                format!("Versicherung/Muster AG/2026{month:02}10_Police_{index:03}.pdf"),
            ),
            2 => place(
                rules[2].0,
                "company",
                format!("Eingang/2026/{month:02}/2026{month:02}03_Company A_{index:03}.pdf"),
            ),
            _ => place(
                rules[3].0,
                "documents",
                format!("Gehalt/Company A/2026/2026{month:02}_{index:03}.pdf"),
            ),
        }
    }

    fn state_of(index: usize, rules: &[(RuleId, String)]) -> RowState {
        match index {
            3 | 12 | 21 => RowState::AlreadyThere {
                target: id("documents"),
                path: PathBuf::from(format!(
                    "/home/anna/Dokumente/Gehalt/Company A/2026/alt-{index:03}.pdf"
                )),
            },
            6 | 14 | 25 | 33 => RowState::NoRule,
            9 | 29 => RowState::NoText,
            17 => RowState::Several(vec![
                by_rule(16, rules),
                place(
                    rules[4].0,
                    "company",
                    "Behörden/Beispielbank/2026/20260928.pdf".to_owned(),
                ),
            ]),
            _ => RowState::Place(by_rule(index, rules)),
        }
    }

    fn row(index: usize, state: RowState) -> Row {
        let relative = PathBuf::from(format!("scan-{index:03}.pdf"));
        Row {
            path: Path::new("/home/anna/Scans").join(&relative),
            relative,
            hash: Some(ContentHash([1; 32])),
            document: i64::try_from(index).ok().map(DocumentId::from_number),
            state,
        }
    }

    fn session(once: bool) -> Session {
        let mut session = Session::new(source(once), once);
        let rules = rules();
        session.targets = vec![
            target("documents", "Dokumente", "/home/anna/Dokumente"),
            target("company", "Firma", "/home/anna/Firma/Belege"),
            target("archive", "Ablage", "/home/anna/Ablage"),
        ];
        session.rows = (0..FILES)
            .map(|index| row(index, state_of(index, &rules)))
            .collect();
        session.rule_names = rules.into_iter().collect();
        session.planned = true;
        session.trash_can_restore = true;
        session.selected = Some(PathBuf::from("scan-006.pdf"));
        session.existing.insert(
            (id("documents"), PathBuf::from("Versicherung/Muster AG")),
            (0..5)
                .map(|number| format!("alt-{number:03}.pdf"))
                .collect(),
        );
        session
    }

    fn set(session: &mut Session, index: usize, state: RowState) {
        session.rows[index].state = state;
    }

    fn picture_in(session: Session, lang: Language, image: &str) {
        let state = State {
            session: Some(session),
        };
        let palette = ThemePreset::Cosmic.palette();
        let ctx = ViewCtx {
            lang,
            palette: &palette,
            window: WINDOW,
            home: Path::new("/home/anna"),
        };
        let mut simulator = Simulator::with_size(
            iced::Settings {
                default_font: ncrs_ui_kit::APP_FONT,
                ..iced::Settings::default()
            },
            WINDOW,
            view(&state, ctx).map(|message: Message| message),
        );
        let matches = simulator
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_reference_image(image)
            .unwrap();
        assert!(matches, "the filing picture differs from {image}");
    }

    fn picture(session: Session, image: &str) {
        picture_in(session, Language::German, image);
    }

    #[test]
    fn filing_plan_picture() {
        let mut session = session(true);
        session.marked_link = Some(1);
        picture(session, "tests/snapshots/filing-plan-cosmic");
    }

    #[test]
    fn filing_plan_english_picture() {
        picture_in(
            session(true),
            Language::English,
            "tests/snapshots/filing-plan-english-cosmic",
        );
    }

    #[test]
    fn filing_plan_still_reading_picture() {
        let mut session = session(true);
        for index in 34..FILES {
            set(&mut session, index, RowState::NotRead(ReadProblem::Unknown));
        }
        session.planning = true;
        session.reading = true;
        session.progress.total.store(FILES, Ordering::Relaxed);
        session.progress.done.store(34, Ordering::Relaxed);
        picture(session, "tests/snapshots/filing-plan-reading-cosmic");
    }

    /// The rarer states, and what the user decided.
    #[test]
    fn filing_plan_problems_picture() {
        let mut session = session(true);
        let rules: Vec<(RuleId, String)> = rules();
        let place = by_rule(1, &rules);
        let rule = rules[0].0;
        set(
            &mut session,
            0,
            RowState::NameTaken {
                place: place.clone(),
                by: Taken::OnDisk,
            },
        );
        set(
            &mut session,
            1,
            RowState::MissingValue {
                target: id("documents"),
                rule,
                field: Field::Date,
            },
        );
        set(
            &mut session,
            2,
            RowState::MissingValue {
                target: id("documents"),
                rule,
                field: Field::Number,
            },
        );
        set(
            &mut session,
            4,
            RowState::BadName {
                target: id("documents"),
                rule,
            },
        );
        for (index, problem) in [
            (5, ReadProblem::StillWriting),
            (7, ReadProblem::Unsupported),
            (8, ReadProblem::LibraryMissing),
        ] {
            set(&mut session, index, RowState::NotRead(problem));
        }
        set(&mut session, 10, RowState::SetAside);
        session
            .changes
            .insert(PathBuf::from("scan-011.pdf"), Change::Stay);
        session.changes.insert(
            PathBuf::from("scan-013.pdf"),
            Change::Places(vec![(id("company"), PathBuf::from("Eingang/Heizung.pdf"))]),
        );
        session.selected = Some(PathBuf::from("scan-015.pdf"));
        session.renaming = Some("202604_Gehalt.pdf".to_owned());
        picture(session, "tests/snapshots/filing-plan-problems-cosmic");
    }

    #[test]
    fn filing_question_two_targets_picture() {
        let mut session = session(true);
        ask(&mut session);
        picture(session, "tests/snapshots/filing-question-cosmic");
    }

    #[test]
    fn filing_question_three_targets_picture() {
        let mut session = session(true);
        session.source.targets.push(id("archive"));
        let rules = rules();
        if let RowState::Several(places) = &mut session.rows[17].state {
            places.push(place(
                rules[4].0,
                "archive",
                "2026/Beispielbank.pdf".to_owned(),
            ));
        }
        ask(&mut session);
        if let Some(question) = session.question.as_mut() {
            question.toggle(1);
            question.move_mark(true);
        }
        picture(session, "tests/snapshots/filing-question-list-cosmic");
    }

    fn ask(session: &mut Session) {
        let asked = session.rows[17].clone();
        let RowState::Several(places) = &asked.state else {
            panic!("expected a file for several targets, got {asked:?}");
        };
        session.selected = Some(asked.relative.clone());
        let question = Question::new(session, asked.relative.clone(), places, Language::German);
        session.question = Some(question);
    }

    #[test]
    fn filing_running_picture() {
        let mut session = session(true);
        session.running = Some(Arc::new(RunProgress::new(54)));
        picture(session, "tests/snapshots/filing-running-cosmic");
    }

    #[test]
    fn filing_done_picture() {
        let mut session = session(true);
        session.source.transfer = LocationTransfer::Move;
        session
            .rows
            .retain(|row| !matches!(row.state, RowState::Place(_)));
        session.outcome = Some(Outcome {
            filed: 54,
            left: 8,
            trashed: 2,
            problems: (40..47)
                .map(|index| format!("scan-{index:03}.pdf: Der Name ist am Ziel schon vergeben."))
                .collect(),
        });
        picture(session, "tests/snapshots/filing-done-cosmic");
    }

    #[test]
    fn filing_waiting_target_picture() {
        let mut session = session(true);
        session.waiting = vec![(id("documents"), WaitReason::NotMounted)];
        picture(session, "tests/snapshots/filing-waiting-cosmic");
    }

    #[test]
    fn filing_open_questions_picture() {
        let mut session = session(false);
        session.rows.retain(|row| {
            matches!(
                row.state,
                RowState::NoRule | RowState::NoText | RowState::Several(_)
            )
        });
        session.selected = Some(PathBuf::from("scan-017.pdf"));
        picture(session, "tests/snapshots/filing-questions-cosmic");
    }
}
