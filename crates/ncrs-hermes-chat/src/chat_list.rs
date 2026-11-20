//! The list of chats: which ones exist, which one is shown, what is marked,
//! searched, renamed or about to be deleted. Plain data and rules; the
//! conversation itself stays in `State`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use chrono::{Local, NaiveDate, TimeZone as _};

use crate::chat_store::{self, ChatId, ChatStatus, Line, Speaker, StoredChat};

const SECONDS_PER_DAY: i64 = 86_400;
const TITLE_CHARS: usize = 48;
const ELLIPSIS: char = '\u{2026}';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayGroup {
    Today,
    Yesterday,
    Earlier,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListRow {
    pub id: ChatId,
    /// Empty for a chat that has no sentence of the user yet.
    pub title: String,
    pub status: ChatStatus,
    pub updated: i64,
    pub active: bool,
    pub marked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListGroup {
    pub day: DayGroup,
    pub rows: Vec<ListRow>,
}

/// The title a chat gets from the first sentence of the user: its first
/// line, shortened.
pub fn title_from(sentence: &str) -> String {
    let flat = sentence.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= TITLE_CHARS {
        return flat;
    }
    let mut short: String = flat.chars().take(TITLE_CHARS - 1).collect();
    short.push(ELLIPSIS);
    short
}

pub fn day_group(updated: i64, today: NaiveDate) -> DayGroup {
    let Some(day) = Local
        .timestamp_opt(updated, 0)
        .single()
        .map(|moment| moment.date_naive())
    else {
        return DayGroup::Earlier;
    };
    if day >= today {
        DayGroup::Today
    } else if today.pred_opt() == Some(day) {
        DayGroup::Yesterday
    } else {
        DayGroup::Earlier
    }
}

#[derive(Debug, Default)]
pub struct ChatList {
    dir: Option<PathBuf>,
    chats: Vec<StoredChat>,
    /// The chat the running conversation of `State` belongs to. The others
    /// are shown to read only.
    live: Option<ChatId>,
    shown: Option<ChatId>,
    marked: BTreeSet<ChatId>,
    search: String,
    renaming: Option<(ChatId, String)>,
    menu: Option<ChatId>,
    /// Chats that wait for a yes before they are deleted.
    asking: Option<Vec<ChatId>>,
    keep_days: Option<u32>,
}

impl ChatList {
    /// Reads the chats saved in `dir`, and keeps saving there.
    pub fn attach(&mut self, dir: PathBuf, now: i64) {
        let live = self.live.clone();
        let mut saved = chat_store::load_all(&dir);
        saved.retain(|chat| Some(&chat.id) != live.as_ref());
        self.chats.retain(|chat| Some(&chat.id) == live.as_ref());
        self.chats.extend(saved);
        self.dir = Some(dir);
        self.purge_old(now);
    }

    pub fn set_keep_days(&mut self, days: Option<u32>) {
        self.keep_days = days;
    }

    pub fn keep_days(&self) -> Option<u32> {
        self.keep_days
    }

    /// Deletes the chats not used for as long as the setting says, except the
    /// running conversation.
    pub fn purge_old(&mut self, now: i64) -> Vec<ChatId> {
        let Some(days) = self.keep_days else {
            return Vec::new();
        };
        let limit = now.saturating_sub(i64::from(days) * SECONDS_PER_DAY);
        let old: Vec<ChatId> = self
            .chats
            .iter()
            .filter(|chat| chat.updated < limit && Some(&chat.id) != self.live.as_ref())
            .map(|chat| chat.id.clone())
            .collect();
        self.remove(&old);
        old
    }

    pub fn live(&self) -> Option<&ChatId> {
        self.live.as_ref()
    }

    pub fn shown(&self) -> Option<&ChatId> {
        self.shown.as_ref()
    }

    pub fn chat(&self, id: &ChatId) -> Option<&StoredChat> {
        self.chats.iter().find(|chat| &chat.id == id)
    }

    fn chat_mut(&mut self, id: &ChatId) -> Option<&mut StoredChat> {
        self.chats.iter_mut().find(|chat| &chat.id == id)
    }

    /// The chat on screen is an old one, to read only.
    pub fn is_read_only(&self) -> bool {
        self.shown.is_some() && self.shown != self.live
    }

    /// The chat on screen when it is not the running conversation.
    pub fn shown_stored(&self) -> Option<&StoredChat> {
        let id = self
            .shown
            .as_ref()
            .filter(|id| Some(*id) != self.live.as_ref())?;
        self.chat(id)
    }

    pub fn title_of_shown(&self) -> Option<&str> {
        self.shown
            .as_ref()
            .and_then(|id| self.chat(id))
            .map(|chat| chat.title.as_str())
            .filter(|title| !title.is_empty())
    }

    /// Starts the entry of a new conversation. A running conversation that
    /// has said nothing yet is used again.
    pub fn start_live(&mut self, now: i64) {
        let reusable = self
            .live
            .as_ref()
            .and_then(|id| self.chat(id))
            .is_some_and(|chat| !chat.has_user_line());
        if !reusable {
            let id = ChatId::from_time(now, |candidate| self.chat(candidate).is_some());
            self.chats.push(StoredChat {
                id: id.clone(),
                title: String::new(),
                created: now,
                updated: now,
                status: ChatStatus::Question,
                lines: Vec::new(),
            });
            self.live = Some(id);
        }
        self.shown.clone_from(&self.live);
        self.prune_empty();
    }

    pub fn forget_live(&mut self) {
        self.live = None;
    }

    /// Takes over what the running conversation says now and saves it.
    pub fn sync_live(&mut self, lines: Vec<Line>, status: ChatStatus, now: i64) {
        if self.live.is_none() {
            self.start_live(now);
        }
        let Some(id) = self.live.clone() else {
            return;
        };
        let Some(chat) = self.chat_mut(&id) else {
            return;
        };
        if chat.lines == lines && chat.status == status {
            return;
        }
        if chat.title.is_empty() {
            if let Some(first) = lines.iter().find(|line| line.speaker == Speaker::User) {
                chat.title = title_from(&first.text);
            }
        }
        chat.lines = lines;
        chat.status = status;
        chat.updated = now;
        self.save(&id);
    }

    fn save(&self, id: &ChatId) {
        let (Some(dir), Some(chat)) = (&self.dir, self.chat(id)) else {
            return;
        };
        if chat.has_user_line() {
            // A chat that cannot be saved stays in the list for this session.
            let _ = chat_store::write(dir, chat);
        }
    }

    /// Chats nobody wrote in are not kept once they are left.
    fn prune_empty(&mut self) {
        let (live, shown) = (self.live.clone(), self.shown.clone());
        self.chats.retain(|chat| {
            chat.has_user_line()
                || Some(&chat.id) == live.as_ref()
                || Some(&chat.id) == shown.as_ref()
        });
    }

    pub fn show(&mut self, id: &ChatId) {
        if self.chat(id).is_some() {
            self.shown = Some(id.clone());
            self.prune_empty();
        }
    }

    pub fn show_live(&mut self) {
        self.shown.clone_from(&self.live);
    }

    // ---- what the list shows ----

    pub fn search(&self) -> &str {
        &self.search
    }

    pub fn set_search(&mut self, text: String) {
        self.search = text;
    }

    fn matches_search(&self, chat: &StoredChat) -> bool {
        let wanted = self.search.trim().to_lowercase();
        wanted.is_empty()
            || chat.title.to_lowercase().contains(&wanted)
            || chat
                .lines
                .iter()
                .any(|line| line.text.to_lowercase().contains(&wanted))
    }

    /// The chats the list shows, newest first.
    fn visible(&self) -> Vec<&StoredChat> {
        let mut shown: Vec<&StoredChat> = self
            .chats
            .iter()
            .filter(|chat| chat.has_user_line() || Some(&chat.id) == self.shown.as_ref())
            .filter(|chat| self.matches_search(chat))
            .collect();
        shown.sort_by(|left, right| {
            right
                .updated
                .cmp(&left.updated)
                .then_with(|| right.id.cmp(&left.id))
        });
        shown
    }

    pub fn is_empty(&self) -> bool {
        self.chats.iter().all(|chat| !chat.has_user_line())
    }

    pub fn groups(&self, today: NaiveDate) -> Vec<ListGroup> {
        let mut groups: Vec<ListGroup> = Vec::new();
        for chat in self.visible() {
            let day = day_group(chat.updated, today);
            let row = ListRow {
                id: chat.id.clone(),
                title: chat.title.clone(),
                status: chat.status,
                updated: chat.updated,
                active: Some(&chat.id) == self.shown.as_ref(),
                marked: self.marked.contains(&chat.id),
            };
            match groups.last_mut() {
                Some(group) if group.day == day => group.rows.push(row),
                _ => groups.push(ListGroup {
                    day,
                    rows: vec![row],
                }),
            }
        }
        groups
    }

    /// Shows the chat `by` places further down the list (negative: up).
    pub fn step(&mut self, by: isize) {
        let ids: Vec<ChatId> = self.visible().iter().map(|chat| chat.id.clone()).collect();
        if ids.is_empty() {
            return;
        }
        let at = self
            .shown
            .as_ref()
            .and_then(|shown| ids.iter().position(|id| id == shown));
        let next = match at {
            Some(position) => position
                .saturating_add_signed(by)
                .min(ids.len().saturating_sub(1)),
            None => 0,
        };
        if let Some(id) = ids.get(next) {
            self.shown = Some(id.clone());
            self.prune_empty();
        }
    }

    // ---- marking, renaming, menu ----

    pub fn marked(&self) -> &BTreeSet<ChatId> {
        &self.marked
    }

    pub fn toggle_mark_shown(&mut self) {
        let Some(id) = self.shown.clone() else {
            return;
        };
        if !self.marked.remove(&id) {
            self.marked.insert(id);
        }
    }

    pub fn menu(&self) -> Option<&ChatId> {
        self.menu.as_ref()
    }

    pub fn toggle_menu(&mut self, id: ChatId) {
        self.menu = if self.menu.as_ref() == Some(&id) {
            None
        } else {
            Some(id)
        };
    }

    pub fn close_menu(&mut self) {
        self.menu = None;
    }

    pub fn renaming(&self) -> Option<(&ChatId, &str)> {
        self.renaming
            .as_ref()
            .map(|(id, draft)| (id, draft.as_str()))
    }

    pub fn start_rename(&mut self, id: &ChatId) {
        self.menu = None;
        if let Some(chat) = self.chat(id) {
            self.renaming = Some((id.clone(), chat.title.clone()));
        }
    }

    pub fn rename_input(&mut self, draft: String) {
        if let Some((_, current)) = self.renaming.as_mut() {
            *current = draft;
        }
    }

    pub fn cancel_rename(&mut self) {
        self.renaming = None;
    }

    /// Takes the typed title; an empty one leaves the old title.
    pub fn finish_rename(&mut self) {
        let Some((id, draft)) = self.renaming.take() else {
            return;
        };
        let title = draft.trim();
        if title.is_empty() {
            return;
        }
        if let Some(chat) = self.chat_mut(&id) {
            chat.title = title.to_owned();
        }
        self.save(&id);
    }

    // ---- deleting ----

    /// What the delete key and the button act on: the marked chats, or the
    /// one on screen when nothing is marked.
    pub fn delete_targets(&self) -> Vec<ChatId> {
        if self.marked.is_empty() {
            self.shown.iter().cloned().collect()
        } else {
            self.marked.iter().cloned().collect()
        }
    }

    pub fn done_chats(&self) -> Vec<ChatId> {
        self.chats
            .iter()
            .filter(|chat| chat.status == ChatStatus::Done && chat.has_user_line())
            .map(|chat| chat.id.clone())
            .collect()
    }

    /// Deletes `ids` at once, unless one of them keeps filing: then the list
    /// asks first and nothing is deleted yet. Returns what was deleted.
    pub fn request_delete(&mut self, ids: Vec<ChatId>) -> Vec<ChatId> {
        self.menu = None;
        let keeps_filing = ids
            .iter()
            .filter_map(|id| self.chat(id))
            .any(|chat| chat.status == ChatStatus::Running);
        if keeps_filing {
            self.asking = Some(ids);
            return Vec::new();
        }
        self.remove(&ids)
    }

    pub fn asking(&self) -> Option<&[ChatId]> {
        self.asking.as_deref()
    }

    /// How many of the chats that wait for a yes keep filing.
    pub fn asking_running(&self) -> usize {
        self.asking
            .iter()
            .flatten()
            .filter_map(|id| self.chat(id))
            .filter(|chat| chat.status == ChatStatus::Running)
            .count()
    }

    pub fn confirm_delete(&mut self) -> Vec<ChatId> {
        match self.asking.take() {
            Some(ids) => self.remove(&ids),
            None => Vec::new(),
        }
    }

    pub fn cancel_delete(&mut self) {
        self.asking = None;
    }

    /// Anything the user opened and can leave with Escape is closed.
    pub fn dismiss_open_thing(&mut self) -> bool {
        let open = self.renaming.is_some() || self.menu.is_some() || self.asking.is_some();
        self.renaming = None;
        self.menu = None;
        self.asking = None;
        open
    }

    fn remove(&mut self, ids: &[ChatId]) -> Vec<ChatId> {
        let existing: Vec<ChatId> = ids
            .iter()
            .filter(|id| self.chat(id).is_some())
            .cloned()
            .collect();
        for id in &existing {
            if let Some(dir) = &self.dir {
                chat_store::remove(dir, id);
            }
            self.marked.remove(id);
            if self.live.as_ref() == Some(id) {
                self.live = None;
            }
        }
        self.chats.retain(|chat| !existing.contains(&chat.id));
        if self
            .shown
            .as_ref()
            .is_some_and(|shown| self.chat(shown).is_none())
        {
            self.shown = self
                .live
                .clone()
                .or_else(|| self.visible().first().map(|chat| chat.id.clone()));
        }
        existing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = SECONDS_PER_DAY;

    fn at(day: u32, hour: u32) -> i64 {
        Local
            .with_ymd_and_hms(2031, 3, day, hour, 0, 0)
            .single()
            .expect("a local time")
            .timestamp()
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2031, 3, 20).expect("a date")
    }

    fn say(list: &mut ChatList, sentence: &str, status: ChatStatus, now: i64) {
        let lines = vec![
            Line {
                speaker: Speaker::Hermes,
                text: "Hello".to_owned(),
            },
            Line {
                speaker: Speaker::User,
                text: sentence.to_owned(),
            },
        ];
        list.sync_live(lines, status, now);
    }

    fn titles(list: &ChatList) -> Vec<String> {
        list.groups(today())
            .iter()
            .flat_map(|group| group.rows.iter().map(|row| row.title.clone()))
            .collect()
    }

    fn id_of(list: &ChatList, title: &str) -> ChatId {
        list.chats
            .iter()
            .find(|chat| chat.title == title)
            .map(|chat| chat.id.clone())
            .expect("a chat with that title")
    }

    fn with_three() -> ChatList {
        let mut list = ChatList::default();
        list.start_live(at(10, 9));
        say(
            &mut list,
            "Rechnungen einsortieren",
            ChatStatus::Done,
            at(10, 9),
        );
        list.start_live(at(19, 10));
        say(
            &mut list,
            "Downloads aufraeumen",
            ChatStatus::Done,
            at(19, 10),
        );
        list.start_live(at(20, 8));
        say(
            &mut list,
            "Scans laufend einsortieren",
            ChatStatus::Running,
            at(20, 8),
        );
        list
    }

    #[test]
    fn the_title_is_the_first_sentence_shortened() {
        let mut list = ChatList::default();
        let long =
            "Bitte raeume alle meine Rechnungen aus dem Downloads Ordner auf und sortiere sie";
        say(&mut list, long, ChatStatus::Question, at(20, 8));
        let title = list.chats[0].title.clone();
        assert_eq!(title.chars().count(), TITLE_CHARS);
        assert!(title.ends_with(ELLIPSIS));
        assert!(long.starts_with(title.trim_end_matches(ELLIPSIS)));
    }

    #[test]
    fn a_new_chat_keeps_the_old_one_and_an_unused_one_is_reused() {
        let mut list = ChatList::default();
        list.start_live(at(20, 8));
        say(&mut list, "First", ChatStatus::Question, at(20, 8));
        list.start_live(at(20, 9));
        assert_eq!(list.chats.len(), 2);
        list.start_live(at(20, 10));
        assert_eq!(list.chats.len(), 2, "the empty chat is used again");
        assert_eq!(titles(&list), vec!["", "First"]);
    }

    #[test]
    fn switching_shows_an_old_chat_read_only_and_back() {
        let mut list = with_three();
        let old = id_of(&list, "Downloads aufraeumen");
        assert!(!list.is_read_only());
        list.show(&old);
        assert!(list.is_read_only());
        assert_eq!(
            list.shown_stored().map(|chat| chat.title.as_str()),
            Some("Downloads aufraeumen")
        );
        list.show_live();
        assert!(!list.is_read_only());
    }

    #[test]
    fn stepping_goes_down_the_list_and_stops_at_the_ends() {
        let mut list = with_three();
        assert_eq!(list.title_of_shown(), Some("Scans laufend einsortieren"));
        list.step(1);
        assert_eq!(list.title_of_shown(), Some("Downloads aufraeumen"));
        list.step(1);
        list.step(1);
        assert_eq!(list.title_of_shown(), Some("Rechnungen einsortieren"));
        list.step(-1);
        list.step(-5);
        assert_eq!(list.title_of_shown(), Some("Scans laufend einsortieren"));
    }

    #[test]
    fn the_groups_follow_the_injected_day() {
        let list = with_three();
        let days: Vec<DayGroup> = list.groups(today()).iter().map(|group| group.day).collect();
        assert_eq!(
            days,
            vec![DayGroup::Today, DayGroup::Yesterday, DayGroup::Earlier]
        );
        let later = NaiveDate::from_ymd_opt(2031, 3, 21).expect("a date");
        let later_days: Vec<DayGroup> = list.groups(later).iter().map(|group| group.day).collect();
        assert_eq!(later_days, vec![DayGroup::Yesterday, DayGroup::Earlier]);
    }

    #[test]
    fn a_chat_is_renamed_and_an_empty_title_changes_nothing() {
        let mut list = with_three();
        let id = id_of(&list, "Downloads aufraeumen");
        list.start_rename(&id);
        list.rename_input("  Downloads  ".to_owned());
        list.finish_rename();
        assert_eq!(
            list.chat(&id).map(|chat| chat.title.as_str()),
            Some("Downloads")
        );
        list.start_rename(&id);
        list.rename_input("   ".to_owned());
        list.finish_rename();
        assert_eq!(
            list.chat(&id).map(|chat| chat.title.as_str()),
            Some("Downloads")
        );
    }

    #[test]
    fn marked_chats_are_deleted_together() {
        let mut list = with_three();
        let first = id_of(&list, "Rechnungen einsortieren");
        let second = id_of(&list, "Downloads aufraeumen");
        list.show(&first);
        list.toggle_mark_shown();
        list.show(&second);
        list.toggle_mark_shown();
        assert_eq!(list.marked().len(), 2);
        let removed = list.request_delete(list.delete_targets());
        assert_eq!(removed.len(), 2);
        assert_eq!(titles(&list), vec!["Scans laufend einsortieren"]);
        assert!(list.marked().is_empty());
    }

    #[test]
    fn all_finished_chats_are_deleted_and_the_running_one_stays() {
        let mut list = with_three();
        let removed = list.request_delete(list.done_chats());
        assert_eq!(removed.len(), 2);
        assert_eq!(titles(&list), vec!["Scans laufend einsortieren"]);
    }

    #[test]
    fn a_running_chat_asks_before_it_is_deleted() {
        let mut list = with_three();
        let running = id_of(&list, "Scans laufend einsortieren");
        assert!(list.request_delete(vec![running.clone()]).is_empty());
        assert_eq!(list.asking(), Some(&[running.clone()][..]));
        assert_eq!(list.asking_running(), 1);
        assert_eq!(titles(&list).len(), 3);
        list.cancel_delete();
        assert_eq!(titles(&list).len(), 3);
        list.request_delete(vec![running.clone()]);
        assert_eq!(list.confirm_delete(), vec![running]);
        assert_eq!(titles(&list).len(), 2);
    }

    #[test]
    fn old_chats_are_deleted_by_their_updated_time() {
        let mut list = with_three();
        list.start_live(at(20, 9));
        list.set_keep_days(None);
        assert!(list.purge_old(at(20, 12)).is_empty());
        list.set_keep_days(Some(30));
        assert!(list.purge_old(at(20, 12)).is_empty());
        list.set_keep_days(Some(5));
        let removed = list.purge_old(at(20, 12));
        assert_eq!(
            removed.len(),
            1,
            "only the chat of the 10th is older than 5 days"
        );
        assert_eq!(titles(&list).len(), 3, "two chats and the empty new one");
        assert!(at(20, 12) - at(10, 9) > 5 * DAY);
    }

    #[test]
    fn the_search_looks_in_titles_and_messages() {
        let mut list = with_three();
        list.set_search("SCANS".to_owned());
        assert_eq!(titles(&list), vec!["Scans laufend einsortieren"]);
        let mut lines = list.chats[0].lines.clone();
        lines.push(Line {
            speaker: Speaker::Hermes,
            text: "Steuer 2025 fehlt".to_owned(),
        });
        list.chats[0].lines = lines;
        list.set_search("steuer".to_owned());
        assert_eq!(titles(&list), vec!["Rechnungen einsortieren"]);
        list.set_search("nothing like it".to_owned());
        assert!(titles(&list).is_empty());
    }

    #[test]
    fn chats_survive_a_restart_through_the_folder() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut list = ChatList::default();
        list.attach(dir.path().join("chats"), at(20, 8));
        list.start_live(at(20, 8));
        say(&mut list, "Rechnungen", ChatStatus::PlanOpen, at(20, 8));
        list.start_live(at(20, 9));
        let mut restarted = ChatList::default();
        restarted.attach(dir.path().join("chats"), at(21, 8));
        assert_eq!(titles(&restarted), vec!["Rechnungen"]);
        assert_eq!(
            restarted.chats.first().map(|chat| chat.status),
            Some(ChatStatus::PlanOpen)
        );
    }
}
