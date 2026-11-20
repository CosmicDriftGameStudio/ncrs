//! The right half: the folders of the write folder that get something.

use iced::widget::{column, row};
use iced::{Element, Length};
use ncrs_i18n::{Language, Msg};
use ncrs_ui_kit::format::ellipsize_middle;
use ncrs_ui_kit::frame::{chars_that_fit, AVERAGE_GLYPH_WIDTH};
use ncrs_ui_kit::kit::{checkbox, dim_line, one_line, text_field, Check};
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::plan_rows::{editing_row, marked_row, new_file_row, pane_scroll, NewFileRow};
use ncrs_ui_kit::theme::spacing;
use ncrs_ui_kit::tree_list::{tree_row, TreeRow};

use crate::message::Message;
use crate::scroll::{RIGHT_ROW_ID, RIGHT_SCROLL_ID};
use crate::state::Session;
use crate::texts::{say, HEADER_MARGIN};
use crate::tree::{tree_items, Item};
use crate::update::RENAME_FIELD;

fn folder_label(session: &Session, path: &std::path::Path, new_files: usize) -> String {
    let name = if path.as_os_str().is_empty() {
        session
            .target
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
    } else {
        path.file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
    };
    if new_files > 0 {
        format!("{name}  +{new_files}")
    } else {
        name
    }
}

pub fn right<'a>(
    session: &'a Session,
    plan: &ncrs_filing::chat_work::LivePlan,
    half_width: f32,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let only_new_label = lang.text(Msg::FilingViewOnlyNew);
    let label_width = only_new_label.chars().count() as f32 * AVERAGE_GLYPH_WIDTH;
    let rows = tree_items(session, plan)
        .into_iter()
        .map(|item| match item {
            Item::Folder {
                path,
                depth,
                open,
                new_files,
            } => tree_row(
                TreeRow {
                    depth,
                    open: Some(open),
                    label: folder_label(session, &path, new_files),
                    chosen: false,
                    dim: false,
                    on_press: Message::ToggleFolder(path),
                },
                palette,
            ),
            Item::NewFile {
                from,
                depth,
                new_name,
                old_name,
            } => {
                let chosen = session.selected.as_ref() == Some(&from);
                let line = match (&session.renaming, chosen) {
                    (Some(typed), true) => editing_row(
                        depth,
                        text_field(
                            lang.text(Msg::FilingViewRenamePlaceholder),
                            typed,
                            RENAME_FIELD,
                            Some(Message::RenameInput),
                            Message::RenameSubmit,
                            palette,
                        ),
                    ),
                    _ => new_file_row(
                        NewFileRow {
                            depth,
                            new_name,
                            old_name,
                            chosen,
                            on_press: Message::Select(from.clone()),
                            on_double_press: Message::StartRename,
                        },
                        palette,
                    ),
                };
                if chosen {
                    marked_row(line, RIGHT_ROW_ID)
                } else {
                    line
                }
            }
            Item::Existing {
                folder,
                depth,
                name,
            } => tree_row(
                TreeRow {
                    depth,
                    open: None,
                    label: name,
                    chosen: false,
                    dim: true,
                    on_press: Message::ToggleFolder(folder),
                },
                palette,
            ),
            Item::More {
                folder,
                depth,
                count,
            } => tree_row(
                TreeRow {
                    depth,
                    open: None,
                    label: say(
                        lang,
                        Msg::FilingViewMoreThere,
                        &[("count", &count.to_string())],
                    ),
                    chosen: false,
                    dim: true,
                    on_press: Message::ToggleFolder(folder),
                },
                palette,
            ),
        })
        .collect();
    column![
        row![
            dim_line(
                ellipsize_middle(
                    &say(
                        lang,
                        Msg::FilingViewWriteFolder,
                        &[("path", &session.target.display().to_string())],
                    ),
                    chars_that_fit(half_width - label_width - HEADER_MARGIN),
                ),
                palette,
            ),
            iced::widget::Space::new().width(Length::Fill),
            checkbox(
                if session.only_new {
                    Check::On
                } else {
                    Check::Off
                },
                Some(Message::ToggleOnlyNew),
                palette,
            ),
            one_line(only_new_label.to_owned(), palette),
        ]
        .spacing(spacing::XS),
        pane_scroll(RIGHT_SCROLL_ID, rows),
    ]
    .into()
}
