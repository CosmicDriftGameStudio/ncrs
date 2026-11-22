//! State of the reviews page: the documents that wait for a decision, grouped
//! by the question Hermes asks about them, and which groups are unfolded or
//! put off. No widgets and no tasks; the view is `ui::reviews`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use ncrs_filing::review_work::ReviewItem;
use ncrs_index::{Field, Kinds, ReviewReason};

/// What Hermes asks about a document. Documents with the same question are
/// answered together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Question {
    /// Hermes is not sure who sent it.
    Sender,
    /// There is no date in it.
    Date,
    /// The place the rules chose is taken by an equal file.
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

/// An answer a group offers; each one is an action the quick-fix menu knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// File the documents that have a suggestion.
    Accept,
    /// Open the chat, which asks for what is missing.
    SetFields,
    /// Open the rule form on the first document of the group.
    CreateRule,
    Exclude,
    /// Hide the group until the page is opened again.
    Later,
}

impl Question {
    /// The answers of the group, the first is the main one. `can_file` is
    /// whether any document of the group has a suggestion.
    pub fn answers(self, can_file: bool) -> Vec<Answer> {
        let accept = can_file.then_some(Answer::Accept);
        let answers = match self {
            Self::Sender | Self::Unsure(_) => {
                vec![accept, Some(Answer::SetFields), Some(Answer::Later)]
            }
            Self::Date | Self::Missing(_) => vec![Some(Answer::SetFields), Some(Answer::Later)],
            Self::TargetTaken => vec![Some(Answer::Exclude), Some(Answer::Later)],
            Self::RuleUnconfirmed => {
                vec![Some(Answer::CreateRule), accept, Some(Answer::Later)]
            }
            Self::NoRule => vec![Some(Answer::SetFields)],
        };
        answers.into_iter().flatten().collect()
    }
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
    put_off: HashSet<Question>,
}

impl ReviewsPage {
    pub fn new() -> Self {
        Self {
            reading: Reading::Loading,
            items: Vec::new(),
            targets: Vec::new(),
            groups: Vec::new(),
            unfolded: HashSet::new(),
            put_off: HashSet::new(),
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

    /// The documents of the groups that are not put off.
    pub fn total(&self) -> usize {
        self.questions()
            .map(|question| self.positions(question).len())
            .sum()
    }

    /// The questions to show: most documents first, "no rule fits" last.
    pub fn questions(&self) -> impl Iterator<Item = Question> + '_ {
        self.groups
            .iter()
            .map(|group| group.question)
            .filter(|question| !self.put_off.contains(question))
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

    /// The members that have a suggestion, which is what accepting files.
    pub fn fileable(&self, question: Question) -> Vec<ReviewItem> {
        self.members_with_target(question)
            .into_iter()
            .filter(|(_, target)| target.is_some())
            .map(|(item, _)| item.clone())
            .collect()
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

    pub fn put_off(&mut self, question: Question) {
        self.put_off.insert(question);
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
