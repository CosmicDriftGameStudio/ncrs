//! State of the reviews page: the documents that wait for a decision, grouped
//! by the question Hermes asks about them, and which groups are unfolded. No
//! widgets and no tasks; the view is `ui::reviews`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use ncrs_filing::review_work::{ReviewItem, TargetComparison};
use ncrs_index::{DocumentId, Field, Kinds, ReviewReason};

/// What Hermes asks about a document. Documents with the same question are
/// answered together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Question {
    /// Hermes is not sure who sent it.
    Sender,
    /// There is no date in it.
    Date,
    /// The place the rules chose is taken by a file.
    TargetTaken,
    /// A rule suggests a folder that the user has not confirmed.
    RuleUnconfirmed,
    /// No rule fits.
    NoRule,
    Missing(Field),
    Unsure(Field),
}

impl Question {
    pub fn of(reason: ReviewReason) -> Self {
        match reason {
            ReviewReason::NoMatchingRule => Self::NoRule,
            ReviewReason::UnconfirmedRule => Self::RuleUnconfirmed,
            ReviewReason::TargetExists => Self::TargetTaken,
            ReviewReason::MissingField(Field::Date) => Self::Date,
            ReviewReason::MissingField(field) => Self::Missing(field),
            ReviewReason::LowConfidence(Field::Sender) => Self::Sender,
            ReviewReason::LowConfidence(field) => Self::Unsure(field),
        }
    }
}

/// What a person can do about a question, for one file or for a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// File where Hermes suggests; for a missing field, without that field.
    Accept,
    /// Open the chat, which asks for what is missing.
    SetFields,
    /// A group: unfold it and put the cursor in the input of its first file.
    EnterValue,
    /// A file: file it with the date or sender typed in its input.
    FileTyped,
    /// Open the rule form on the first document.
    CreateRule,
    /// Hermes leaves the file alone for good.
    Exclude,
    /// The target holds the same content: the incoming file goes to the trash.
    DeleteDuplicate,
    /// The old file of the target goes to the trash, the new one takes its place.
    Overwrite,
    /// The new file is filed next to the old one under the next free number.
    KeepBoth,
    /// Ask for a folder and file there.
    PickFolder,
}

impl Question {
    /// The answers of a group, the first is the main one.
    fn group_answers(self) -> &'static [Answer] {
        match self {
            Self::Sender | Self::Date => &[Answer::Accept, Answer::EnterValue, Answer::Exclude],
            Self::Unsure(_) | Self::Missing(_) => {
                &[Answer::Accept, Answer::SetFields, Answer::Exclude]
            }
            Self::TargetTaken => &[
                Answer::DeleteDuplicate,
                Answer::Overwrite,
                Answer::KeepBoth,
                Answer::Accept,
                Answer::Exclude,
            ],
            Self::RuleUnconfirmed => &[Answer::CreateRule, Answer::Accept, Answer::Exclude],
            Self::NoRule => &[Answer::PickFolder, Answer::SetFields, Answer::Exclude],
        }
    }

    /// The answers of one file: a date or a sender is typed in the row.
    fn file_answers(self) -> &'static [Answer] {
        match self {
            Self::Sender => &[Answer::FileTyped, Answer::Exclude],
            Self::Date => &[Answer::FileTyped, Answer::Accept, Answer::Exclude],
            _ => self.group_answers(),
        }
    }

    /// Whether the question is answered by typing a value in the row.
    pub fn is_typed(self) -> bool {
        matches!(self, Self::Sender | Self::Date)
    }
}

/// How the file in the place of a document compares, for the questions about
/// a taken place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TakenBy {
    /// Not looked at yet.
    Unknown,
    SameContent,
    OtherContent,
    /// Nothing lies in the place any more.
    Free,
}

