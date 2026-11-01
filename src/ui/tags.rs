//! The tags page: every tag in groups that fold, with the tick that shows it
//! in the file lists and the number of documents that have it. A page of the
//! Ablage tab. Pure view over [`TagsPage`].

use iced::widget::{container, mouse_area, row, Column, Row, Space};
use iced::{alignment, Element, Length};

use super::format;
use super::hermes::{self, Badges};
use super::hermes_kit::{
    self as page_kit, ChipKind, MenuItem, MenuLook, FORM_WIDTH, GAP_L, GAP_M, GAP_S, ROW_ONE_LINE,
};
use super::kit::Check;
use crate::app::fill_template;
use crate::file_list_tags::{FileListTags, TagGroup};
use crate::i18n::{Language, Msg};
use crate::messages::{HermesPage, Message, SettingsMessage, TagsMessage};
use crate::palette::Palette;
use crate::rules_page::rule_sentence;
use crate::tags_page::{TagRow, TagStep, TagsPage, TagsProblem, VISIBLE_ROWS};

pub const SEARCH_ID: &str = "tags-search";
pub const RENAME_ID: &str = "tags-rename";

const SEARCH_WIDTH: f32 = 220.0;
const RENAME_WIDTH: f32 = 220.0;
const SHOWN_COLUMN_WIDTH: f32 = 200.0;
const DOCUMENTS_COLUMN_WIDTH: f32 = 120.0;
/// The room the menu button of a row keeps free, so the numbers line up.
const MENU_ROOM: f32 = 28.0;
const RULE_CHARS: usize = 40;

fn send(message: TagsMessage) -> Message {
    Message::Tags(message)
}

pub fn view<'a>(
    state: &'a TagsPage,
    setting: &FileListTags,
    badges: Badges,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    page_kit::tab_page(
        hermes::tab_nav(HermesPage::Tags, badges, lang, palette),
        header(state, lang, palette),
        body(state, setting, lang, palette),
        Some(action_bar(state, lang, palette)),
        palette,
    )
}

fn header<'a>(state: &TagsPage, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let search = container(page_kit::text_field(
        lang.text(Msg::TagsSearchPlaceholder),
        state.search(),
        SEARCH_ID,
        Some(|typed| send(TagsMessage::SearchChanged(typed))),
        send(TagsMessage::Select(0)),
        palette,
    ))
    .width(SEARCH_WIDTH);
    page_kit::page_header(
        lang.text(Msg::ActionTags),
        None,
        Some(lang.text(Msg::TagsExplain).to_owned()),
        None,
        vec![search.into()],
        palette,
    )
}

fn action_bar<'a>(state: &TagsPage, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let buttons = match state.step() {
        Some(TagStep::ConfirmDelete(_)) => vec![
            page_kit::secondary(
                lang.text(Msg::DialogCancel),
                Some(send(TagsMessage::Close)),
                palette,
            ),
            page_kit::primary(
                lang.text(Msg::TagsDeleteConfirm),
                Some("↵"),
                Some(send(TagsMessage::ConfirmDelete)),
                palette,
            ),
        ],
        Some(TagStep::Rename { .. }) | None => Vec::new(),
    };
    page_kit::action_bar(
        Some(lang.text(Msg::TagsSavedNote).to_owned()),
        buttons,
        palette,
    )
}

fn body<'a>(
    state: &TagsPage,
    setting: &FileListTags,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    if !state.is_loaded() || !state.has_tags() {
        let sentence = if state.problem().is_some() {
            None
        } else if state.is_busy() {
            Some(Msg::TagsLoading)
        } else {
            Some(Msg::TagsEmpty)
        };
        if let Some(note) = sentence {
            return page_kit::centered_note(page_kit::hint(lang.text(note), palette));
        }
    }
    let mut children: Vec<Element<'a, Message>> = Vec::new();
    if let Some(TagStep::ConfirmDelete(tag)) = state.step() {
        children.push(page_kit::error_line(
            fill_template(lang.text(Msg::TagsDeleteQuestion), &[("{tag}", tag)]),
            palette,
        ));
    }
    if let Some(problem) = state.problem() {
        children.push(page_kit::error_line(
            problem_text(problem, state, lang),
            palette,
        ));
    }
    children.push(page_kit::form_row(
        lang.text(Msg::SettingsRowFileTagsShow),
        page_kit::checkbox(
            if setting.show { Check::On } else { Check::Off },
            Some(Message::Settings(SettingsMessage::ToggleFileListTags)),
            None,
            palette,
        ),
        Some(page_kit::hint(lang.text(Msg::TagsShowHint), palette)),
        palette,
    ));
    children.push(table(state, setting, lang, palette));
    let page = container(Column::with_children(children).spacing(GAP_S))
        .max_width(FORM_WIDTH)
        .padding([GAP_M, GAP_L]);
    mouse_area(page)
        .on_scroll(|delta| send(TagsMessage::Scrolled(delta)))
        .into()
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

