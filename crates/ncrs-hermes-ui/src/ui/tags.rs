//! The tags page of the settings: every tag in groups that fold, with the tick
//! that shows it in the file lists and the number of documents that have it.
//! Renaming and deleting an own tag ask in a small dialog. Pure view over
//! [`TagsPage`].

use iced::widget::container;
use iced::{Element, Size};
use ncrs_config::file_list_tags::{FileListTags, TagGroup};
use ncrs_config::palette::Palette;
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_ui_kit::format;
use ncrs_ui_kit::frame::{DialogSize, Frame, KeyHints};
use ncrs_ui_kit::hermes_kit as page_kit;
use ncrs_ui_kit::kit::{self, Tone};
use ncrs_ui_kit::setting_rows::{switch, Group, RowButton, RowKey, SettingRow};

use crate::message::{Message, TagsMessage};
use crate::rules_page::rule_sentence;
use crate::tags_page::{TagRow, TagStep, TagsPage, TagsProblem};

pub const RENAME_ID: &str = "tags-rename";
pub const SEARCH_ID: &str = "tags-search";

const RULE_CHARS: usize = 40;
const RENAME_WIDTH: f32 = 220.0;

fn send(message: TagsMessage) -> Message {
    Message::Tags(message)
}

/// The sections of the page: the switch for the file lists, the search and
/// the tags.
pub fn groups<'a>(
    state: &'a TagsPage,
    setting: &FileListTags,
    lang: Language,
    palette: &Palette,
) -> Vec<Group<'a, Message>> {
    let mut show = SettingRow::text(
        lang.text(Msg::SettingsRowFileTagsShow).to_owned(),
        Some(lang.text(Msg::TagsShowHint).to_owned()),
        switch(setting.show, Message::ToggleFileListTags, lang, palette),
    )
    .keyed(RowKey::Press(Message::ToggleFileListTags));
    show.problem = state
        .problem()
        .map(|problem| problem_text(problem, state, lang));
    let search = SettingRow::text(
        lang.text(Msg::TagsSearchPlaceholder).to_owned(),
        None,
        page_kit::text_field(
            "",
            state.search(),
            SEARCH_ID,
            Some(|typed| send(TagsMessage::SearchChanged(typed))),
            send(TagsMessage::SearchChanged(state.search().to_owned())),
            palette,
        ),
    )
    .keyed(RowKey::Text(SEARCH_ID));
    let mut rows = vec![show, search];
    if !state.is_loaded() || !state.has_tags() {
        let note = if state.problem().is_some() {
            None
        } else if state.is_busy() {
            Some(Msg::TagsLoading)
        } else {
            Some(Msg::TagsEmpty)
        };
        if let Some(sentence) = note {
            rows.push(SettingRow::adding(
                Some(lang.text(sentence).to_owned()),
                None,
                Vec::new(),
            ));
        }
    } else {
        let tag_rows = state.rows();
        if tag_rows.is_empty() {
            rows.push(SettingRow::adding(
                Some(lang.text(Msg::TagsNoMatch).to_owned()),
                None,
                Vec::new(),
            ));
        }
        rows.extend(
            tag_rows
                .iter()
                .map(|tag_row| row_of(state, setting, tag_row, lang, palette)),
        );
    }
    vec![Group {
        intro: None,
        title: String::new(),
        rows,
    }]
}