/// The value typed for a document, as it files: a day for the date question,
/// a name for the sender question. `None` while the text is no valid value.
pub fn typed_item(question: Question, item: &ReviewItem, text: &str) -> Option<ReviewItem> {
    let mut typed = item.clone();
    match question {
        Question::Date => typed.fields.date = Some(ncrs_intake::parse_date(text)?),
        Question::Sender => {
            let sender = text.trim();
            if sender.is_empty() {
                return None;
            }
            typed.fields.sender = Some(sender.to_owned());
        }
        _ => return None,
    }
    Some(typed)
}

#[derive(Debug, Clone, PartialEq)]
enum Reading {
    Loading,
    Loaded,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
struct QuestionGroup {
    question: Question,
    /// Positions in the items.
    members: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewsPage {
    reading: Reading,
    items: Vec<ReviewItem>,
    /// Where each item would be filed, parallel to the items.
    targets: Vec<Option<PathBuf>>,
    groups: Vec<QuestionGroup>,
    unfolded: HashSet<Question>,
    /// The files in the places of the documents whose place is taken. A
    /// document with `None` has nothing in its place any more.
    comparisons: HashMap<DocumentId, Option<TargetComparison>>,
    /// The documents that wait for the folder the user is picking.
    awaiting_folder: Vec<ReviewItem>,
    /// What was typed in the inputs of the rows, by document.
    inputs: HashMap<DocumentId, String>,
}

impl ReviewsPage {
    pub fn new() -> Self {
        Self {
            reading: Reading::Loading,
            items: Vec::new(),
            targets: Vec::new(),
            groups: Vec::new(),
            unfolded: HashSet::new(),
            comparisons: HashMap::new(),
            awaiting_folder: Vec::new(),
            inputs: HashMap::new(),
        }
    }

    /// Reading again after documents were filed: what is shown stays until
    /// the answer comes.
    pub fn loading_again(&mut self) {
        self.reading = Reading::Loading;
    }

    pub fn loaded(&mut self, items: Vec<ReviewItem>, kinds: &Kinds) {
        self.targets = items
            .iter()
            .map(|item| item.suggested_target(kinds))
            .collect();
        self.groups = group_by_question(&items);
        self.items = items;
        self.comparisons.clear();
        self.reading = Reading::Loaded;
    }

    pub fn failed(&mut self, detail: String) {
        self.reading = Reading::Failed(detail);
    }

    pub fn is_loading(&self) -> bool {
        self.reading == Reading::Loading
    }

    pub fn problem(&self) -> Option<&str> {
        match &self.reading {
            Reading::Failed(detail) => Some(detail),
            Reading::Loading | Reading::Loaded => None,
        }
    }

    pub fn total(&self) -> usize {
        self.items.len()
    }

    /// The questions to show: most documents first, "no rule fits" last.
    pub fn questions(&self) -> impl Iterator<Item = Question> + '_ {
        self.groups.iter().map(|group| group.question)
    }

    fn positions(&self, question: Question) -> &[usize] {
        self.groups
            .iter()
            .find(|group| group.question == question)
            .map_or(&[], |group| group.members.as_slice())
    }

    pub fn members(&self, question: Question) -> Vec<&ReviewItem> {
        self.positions(question)
            .iter()
            .filter_map(|position| self.items.get(*position))
            .collect()
    }

    /// The members with where each would be filed.
    pub fn members_with_target(&self, question: Question) -> Vec<(&ReviewItem, Option<&Path>)> {
        self.positions(question)
            .iter()
            .filter_map(|position| {
                let item = self.items.get(*position)?;
                Some((item, self.targets.get(*position)?.as_deref()))
            })
            .collect()
    }

    /// The incoming file and the path below the archive it would take, of the
    /// documents whose place is taken: what to look at in the background.
    pub fn taken_to_compare(&self) -> Vec<(DocumentId, PathBuf, PathBuf)> {
        self.members_with_target(Question::TargetTaken)
            .into_iter()
            .filter_map(|(item, target)| {
                Some((item.document_id, item.path.clone(), target?.to_path_buf()))
            })
            .collect()
    }

