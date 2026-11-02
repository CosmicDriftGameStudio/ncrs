use std::collections::BTreeMap;

use ncrs_index::KindCatalog;

use super::branches::expand;
use super::name_scheme::{learn_scheme, MIN_SENDERS_OF_A_DUMP};
use super::tree::{Node, NodeFile};
use super::types::{Area, LocationStructure, NamePart, StructureDocument, UnorderedFolder};

/// The structure of every area of a location.
pub fn learn_structure(documents: &[StructureDocument], kinds: &KindCatalog) -> LocationStructure {
    let mut tree: BTreeMap<String, Node<'_>> = BTreeMap::new();
    let mut root_files: usize = 0;
    for document in documents {
        let Some((folders, file)) = NodeFile::split(document) else {
            continue;
        };
        match folders.split_first() {
            None => root_files += 1,
            Some((area, below)) => tree
                .entry(area.clone())
                .or_insert_with(Node::new)
                .insert(below, file),
        }
    }

    let mut areas = Vec::new();
    let mut unordered = Vec::new();
    if root_files > 0 {
        unordered.push(UnorderedFolder {
            folder: String::new(),
            documents: count(root_files),
        });
    }
    for (folder, node) in &tree {
        if node.children.is_empty() && is_a_dump(node) {
            unordered.push(UnorderedFolder {
                folder: folder.clone(),
                documents: count(node.documents.len()),
            });
            continue;
        }
        let mut branches = expand(&[node], kinds, folder);
        branches.sort_by_key(|branch| std::cmp::Reverse(branch.documents));
        areas.push(Area {
            folder: folder.clone(),
            documents: count(node.documents.len()),
            branches,
        });
    }
    areas.sort_by(|left, right| {
        right
            .documents
            .cmp(&left.documents)
            .then_with(|| left.folder.cmp(&right.folder))
    });
    unordered.sort_by(|left, right| left.folder.cmp(&right.folder));
    LocationStructure { areas, unordered }
}

/// A flat folder without a name scheme most files follow, or with a scheme of
/// numbers and free text only for letters from several senders.
fn is_a_dump(node: &Node<'_>) -> bool {
    let files: Vec<&NodeFile<'_>> = node.files.iter().collect();
    let (scheme, _following) = learn_scheme(&files);
    let Some(found) = scheme else {
        return true;
    };
    let only_counting = found.parts.iter().all(|part| {
        matches!(
            part,
            NamePart::Separator(_) | NamePart::Number | NamePart::Free
        )
    });
    let mut senders: Vec<String> = node
        .documents
        .iter()
        .filter_map(|document| document.sender.as_deref())
        .map(str::to_lowercase)
        .collect();
    senders.sort();
    senders.dedup();
    only_counting && senders.len() >= MIN_SENDERS_OF_A_DUMP
}

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
