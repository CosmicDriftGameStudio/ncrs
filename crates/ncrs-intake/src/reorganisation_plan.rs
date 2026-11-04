//! Gathers what `plan_reorganisation` needs from the index. Reads only: the
//! files of the location are not touched.

use std::path::{Path, PathBuf};

use ncrs_index::{Index, KindCatalog};

use crate::error::IntakeError;
use crate::reorganisation::PlanFile;
use crate::structure::{learn_structure, LocationStructure, StructureDocument};

/// The structure the documents of a location are laid out in, learned from
/// their fields as they are now.
pub fn location_structure(files: &[PlanFile], kinds: &KindCatalog) -> LocationStructure {
    let documents: Vec<StructureDocument> = files
        .iter()
        .map(|file| StructureDocument {
            relative_path: file.relative_path.clone(),
            date: file.fields.date,
            sender: file.fields.sender.clone(),
            kind: file.fields.kind.clone(),
        })
        .collect();
    learn_structure(&documents, kinds)
}

/// Every document below `root` with its fields and text, and each of its
/// known copies as a file of its own.
pub fn location_documents(index: &Index, root: &Path) -> Result<Vec<PlanFile>, IntakeError> {
    let mut files = Vec::new();
    for (id, path) in index.documents_under(root)? {
        let (Some(document), Ok(relative)) = (index.document(id)?, path.strip_prefix(root)) else {
            continue;
        };
        let text = index.document_text(id)?;
        let shared = |relative_path: PathBuf, recorded_at: i64| PlanFile {
            relative_path,
            hash: document.hash,
            fields: document.fields.clone(),
            text: text.clone(),
            recorded_at,
        };
        files.push(shared(
            relative.to_path_buf(),
            document.received_at.timestamp(),
        ));
        for (copy, seen_at) in index.duplicate_copies(id)? {
            if let Ok(relative_copy) = copy.strip_prefix(root) {
                files.push(shared(relative_copy.to_path_buf(), seen_at.timestamp()));
            }
        }
    }
    Ok(files)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::time::SystemTime;

    use chrono::{NaiveDate, TimeZone as _, Utc};
    use ncrs_index::{
        Condition, ConditionKind, ConflictReason, ContentHash, DocumentFields, DocumentId,
        DocumentSource, FieldOrigin, FileStamp, Finding, FindingKind, NewDocument, NoActionReason,
        PlanItem, PlanOutcome, RuleConditions, RuleEvidence, RuleSpec, SenderAliasKind,
        SenderAliases,
    };
    use tempfile::TempDir;

    use super::*;
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use ncrs_index::{KindLanguage, Kinds, ReorganisationPlan};
    use walkdir::WalkDir;

    use crate::archive_profile::{learn_archive, ArchiveDocument};
    use crate::reorganisation::{plan_reorganisation, PlanRule, ReorganisationInput};

    /// Plans and stores the reorganisation of the location at `root`, as the chat's
    /// reading does, so the tests cover the whole planning.
    pub(super) fn plan_location(
        index: &mut Index,
        root: &Path,
        language: KindLanguage,
    ) -> Result<ReorganisationPlan, IntakeError> {
        let files = location_documents(index, root)?;
        let documents: Vec<ArchiveDocument> = files
            .iter()
            .map(|file| ArchiveDocument {
                relative_path: file.relative_path.clone(),
                date: file.fields.date,
            })
            .collect();
        let profile = learn_archive(&documents);
        let rules = plan_rules(index)?;
        let existing = existing_paths(root)?;
        let kinds = Kinds::new(Arc::clone(index.kinds()), language);
        let unordered = location_structure(&files, &kinds.catalog);
        let sender_aliases = index.confirmed_sender_aliases()?;
        let items = plan_reorganisation(&ReorganisationInput {
            files: &files,
            profile: &profile,
            unordered: &unordered,
            rules: &rules,
            sender_aliases: &sender_aliases,
            existing: &existing,
            kinds: &kinds,
        });
        index.replace_reorganisation_plan(root, &items)?;
        index
            .reorganisation_plan(root)?
            .ok_or_else(|| IntakeError::io(root)(std::io::Error::other("the plan was not stored")))
    }

    fn plan_rules(index: &Index) -> Result<Vec<PlanRule>, IntakeError> {
        index
            .rules()?
            .into_iter()
            .map(|rule| {
                let evidence = index.rule_evidence(rule.id)?;
                Ok(PlanRule { rule, evidence })
            })
            .collect()
    }

    /// Every file and folder below the root, hidden ones too: a target must not
    /// land on any of them. A part that cannot be read fails the plan, because
    /// what is in it is not known.
    fn existing_paths(root: &Path) -> Result<BTreeSet<PathBuf>, IntakeError> {
        let mut existing = BTreeSet::new();
        for step in WalkDir::new(root).follow_links(false).min_depth(1) {
            let entry = step.map_err(|err| {
                let path = err.path().unwrap_or(root).to_path_buf();
                IntakeError::io(path)(std::io::Error::from(err))
            })?;
            if let Ok(relative) = entry.path().strip_prefix(root) {
                existing.insert(relative.to_path_buf());
            }
        }
        Ok(existing)
    }

    use crate::location::{Location, LocationMode};
    use crate::reorganisation::FINDING_PRIORITY;
    use crate::sanitise::stays_below;

    struct Archive {
        _dir: TempDir,
        root: PathBuf,
        index: Index,
        next_seed: u8,
    }

    fn day(year: i32, month: u32, day: u32) -> Option<NaiveDate> {
        NaiveDate::from_ymd_opt(year, month, day)
    }

    fn rule_spec(
        phrase: &str,
        kind: &str,
        pattern: &str,
        sender: Option<&str>,
        confirmed: bool,
    ) -> RuleSpec {
        RuleSpec {
            name: None,
            conditions: RuleConditions::all_of(
                std::iter::once(Condition::new(ConditionKind::Word, phrase))
                    .chain(sender.map(|name| Condition::new(ConditionKind::Sender, name)))
                    .collect(),
            ),
            extract_number: None,
            extract_date: None,
            extract_amount: None,
            kind: Some(kind.to_owned()),
            tags: Default::default(),
            filing_pattern: pattern.to_owned(),
            confirmed,
            enabled: true,
        }
    }

    impl Archive {
        fn new() -> Self {
            let dir = TempDir::new().unwrap();
            let root = Location::new(dir.path(), LocationMode::Archive, false)
                .unwrap()
                .path;
            let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
            // The index file lives next to the archive, not in it.
            let archive_root = root.join("archive");
            fs::create_dir(&archive_root).unwrap();
            Self {
                _dir: dir,
                root: archive_root,
                index,
                next_seed: 0,
            }
        }

        fn write(&self, relative: &str, content: &str) {
            let path = self.root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }

        fn stamp() -> FileStamp {
            FileStamp {
                size: 1,
                mtime: SystemTime::now(),
            }
        }

        fn add(&mut self, relative: &str, date: Option<NaiveDate>, text: &str) -> DocumentId {
            self.next_seed += 1;
            let seed = self.next_seed;
            self.write(relative, &format!("content {seed}"));
            let id = self
                .index
                .insert_document(&NewDocument {
                    hash: ContentHash([seed; 32]),
                    path: self.root.join(relative),
                    source: DocumentSource::Archive,
                    received_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
                    stamp: Some(Self::stamp()),
                })
                .unwrap();
            self.index
                .set_document_fields(
                    id,
                    &DocumentFields {
                        date,
                        ..DocumentFields::default()
                    },
                    |_| FieldOrigin::Unknown,
                )
                .unwrap();
            self.index.set_document_text(id, text).unwrap();
            id
        }

        fn add_copy(&mut self, of: DocumentId, relative: &str, content_of: &str) {
            let original = fs::read_to_string(self.root.join(content_of)).unwrap();
            self.write(relative, &original);
            self.index
                .record_duplicate_path(of, &self.root.join(relative), Self::stamp())
                .unwrap();
        }

        fn rule(&mut self, spec: &RuleSpec) -> ncrs_index::RuleId {
            self.index.create_rule(spec).unwrap()
        }

        fn suggest(&mut self, spec: &RuleSpec, in_folder: u32, of: u32, elsewhere: u32) {
            self.index
                .suggest_rule(
                    spec,
                    RuleEvidence {
                        matching_in_folder: in_folder,
                        folder_documents: of,
                        matching_elsewhere: elsewhere,
                        subtree: None,
                    },
                )
                .unwrap();
        }

        /// Keyed the way the tests write paths, whatever the platform's separator.
        fn plan(&mut self) -> BTreeMap<String, PlanItem> {
            plan_location(&mut self.index, &self.root, KindLanguage::English)
                .unwrap()
                .items
                .into_iter()
                .map(|item| {
                    let key = item
                        .source
                        .components()
                        .map(|part| part.as_os_str().to_str().unwrap())
                        .collect::<Vec<_>>()
                        .join("/");
                    (key, item)
                })
                .collect()
        }

        /// Every path below the archive with its content.
        fn snapshot(&self) -> BTreeMap<PathBuf, Vec<u8>> {
            WalkDir::new(&self.root)
                .into_iter()
                .map(Result::unwrap)
                .filter(|entry| !entry.file_type().is_dir())
                .map(|entry| {
                    (
                        entry.path().strip_prefix(&self.root).unwrap().to_path_buf(),
                        fs::read(entry.path()).unwrap(),
                    )
                })
                .collect()
        }
    }

    /// Company A's insurance letters, filed as `Insurance/<year>/<date>_invoice`.
    fn insurance_archive() -> Archive {
        let mut archive = Archive::new();
        for (year, month) in [(2023, 2), (2023, 3), (2024, 1), (2024, 2), (2024, 3)] {
            archive.add(
                &format!("Insurance/{year}/{year}-{month:02}-01_invoice.pdf"),
                day(year, month, 1),
                "Company A policy letter",
            );
        }
        archive.add(
            "Insurance/2024/2024-06-01_invoice.pdf",
            day(2024, 6, 1),
            "Company A policy letter",
        );
        archive
    }

    fn only_findings(item: &PlanItem) -> Vec<FindingKind> {
        item.findings.iter().map(Finding::kind).collect()
    }

    #[test]
    fn a_sender_rule_matches_a_confirmed_alias_in_the_reorganisation_as_in_intake() {
        let mut archive = Archive::new();
        archive.rule(&rule_spec(
            "policy",
            "invoice",
            "Billing/{date}.pdf",
            Some("Telekom"),
            true,
        ));
        let rules: Vec<PlanRule> = archive
            .index
            .rules()
            .unwrap()
            .into_iter()
            .map(|rule| PlanRule {
                rule,
                evidence: None,
            })
            .collect();
        let files = [PlanFile {
            relative_path: PathBuf::from("Misc/letter.pdf"),
            hash: ContentHash([3; 32]),
            fields: DocumentFields {
                date: day(2024, 1, 1),
                sender: Some("Telekom Deutschland GmbH".to_owned()),
                ..DocumentFields::default()
            },
            text: Some("Telekom Deutschland GmbH policy letter".to_owned()),
            recorded_at: 1,
        }];
        let planned = |aliases: &SenderAliases| {
            plan_reorganisation(&ReorganisationInput {
                files: &files,
                profile: &[],
                unordered: &LocationStructure {
                    areas: Vec::new(),
                    unordered: Vec::new(),
                },
                rules: &rules,
                sender_aliases: aliases,
                existing: &BTreeSet::new(),
                kinds: &Kinds::built_in(KindLanguage::English),
            })
        };
        let confirmed = SenderAliases::from_confirmed([(
            SenderAliasKind::Name,
            "Telekom Deutschland GmbH",
            "Telekom",
        )]);

        let with_alias = planned(&confirmed);
        let without_alias = planned(&SenderAliases::default());

        assert_eq!(with_alias.len(), 1, "{with_alias:?}");
        assert_eq!(
            with_alias[0].outcome,
            PlanOutcome::Move {
                target: PathBuf::from("Billing/2024-01-01.pdf")
            }
        );
        assert!(without_alias.is_empty(), "{without_alias:?}");
    }

    #[test]
    fn a_year_folder_that_disagrees_with_the_date_is_fixed_by_replacing_only_the_year() {
        let mut archive = insurance_archive();
        archive.add(
            "Insurance/2023/2024-05-01_invoice.pdf",
            day(2024, 5, 1),
            "Company A policy letter",
        );

        let plan = archive.plan();

        assert_eq!(plan.len(), 1, "{plan:?}");
        let item = &plan["Insurance/2023/2024-05-01_invoice.pdf"];
        assert_eq!(
            item.findings,
            [Finding::YearMismatch {
                folder_year: 2023,
                date_year: 2024
            }]
        );
        assert_eq!(
            item.outcome,
            PlanOutcome::Move {
                target: PathBuf::from("Insurance/2024/2024-05-01_invoice.pdf")
            }
        );
    }

    #[test]
    fn a_name_that_does_not_follow_the_folder_is_renamed_within_the_folder() {
        let mut archive = insurance_archive();
        archive.add(
            "Insurance/2024/scan 0042.pdf",
            day(2024, 4, 5),
            "Company A policy letter",
        );
        archive.add("Insurance/2024/IMG_7.pdf", None, "Company A policy letter");

        let plan = archive.plan();

        assert_eq!(plan.len(), 2, "{plan:?}");
        let renamed = &plan["Insurance/2024/scan 0042.pdf"];
        assert_eq!(only_findings(renamed), [FindingKind::NameOutlier]);
        assert_eq!(
            renamed.outcome,
            PlanOutcome::Rename {
                target: PathBuf::from("Insurance/2024/2024-04-05_invoice.pdf")
            }
        );
        let unknown = &plan["Insurance/2024/IMG_7.pdf"];
        assert_eq!(only_findings(unknown), [FindingKind::NameOutlier]);
        assert_eq!(
            unknown.outcome,
            PlanOutcome::NoAction(NoActionReason::MissingFields)
        );
    }

    #[test]
    fn a_confirmed_rule_or_a_candidate_with_enough_evidence_points_a_document_elsewhere() {
        let mut archive = insurance_archive();
        let tax = archive.rule(&rule_spec(
            "tax notice",
            "Tax",
            "Taxes/{year}/{date}_{kind}.pdf",
            None,
            true,
        ));
        archive.suggest(
            &rule_spec(
                "late fee",
                "Fees",
                "Fees/{year}/{date}_{kind}.pdf",
                None,
                false,
            ),
            3,
            3,
            0,
        );
        archive.suggest(
            &rule_spec(
                "reminder",
                "Notes",
                "Notes/{year}/{date}_{kind}.pdf",
                None,
                false,
            ),
            1,
            2,
            0,
        );
        archive.suggest(
            &rule_spec(
                "welcome",
                "Hello",
                "Hello/{year}/{date}_{kind}.pdf",
                None,
                false,
            ),
            3,
            3,
            1,
        );
        let discarded = rule_spec("gone", "Gone", "Gone/{year}/{date}_{kind}.pdf", None, false);
        archive.suggest(&discarded, 3, 3, 0);
        let discarded_id = archive
            .index
            .suggested_rules()
            .unwrap()
            .last()
            .unwrap()
            .0
            .id;
        archive.index.discard_rule_suggestion(discarded_id).unwrap();
        archive.add(
            "Insurance/2024/2024-07-01_invoice.pdf",
            day(2024, 7, 1),
            "Agency B Tax Notice",
        );
        archive.add(
            "Insurance/2024/2024-08-01_invoice.pdf",
            day(2024, 8, 1),
            "Company A late fee",
        );
        for (name, text) in [
            ("2024-09-01", "a reminder"),
            ("2024-09-02", "welcome"),
            ("2024-09-03", "gone"),
        ] {
            archive.add(
                &format!("Insurance/2024/{name}_invoice.pdf"),
                day(2024, 9, 1),
                text,
            );
        }

        let plan = archive.plan();

        assert_eq!(plan.len(), 2, "{plan:?}");
        let by_tax = &plan["Insurance/2024/2024-07-01_invoice.pdf"];
        assert_eq!(by_tax.findings, [Finding::Misplaced { rule: tax }]);
        assert_eq!(
            by_tax.outcome,
            PlanOutcome::Move {
                target: PathBuf::from("Taxes/2024/2024-07-01_Tax.pdf")
            }
        );
        let by_candidate = &plan["Insurance/2024/2024-08-01_invoice.pdf"];
        assert_eq!(only_findings(by_candidate), [FindingKind::Misplaced]);
        assert_eq!(
            by_candidate.outcome,
            PlanOutcome::Move {
                target: PathBuf::from("Fees/2024/2024-08-01_Fees.pdf")
            }
        );
    }

    #[test]
    fn a_copy_in_an_unordered_folder_is_noted_and_the_copy_in_an_ordered_folder_stays() {
        let mut archive = insurance_archive();
        let original = archive.add(
            "Insurance/2024/2024-04-01_invoice.pdf",
            day(2024, 4, 1),
            "Company A policy letter",
        );
        archive.add_copy(
            original,
            "Insurance/2023/scan.pdf",
            "Insurance/2024/2024-04-01_invoice.pdf",
        );
        archive.add_copy(
            original,
            "Inbox Leftovers/2024-04-01_invoice.pdf",
            "Insurance/2024/2024-04-01_invoice.pdf",
        );

        let plan = archive.plan();

        let noted: Vec<(&String, &PlanItem)> = plan
            .iter()
            .filter(|(_, item)| item.findings[0].kind() == FindingKind::Duplicate)
            .collect();
        assert_eq!(noted.len(), 1, "{plan:?}");
        let (source, item) = noted[0];
        assert_eq!(source, "Inbox Leftovers/2024-04-01_invoice.pdf");
        assert_eq!(
            item.findings[0],
            Finding::Duplicate {
                keeper: PathBuf::from("Insurance/2024/2024-04-01_invoice.pdf")
            }
        );
        assert_eq!(
            item.outcome,
            PlanOutcome::NoAction(NoActionReason::DuplicateCopy)
        );
        assert_eq!(
            item.hash,
            archive.index.document(original).unwrap().unwrap().hash
        );
        assert!(!plan.contains_key("Insurance/2024/2024-04-01_invoice.pdf"));
        assert!(plan
            .get("Insurance/2023/scan.pdf")
            .is_none_or(|other| !only_findings(other).contains(&FindingKind::Duplicate)));
    }

    #[test]
    fn copies_that_all_lie_in_ordered_folders_stay() {
        let mut archive = insurance_archive();
        let first = archive.add(
            "Insurance/2024/2024-04-01_invoice.pdf",
            day(2024, 4, 1),
            "Company A policy letter",
        );
        archive.add_copy(
            first,
            "Insurance/2024/copy of 2024-04-01_invoice.pdf",
            "Insurance/2024/2024-04-01_invoice.pdf",
        );

        let plan = archive.plan();

        for item in plan.values() {
            assert!(!only_findings(item).contains(&FindingKind::Duplicate));
        }
    }

    #[test]
    fn when_all_copies_are_unordered_the_one_seen_first_stays() {
        let mut archive = insurance_archive();
        let first = archive.add("Misc/a.pdf", None, "Loose letter");
        archive.add_copy(first, "Junk/b.pdf", "Misc/a.pdf");

        let plan = archive.plan();

        let item = &plan["Junk/b.pdf"];
        assert_eq!(
            item.findings[0],
            Finding::Duplicate {
                keeper: PathBuf::from("Misc/a.pdf")
            }
        );
        assert_eq!(
            item.outcome,
            PlanOutcome::NoAction(NoActionReason::DuplicateCopy)
        );
        assert!(!plan.contains_key("Misc/a.pdf"));
    }

    #[test]
    fn a_duplicate_is_never_proposed_for_deletion() {
        let mut archive = insurance_archive();
        let original = archive.add(
            "Insurance/2024/2024-04-01_invoice.pdf",
            day(2024, 4, 1),
            "Company A policy letter",
        );
        archive.add_copy(
            original,
            "Misc/2024-04-01_invoice.pdf",
            "Insurance/2024/2024-04-01_invoice.pdf",
        );

        let plan = archive.plan();

        for item in plan.values() {
            assert!(
                matches!(
                    item.outcome,
                    PlanOutcome::Move { .. }
                        | PlanOutcome::Rename { .. }
                        | PlanOutcome::Conflict { .. }
                        | PlanOutcome::NoAction(_)
                ),
                "{item:?}"
            );
        }
        let noted = plan
            .values()
            .filter(|item| item.findings[0].kind() == FindingKind::Duplicate)
            .count();
        assert_eq!(noted, 1);
        assert!(archive.root.join("Misc/2024-04-01_invoice.pdf").is_file());
        assert!(archive
            .root
            .join("Insurance/2024/2024-04-01_invoice.pdf")
            .is_file());
    }

    #[test]
    fn a_target_on_disk_or_wanted_twice_is_a_conflict() {
        let mut archive = insurance_archive();
        archive.add(
            "Insurance/2023/2024-06-01_invoice.pdf",
            day(2024, 6, 1),
            "Company A policy letter",
        );
        archive.add(
            "Insurance/2023/2024-10-01_invoice.pdf",
            day(2024, 10, 1),
            "Company A policy letter",
        );
        archive.add(
            "Insurance/2022/2024-10-01_invoice.pdf",
            day(2024, 10, 1),
            "Company A policy letter",
        );

        let plan = archive.plan();

        assert_eq!(
            plan["Insurance/2023/2024-06-01_invoice.pdf"].outcome,
            PlanOutcome::Conflict {
                target: Some(PathBuf::from("Insurance/2024/2024-06-01_invoice.pdf")),
                reason: ConflictReason::TargetExists,
            }
        );
        for source in [
            "Insurance/2023/2024-10-01_invoice.pdf",
            "Insurance/2022/2024-10-01_invoice.pdf",
        ] {
            assert_eq!(
                plan[source].outcome,
                PlanOutcome::Conflict {
                    target: Some(PathBuf::from("Insurance/2024/2024-10-01_invoice.pdf")),
                    reason: ConflictReason::TargetTakenByOtherItem,
                },
                "{source}"
            );
        }
        assert!(archive
            .root
            .join("Insurance/2024/2024-06-01_invoice.pdf")
            .is_file());
    }

    #[test]
    fn a_value_from_a_document_cannot_lead_a_target_out_of_the_location() {
        let mut archive = insurance_archive();
        archive.rule(&rule_spec(
            "tax notice",
            "Tax",
            "{sender}/{date}.pdf",
            None,
            true,
        ));
        let id = archive.add(
            "Insurance/2024/2024-07-01_invoice.pdf",
            day(2024, 7, 1),
            "Agency B Tax Notice",
        );
        archive
            .index
            .set_document_fields(
                id,
                &DocumentFields {
                    sender: Some("../../outside".to_owned()),
                    date: day(2024, 7, 1),
                    ..DocumentFields::default()
                },
                |_| FieldOrigin::Unknown,
            )
            .unwrap();

        let plan = archive.plan();

        let PlanOutcome::Move { target } = &plan["Insurance/2024/2024-07-01_invoice.pdf"].outcome
        else {
            panic!("{plan:?}");
        };
        assert!(stays_below(target), "{target:?}");
        assert_eq!(target, Path::new("outside/2024-07-01.pdf"));
    }

    #[test]
    fn a_source_path_that_leaves_the_location_gets_a_conflict_without_a_target() {
        let files = [PlanFile {
            relative_path: PathBuf::from("../2022/x.pdf"),
            hash: ContentHash([7; 32]),
            fields: DocumentFields {
                date: day(2024, 1, 1),
                ..DocumentFields::default()
            },
            text: None,
            recorded_at: 1,
        }];

        let items = plan_reorganisation(&ReorganisationInput {
            files: &files,
            profile: &[],
            unordered: &LocationStructure {
                areas: Vec::new(),
                unordered: Vec::new(),
            },
            rules: &[],
            sender_aliases: &SenderAliases::default(),
            existing: &BTreeSet::new(),
            kinds: &Kinds::built_in(KindLanguage::English),
        });

        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].outcome,
            PlanOutcome::Conflict {
                target: None,
                reason: ConflictReason::TargetLeavesLocation,
            }
        );
    }

    #[test]
    fn when_several_findings_apply_the_named_priority_decides_and_all_are_stored() {
        assert_eq!(
            FINDING_PRIORITY,
            [
                FindingKind::Duplicate,
                FindingKind::Misplaced,
                FindingKind::YearMismatch,
                FindingKind::NameOutlier,
            ]
        );
        let mut archive = insurance_archive();
        archive.rule(&rule_spec(
            "tax notice",
            "Tax",
            "Taxes/{year}/{date}_{kind}.pdf",
            None,
            true,
        ));
        let original = archive.add(
            "Insurance/2024/2024-04-01_invoice.pdf",
            day(2024, 4, 1),
            "Agency B Tax Notice",
        );
        archive.add_copy(
            original,
            "Inbox Leftovers/scan.pdf",
            "Insurance/2024/2024-04-01_invoice.pdf",
        );
        // The original is misplaced by the rule too, the copy is noted as a copy first.
        let plan = archive.plan();

        let copy = &plan["Inbox Leftovers/scan.pdf"];
        assert_eq!(
            only_findings(copy),
            [FindingKind::Duplicate, FindingKind::Misplaced]
        );
        assert_eq!(
            copy.outcome,
            PlanOutcome::NoAction(NoActionReason::DuplicateCopy)
        );
        let kept = &plan["Insurance/2024/2024-04-01_invoice.pdf"];
        assert_eq!(only_findings(kept), [FindingKind::Misplaced]);
    }

    #[test]
    fn items_keep_hash_and_source_and_a_new_plan_replaces_the_old_one() {
        let mut archive = insurance_archive();
        let id = archive.add(
            "Insurance/2023/2024-05-01_invoice.pdf",
            day(2024, 5, 1),
            "Company A policy letter",
        );
        let hash = archive.index.document(id).unwrap().unwrap().hash;

        let first =
            plan_location(&mut archive.index, &archive.root, KindLanguage::English).unwrap();
        let second =
            plan_location(&mut archive.index, &archive.root, KindLanguage::English).unwrap();

        assert_eq!(first.items[0].hash, hash);
        assert_eq!(
            first.items[0].source,
            Path::new("Insurance/2023/2024-05-01_invoice.pdf")
        );
        assert_ne!(first.id, second.id);
        let stored = archive.index.reorganisation_plan(&archive.root).unwrap();
        assert_eq!(stored.unwrap().id, second.id);
    }

    #[test]
    fn building_the_plan_leaves_every_file_unchanged() {
        let mut archive = insurance_archive();
        let original = archive.add(
            "Insurance/2024/2024-04-01_invoice.pdf",
            day(2024, 4, 1),
            "Agency B Tax Notice",
        );
        archive.add_copy(
            original,
            "Misc/2024-04-01_invoice.pdf",
            "Insurance/2024/2024-04-01_invoice.pdf",
        );
        archive.add(
            "Insurance/2023/2024-05-01_invoice.pdf",
            day(2024, 5, 1),
            "Company A policy letter",
        );
        archive.add("Insurance/2024/scan.pdf", day(2024, 4, 5), "x");
        archive.rule(&rule_spec(
            "tax notice",
            "Tax",
            "Taxes/{year}/{date}_{kind}.pdf",
            None,
            true,
        ));
        let before = archive.snapshot();

        let plan = archive.plan();

        assert!(plan.len() >= 3, "{plan:?}");
        assert_eq!(archive.snapshot(), before);
    }
}