    pub fn compared(&mut self, found: Vec<(DocumentId, Option<TargetComparison>)>) {
        self.comparisons.extend(found);
    }

    /// The two files of a document whose place is taken, once looked at and
    /// when something still lies in the place.
    pub fn comparison(&self, document: DocumentId) -> Option<&TargetComparison> {
        self.comparisons.get(&document)?.as_ref()
    }

    pub fn taken_by(&self, document: DocumentId) -> TakenBy {
        match self.comparisons.get(&document) {
            None => TakenBy::Unknown,
            Some(None) => TakenBy::Free,
            Some(Some(found)) if found.same_content => TakenBy::SameContent,
            Some(Some(_)) => TakenBy::OtherContent,
        }
    }

    /// Whether `answer` fits this document: filing needs a suggestion, and
    /// for a taken place what lies there decides.
    fn offers(
        &self,
        question: Question,
        answer: Answer,
        item: &ReviewItem,
        target: Option<&Path>,
    ) -> bool {
        let taken = self.taken_by(item.document_id);
        let has_target = target.is_some();
        match answer {
            Answer::Accept => {
                has_target && (question != Question::TargetTaken || taken == TakenBy::Free)
            }
            Answer::DeleteDuplicate => has_target && taken == TakenBy::SameContent,
            Answer::Overwrite | Answer::KeepBoth => has_target && taken == TakenBy::OtherContent,
            Answer::SetFields
            | Answer::EnterValue
            | Answer::FileTyped
            | Answer::CreateRule
            | Answer::Exclude
            | Answer::PickFolder => true,
        }
    }

    /// The answers of a group with how many of its files each one fits; those
    /// that fit no file are left out.
    pub fn group_answers(&self, question: Question) -> Vec<(Answer, usize)> {
        let members = self.members_with_target(question);
        question
            .group_answers()
            .iter()
            .map(|&answer| {
                let fits = members
                    .iter()
                    .filter(|(item, target)| self.offers(question, answer, item, *target))
                    .count();
                (answer, fits)
            })
            .filter(|(_, fits)| *fits > 0)
            .collect()
    }

    /// The answers of one file.
    pub fn file_answers(&self, question: Question, document: DocumentId) -> Vec<Answer> {
        let Some((item, target)) = self
            .members_with_target(question)
            .into_iter()
            .find(|(item, _)| item.document_id == document)
        else {
            return Vec::new();
        };
        question
            .file_answers()
            .iter()
            .copied()
            .filter(|&answer| self.offers(question, answer, item, target))
            .collect()
    }

    /// The documents an answer acts on: those of the group, or the one
    /// `document`, that the answer fits.
    pub fn answer_items(
        &self,
        question: Question,
        answer: Answer,
        document: Option<DocumentId>,
    ) -> Vec<ReviewItem> {
        self.members_with_target(question)
            .into_iter()
            .filter(|(item, _)| document.is_none_or(|wanted| wanted == item.document_id))
            .filter(|(item, target)| self.offers(question, answer, item, *target))
            .map(|(item, _)| item.clone())
            .collect()
    }

    /// What is typed in the input of a document: the date, or the sender,
    /// which starts as what Hermes read.
    pub fn input_of(&self, question: Question, item: &ReviewItem) -> String {
        self.inputs
            .get(&item.document_id)
            .cloned()
            .unwrap_or_else(|| match question {
                Question::Sender => item.fields.sender.clone().unwrap_or_default(),
                _ => String::new(),
            })
    }

    pub fn type_into(&mut self, document: DocumentId, text: String) {
        self.inputs.insert(document, text);
    }

    /// The first file of the group that is answered by typing.
    pub fn first_input(&self, question: Question) -> Option<DocumentId> {
        question
            .is_typed()
            .then(|| self.members(question).first().map(|item| item.document_id))
            .flatten()
    }

