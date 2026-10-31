//! The quick-fix menu of the Hermes column (Cmd+.): what can be done about the
//! inquiries under the cursor or among the marked files, grouped by where their
//! suggestion would file them. State only; `App` runs the work, the view is
//! `ui::quick_fix`.

use std::path::{Path, PathBuf};

use iced::{Point, Size};
use ncrs_index::{Field, Kinds, ReviewReason};

use crate::context_menu::place;
use crate::review_list::FormField;
use crate::review_work::ReviewItem;
use crate::ui::layout::{MENU_ITEM_HEIGHT, MENU_PADDING, QUICK_FIX_HEADER_HEIGHT, QUICK_FIX_WIDTH};

/// What an entry does. The state only knows which one is highlighted; the app
/// carries it out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickFixAction {
    /// File the members of this group under their suggestion.
    Accept(usize),
    /// Open the review page on the documents without a suggestion (or the one
    /// document) to fill in a field.
    SetFields,
    CreateRule,
    Later,
    Exclude,
    /// The summary of a group is up and this answers it.
    FileGroup(usize),
    Back,
}

/// One line of the menu with what the view needs to word it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub action: QuickFixAction,
    /// How many documents it acts on.
    pub count: usize,
    /// The target directory of a group, or the target path of a single file.
    pub target: Option<PathBuf>,
}

/// The documents whose suggestion lands in one directory below the archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub directory: PathBuf,
    /// Positions in the items.
    pub members: Vec<usize>,
}

/// Splits `items` by the directory their suggestion files them in, the biggest
/// group first, and lists those that have no suggestion.
pub fn group_by_target(items: &[ReviewItem], kinds: &Kinds) -> (Vec<Group>, Vec<usize>) {
    let mut groups: Vec<Group> = Vec::new();
    let mut without = Vec::new();
    for (position, item) in items.iter().enumerate() {
        let Some(target) = item.suggested_target(kinds) else {
            without.push(position);
            continue;
        };
        let directory = target.parent().map(Path::to_path_buf).unwrap_or_default();
        match groups.iter_mut().find(|group| group.directory == directory) {
            Some(group) => group.members.push(position),
            None => groups.push(Group {
                directory,
                members: vec![position],
            }),
        }
    }
    groups.sort_by(|first, second| {
        second
            .members
            .len()
            .cmp(&first.members.len())
            .then_with(|| first.directory.cmp(&second.directory))
    });
    (groups, without)
}

