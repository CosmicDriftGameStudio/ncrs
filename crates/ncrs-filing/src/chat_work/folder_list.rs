//! The folders of the target as the model sees them: a list without the year
//! folders, each with a few names of files lying in it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::Datelike;
use ncrs_index::DocumentFields;
use ncrs_intake::PlanFile;

/// File names shown per folder.
const EXAMPLES: usize = 3;
const EXAMPLE_CHARS: usize = 40;

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
    year_places: BTreeMap<PathBuf, BTreeMap<usize, usize>>,
}

impl FolderList {
    pub fn of(files: &[PlanFile]) -> Self {
        let mut names: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
        let mut year_places: BTreeMap<PathBuf, BTreeMap<usize, usize>> = BTreeMap::new();
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
            }
            if let (false, Some(name)) =
                (kept.as_os_str().is_empty(), file.relative_path.file_name())
            {
                let name = name.to_string_lossy();
                if !name.starts_with('.') {
                    names.entry(kept).or_default().push(name.into_owned());
                }
            }
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
            file("Steuer/2022/Leipzig/a.pdf"),
            file("Bank/Giro/2021/b.pdf"),
            file("Bank/Giro/2020/c.pdf"),
            file("Post/d.pdf"),
        ]);

        assert_eq!(
            list.with_year(PathBuf::from("Steuer/Leipzig"), &dated(2020)),
            PathBuf::from("Steuer/2020/Leipzig")
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
}
