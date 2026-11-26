//! The group "Targets and their rules", the rule dialog with its trial run and
//! the dialog of a target, rendered headless.

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

    use iced::widget::container;
    use iced::{Element, Length, Size};
    use iced_test::Simulator;
    use ncrs_config::config::{Filing, SourceSetting, TargetSetting};
    use ncrs_config::file_list_tags::FileListTags;
    use ncrs_config::palette::ThemePreset;
    use ncrs_filing::rules_work::{ListedRule, RulesLoad, SortTry, SortTryRow, SortTryState};
    use ncrs_filing::sorting::{TargetState, WaitReason};
    use ncrs_hermes_ui::condition_editor::ConditionEdit;
    use ncrs_hermes_ui::message::RuleFormMessage;
    use ncrs_hermes_ui::rule_form::{RuleForm, TryStatus};
    use ncrs_hermes_ui::rules_page::RulesPage;
    use ncrs_hermes_ui::sorting_page::{SortingPage, SourceState};
    use ncrs_hermes_ui::target_dialog::TargetDialog;
    use ncrs_hermes_ui::{dialogs, page_content, Hermes, HermesPage, Message, ViewCtx};
    use ncrs_i18n::Language;
    use ncrs_index::{
        Condition, ConditionKind, Field, Index, Rule, RuleConditions, RuleId, RuleOrigin, RuleSpec,
        TargetId,
    };
    use ncrs_intake::{LocationTransfer, OnNoText, OnSeveralTargets, SortSource, SortTarget};
    use ncrs_ui_kit::layout::SETTINGS_WIDE_PAGE_MAX_WIDTH;
    use ncrs_ui_kit::snapshot_support::ReferenceSnapshot as _;

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

    fn source(folder: &str, targets: &[&str]) -> SourceSetting {
        SourceSetting {
            source: SortSource {
                path: home(folder),
                include: "*.pdf".to_owned(),
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
        Filing {
            enabled: true,
            targets: vec![
                target("documents", "Documents", home("Documents")),
                target("company", "Company", home("Company/Receipts")),
                target("family", "Family", home("Family")),
            ],
            sources: vec![
                source("Downloads", &["documents", "company"]),
                source("Scans", &["documents"]),
            ],
            ..Filing::default()
        }
    }

    /// The ids an index hands out, one per rule.
    fn rule_ids(count: usize) -> Vec<RuleId> {
        let dir = tempfile::TempDir::new().unwrap();
        let mut index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        let spec = rule_spec("probe", "probe");
        std::iter::repeat_with(|| index.create_rule(&spec).unwrap())
            .take(count)
            .collect()
    }

    fn rule_spec(name: &str, word: &str) -> RuleSpec {
        RuleSpec {
            name: Some(name.to_owned()),
            conditions: RuleConditions::all_of(vec![Condition::new(ConditionKind::Word, word)]),
            extract_number: None,
            extract_date: None,
            extract_amount: None,
            kind: None,
            tags: BTreeSet::new(),
            filing_pattern: "{year}/{sender}".to_owned(),
            confirmed: true,
            enabled: true,
        }
    }

    /// Eight rules in the first target, three in the second, none in the third.
    fn listed_rules() -> Vec<ListedRule> {
        let layout: [(&str, usize); 2] = [("documents", 8), ("company", 3)];
        let ids = rule_ids(layout.iter().map(|(_, count)| count).sum());
        let mut unused_ids = ids.into_iter();
        let mut rules = Vec::new();
        for (target_name, count) in layout {
            for at in 0..count {
                let global_index = rules.len();
                rules.push(ListedRule {
                    rule: Rule {
                        id: unused_ids.next().unwrap(),
                        position: i64::try_from(global_index).unwrap() + 1,
                        origin: RuleOrigin::Hand,
                        target: id(target_name),
                        spec: rule_spec(&format!("Rule {}", at + 1), &format!("word{at}")),
                    },
                    global_index,
                    hits: u64::try_from(at).unwrap() * 3,
                });
            }
        }
        rules
    }

    fn rules_page_of(rules: Vec<ListedRule>, states: Vec<(TargetId, TargetState)>) -> RulesPage {
        let mut page = RulesPage::new();
        page.loaded(RulesLoad { rules });
        page.set_target_states(states);
        page
    }

    fn page_with(filing: Filing, rules: RulesPage) -> Hermes {
        let sources = filing.sources.len();
        Hermes {
            filing_config: filing,
            sorting: Some(SortingPage {
                states: vec![SourceState::Ready { questions: 0 }; sources],
                ..SortingPage::default()
            }),
            rules: Some(rules),
            ..Hermes::default()
        }
    }

    fn rule_dialog() -> Hermes {
        let filing = filing();
        let mut form = RuleForm::new().in_target(id("company"));
        form.update(RuleFormMessage::Name("Telekom invoices".to_owned()));
        form.update(RuleFormMessage::Condition(ConditionEdit::Value(
            0,
            "Telekom".to_owned(),
        )));
        form.update(RuleFormMessage::Pattern("{year}/{sender}/".to_owned()));
        let filed = |name: &str| SortTryRow {
            relative: PathBuf::from(name),
            state: SortTryState::Filed(PathBuf::from(format!("2026/Telekom/{name}"))),
        };
        let mut rows: Vec<SortTryRow> = (1..=9)
            .map(|number| filed(&format!("invoice-{number}.pdf")))
            .collect();
        rows.push(SortTryRow {
            relative: PathBuf::from("scan-0412.pdf"),
            state: SortTryState::MissingValue(Field::Date),
        });
        form.live_try.status = TryStatus::Ready(SortTry {
            rows,
            more: 0,
            no_text: 2,
        });
        let mut page = RulesPage::new();
        page.open_form(form);
        Hermes {
            filing_config: filing,
            rules: Some(page),
            ..Hermes::default()
        }
    }

    fn target_dialog() -> Hermes {
        let filing = filing();
        let mut dialog = TargetDialog::new(Some(&filing.targets[1]));
        dialog.folder_exists = Some(true);
        Hermes {
            filing_config: filing,
            target_dialog: Some(dialog),
            ..Hermes::default()
        }
    }

    fn picture_in(hermes: &Hermes, lang: Language, name: &str) {
        let palette = ThemePreset::Cosmic.palette();
        let tags = FileListTags::default();
        let ctx = ViewCtx {
            lang,
            palette: &palette,
            window_size: WINDOW,
            file_list_tags: &tags,
            jobs_busy: false,
            ai_download: None,
            home: Path::new(HOME),
        };
        let shown: Element<'_, Message> = match dialogs(hermes, &ctx).pop() {
            Some(dialog) => container(dialog).center(Length::Fill).into(),
            None => container(
                container(page_content(
                    hermes,
                    HermesPage::Sorting,
                    &ctx,
                    None,
                    |message| message,
                    |_, pressed| pressed,
                ))
                .max_width(SETTINGS_WIDE_PAGE_MAX_WIDTH),
            )
            .center_x(Length::Fill)
            .into(),
        };
        let mut simulator = Simulator::with_size(
            iced::Settings {
                default_font: ncrs_ui_kit::APP_FONT,
                ..iced::Settings::default()
            },
            WINDOW,
            shown,
        );
        let matches = simulator
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_reference_image(format!("tests/snapshots/{name}"))
            .unwrap();
        assert!(matches, "the picture differs from {name}");
    }

    fn picture(hermes: &Hermes, name: &str) {
        picture_in(hermes, Language::German, name);
    }

    #[test]
    fn targets_and_their_rules_picture() {
        let hermes = page_with(filing(), rules_page_of(listed_rules(), Vec::new()));
        picture(&hermes, "targets-group");
    }

    #[test]
    fn targets_and_their_rules_english_picture() {
        let hermes = page_with(filing(), rules_page_of(listed_rules(), Vec::new()));
        picture_in(&hermes, Language::English, "targets-group-en");
    }

    #[test]
    fn a_target_without_rules_picture() {
        let hermes = page_with(filing(), rules_page_of(Vec::new(), Vec::new()));
        picture(&hermes, "targets-group-empty");
    }

    #[test]
    fn a_target_that_waits_for_its_confirmation_picture() {
        let states = vec![(
            id("documents"),
            TargetState::Waiting(WaitReason::Unconfirmed),
        )];
        let hermes = page_with(filing(), rules_page_of(listed_rules(), states));
        picture(&hermes, "targets-group-unconfirmed");
    }

    #[test]
    fn the_rule_dialog_with_its_trial_run_picture() {
        picture(&rule_dialog(), "rule-dialog-trial");
    }

    #[test]
    fn the_rule_dialog_in_english_picture() {
        picture_in(&rule_dialog(), Language::English, "rule-dialog-trial-en");
    }

    #[test]
    fn the_target_dialog_picture() {
        picture(&target_dialog(), "target-dialog");
    }

    #[test]
    fn the_target_dialog_in_english_picture() {
        picture_in(&target_dialog(), Language::English, "target-dialog-en");
    }
}
