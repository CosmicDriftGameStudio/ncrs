//! What the list of chats shows: the rows with their status and time, the
//! menu of a row and the footer.

use chrono::{DateTime, Local, TimeZone};
use ncrs_i18n::{Language, Msg};
use ncrs_ui_kit::hermes_kit::{
    ChatListFooter, ChatListGroup, ChatListProps, ChatListRename, ChatListRow,
};
use ncrs_ui_kit::popup_menu::PopupEntry;

use crate::chat_list::{DayGroup, ListRow};
use crate::chat_list_update::ListMessage;
use crate::chat_store::{ChatId, ChatStatus};
use crate::message::Message;
use crate::state::State;
use crate::texts::fill;
use crate::view::ViewCtx;

const SEARCH_ID: &str = "hermes-chat-search";
const RENAME_ID: &str = "hermes-chat-rename";

fn list(message: ListMessage) -> Message {
    Message::List(message)
}

/// The key that goes with the letter or arrow of a chat key.
fn modifier_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "\u{2318}"
    } else {
        "Ctrl+"
    }
}

pub fn keys_line(lang: Language) -> String {
    let keys = fill(
        lang,
        Msg::HermesChatListKeysHint,
        &[("mod", modifier_name())],
    );
    format!("{keys} \u{b7} {}", lang.text(Msg::HermesChatListMarkHint))
}

fn status_text(lang: Language, status: ChatStatus) -> &'static str {
    lang.text(match status {
        ChatStatus::PlanOpen => Msg::HermesChatStatusPlanOpen,
        ChatStatus::Done => Msg::HermesChatStatusDone,
        ChatStatus::Question => Msg::HermesChatStatusQuestion,
        ChatStatus::Running => Msg::HermesChatStatusRunning,
    })
}

/// The time of day for a chat of today, else the date.
fn when_text(lang: Language, moment: DateTime<Local>, day: DayGroup) -> String {
    let pattern = match (day, lang.base()) {
        (DayGroup::Today, _) => "%H:%M",
        (_, Language::German) => "%d.%m.",
        _ => "%m/%d",
    };
    moment.format(pattern).to_string()
}

fn row_detail(lang: Language, row: &ListRow, day: DayGroup) -> String {
    let when = Local
        .timestamp_opt(row.updated, 0)
        .single()
        .map(|moment| when_text(lang, moment, day))
        .unwrap_or_default();
    fill(
        lang,
        Msg::HermesChatListDetail,
        &[("status", status_text(lang, row.status)), ("when", &when)],
    )
}

fn menu_entries(lang: Language, id: &ChatId) -> Vec<PopupEntry<Message>> {
    let entry = |label: Msg, message: ListMessage| PopupEntry {
        label: lang.text(label).to_owned(),
        key: None,
        is_selected: false,
        on_press: Some(list(message)),
        on_hover: None,
    };
    vec![
        entry(Msg::HermesChatMenuRename, ListMessage::Rename(id.clone())),
        entry(
            Msg::HermesChatMenuDelete,
            ListMessage::DeleteOne(id.clone()),
        ),
        entry(Msg::HermesChatMenuDeleteDone, ListMessage::DeleteDone),
    ]
}

fn footer(state: &State, lang: Language) -> Option<ChatListFooter<'static, Message>> {
    let chats = &state.chats;
    if let Some(asked) = chats.asking() {
        let text = match asked {
            [only] => fill(
                lang,
                Msg::HermesChatAskDeleteRunning,
                &[(
                    "title",
                    chats.chat(only).map_or("", |chat| chat.title.as_str()),
                )],
            ),
            _ => fill(
                lang,
                Msg::HermesChatAskDeleteRunningMany,
                &[("count", &chats.asking_running().to_string())],
            ),
        };
        return Some(ChatListFooter::Ask {
            text,
            yes_label: lang.text(Msg::HermesChatListDelete),
            on_yes: list(ListMessage::ConfirmDelete),
            no_label: lang.text(Msg::HermesChatDeleteRunningKeep),
            on_no: list(ListMessage::KeepChats),
        });
    }
    (!chats.marked().is_empty()).then(|| ChatListFooter::Marked {
        text: fill(
            lang,
            Msg::HermesChatListMarked,
            &[("count", &chats.marked().len().to_string())],
        ),
        delete_label: lang.text(Msg::HermesChatListDelete),
        on_delete: list(ListMessage::DeleteMarked),
    })
}

pub fn list_props(state: &State, ctx: ViewCtx<'_>) -> ChatListProps<'static, Message> {
    let lang = ctx.lang;
    let chats = &state.chats;
    let today = Local::now().date_naive();
    let groups = chats
        .groups(today)
        .into_iter()
        .map(|group| {
            let heading = lang.text(match group.day {
                DayGroup::Today => Msg::HermesChatListToday,
                DayGroup::Yesterday => Msg::HermesChatListYesterday,
                DayGroup::Earlier => Msg::HermesChatListEarlier,
            });
            let rows = group
                .rows
                .iter()
                .map(|row| {
                    let title = if row.title.is_empty() {
                        lang.text(Msg::HermesChatNewChat).to_owned()
                    } else {
                        row.title.clone()
                    };
                    let rename =
                        chats
                            .renaming()
                            .filter(|(id, _)| **id == row.id)
                            .map(|(_, draft)| ChatListRename {
                                draft: draft.to_owned(),
                                id: RENAME_ID,
                                on_input: |draft| list(ListMessage::RenameInput(draft)),
                                on_submit: list(ListMessage::RenameSubmit),
                            });
                    let menu = if chats.menu() == Some(&row.id) {
                        menu_entries(lang, &row.id)
                    } else {
                        Vec::new()
                    };
                    ChatListRow {
                        title,
                        detail: row_detail(lang, row, group.day),
                        active: row.active,
                        marked: row.marked,
                        on_press: list(ListMessage::Show(row.id.clone())),
                        on_right_press: list(ListMessage::OpenMenu(row.id.clone())),
                        rename,
                        menu,
                    }
                })
                .collect();
            ChatListGroup {
                heading: heading.to_owned(),
                rows,
            }
        })
        .collect();
    ChatListProps {
        new_label: lang.text(Msg::HermesChatListNew),
        new_key: fill(lang, Msg::HermesChatListNewKey, &[("mod", modifier_name())]),
        on_new: Message::NewChat,
        search_placeholder: lang.text(Msg::HermesChatListSearch),
        search: chats.search().to_owned(),
        search_id: SEARCH_ID,
        on_search: |text| list(ListMessage::Search(text)),
        on_search_submit: list(ListMessage::Search(chats.search().to_owned())),
        groups,
        empty_note: lang.text(if chats.search().trim().is_empty() {
            Msg::HermesChatListEmpty
        } else {
            Msg::HermesChatListNoMatch
        }),
        footer: footer(state, lang),
    }
}
