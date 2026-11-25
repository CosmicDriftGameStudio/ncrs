//! The page Einsortieren and its source dialog in all their states, rendered
//! headless.

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
    use std::path::{Path, PathBuf};

    use std::collections::BTreeSet;

    use iced::widget::{container, row, scrollable, Space};
    use iced::{Element, Length, Size};
    use iced_test::Simulator;
    use ncrs_config::config::{Filing, SourceSetting, TargetSetting};
    use ncrs_config::file_list_tags::FileListTags;
    use ncrs_config::palette::{Palette, ThemePreset};
    use ncrs_core::facts::{RunDay, RunFacts};
    use ncrs_filing::rules_work::{ListedRule, RulesLoad};
    use ncrs_hermes_ui::rules_page::RulesPage;
    use ncrs_hermes_ui::sorting_page::{SortingPage, SourceState};
    use ncrs_hermes_ui::source_dialog::{FolderCheck, SourceDialog, SourceField};
    use ncrs_hermes_ui::{
        dialogs, page_content, Hermes, HermesPage, Message, SortingMessage, SourceDialogKind,
        ViewCtx,
    };
    use ncrs_i18n::Language;
    use ncrs_index::{
        Condition, ConditionKind, Index, Rule, RuleConditions, RuleOrigin, RuleSpec, TargetId,
    };
    use ncrs_intake::{LocationTransfer, OnNoText, OnSeveralTargets, SortSource, SortTarget};
    use ncrs_ui_kit::layout::{SETTINGS_NAV_WIDTH, SETTINGS_WIDE_PAGE_MAX_WIDTH};
    use ncrs_ui_kit::snapshot_support::ReferenceSnapshot as _;
    use ncrs_ui_kit::theme::spacing;

    const WINDOW: Size = Size::new(1740.0, 1130.0);
    const HOME: &str = "/home/anna";

    fn id(name: &str) -> TargetId {
        TargetId::new(name).unwrap()
    }

    fn home(below: &str) -> PathBuf {
        Path::new(HOME).join(below)
    }

    fn target(name: &str, shown: &str, path: PathBuf) -> TargetSetting {
        TargetSetting {
            target: SortTarget {
                id: id(name),
                name: shown.to_owned(),
                path,
                own_volume: Some(false),
            },
            paused: None,
        }
    }

    fn source(folder: &str, include: &str, targets: &[&str]) -> SourceSetting {
        SourceSetting {
            source: SortSource {
                path: home(folder),
                include: include.to_owned(),
                exclude: String::new(),
                targets: targets.iter().map(|name| id(name)).collect(),
                watch: true,
                transfer: LocationTransfer::Move,
                no_text: OnNoText::Ask,
                several: OnSeveralTargets::Ask,
                own_volume: Some(false),
            },
            paused: None,
        }
    }

    fn filing() -> Filing {
        let mut documents = source("Documents", "**/*.pdf", &["documents"]);
        documents.source.transfer = LocationTransfer::Copy;
        documents.source.no_text = OnNoText::Skip;
        Filing {
            enabled: true,
            targets: vec![
                target(
                    "documents",
                    "Dokumente",
                    PathBuf::from("/Volumes/Tresor/Dokumente"),
                ),
                target("firm", "Firma", home("Firma/Belege")),
                target("family", "Familie", home("Familie")),
            ],
            sources: vec![
                source("Downloads", "*.pdf", &["documents"]),
                source("Scans", "*.{pdf,jpg}", &["documents", "firm"]),
                documents,
            ],
            ..Filing::default()
        }
    }

    fn run(batch: i64, day: RunDay, undoable: bool) -> RunFacts {
        RunFacts {
            batch,
            day,
            time: "14:02".to_owned(),
            files: 12,
            from: "Downloads".to_owned(),
            to: "T1 Dokumente".to_owned(),
            examples: vec![
                "Bank/Beispielbank/2026".to_owned(),
                "Versicherung/Muster AG".to_owned(),
            ],
            undoable,
        }
    }

    /// The rules of the targets in [`filing`]; the ids are the ones an index
    /// hands out.
    fn rules() -> RulesPage {
        use ConditionKind::{Iban, Kind, Recipient, Sender, TaxId, Word};
        let all = |list: &[(ConditionKind, &str)]| {
            let list = list
                .iter()
                .map(|(kind, value)| Condition::new(*kind, *value));
            RuleConditions::all_of(list.collect())
        };
        let either = |list: &[(ConditionKind, &str)]| {
            let list = list
                .iter()
                .map(|(kind, value)| Condition::new(*kind, *value));
            RuleConditions::any_of(list.collect())
        };
        let firm = (Recipient, "Beispiel AG");
        let rows = [
            ("documents", "Bank", all(&[(Kind, "bank_statement")]), "Bank/{sender}/{year}/{year}{month}{day}_{kind}_{number}.pdf", 48),
            ("documents", "Versicherung", either(&[(Word, "Police"), (Word, "Versicherungsschein")]), "Versicherung/{sender}/{year}{month}{day}_{kind}.pdf", 17),
            ("documents", "Steuer", all(&[(TaxId, "12/345/67890")]), "Steuer/{year}/{year}{month}{day}_{kind}.pdf", 41),
            ("documents", "Telekom", all(&[(Sender, "Company A"), (Word, "Rechnung")]), "Telefon/{sender}/{year}/{year}{month}_{number}.pdf", 12),
            ("documents", "Strom und Gas", all(&[(Iban, "DE89 3704 0044 0532 0130 00")]), "Wohnung/Energie/{year}/{year}{month}{day}_{sender}.pdf", 9),
            ("documents", "Arzt", all(&[(Kind, "Arztrechnung")]), "Gesundheit/{recipient}/{year}/{year}{month}{day}_{sender}.pdf", 23),
            ("documents", "Gehalt", all(&[(Kind, "Gehaltsabrechnung")]), "Gehalt/{sender}/{year}/{year}{month}.pdf", 36),
            ("documents", "Auto", all(&[(Word, "Kennzeichen B-XY 123")]), "Auto/{kind}/{year}{month}{day}_{sender}.pdf", 7),
            ("firm", "Eingangsrechnungen", all(&[firm, (Kind, "invoice")]), "Eingang/{year}/{month}/{year}{month}{day}_{sender}_{number}.pdf", 64),
            ("firm", "Verträge", all(&[firm, (Kind, "contract")]), "Verträge/{sender}/{year}{month}{day}_{kind}.pdf", 5),
            ("firm", "Bescheide", all(&[firm, (Kind, "notice")]), "Behörden/{sender}/{year}/{year}{month}{day}.pdf", 8),
        ];
        let dir = tempfile::TempDir::new().unwrap();
        let mut index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        let rules = rows
            .into_iter()
            .enumerate()
            .map(|(at, (target, name, conditions, pattern, hits))| {
                let spec = RuleSpec {
                    name: Some(name.to_owned()),
                    conditions,
                    extract_number: None,
                    extract_date: None,
                    extract_amount: None,
                    kind: None,
                    tags: BTreeSet::new(),
                    filing_pattern: pattern.to_owned(),
                    confirmed: true,
                    enabled: true,
                };
                let target = id(target);
                ListedRule {
                    rule: Rule {
                        id: index
                            .create_rule_in(&spec, RuleOrigin::Hand, &target)
                            .unwrap(),
                        position: i64::try_from(at).unwrap() + 1,
                        origin: RuleOrigin::Hand,
                        target,
                        spec,
                    },
                    global_index: at,
                    hits,
                }
            })
            .collect();
        let mut page = RulesPage::new();
        page.loaded(RulesLoad { rules });
        page
    }

    fn page_of(filing: Filing, page: SortingPage) -> Hermes {
        Hermes {
            filing_config: filing,
            sorting: Some(page),
            ..Hermes::default()
        }
    }

    fn with_rules(hermes: Hermes) -> Hermes {
        Hermes {
            rules: Some(rules()),
            ..hermes
        }
    }

    fn filled_page() -> Hermes {
        let ready = |questions| SourceState::Ready { questions };
        with_rules(page_of(
            filing(),
            SortingPage {
                states: vec![ready(0), ready(4), ready(0)],
                runs: vec![run(1, RunDay::Today, true)],
                ..SortingPage::default()
            },
        ))
    }

    fn page_with_runs(all_runs: bool) -> Hermes {
        page_of(
            filing(),
            SortingPage {
                runs: vec![
                    run(4, RunDay::Today, false),
                    run(3, RunDay::Yesterday, true),
                    run(2, RunDay::On("2026-10-30".to_owned()), true),
                    run(1, RunDay::On("2026-10-28".to_owned()), true),
                ],
                all_runs,
                ..SortingPage::default()
            },
        )
    }

    fn page_with_state(state: SourceState) -> Hermes {
        let filing = filing();
        let states = vec![state; filing.sources.len()];
        page_of(
            filing,
            SortingPage {
                states,
                ..SortingPage::default()
            },
        )
    }

    fn dialog_of(kind: SourceDialogKind) -> SourceDialog {
        let mut dialog = SourceDialog::new(kind, Some(&home("Scans")), &filing());
        dialog.folder_check = FolderCheck::Found(home("Scans"));
        dialog
    }

    fn with_dialog(dialog: SourceDialog) -> Hermes {
        Hermes {
            filing_config: filing(),
            source_dialog: Some(dialog),
            ..Hermes::default()
        }
    }

    fn rendered<T>(
        hermes: &Hermes,
        lang: Language,
        look: impl FnOnce(Simulator<'_, Message, iced::Theme>) -> T,
    ) -> T {
        let palette: Palette = ThemePreset::Cosmic.palette();
        let tags = FileListTags::default();
        let ctx = ViewCtx {
            lang,
            palette: &palette,
            window_size: WINDOW,
            file_list_tags: &tags,
            jobs_busy: false,
            ai_download: None,
            system_reads_scans: true,
            home: Path::new(HOME),
        };
        let shown: Element<'_, Message> = match dialogs(hermes, &ctx).pop() {
            Some(dialog) => container(dialog).center(Length::Fill).into(),
            // As the settings tab frames it: beside the list of the features.
            None => row![
                Space::new().width(SETTINGS_NAV_WIDTH),
                scrollable(
                    container(page_content(
                        hermes,
                        HermesPage::Sorting,
                        &ctx,
                        None,
                        |message| message,
                        |_, pressed| pressed,
                    ))
                    .max_width(SETTINGS_WIDE_PAGE_MAX_WIDTH)
                    .padding(spacing::XXL),
                )
                .width(Length::Fill)
                .height(Length::Fill),
            ]
            .into(),
        };
        look(Simulator::with_size(
            iced::Settings {
                default_font: ncrs_ui_kit::APP_FONT,
                ..iced::Settings::default()
            },
            WINDOW,
            shown,
        ))
    }

    fn picture_in(hermes: &Hermes, lang: Language, name: &str) {
        let matches = rendered(hermes, lang, |mut simulator| {
            simulator
                .snapshot(&iced::Theme::Dark)
                .unwrap()
                .matches_reference_image(format!("tests/snapshots/{name}"))
                .unwrap()
        });
        assert!(matches, "the picture differs from {name}");
    }

    fn picture(hermes: &Hermes, name: &str) {
        picture_in(hermes, Language::German, name);
    }

    fn shows(hermes: &Hermes, text: &str) -> bool {
        rendered(hermes, Language::English, |mut simulator| {
            simulator.find(text).is_ok()
        })
    }

    fn clicked(hermes: &Hermes, text: &str) -> Vec<Message> {
        rendered(hermes, Language::English, |mut simulator| {
            simulator.click(text).expect("shown");
            simulator.into_messages().collect()
        })
    }

    #[test]
    fn sorting_page_picture() {
        picture(&filled_page(), "sorting-page");
    }

    #[test]
    fn sorting_page_english_picture() {
        picture_in(&filled_page(), Language::English, "sorting-page-en");
    }

    #[test]
    fn empty_sorting_page_picture() {
        let empty = page_of(Filing::default(), SortingPage::default());
        picture(&empty, "sorting-page-empty");
    }

    #[test]
    fn sorting_page_with_sources_that_wait_picture() {
        let waiting = page_of(
            filing(),
            SortingPage {
                states: vec![
                    SourceState::DriveMissing,
                    SourceState::Paused,
                    SourceState::Ready { questions: 4 },
                ],
                ..SortingPage::default()
            },
        );
        picture(&with_rules(waiting), "sorting-page-waiting");
    }

    #[test]
    fn source_dialog_picture() {
        let mut dialog = dialog_of(SourceDialogKind::Watched);
        dialog.exclude = "tmp*".to_owned();
        picture(&with_dialog(dialog), "source-dialog");
    }

    #[test]
    fn source_dialog_with_the_keyboard_on_a_target_picture() {
        let mut dialog = dialog_of(SourceDialogKind::Watched);
        dialog.focus = SourceField::Target(1);
        picture(&with_dialog(dialog), "source-dialog-target-focused");
    }

    #[test]
    fn once_dialog_picture() {
        let folder = Path::new("/Volumes/USB-Stick/Alt");
        let mut dialog = SourceDialog::new(SourceDialogKind::Once, Some(folder), &filing());
        dialog.folder_check = FolderCheck::Found(folder.to_owned());
        picture(&with_dialog(dialog), "source-dialog-once");
    }

    #[test]
    fn source_dialog_with_field_errors_picture() {
        let mut dialog = dialog_of(SourceDialogKind::Watched);
        dialog.folder_check = FolderCheck::Missing;
        dialog.include = "[".to_owned();
        dialog.targets.clear();
        picture(&with_dialog(dialog), "source-dialog-errors");
    }

    #[test]
    fn only_a_run_that_can_be_put_back_has_the_undo_button() {
        assert_eq!(
            clicked(&page_with_runs(false), "Undo"),
            vec![Message::Sorting(SortingMessage::Undo(3))]
        );
    }

    #[test]
    fn show_all_lists_the_runs_beyond_the_last_three() {
        let fourth = "2026-10-28 14:02 · 12 files from Downloads → T1 Dokumente";
        assert!(!shows(&page_with_runs(false), fourth));
        assert!(shows(&page_with_runs(true), fourth));
    }

    #[test]
    fn a_source_out_of_reach_offers_to_choose_its_folder_again() {
        let button = "Choose folder again…";
        assert!(!shows(
            &page_with_state(SourceState::Ready { questions: 0 }),
            button
        ));
        assert_eq!(
            clicked(&page_with_state(SourceState::Unreadable), button),
            vec![Message::Sorting(SortingMessage::RepickSource(0))]
        );
    }

    #[test]
    fn a_click_on_the_open_questions_opens_them() {
        assert_eq!(
            clicked(
                &page_with_state(SourceState::Ready { questions: 4 }),
                "4 questions open"
            ),
            vec![Message::Sorting(SortingMessage::OpenQuestions(0))]
        );
    }

    #[test]
    fn the_heading_of_the_sources_adds_one() {
        let empty = page_of(Filing::default(), SortingPage::default());
        for page in [empty, filled_page()] {
            assert_eq!(
                clicked(&page, "+ Source…"),
                vec![Message::Sorting(SortingMessage::AddSource)]
            );
        }
    }

    #[test]
    fn a_rule_shows_its_condition_and_its_pattern_in_columns_of_their_own() {
        let page = filled_page();
        for cell in [
            "Telekom",
            "Telefon/{sender}/{year}/{year}{month}_{number}.pdf",
        ] {
            assert!(shows(&page, cell), "{cell}");
        }
    }

    #[test]
    fn a_click_into_a_text_field_moves_the_keyboard_to_it() {
        let dialog = with_dialog(dialog_of(SourceDialogKind::Watched));
        let messages = rendered(&dialog, Language::English, |mut simulator| {
            simulator
                .click(iced::widget::Id::new(
                    ncrs_hermes_ui::ui::source_dialog::EXCLUDE_FIELD_ID,
                ))
                .expect("the field");
            simulator.into_messages().collect::<Vec<_>>()
        });
        assert_eq!(
            messages,
            vec![Message::Sorting(SortingMessage::Focus(SourceField::Exclude))]
        );
    }
}
