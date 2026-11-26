//! Which of several copies of a document can go: a copy lying in an unordered
//! folder, when another one stays. Pure: nothing here touches a file.

use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::Datelike as _;
use ncrs_index::{ContentHash, DocumentFields, SubtreePattern};

use crate::archive_profile::{pattern_per_document, year_of, ArchiveDocument, DocumentPattern};
use crate::structure::LocationStructure;

/// A file of the location, as the index and the disk know it. A copy of a
/// document is a file of its own with the document's hash, fields and text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanFile {
    /// Below the root of the location.
    pub relative_path: PathBuf,
    pub hash: ContentHash,
    pub fields: DocumentFields,
    pub text: Option<String>,
    /// Seconds since the epoch when the index first saw this path; of copies
    /// with the same hash, the oldest one stays when none fits its folder.
    pub recorded_at: i64,
}

/// A surplus copy in an unordered folder and the copy that stays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateCopy {
    pub source: PathBuf,
    pub hash: ContentHash,
    pub keeper: PathBuf,
}

/// The surplus copies, sorted by source. `profile` and `unordered` are what
/// `learn_archive` and `learn_structure` return for these files.
pub fn duplicate_copies(
    files: &[PlanFile],
    profile: &[SubtreePattern],
    unordered: &LocationStructure,
) -> Vec<DuplicateCopy> {
    let mut ordered: Vec<&PlanFile> = files.iter().collect();
    ordered.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

    let documents: Vec<ArchiveDocument> = ordered
        .iter()
        .map(|file| ArchiveDocument {
            relative_path: file.relative_path.clone(),
            date: file.fields.date,
        })
        .collect();
    let patterns = pattern_per_document(&documents);
    let mut learned: BTreeMap<&str, &str> = BTreeMap::new();
    for entry in profile {
        if let Some(pattern) = entry.pattern.as_deref() {
            learned.entry(entry.subtree.as_str()).or_insert(pattern);
        }
    }
    let context = Context {
        learned,
        unordered,
        files: &ordered,
        patterns: &patterns,
    };
    context
        .keepers()
        .into_iter()
        .filter_map(|(copy, keeper)| {
            let file = ordered.get(copy)?;
            Some(DuplicateCopy {
                source: file.relative_path.clone(),
                hash: file.hash,
                keeper: ordered.get(keeper)?.relative_path.clone(),
            })
        })
        .collect()
}

struct Context<'a> {
    /// The first learned pattern of each subtree.
    learned: BTreeMap<&'a str, &'a str>,
    unordered: &'a LocationStructure,
    files: &'a [&'a PlanFile],
    patterns: &'a [Option<DocumentPattern>],
}

impl Context<'_> {
    /// For every surplus copy lying in an unordered folder, the index of the copy
    /// that stays. Copies in ordered folders stay and have no entry. The keeper is
    /// one of them; when all copies are unordered, one of those. Of several, the
    /// one that fits its folder (its name follows the learned pattern and its year
    /// folder is its date's year), then the one the index saw first, then the
    /// first path.
    fn keepers(&self) -> BTreeMap<usize, usize> {
        let mut groups: BTreeMap<[u8; 32], Vec<usize>> = BTreeMap::new();
        for (index, file) in self.files.iter().enumerate() {
            groups.entry(file.hash.0).or_default().push(index);
        }
        let mut keepers = BTreeMap::new();
        for members in groups.values().filter(|members| members.len() > 1) {
            let fits = |index: &usize| {
                !self.name_deviates(*index)
                    && self
                        .files
                        .get(*index)
                        .is_some_and(|file| !lies_in_another_year(file))
            };
            let is_unordered = |index: &usize| {
                self.files
                    .get(*index)
                    .is_some_and(|file| self.unordered.is_unordered(&file.relative_path))
            };
            let ordered: Vec<usize> = members
                .iter()
                .copied()
                .filter(|index| !is_unordered(index))
                .collect();
            let candidates = if ordered.is_empty() {
                members.clone()
            } else {
                ordered
            };
            let fitting: Vec<usize> = candidates.iter().copied().filter(fits).collect();
            let pool = if fitting.is_empty() {
                candidates
            } else {
                fitting
            };
            let seniority = |index: &usize| {
                self.files
                    .get(*index)
                    .map(|file| (file.recorded_at, file.relative_path.clone()))
            };
            let Some(keeper) = pool.into_iter().min_by_key(seniority) else {
                continue;
            };
            let surplus = members
                .iter()
                .filter(|&&member| member != keeper && is_unordered(&member));
            for &member in surplus {
                keepers.insert(member, keeper);
            }
        }
        keepers
    }

    /// Whether the folder follows another file name pattern than the file does.
    fn name_deviates(&self, index: usize) -> bool {
        let Some(own) = self.patterns.get(index).and_then(Option::as_ref) else {
            return false;
        };
        self.learned
            .get(own.subtree.as_str())
            .is_some_and(|learned| file_name_pattern(&own.pattern) != file_name_pattern(learned))
    }
}