fn table<'a>(
    state: &TagsPage,
    setting: &FileListTags,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let rows = state.rows();
    let mut lines: Vec<Element<'a, Message>> = vec![column_heads(lang, palette)];
    if rows.is_empty() {
        lines.push(page_kit::hint(lang.text(Msg::TagsNoMatch), palette));
    }
    let first = state.first_visible();
    for (index, tag_row) in rows.iter().enumerate().skip(first).take(VISIBLE_ROWS) {
        let selected = state.cursor() == index;
        lines.push(match tag_row {
            TagRow::Group(group) => {
                group_row(state, setting, *group, index, selected, lang, palette)
            }
            TagRow::Tag {
                group,
                tag,
                documents,
            } => tag_row_view(
                state,
                setting,
                TagFacts {
                    group: *group,
                    tag,
                    documents: *documents,
                },
                RowPlace { index, selected },
                lang,
                palette,
            ),
            TagRow::More { group, rest } => page_kit::list_row_one(
                page_kit::link(
                    &fill_template(lang.text(Msg::TagsMore), &[("{count}", &rest.to_string())]),
                    Some(send(TagsMessage::Unfold(*group))),
                    palette,
                ),
                None,
                true,
                selected,
                Some(send(TagsMessage::Select(index))),
                palette,
            ),
        });
    }
    Column::with_children(lines).into()
}

fn column_heads<'a>(lang: Language, palette: &Palette) -> Element<'a, Message> {
    container(
        row![
            container(page_kit::hint(lang.text(Msg::TagsColumnShown), palette))
                .width(SHOWN_COLUMN_WIDTH),
            Space::new().width(Length::Fill),
            container(page_kit::hint(lang.text(Msg::TagsColumnDocuments), palette))
                .width(DOCUMENTS_COLUMN_WIDTH)
                .align_x(alignment::Horizontal::Right),
            Space::new().width(MENU_ROOM),
        ]
        .spacing(GAP_S)
        .align_y(alignment::Vertical::Center),
    )
    .padding([0.0, GAP_M])
    .height(ROW_ONE_LINE)
    .center_y(ROW_ONE_LINE)
    .into()
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

fn group_row<'a>(
    state: &TagsPage,
    setting: &FileListTags,
    group: TagGroup,
    index: usize,
    selected: bool,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let fold = mouse_area(page_kit::value_dim(
        if state.is_open(group) { "▾" } else { "▸" },
        palette,
    ))
    .on_press(send(TagsMessage::ToggleOpen(group)));
    let content = row![
        fold,
        page_kit::checkbox(
            state.group_check(setting, group),
            Some(send(TagsMessage::ToggleGroup(group))),
            Some(lang.text(group_label(group))),
            palette,
        ),
        Space::new().width(Length::Fill),
        page_kit::hint(state.tag_count(group).to_string(), palette),
    ]
    .spacing(GAP_S)
    .align_y(alignment::Vertical::Center);
    page_kit::list_row_one(
        content.into(),
        None,
        false,
        selected,
        Some(send(TagsMessage::Select(index))),
        palette,
    )
}

/// A tag of the table.
struct TagFacts<'t> {
    group: TagGroup,
    tag: &'t str,
    documents: u64,
}

/// Where a row stands in the table.
struct RowPlace {
    index: usize,
    selected: bool,
}

fn tag_row_view<'a>(
    state: &TagsPage,
    setting: &FileListTags,
    facts: TagFacts<'_>,
    place: RowPlace,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let TagFacts {
        group,
        tag,
        documents,
    } = facts;
    let RowPlace { index, selected } = place;
    let own = group == TagGroup::Own;
    let renaming =
        matches!(state.step(), Some(TagStep::Rename { tag: renamed, .. }) if renamed == tag);
    let name: Element<'a, Message> = match state.step() {
        Some(TagStep::Rename { text, .. }) if renaming => container(page_kit::text_field(
            "",
            text,
            RENAME_ID,
            Some(|typed| send(TagsMessage::RenameChanged(typed))),
            send(TagsMessage::ConfirmRename),
            palette,
        ))
        .width(RENAME_WIDTH)
        .into(),
        Some(TagStep::Rename { .. } | TagStep::ConfirmDelete(_)) | None => page_kit::chip(
            &state.label(group, tag),
            if own {
                ChipKind::Own
            } else {
                ChipKind::Automatic
            },
            None,
            palette,
        ),
    };
    let menu: Element<'a, Message> = if own {
        page_kit::menu_button(
            "",
            &[
                MenuItem::new(
                    lang.text(Msg::TagsMenuRename),
                    send(TagsMessage::StartRename(tag.to_owned())),
                ),
                MenuItem::new(
                    lang.text(Msg::ActionDelete),
                    send(TagsMessage::AskDelete(tag.to_owned())),
                ),
            ],
            MenuLook::Icon,
            palette,
        )
    } else {
        Space::new().width(MENU_ROOM).into()
    };
    let content = Row::new()
        .push(page_kit::checkbox(
            state.tag_check(setting, group, tag),
            Some(send(TagsMessage::ToggleTag {
                group,
                tag: tag.to_owned(),
            })),
            None,
            palette,
        ))
        .push(name)
        .push(Space::new().width(Length::Fill))
        .push(
            container(page_kit::hint(format::grouped(documents), palette))
                .width(DOCUMENTS_COLUMN_WIDTH)
                .align_x(alignment::Horizontal::Right),
        )
        .push(menu)
        .spacing(GAP_S)
        .align_y(alignment::Vertical::Center);
    page_kit::list_row_one(
        content.into(),
        None,
        true,
        selected,
        Some(send(TagsMessage::Select(index))),
        palette,
    )
}
