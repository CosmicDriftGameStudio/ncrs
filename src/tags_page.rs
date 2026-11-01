//! State of the tags page: every tag with the documents that have it, in
//! groups that fold, which of them the file list shows, and the steps of
//! renaming and deleting an own tag. No widgets and no tasks; `App` owns it and
//! changes it from `update`, the view is `ui::tags`.

use iced::mouse::ScrollDelta;
use ncrs_index::{tag_name, Kinds, RuleSpec};

use crate::file_list_tags::{FileListTags, TagGroup};
use crate::list_window::ListWindow;
use crate::tags_work::{TagsFailure, TagsLoaded};
use crate::ui::kit::Check;

/// Most rows of the table shown at once; the table follows the cursor.
pub const VISIBLE_ROWS: usize = 14;

/// A group with more tags than this shows [`SHOWN_OF_LONG_GROUP`] of them and
/// a link for the rest.
const SHORT_GROUP: usize = 3;
const SHOWN_OF_LONG_GROUP: usize = 2;

/// The groups in the order of the page.
pub const PAGE_GROUPS: [TagGroup; 5] = [
    TagGroup::Own,
    TagGroup::Kind,
    TagGroup::Sender,
    TagGroup::Recipient,
    TagGroup::Year,
];

const GROUPS_CLOSED_AT_FIRST: [TagGroup; 3] =
    [TagGroup::Sender, TagGroup::Recipient, TagGroup::Year];

/// One row of the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagRow {
    Group(TagGroup),
    Tag {
        group: TagGroup,
        /// As the index spells it; what the exceptions are keyed by.
        tag: String,
        documents: u64,
    },
    /// The link behind a long group.
    More {
        group: TagGroup,
        rest: usize,
    },
}