/// The form field to put the cursor in for a document: the one its reason
/// names, else the first of sender, kind and date that is still empty.
pub fn field_to_fix(item: &ReviewItem) -> FormField {
    if let ReviewReason::MissingField(field) | ReviewReason::LowConfidence(field) = item.reason {
        match field {
            Field::Sender => return FormField::Sender,
            Field::Recipient => return FormField::Recipient,
            Field::Kind => return FormField::Kind,
            Field::Date => return FormField::Date,
            Field::Number => return FormField::Number,
            Field::Amount => return FormField::Amount,
        }
    }
    let fields = &item.fields;
    if fields.sender.is_none() {
        FormField::Sender
    } else if fields.kind.is_none() {
        FormField::Kind
    } else if fields.date.is_none() {
        FormField::Date
    } else {
        FormField::Sender
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Stage {
    Loading,
    Menu,
    /// The summary before a group is filed.
    Summary(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub struct QuickFix {
    anchor: Point,
    window: Size,
    stage: Stage,
    items: Vec<ReviewItem>,
    groups: Vec<Group>,
    without: Vec<usize>,
    selected: usize,
    kinds: Kinds,
}

impl QuickFix {
    /// Opens loading: the reviews of `anchor`'s files come from the index.
    pub fn new(anchor: Point, window: Size) -> Self {
        Self {
            anchor,
            window,
            stage: Stage::Loading,
            items: Vec::new(),
            groups: Vec::new(),
            without: Vec::new(),
            selected: 0,
            kinds: Kinds::default(),
        }
    }

    pub fn with_kinds(self, kinds: Kinds) -> Self {
        Self { kinds, ..self }
    }

    pub fn kinds(&self) -> &Kinds {
        &self.kinds
    }

    pub fn items(&self) -> &[ReviewItem] {
        &self.items
    }

    /// The group whose summary is up.
    pub fn summarised_group(&self) -> Option<&Group> {
        match self.stage {
            Stage::Summary(group) => self.groups.get(group),
            Stage::Loading | Stage::Menu => None,
        }
    }

    /// The reviews the work returned.
    pub fn loaded(&mut self, items: Vec<ReviewItem>) {
        (self.groups, self.without) = group_by_target(&items, &self.kinds);
        self.items = items;
        self.stage = Stage::Menu;
        self.selected = 0;
    }

    pub fn entries(&self) -> Vec<Entry> {
        match self.stage {
            Stage::Loading => Vec::new(),
            Stage::Summary(group) => self.summary_entries(group),
            Stage::Menu => self.menu_entries(),
        }
    }

    fn summary_entries(&self, group: usize) -> Vec<Entry> {
        let count = self
            .groups
            .get(group)
            .map_or(0, |found| found.members.len());
        vec![
            Entry {
                action: QuickFixAction::FileGroup(group),
                count,
                target: None,
            },
            Entry {
                action: QuickFixAction::Back,
                count: 0,
                target: None,
            },
        ]
    }

    fn menu_entries(&self) -> Vec<Entry> {
        let single = self.items.len() == 1;
        let mut entries: Vec<Entry> = self
            .groups
            .iter()
            .enumerate()
            .map(|(position, group)| Entry {
                action: QuickFixAction::Accept(position),
                count: group.members.len(),
                target: if single {
                    self.items
                        .first()
                        .and_then(|item| item.suggested_target(&self.kinds))
                } else {
                    Some(group.directory.clone())
                },
            })
            .collect();
        if !self.without.is_empty() {
            entries.push(Entry {
                action: QuickFixAction::SetFields,
                count: self.without.len(),
                target: None,
            });
        } else if single {
            entries.push(Entry {
                action: QuickFixAction::SetFields,
                count: 1,
                target: None,
            });
        }
        if single {
            entries.push(Entry {
                action: QuickFixAction::CreateRule,
                count: 1,
                target: None,
            });
        }
        let all = self.items.len();
        for action in [QuickFixAction::Later, QuickFixAction::Exclude] {
            entries.push(Entry {
                action,
                count: all,
                target: None,
            });
        }
        entries
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn selected_action(&self) -> Option<QuickFixAction> {
        self.entries().get(self.selected).map(|entry| entry.action)
    }

    pub fn select(&mut self, index: usize) {
        if index < self.entries().len() {
            self.selected = index;
        }
    }

    pub fn move_by(&mut self, delta: isize) {
        let last = self.entries().len().saturating_sub(1);
        self.selected = self.selected.saturating_add_signed(delta).min(last);
    }

    pub fn select_first(&mut self) {
        self.selected = 0;
    }

    pub fn select_last(&mut self) {
        self.selected = self.entries().len().saturating_sub(1);
    }

    /// Shows the summary of a group before anything is filed.
    pub fn summarise(&mut self, group: usize) {
        if group < self.groups.len() {
            self.stage = Stage::Summary(group);
            self.selected = 0;
        }
    }

    pub fn back(&mut self) {
        self.stage = Stage::Menu;
        self.selected = 0;
    }

    /// The items of a group, in the order of the index.
    pub fn group_items(&self, group: usize) -> Vec<ReviewItem> {
        self.groups.get(group).map_or_else(Vec::new, |found| {
            found
                .members
                .iter()
                .filter_map(|&position| self.items.get(position).cloned())
                .collect()
        })
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        self.items.iter().map(|item| item.path.clone()).collect()
    }

    /// The item to open the review page on, with the field to fill in.
    pub fn item_to_fix(&self) -> Option<&ReviewItem> {
        let position = self.without.first().copied().unwrap_or(0);
        self.items.get(position)
    }

    pub fn height(&self) -> f32 {
        let lines = self.entries().len().max(1);
        QUICK_FIX_HEADER_HEIGHT + lines as f32 * MENU_ITEM_HEIGHT + 2.0 * MENU_PADDING
    }

    /// Where the menu sits: below the row, folded up or left where it would
    /// leave the window.
    pub fn origin(&self) -> Point {
        Point::new(
            place(self.anchor.x, QUICK_FIX_WIDTH, self.window.width),
            place(self.anchor.y, self.height(), self.window.height),
        )
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::review_list::tests::loaded_list;
    use ncrs_index::KindLanguage;

    fn items() -> Vec<ReviewItem> {
        let list = loaded_list();
        let first = list.rows()[0].clone();
        let second = list.rows()[1].clone();
        let mut same_directory = second.clone();
        same_directory.fields.sender = first.fields.sender.clone();
        let mut without_rule = first.clone();
        without_rule.proposed_pattern = None;
        vec![first, same_directory, second, without_rule]
    }

    fn menu_of(items: Vec<ReviewItem>) -> QuickFix {
        let mut menu = QuickFix::new(Point::new(10.0, 10.0), Size::new(800.0, 600.0));
        menu.loaded(items);
        menu
    }

    #[test]
    fn documents_are_grouped_by_the_directory_of_their_target() {
        let all = items();
        let (groups, without) = group_by_target(&all, &Kinds::built_in(KindLanguage::English));
        assert_eq!(without, [3]);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].directory, PathBuf::from("Company A"));
        assert_eq!(groups[0].members, [0, 1]);
        assert_eq!(groups[1].directory, PathBuf::from("Agency B"));
        assert_eq!(groups[1].members, [2]);
    }

    #[test]
    fn several_files_offer_a_group_per_target_and_the_rest_to_fix() {
        let menu = menu_of(items());
        let actions: Vec<_> = menu.entries().iter().map(|e| (e.action, e.count)).collect();
        assert_eq!(
            actions,
            [
                (QuickFixAction::Accept(0), 2),
                (QuickFixAction::Accept(1), 1),
                (QuickFixAction::SetFields, 1),
                (QuickFixAction::Later, 4),
                (QuickFixAction::Exclude, 4),
            ]
        );
    }

    #[test]
    fn one_file_with_a_suggestion_offers_accept_first_with_its_path() {
        let menu = menu_of(vec![items().remove(0)]);
        let entries = menu.entries();
        assert_eq!(entries[0].action, QuickFixAction::Accept(0));
        assert_eq!(
            entries[0].target.as_deref(),
            Some(Path::new("Company A/2024-03-09_Invoice"))
        );
        assert_eq!(
            entries.iter().map(|e| e.action).collect::<Vec<_>>(),
            [
                QuickFixAction::Accept(0),
                QuickFixAction::SetFields,
                QuickFixAction::CreateRule,
                QuickFixAction::Later,
                QuickFixAction::Exclude,
            ]
        );
    }

    #[test]
    fn one_file_without_a_suggestion_starts_on_set_field() {
        let mut item = items().remove(0);
        item.proposed_pattern = None;
        let menu = menu_of(vec![item]);
        assert_eq!(menu.selected_action(), Some(QuickFixAction::SetFields));
    }

    #[test]
    fn the_keys_move_within_the_entries_and_stop_at_both_ends() {
        let mut menu = menu_of(items());
        assert_eq!(menu.selected_action(), Some(QuickFixAction::Accept(0)));
        menu.move_by(-1);
        assert_eq!(menu.selected(), 0);
        menu.move_by(1);
        assert_eq!(menu.selected_action(), Some(QuickFixAction::Accept(1)));
        menu.select_last();
        assert_eq!(menu.selected_action(), Some(QuickFixAction::Exclude));
        menu.move_by(1);
        assert_eq!(menu.selected_action(), Some(QuickFixAction::Exclude));
        menu.select_first();
        assert_eq!(menu.selected(), 0);
        menu.select(99);
        assert_eq!(menu.selected(), 0);
        menu.select(3);
        assert_eq!(menu.selected_action(), Some(QuickFixAction::Later));
    }

    #[test]
    fn a_group_is_summarised_before_it_is_filed_and_back_returns() {
        let mut menu = menu_of(items());
        menu.summarise(0);
        assert_eq!(
            menu.summarised_group().map(|group| group.members.len()),
            Some(2)
        );
        assert_eq!(menu.selected_action(), Some(QuickFixAction::FileGroup(0)));
        menu.move_by(1);
        assert_eq!(menu.selected_action(), Some(QuickFixAction::Back));
        menu.back();
        assert!(menu.summarised_group().is_none());
        assert_eq!(menu.selected_action(), Some(QuickFixAction::Accept(0)));
        assert_eq!(menu.group_items(0).len(), 2);
    }

    #[test]
    fn the_field_to_fix_is_the_one_the_reason_names_else_the_first_empty() {
        let mut item = items().remove(0);
        item.reason = ReviewReason::MissingField(Field::Date);
        assert_eq!(field_to_fix(&item), FormField::Date);
        item.reason = ReviewReason::NoMatchingRule;
        item.fields.kind = None;
        assert_eq!(field_to_fix(&item), FormField::Kind);
    }

    #[test]
    fn the_menu_stays_inside_the_window() {
        let mut menu = QuickFix::new(Point::new(790.0, 590.0), Size::new(800.0, 600.0));
        menu.loaded(items());
        let origin = menu.origin();
        assert!(origin.x + QUICK_FIX_WIDTH <= 800.0);
        assert!(origin.y + menu.height() <= 600.0);
    }
}
