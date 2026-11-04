//! The documents no rule could place: counted, grouped by the folder they
//! would go to, and filed by the same journaled move as the review list.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::ChatAccess;
use crate::review_work::{AcceptSummary, ReviewAccess, ReviewItem};

const NO_REVIEW_ACCESS: &str = "the waiting documents are not reachable";

/// Waiting documents that go to one folder below the target, or, with no
/// folder, one document nothing is sure about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingGroup {
    pub folder: Option<PathBuf>,
    pub items: Vec<ReviewItem>,
}

fn review(access: &ChatAccess) -> Result<&ReviewAccess, String> {
    access
        .review
        .as_ref()
        .ok_or_else(|| NO_REVIEW_ACCESS.to_owned())
}

pub(in crate::chat_work) fn count(access: &ChatAccess) -> Result<usize, String> {
    review(access)?.waiting_count()
}

/// The sure groups first, the biggest first; the unsure documents last, one
/// group each.
pub(in crate::chat_work) fn load(access: &ChatAccess) -> Result<Vec<WaitingGroup>, String> {
    let mut by_folder: BTreeMap<PathBuf, Vec<ReviewItem>> = BTreeMap::new();
    let mut unsure = Vec::new();
    for (item, target) in review(access)?.load_with_folders()? {
        match target {
            Some(folder) => by_folder.entry(folder).or_default().push(item),
            None => unsure.push(WaitingGroup {
                folder: None,
                items: vec![item],
            }),
        }
    }
    let mut groups: Vec<WaitingGroup> = by_folder
        .into_iter()
        .map(|(folder, items)| WaitingGroup {
            folder: Some(folder),
            items,
        })
        .collect();
    groups.sort_by_key(|group| std::cmp::Reverse(group.items.len()));
    groups.extend(unsure);
    Ok(groups)
}

pub(in crate::chat_work) fn file(
    access: &ChatAccess,
    items: &[ReviewItem],
) -> Result<AcceptSummary, String> {
    Ok(review(access)?.accept(items))
}

pub(in crate::chat_work) fn file_into(
    access: &ChatAccess,
    items: &[ReviewItem],
    folder: &Path,
) -> Result<AcceptSummary, String> {
    Ok(review(access)?.accept_into(items, folder))
}