/// The step that waits for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagStep {
    Rename { tag: String, text: String },
    ConfirmDelete(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagsProblem {
    /// Rules ask for this tag, so it stays.
    InUse { tag: String, rules: Vec<RuleSpec> },
    /// The work on the index failed; the technical detail stays English.
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct TagsPage {
    loaded: Option<TagsLoaded>,
    closed: Vec<TagGroup>,
    unfolded: Vec<TagGroup>,
    search: String,
    cursor: usize,
    window: ListWindow,
    step: Option<TagStep>,
    /// The tag whose deletion is running.
    deleting: Option<String>,
    problem: Option<TagsProblem>,
    /// A change is running; the page takes no new one.
    busy: bool,
    kinds: Kinds,
}

impl TagsPage {
    pub fn new(kinds: Kinds) -> Self {
        Self {
            loaded: None,
            closed: GROUPS_CLOSED_AT_FIRST.to_vec(),
            unfolded: Vec::new(),
            search: String::new(),
            cursor: 0,
            window: ListWindow::default(),
            step: None,
            deleting: None,
            problem: None,
            busy: true,
            kinds,
        }
    }

    pub fn set_kinds(&mut self, kinds: Kinds) {
        self.kinds = kinds;
    }

    pub fn kinds(&self) -> &Kinds {
        &self.kinds
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded.is_some()
    }

    pub fn has_tags(&self) -> bool {
        PAGE_GROUPS
            .into_iter()
            .any(|group| !self.tags_of(group).is_empty())
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    pub fn begin_change(&mut self) {
        self.busy = true;
    }

    pub fn search(&self) -> &str {
        &self.search
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn first_visible(&self) -> usize {
        self.window.offset(self.rows().len(), VISIBLE_ROWS)
    }

    pub fn step(&self) -> Option<&TagStep> {
        self.step.as_ref()
    }

    pub fn problem(&self) -> Option<&TagsProblem> {
        self.problem.as_ref()
    }

    pub fn is_open(&self, group: TagGroup) -> bool {
        !self.closed.contains(&group)
    }

    /// Typing in the fields takes the keys: the search and the new name.
    pub fn is_editing(&self) -> bool {
        matches!(self.step, Some(TagStep::Rename { .. }))
    }

    fn tags_of(&self, group: TagGroup) -> &[(String, u64)] {
        let Some(loaded) = &self.loaded else {
            return &[];
        };
        match group {
            TagGroup::Own => &loaded.tally.own,
            TagGroup::Kind => &loaded.tally.kind,
            TagGroup::Sender => &loaded.tally.sender,
            TagGroup::Recipient => &loaded.tally.recipient,
            TagGroup::Year => &loaded.tally.year,
        }
    }

    /// How many tags the group has.
    pub fn tag_count(&self, group: TagGroup) -> usize {
        self.tags_of(group).len()
    }

    /// The tag as the page and the file list word it.
    pub fn label(&self, group: TagGroup, tag: &str) -> String {
        match group {
            TagGroup::Kind => self.kinds.kind_tag_word(tag),
            TagGroup::Own | TagGroup::Sender | TagGroup::Recipient | TagGroup::Year => {
                tag.to_owned()
            }
        }
    }

    fn matches_search(&self, group: TagGroup, tag: &str) -> bool {
        let wanted = self.search.trim().to_lowercase();
        wanted.is_empty()
            || tag.contains(&wanted)
            || self.label(group, tag).to_lowercase().contains(&wanted)
    }

    fn is_searching(&self) -> bool {
        !self.search.trim().is_empty()
    }

    /// What the table shows now: the groups, and below the open ones their
    /// tags. A search shows the tags it finds in every group.
    pub fn rows(&self) -> Vec<TagRow> {
        let searching = self.is_searching();
        let mut rows = Vec::new();
        for group in PAGE_GROUPS {
            let tags: Vec<&(String, u64)> = self
                .tags_of(group)
                .iter()
                .filter(|(tag, _)| self.matches_search(group, tag))
                .collect();
            if searching && tags.is_empty() {
                continue;
            }
            rows.push(TagRow::Group(group));
            if !searching && !self.is_open(group) {
                continue;
            }
            let all = searching || self.unfolded.contains(&group) || tags.len() <= SHORT_GROUP;
            let shown = if all { tags.len() } else { SHOWN_OF_LONG_GROUP };
            rows.extend(tags.iter().take(shown).map(|(tag, documents)| TagRow::Tag {
                group,
                tag: tag.clone(),
                documents: *documents,
            }));
            if !all {
                rows.push(TagRow::More {
                    group,
                    rest: tags.len().saturating_sub(shown),
                });
            }
        }
        rows
    }

    pub fn selected(&self) -> Option<TagRow> {
        self.rows().into_iter().nth(self.cursor)
    }

    /// Whether the file list shows the tag: its exception, else its group.
    pub fn tag_check(&self, setting: &FileListTags, group: TagGroup, tag: &str) -> Check {
        let shown = self
            .loaded
            .as_ref()
            .and_then(|loaded| loaded.exceptions.get(tag).copied())
            .unwrap_or_else(|| setting.has_group(group));
        if shown {
            Check::On
        } else {
            Check::Off
        }
    }

    /// The default of the group; mixed when tags of it are exceptions.
    pub fn group_check(&self, setting: &FileListTags, group: TagGroup) -> Check {
        let has_exception = self.loaded.as_ref().is_some_and(|loaded| {
            self.tags_of(group)
                .iter()
                .any(|(tag, _)| loaded.exceptions.contains_key(tag))
        });
        if has_exception {
            Check::Mixed
        } else if setting.has_group(group) {
            Check::On
        } else {
            Check::Off
        }
    }

    /// The exception to store when the tag is switched: none when the new
    /// state is the default of its group.
    pub fn exception_after_toggle(
        &self,
        setting: &FileListTags,
        group: TagGroup,
        tag: &str,
    ) -> Option<bool> {
        let now = self.tag_check(setting, group, tag) == Check::On;
        let default = setting.has_group(group);
        (!now != default).then_some(!now)
    }

    /// The tags of the group that have an exception.
    pub fn exceptional_tags(&self, group: TagGroup) -> Vec<String> {
        let Some(loaded) = &self.loaded else {
            return Vec::new();
        };
        self.tags_of(group)
            .iter()
            .filter(|(tag, _)| loaded.exceptions.contains_key(tag))
            .map(|(tag, _)| tag.clone())
            .collect()
    }

    /// The group is switched on unless it is on whole already.
    pub fn group_will_be_on(&self, setting: &FileListTags, group: TagGroup) -> bool {
        self.group_check(setting, group) != Check::On
    }

    /// What the work returned; a step that was up is over.
    pub fn loaded(&mut self, loaded: TagsLoaded) {
        self.loaded = Some(loaded);
        self.busy = false;
        self.step = None;
        self.deleting = None;
        self.problem = None;
        self.keep_cursor_in_table();
    }

    pub fn failed(&mut self, failure: TagsFailure) {
        self.busy = false;
        self.step = None;
        self.problem = Some(match failure {
            TagsFailure::InUse(rules) => TagsProblem::InUse {
                tag: self.deleting.take().unwrap_or_default(),
                rules,
            },
            TagsFailure::Other(detail) => TagsProblem::Failed(detail),
        });
    }

    fn keep_cursor_in_table(&mut self) {
        let len = self.rows().len();
        self.cursor = self.cursor.min(len.saturating_sub(1));
        self.window.ensure_visible(self.cursor, len, VISIBLE_ROWS);
    }

    pub fn move_cursor(&mut self, delta: isize) {
        self.select(self.cursor.saturating_add_signed(delta));
    }

    pub fn select(&mut self, index: usize) {
        let len = self.rows().len();
        if self.step.is_some() || len == 0 {
            return;
        }
        self.cursor = index.min(len.saturating_sub(1));
        self.window.ensure_visible(self.cursor, len, VISIBLE_ROWS);
        self.problem = None;
    }

    /// The wheel moves the window; the cursor follows it when it would leave.
    pub fn scroll(&mut self, delta: ScrollDelta) {
        let len = self.rows().len();
        let mut cursor = self.cursor;
        self.window.scroll(delta, len, VISIBLE_ROWS, &mut cursor);
        self.cursor = cursor;
    }

    pub fn toggle_open(&mut self, group: TagGroup) {
        if let Some(position) = self.closed.iter().position(|known| *known == group) {
            self.closed.remove(position);
        } else {
            self.closed.push(group);
        }
        self.keep_cursor_in_table();
    }

    pub fn unfold(&mut self, group: TagGroup) {
        if !self.unfolded.contains(&group) {
            self.unfolded.push(group);
        }
        self.keep_cursor_in_table();
    }

    pub fn set_search(&mut self, text: String) {
        self.search = text;
        self.cursor = 0;
        self.window = ListWindow::default();
    }

    pub fn start_rename(&mut self, tag: String) {
        if self.busy {
            return;
        }
        self.problem = None;
        self.step = Some(TagStep::Rename {
            text: tag.clone(),
            tag,
        });
    }

    pub fn set_rename_text(&mut self, typed: String) {
        if let Some(TagStep::Rename { text, .. }) = &mut self.step {
            *text = typed;
        }
    }

    /// Enter in the field: the tag and the name to make of it, unless the name
    /// stays or is empty.
    pub fn begin_rename(&mut self) -> Option<(String, String)> {
        let Some(TagStep::Rename { tag, text }) = &self.step else {
            return None;
        };
        let (old_name, new_name) = (tag.clone(), tag_name(text));
        if self.busy || new_name.is_empty() || new_name == old_name {
            self.step = None;
            return None;
        }
        self.busy = true;
        Some((old_name, new_name))
    }

    pub fn ask_delete(&mut self, tag: String) {
        if self.busy {
            return;
        }
        self.problem = None;
        self.step = Some(TagStep::ConfirmDelete(tag));
    }

    pub fn begin_delete(&mut self) -> Option<String> {
        let Some(TagStep::ConfirmDelete(tag)) = &self.step else {
            return None;
        };
        let name = tag.clone();
        if self.busy {
            return None;
        }
        self.busy = true;
        self.deleting = Some(name.clone());
        Some(name)
    }

    /// Escape backs out of the step that is up first. Whether there was one.
    pub fn cancel_step(&mut self) -> bool {
        self.step.take().is_some()
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
pub mod tests {
    use ncrs_index::{KindLanguage, TagTally};

    use super::*;
    use crate::file_list_tags::TagExceptions;

    fn tags(names: &[&str]) -> Vec<(String, u64)> {
        names.iter().map(|name| ((*name).to_owned(), 3)).collect()
    }

    pub fn loaded_page() -> TagsPage {
        let mut page = TagsPage::new(Kinds::built_in(KindLanguage::English));
        page.loaded(TagsLoaded {
            tally: TagTally {
                own: tags(&["steuer", "offen", "privat"]),
                kind: tags(&["invoice", "contract", "decision"]),
                sender: tags(&["telekom", "stadtwerke", "bank", "versicherung"]),
                recipient: tags(&["firma-a-gmbh"]),
                year: tags(&["2026", "2025"]),
            },
            exceptions: TagExceptions::from([("decision".to_owned(), false)]),
        });
        page
    }

    fn groups_and_tags(page: &TagsPage) -> Vec<String> {
        page.rows()
            .into_iter()
            .map(|row| match row {
                TagRow::Group(group) => format!("group {}", group.name()),
                TagRow::Tag { tag, .. } => tag,
                TagRow::More { rest, .. } => format!("{rest} more"),
            })
            .collect()
    }

    #[test]
    fn the_first_groups_are_open_and_the_others_folded() {
        assert_eq!(
            groups_and_tags(&loaded_page()),
            [
                "group own",
                "steuer",
                "offen",
                "privat",
                "group kind",
                "invoice",
                "contract",
                "decision",
                "group sender",
                "group recipient",
                "group year",
            ]
        );
    }

    #[test]
    fn a_long_group_shows_two_tags_and_the_rest_behind_a_link() {
        let mut page = loaded_page();
        page.toggle_open(TagGroup::Sender);

        let rows = groups_and_tags(&page);
        assert!(rows
            .windows(4)
            .any(|part| part == ["group sender", "telekom", "stadtwerke", "2 more"]));

        page.unfold(TagGroup::Sender);
        assert!(groups_and_tags(&page).contains(&"versicherung".to_owned()));
    }

    #[test]
    fn a_search_lists_the_matches_of_every_group_even_in_folded_ones() {
        let mut page = loaded_page();
        page.set_search("tele".to_owned());
        assert_eq!(groups_and_tags(&page), ["group sender", "telekom"]);

        page.set_search("invoice".to_owned());
        assert_eq!(groups_and_tags(&page), ["group kind", "invoice"]);
    }

    #[test]
    fn an_exception_makes_the_group_mixed_and_flips_only_its_tag() {
        let page = loaded_page();
        let setting = FileListTags::default();

        assert_eq!(page.group_check(&setting, TagGroup::Kind), Check::Mixed);
        assert_eq!(page.group_check(&setting, TagGroup::Own), Check::On);
        assert_eq!(page.group_check(&setting, TagGroup::Sender), Check::Off);
        assert_eq!(
            page.tag_check(&setting, TagGroup::Kind, "decision"),
            Check::Off
        );
        assert_eq!(
            page.tag_check(&setting, TagGroup::Kind, "invoice"),
            Check::On
        );
    }

    #[test]
    fn switching_a_tag_back_to_its_group_default_removes_the_exception() {
        let page = loaded_page();
        let setting = FileListTags::default();

        assert_eq!(
            page.exception_after_toggle(&setting, TagGroup::Kind, "decision"),
            None
        );
        assert_eq!(
            page.exception_after_toggle(&setting, TagGroup::Kind, "invoice"),
            Some(false)
        );
        assert_eq!(
            page.exception_after_toggle(&setting, TagGroup::Sender, "telekom"),
            Some(true)
        );
    }

    #[test]
    fn renaming_keeps_the_page_still_when_the_name_stays_or_is_empty() {
        let mut page = loaded_page();
        page.start_rename("steuer".to_owned());
        assert!(page.is_editing());
        assert_eq!(page.begin_rename(), None);
        assert!(!page.is_editing());

        page.start_rename("steuer".to_owned());
        page.set_rename_text("Steuer 2026".to_owned());
        assert_eq!(
            page.begin_rename(),
            Some(("steuer".to_owned(), "steuer-2026".to_owned()))
        );
        assert!(page.is_busy());
    }

    #[test]
    fn escape_backs_out_of_a_step_and_a_failure_shows_in_the_page() {
        let mut page = loaded_page();
        page.ask_delete("steuer".to_owned());
        assert!(page.cancel_step());
        assert!(!page.cancel_step());

        page.failed(TagsFailure::Other("disk".to_owned()));
        assert_eq!(
            page.problem(),
            Some(&TagsProblem::Failed("disk".to_owned()))
        );
    }
}
