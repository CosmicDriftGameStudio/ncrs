use std::collections::BTreeMap;
use std::path::Component;

use super::types::StructureDocument;
use crate::archive_profile::{name_tokens, NameToken};

/// A file with its name split into tokens.
pub(in super) struct NodeFile<'a> {
    pub tokens: Vec<NameToken>,
    pub document: &'a StructureDocument,
}

impl<'a> NodeFile<'a> {
    /// The folders of the document's path and the file itself; `None` for a
    /// path that cannot be a document of the location.
    pub fn split(document: &'a StructureDocument) -> Option<(Vec<String>, Self)> {
        let mut folders = Vec::new();
        for component in document.relative_path.parent()?.components() {
            let Component::Normal(part) = component else {
                return None;
            };
            folders.push(part.to_str()?.to_owned());
        }
        let stem = document.relative_path.file_stem()?.to_str()?;
        Some((
            folders,
            Self {
                tokens: name_tokens(stem),
                document,
            },
        ))
    }
}

/// A folder with everything below it.
pub(in super) struct Node<'a> {
    pub children: BTreeMap<String, Node<'a>>,
    pub files: Vec<NodeFile<'a>>,
    /// Every document below, including those of the subfolders.
    pub documents: Vec<&'a StructureDocument>,
}

impl<'a> Node<'a> {
    pub fn new() -> Self {
        Self {
            children: BTreeMap::new(),
            files: Vec::new(),
            documents: Vec::new(),
        }
    }

    pub fn insert(&mut self, folders: &[String], file: NodeFile<'a>) {
        self.documents.push(file.document);
        match folders.split_first() {
            None => self.files.push(file),
            Some((first, rest)) => self
                .children
                .entry(first.clone())
                .or_insert_with(Node::new)
                .insert(rest, file),
        }
    }
}