fn file_name_pattern(pattern: &str) -> &str {
    pattern.rsplit('/').next().unwrap_or(pattern)
}

/// Whether a year folder of the path is not the year of the document's date
/// and its name does not say why: a notice dated after its folder's year is
/// normal when its name names that year.
fn lies_in_another_year(file: &PlanFile) -> bool {
    let (Some(date), Some(folders)) = (file.fields.date, file.relative_path.parent()) else {
        return false;
    };
    folders
        .components()
        .filter_map(|component| year_of(component.as_os_str().to_str()?))
        .find(|&year| year != date.year())
        .is_some_and(|folder_year| !mentions_year(file, folder_year))
}

fn mentions_year(file: &PlanFile, year: i32) -> bool {
    let wanted = format!("{year:04}");
    file.relative_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| {
            stem.split(|character: char| !character.is_alphanumeric())
                .any(|token| token == wanted)
        })
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use ncrs_index::{KindCatalog, SenderAliases};
    use sha2::{Digest as _, Sha256};

    use super::*;
    use crate::archive_profile::learn_archive;
    use crate::field_detection::{detect_fields, OwnNames};
    use crate::location_documents::location_structure;
    use crate::test_archive::{test_archive, OWN_NAME};

    const DOWNLOADS_FOLDER: &str = "Downloads alt";
    const DOWNLOAD_COPIES: usize = 40;

    fn copies_of(files: &[PlanFile]) -> Vec<DuplicateCopy> {
        let documents: Vec<ArchiveDocument> = files
            .iter()
            .map(|file| ArchiveDocument {
                relative_path: file.relative_path.clone(),
                date: file.fields.date,
            })
            .collect();
        duplicate_copies(
            files,
            &learn_archive(&documents),
            &location_structure(files, &KindCatalog::built_in()),
        )
    }

    fn file(path: &str, seed: u8, date: Option<NaiveDate>, recorded_at: i64) -> PlanFile {
        PlanFile {
            relative_path: PathBuf::from(path),
            hash: ContentHash([seed; 32]),
            fields: DocumentFields {
                date,
                ..DocumentFields::default()
            },
            text: None,
            recorded_at,
        }
    }

    fn day(year: i32, month: u32) -> Option<NaiveDate> {
        NaiveDate::from_ymd_opt(year, month, 1)
    }

    /// Company A's insurance letters, filed as `Insurance/<year>/<date>_invoice`.
    fn insurance_archive() -> Vec<PlanFile> {
        [
            (2023, 2),
            (2023, 3),
            (2024, 1),
            (2024, 2),
            (2024, 3),
            (2024, 6),
        ]
        .into_iter()
        .zip(1..)
        .map(|((year, month), seed)| {
            file(
                &format!("Insurance/{year}/{year}-{month:02}-01_invoice.pdf"),
                seed,
                day(year, month),
                1,
            )
        })
        .collect()
    }

    fn files_of_the_test_archive() -> Vec<PlanFile> {
        let names = [OWN_NAME.to_owned()];
        let own = OwnNames {
            names: &names,
            ibans: &[],
            tax_ids: &[],
        };
        test_archive()
            .archive
            .iter()
            .zip(0..)
            .map(|(archived, recorded_at)| {
                let found = archived.text().map(|text| {
                    detect_fields(
                        text,
                        &DocumentFields::default(),
                        &SenderAliases::default(),
                        &own,
                        &KindCatalog::built_in(),
                    )
                });
                PlanFile {
                    relative_path: archived.relative_path.clone(),
                    hash: ContentHash(Sha256::digest(archived.bytes()).into()),
                    fields: DocumentFields {
                        date: found
                            .as_ref()
                            .and_then(|detected| detected.date)
                            .or(archived.date),
                        sender: found.as_ref().and_then(|detected| detected.sender.clone()),
                        kind: found.and_then(|detected| detected.kind),
                        ..DocumentFields::default()
                    },
                    text: archived.text().map(str::to_owned),
                    recorded_at,
                }
            })
            .collect()
    }

    #[test]
    fn a_copy_in_an_unordered_folder_is_noted_and_the_copy_in_an_ordered_folder_stays() {
        let mut files = insurance_archive();
        files.push(file(
            "Insurance/2024/2024-04-01_invoice.pdf",
            9,
            day(2024, 4),
            3,
        ));
        files.push(file("Insurance/2023/scan.pdf", 9, day(2024, 4), 1));
        files.push(file(
            "Inbox Leftovers/2024-04-01_invoice.pdf",
            9,
            day(2024, 4),
            2,
        ));

        assert_eq!(
            copies_of(&files),
            [DuplicateCopy {
                source: PathBuf::from("Inbox Leftovers/2024-04-01_invoice.pdf"),
                hash: ContentHash([9; 32]),
                keeper: PathBuf::from("Insurance/2024/2024-04-01_invoice.pdf"),
            }]
        );
    }

    #[test]
    fn copies_that_all_lie_in_ordered_folders_stay() {
        let mut files = insurance_archive();
        files.push(file(
            "Insurance/2024/2024-04-01_invoice.pdf",
            9,
            day(2024, 4),
            1,
        ));
        files.push(file(
            "Insurance/2024/copy of 2024-04-01_invoice.pdf",
            9,
            day(2024, 4),
            2,
        ));

        assert!(copies_of(&files).is_empty());
    }

    #[test]
    fn when_all_copies_are_unordered_the_one_seen_first_stays() {
        let mut files = insurance_archive();
        files.push(file("Junk/b.pdf", 9, None, 2));
        files.push(file("Misc/a.pdf", 9, None, 1));

        assert_eq!(
            copies_of(&files),
            [DuplicateCopy {
                source: PathBuf::from("Junk/b.pdf"),
                hash: ContentHash([9; 32]),
                keeper: PathBuf::from("Misc/a.pdf"),
            }]
        );
    }

    #[test]
    fn every_download_copy_of_the_test_archive_is_noted_with_a_keeper_elsewhere() {
        let copies = copies_of(&files_of_the_test_archive());

        let downloads: Vec<&DuplicateCopy> = copies
            .iter()
            .filter(|copy| copy.source.starts_with(DOWNLOADS_FOLDER))
            .collect();
        assert_eq!(downloads.len(), DOWNLOAD_COPIES);
        for copy in downloads {
            assert!(!copy.keeper.starts_with(DOWNLOADS_FOLDER), "{copy:?}");
        }
    }

    #[test]
    fn copies_in_ordered_folders_of_the_test_archive_are_no_duplicates() {
        let copies = copies_of(&files_of_the_test_archive());

        let in_ordered_folders: Vec<&DuplicateCopy> = copies
            .iter()
            .filter(|copy| {
                let tax_receipt = copy.source.starts_with("Steuer")
                    && copy.source.to_string_lossy().contains("Belege");
                tax_receipt || copy.source.starts_with("Bank/Finanzierung")
            })
            .collect();
        assert!(in_ordered_folders.is_empty(), "{in_ordered_folders:?}");
    }

    #[test]
    fn a_notice_that_names_its_folders_year_fits_although_its_date_is_later() {
        assert!(!lies_in_another_year(&file(
            "Tax/2023/Notice 2023.pdf",
            1,
            day(2024, 5),
            1
        )));
        assert!(lies_in_another_year(&file(
            "Tax/2023/Notice.pdf",
            1,
            day(2024, 5),
            1
        )));
    }
}