    /// The document with its typed value, as it files; `None` while the text
    /// is no valid value.
    pub fn typed_for(&self, question: Question, document: DocumentId) -> Option<ReviewItem> {
        let item = self
            .members(question)
            .into_iter()
            .find(|item| item.document_id == document)?;
        typed_item(question, item, &self.input_of(question, item))
    }

    /// The members that have a suggestion, which is what accepting files.
    pub fn fileable(&self, question: Question) -> Vec<ReviewItem> {
        self.answer_items(question, Answer::Accept, None)
    }

    /// The folder most members would be filed in, and how many other folders
    /// the rest would land in. `None` when no member has a suggestion.
    pub fn main_folder(&self, question: Question) -> Option<(&Path, usize)> {
        let mut counted: Vec<(&Path, usize)> = Vec::new();
        for (_, target) in self.members_with_target(question) {
            let Some(folder) = target.and_then(Path::parent) else {
                continue;
            };
            match counted.iter_mut().find(|(known, _)| *known == folder) {
                Some((_, count)) => *count += 1,
                None => counted.push((folder, 1)),
            }
        }
        let others = counted.len().checked_sub(1)?;
        let (folder, _) =
            counted
                .into_iter()
                .max_by(|(first, first_count), (second, second_count)| {
                    first_count
                        .cmp(second_count)
                        .then_with(|| second.cmp(first))
                })?;
        Some((folder, others))
    }

    /// The sender of the members when they all have the same one.
    pub fn single_sender(&self, question: Question) -> Option<&str> {
        let mut senders = self
            .members(question)
            .into_iter()
            .map(|item| item.fields.sender.as_deref());
        let first = senders.next()??;
        senders.all(|sender| sender == Some(first)).then_some(first)
    }

    pub fn is_unfolded(&self, question: Question) -> bool {
        self.unfolded.contains(&question)
    }

    pub fn toggle(&mut self, question: Question) {
        if !self.unfolded.remove(&question) {
            self.unfolded.insert(question);
        }
    }

    /// The documents left the list: Hermes leaves them alone from now on.
    pub fn remove(&mut self, documents: &HashSet<DocumentId>) {
        let kept: Vec<(ReviewItem, Option<PathBuf>)> = std::mem::take(&mut self.items)
            .into_iter()
            .zip(std::mem::take(&mut self.targets))
            .filter(|(item, _)| !documents.contains(&item.document_id))
            .collect();
        (self.items, self.targets) = kept.into_iter().unzip();
        self.groups = group_by_question(&self.items);
    }

    pub fn await_folder(&mut self, items: Vec<ReviewItem>) {
        self.awaiting_folder = items;
    }