fn row_of<'a>(
    state: &TagsPage,
    setting: &FileListTags,
    tag_row: &TagRow,
    lang: Language,
    palette: &Palette,
) -> SettingRow<'a, Message> {
    match tag_row {
        TagRow::Group(group) => SettingRow::listed_buttons(
            lang.text(group_label(*group)).to_owned(),
            None,
            Some(state.tag_count(*group).to_string()),
            vec![
                RowButton::new(
                    if state.is_open(*group) { "▾" } else { "▸" },
                    Some(send(TagsMessage::ToggleOpen(*group))),
                    Tone::Secondary,
                ),
                RowButton::check(
                    state.group_check(setting, *group),
                    Some(send(TagsMessage::ToggleGroup(*group))),
                ),
            ],
            palette,
        ),
        TagRow::Tag {
            group,
            tag,
            documents,
        } => {
            let mut actions = vec![RowButton::check(
                state.tag_check(setting, *group, tag),
                Some(send(TagsMessage::ToggleTag {
                    group: *group,
                    tag: tag.clone(),
                })),
            )];
            if *group == TagGroup::Own {
                actions.push(RowButton::new(
                    lang.text(Msg::TagsMenuRename),
                    Some(send(TagsMessage::StartRename(tag.clone()))),
                    Tone::Secondary,
                ));
                actions.push(RowButton::new(
                    lang.text(Msg::SettingsRemove),
                    Some(send(TagsMessage::AskDelete(tag.clone()))),
                    Tone::Secondary,
                ));
            }
            SettingRow::listed_buttons(
                state.label(*group, tag),
                None,
                Some(format::grouped(*documents)),
                actions,
                palette,
            )
            .indented()
        }
        TagRow::More { group, rest } => SettingRow::listed(
            fill_template(lang.text(Msg::TagsMore), &[("{count}", &rest.to_string())]),
            None,
            None,
            vec![kit::button(
                "▾",
                Some(send(TagsMessage::Unfold(*group))),
                Tone::Secondary,
                palette,
            )],
            palette,
        )
        .keyed(RowKey::Press(send(TagsMessage::Unfold(*group)))),
    }
}

fn group_label(group: TagGroup) -> Msg {
    match group {
        TagGroup::Kind => Msg::SettingsFileTagsKind,
        TagGroup::Recipient => Msg::SettingsFileTagsRecipient,
        TagGroup::Sender => Msg::SettingsFileTagsSender,
        TagGroup::Year => Msg::SettingsFileTagsYear,
        TagGroup::Own => Msg::SettingsFileTagsOwn,
    }
}

fn problem_text(problem: &TagsProblem, state: &TagsPage, lang: Language) -> String {
    match problem {
        TagsProblem::InUse { tag, rules } => {
            let names: Vec<String> = rules
                .iter()
                .map(|spec| {
                    let named = spec.name.as_deref().filter(|name| !name.is_empty());
                    let text = named.map_or_else(
                        || rule_sentence(spec, lang, &state.kinds().catalog),
                        str::to_owned,
                    );
                    format::ellipsize_end(&text, RULE_CHARS)
                })
                .collect();
            fill_template(
                lang.text(Msg::TagsInUse),
                &[("{tag}", tag.as_str()), ("{rules}", &names.join(", "))],
            )
        }
        TagsProblem::Failed(detail) => fill_template(
            lang.text(Msg::TagsProblemFailed),
            &[("{detail}", detail.as_str())],
        ),
    }
}

/// The dialog that is up: the new name of a tag, or the question before it
/// is deleted.
pub fn dialog<'a>(
    state: &'a TagsPage,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Option<Element<'a, Message>> {
    let (title, body, confirm_label, confirm, tone): (
        Msg,
        Element<'a, Message>,
        Msg,
        Message,
        Tone,
    ) = match state.step()? {
        TagStep::Rename { text, .. } => (
            Msg::TagsRenameTitle,
            container(page_kit::text_field(
                "",
                text,
                RENAME_ID,
                Some(|typed| send(TagsMessage::RenameChanged(typed))),
                send(TagsMessage::ConfirmRename),
                palette,
            ))
            .width(RENAME_WIDTH)
            .into(),
            Msg::RulesFormSave,
            send(TagsMessage::ConfirmRename),
            Tone::Primary,
        ),
        TagStep::ConfirmDelete(tag) => (
            Msg::TagsDeleteConfirm,
            kit::paragraph(
                fill_template(lang.text(Msg::TagsDeleteQuestion), &[("{tag}", tag)]),
                palette,
            ),
            Msg::TagsDeleteConfirm,
            send(TagsMessage::ConfirmDelete),
            Tone::Danger,
        ),
    };
    Some(
        Frame::new(
            lang.text(title),
            body,
            DialogSize::Small,
            window,
            lang,
            palette,
        )
        .buttons(vec![
            kit::button(
                lang.text(Msg::DialogCancel),
                Some(send(TagsMessage::CancelStep)),
                Tone::Secondary,
                palette,
            ),
            kit::button(lang.text(confirm_label), Some(confirm), tone, palette),
        ])
        .on_close(send(TagsMessage::CancelStep))
        .hints(KeyHints {
            enter: false,
            tab: false,
        })
        .into_element(),
    )
}
