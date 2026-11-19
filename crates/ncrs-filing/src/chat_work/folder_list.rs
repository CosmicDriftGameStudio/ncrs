//! The folders of the target as the model sees them: a list without the year
//! folders, each with a few names of files lying in it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::Datelike as _;
use ncrs_index::DocumentFields;
use ncrs_intake::PlanFile;

/// File names shown per folder.
const EXAMPLES: usize = 3;
const EXAMPLE_CHARS: usize = 40;
/// Folders the model is shown at most: the list is read again for every page,
/// and an archive with thousands of folders would not fit the context.
const MAX_FOLDERS: usize = 400;

fn is_year_folder(segment: &str) -> bool {
    segment.len() == 4
        && (segment.starts_with("19") || segment.starts_with("20"))
        && segment.bytes().all(|byte| byte.is_ascii_digit())
}

#[derive(Debug, Default)]
pub struct FolderList {
    /// Folder below the target and the example names, in list order.
    pub entries: Vec<(String, Vec<String>)>,
    /// For folders whose files lay in year folders: how often the year
    /// folder stood after how many of the folder's own segments.
    pub year_places: BTreeMap<PathBuf, BTreeMap<usize, usize>>,
}

impl FolderList {
    pub fn of(files: &[PlanFile]) -> Self {
        let mut names: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
        let mut year_places: BTreeMap<PathBuf, BTreeMap<usize, usize>> = BTreeMap::new();
        let mut weight: BTreeMap<PathBuf, usize> = BTreeMap::new();
        for file in files {
            let Some(parent) = file.relative_path.parent() else {
                continue;
            };
            let kept: PathBuf = parent
                .iter()
                .filter(|segment| !is_year_folder(&segment.to_string_lossy()))
                .collect();
            let year_place = parent
                .iter()
                .position(|segment| is_year_folder(&segment.to_string_lossy()))
                .map(|at| {
                    parent
                        .iter()
                        .take(at)
                        .filter(|segment| !is_year_folder(&segment.to_string_lossy()))
                        .count()
                });
            if let Some(place) = year_place {
                *year_places
                    .entry(kept.clone())
                    .or_default()
                    .entry(place)
                    .or_default() += 1;
            }
            let mut ancestor = PathBuf::new();
            for segment in &kept {
                ancestor.push(segment);
                names.entry(ancestor.clone()).or_default();
                *weight.entry(ancestor.clone()).or_default() += 1;
            }
            if let (false, Some(raw_name)) =
                (kept.as_os_str().is_empty(), file.relative_path.file_name())
            {
                let name = raw_name.to_string_lossy();
                if !name.starts_with('.') {
                    names.entry(kept).or_default().push(name.into_owned());
                }
            }
        }
        if names.len() > MAX_FOLDERS {
            let mut by_use: Vec<&PathBuf> = names.keys().collect();
            by_use.sort_by_key(|folder| (std::cmp::Reverse(weight.get(*folder)), *folder));
            let kept: std::collections::BTreeSet<PathBuf> =
                by_use.into_iter().take(MAX_FOLDERS).cloned().collect();
            names.retain(|folder, _| kept.contains(folder));
        }
        let entries = names
            .into_iter()
            .map(|(folder, mut examples)| {
                examples.sort();
                examples.truncate(EXAMPLES);
                let shortened = examples
                    .iter()
                    .map(|name| name.chars().take(EXAMPLE_CHARS).collect())
                    .collect();
                (folder.to_string_lossy().replace('\\', "/"), shortened)
            })
            .collect();
        Self {
            entries,
            year_places,
        }
    }

    /// `folder` with the year of the document put back where its files had
    /// it: the most common place, the earliest on a tie.
    pub fn with_year(&self, folder: PathBuf, fields: &DocumentFields) -> PathBuf {
        let Some(date) = fields.date else {
            return folder;
        };
        let Some(place) = self.year_places.get(&folder).and_then(|places| {
            places
                .iter()
                .max_by_key(|(place, count)| (**count, std::cmp::Reverse(**place)))
                .map(|(place, _)| *place)
        }) else {
            return folder;
        };
        let mut segments: Vec<std::ffi::OsString> =
            folder.iter().map(std::ffi::OsStr::to_os_string).collect();
        segments.insert(place.min(segments.len()), date.year().to_string().into());
        segments.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use ncrs_index::{ContentHash, DocumentFields};

    use super::*;

    fn file(path: &str) -> PlanFile {
        PlanFile {
            relative_path: PathBuf::from(path),
            hash: ContentHash([0; 32]),
            fields: DocumentFields::default(),
            text: None,
            recorded_at: 0,
        }
    }

    fn dated(year: i32) -> DocumentFields {
        DocumentFields {
            date: NaiveDate::from_ymd_opt(year, 5, 1),
            ..DocumentFields::default()
        }
    }

    #[test]
    fn the_year_goes_back_where_the_files_had_it() {
        let list = FolderList::of(&[
            file("Steuer/2022/Musterstadt/a.pdf"),
            file("Bank/Giro/2021/b.pdf"),
            file("Bank/Giro/2020/c.pdf"),
            file("Post/d.pdf"),
        ]);

        assert_eq!(
            list.with_year(PathBuf::from("Steuer/Musterstadt"), &dated(2020)),
            PathBuf::from("Steuer/2020/Musterstadt")
        );
        assert_eq!(
            list.with_year(PathBuf::from("Bank/Giro"), &dated(2020)),
            PathBuf::from("Bank/Giro/2020")
        );
        assert_eq!(
            list.with_year(PathBuf::from("Post"), &dated(2020)),
            PathBuf::from("Post")
        );
    }

    #[test]
    fn a_huge_archive_is_shown_with_its_fullest_folders() {
        let mut files: Vec<PlanFile> = (0..MAX_FOLDERS + 50)
            .map(|number| file(&format!("Folder{number:04}/a.pdf")))
            .collect();
        files.extend((0..5).map(|number| file(&format!("Folder0449/{number}.pdf"))));

        let list = FolderList::of(&files);

        assert_eq!(list.entries.len(), MAX_FOLDERS);
        assert!(list.entries.iter().any(|(name, _)| name == "Folder0449"));
        let names: Vec<&String> = list.entries.iter().map(|(name, _)| name).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }
}