    /// The documents that waited for the picked folder.
    pub fn take_awaiting_folder(&mut self) -> Vec<ReviewItem> {
        std::mem::take(&mut self.awaiting_folder)
    }
}

/// Splits `items` by their question, the biggest group first and the one
/// where no rule fits last.
fn group_by_question(items: &[ReviewItem]) -> Vec<QuestionGroup> {
    let mut groups: Vec<QuestionGroup> = Vec::new();
    for (position, item) in items.iter().enumerate() {
        let question = Question::of(item.reason);
        match groups.iter_mut().find(|group| group.question == question) {
            Some(group) => group.members.push(position),
            None => groups.push(QuestionGroup {
                question,
                members: vec![position],
            }),
        }
    }
    groups.sort_by_key(|group| {
        (
            group.question == Question::NoRule,
            std::cmp::Reverse(group.members.len()),
        )
    });
    groups
}

impl Default for ReviewsPage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, TimeZone as _, Utc};
    use ncrs_filing::review_work::{FileFacts, TargetComparison, TextState};
    use ncrs_index::{
        ContentHash, DocumentFields, DocumentSource, Index, KindLanguage, NewDocument,
    };

    use super::*;

    fn items_of(reason: ReviewReason, count: u8) -> Vec<ReviewItem> {
        let dir = tempfile::TempDir::new().unwrap();
        let mut index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        (1..=count)
            .map(|seed| {
                let document = index
                    .insert_document(&NewDocument {
                        hash: ContentHash([seed; 32]),
                        path: dir.path().join(format!("scan-{seed}.pdf")),
                        source: DocumentSource::Inbox,
                        received_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
                        stamp: None,
                    })
                    .unwrap();
                let entry = index.add_review(document, reason).unwrap();
                ReviewItem {
                    entry,
                    document_id: document,
                    path: PathBuf::from(format!("inbox/scan-{seed}.pdf")),
                    reason,
                    text_state: TextState::Read,
                    fields: DocumentFields {
                        sender: Some("Company A".to_owned()),
                        kind: Some("invoice".to_owned()),
                        ..DocumentFields::default()
                    },
                    proposed_pattern: Some("{sender}/{kind}".to_owned()),
                    origins: ncrs_index::FieldOrigins::default(),
                    passages: Vec::new(),
                    placement: None,
                }
            })
            .collect()
    }

    fn page_of(items: Vec<ReviewItem>) -> ReviewsPage {
        let mut page = ReviewsPage::new();
        page.loaded(items, &Kinds::built_in(KindLanguage::English));
        page
    }

    fn comparison(same_content: bool) -> Option<TargetComparison> {
        let facts = FileFacts {
            size: 10,
            modified: None,
        };
        Some(TargetComparison {
            incoming: facts,
            existing: facts,
            same_content,
        })
    }

    #[test]
    fn a_typed_date_files_the_document_with_that_date() {
        let items = items_of(ReviewReason::MissingField(Field::Date), 1);
        let item = &items[0];
        let typed = typed_item(Question::Date, item, "07.03.2025").unwrap();
        assert_eq!(typed.fields.date, NaiveDate::from_ymd_opt(2025, 3, 7));
        assert!(typed_item(Question::Date, item, "31.02.2025").is_none());
        assert!(typed_item(Question::Date, item, "").is_none());
    }

    #[test]
    fn a_typed_sender_files_the_document_with_that_sender() {
        let items = items_of(ReviewReason::LowConfidence(Field::Sender), 1);
        let page = page_of(items.clone());
        let document = items[0].document_id;
        assert_eq!(page.input_of(Question::Sender, &items[0]), "Company A");
        let mut page = page;
        page.type_into(document, "  Agency B ".to_owned());
        let typed = page.typed_for(Question::Sender, document).unwrap();
        assert_eq!(typed.fields.sender.as_deref(), Some("Agency B"));
        page.type_into(document, "  ".to_owned());
        assert!(page.typed_for(Question::Sender, document).is_none());
    }

    #[test]
    fn a_taken_group_says_how_many_files_each_answer_hits() {
        let items = items_of(ReviewReason::TargetExists, 4);
        let documents: Vec<_> = items.iter().map(|item| item.document_id).collect();
        let mut page = page_of(items);
        page.compared(vec![
            (documents[0], comparison(true)),
            (documents[1], comparison(true)),
            (documents[2], comparison(false)),
            (documents[3], None),
        ]);

        assert_eq!(
            page.group_answers(Question::TargetTaken),
            [
                (Answer::DeleteDuplicate, 2),
                (Answer::Overwrite, 1),
                (Answer::KeepBoth, 1),
                (Answer::Accept, 1),
                (Answer::Exclude, 4),
            ]
        );
        assert_eq!(
            page.file_answers(Question::TargetTaken, documents[3]),
            [Answer::Accept, Answer::Exclude]
        );
        assert_eq!(
            page.file_answers(Question::TargetTaken, documents[0]),
            [Answer::DeleteDuplicate, Answer::Exclude]
        );
    }
}
