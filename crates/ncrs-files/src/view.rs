//! The view of a file panel: a pure function of [`PanelState`]. The component
//! owns no state; it lives in the app state and is only mutated by `update`.

use iced::widget::{column, mouse_area, Column, Row};
use iced::{Element, Length};
use ncrs_i18n::{Language, Msg};
use ncrs_ui_kit::compare_cell::CompareCell;
use ncrs_ui_kit::file_row::{file_row, tag_star, RowClick, RowExtras, SizeProgress};
use ncrs_ui_kit::git_mark::{git_legend, GitCell};
use ncrs_ui_kit::kit::panel_frame;
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::panel_footer::panel_footer;
use ncrs_ui_kit::rename_row::{rename_row, RenameRow};

use crate::ctx::ViewCtx;
use crate::message::PanelMessage;
use crate::panel::PanelState;
use crate::panel_totals::{footer_text, panel_totals};
use crate::special_folders::special_folder_of;
use ncrs_ui_kit::column_header::{column_header, HeaderEvents, TrailingColumn};

/// Renders one panel.
///
/// A row is one clickable thing with several meanings, so it sends one message
/// with what the click did. A right click sends [`PanelMessage::RowContext`].
pub fn view<'a>(state: &'a PanelState, ctx: ViewCtx<'a>) -> Element<'a, PanelMessage> {
    let side = ctx.side;
    let lang = ctx.lang;
    let palette = &ctx.palette;
    let compare_marks = ctx.compare_marks;
    let hermes_cell = ctx
        .hermes_cell
        .as_deref()
        .filter(|_| compare_marks.is_none());
    let trailing = if compare_marks.is_some() {
        TrailingColumn::Compare
    } else if hermes_cell.is_some() {
        TrailingColumn::Hermes
    } else {
        TrailingColumn::None
    };
    let layout = ctx
        .columns
        .layout(side, lang)
        .with_git(state.git.shows_column());
    let on_row_click = move |index, click| match click {
        RowClick::Open => PanelMessage::RowActivated {
            side,
            index,
            name: state
                .entries
                .get(index)
                .map(|entry| entry.name.clone())
                .unwrap_or_default(),
        },
        RowClick::Tag | RowClick::Cursor => PanelMessage::RowClicked {
            side,
            index,
            on_tag: click == RowClick::Tag,
        },
    };
    let rows = state
        .entries
        .iter()
        .enumerate()
        .skip(state.scroll_offset)
        .take(state.rows_in_view(ctx.visible_rows))
        .map(|(index, entry)| {
            let mark = state
                .selection
                .state_of(index, &state.entries, state.selected);
            // Two targets per row, as in classic dual-panel file managers: the star itself toggles the tag,
            // the rest of the row moves the cursor. iced gives a button one
            // message and no click position, so the star has to be its own
            // button rather than a region of the row button.
            let row = Row::new().push(tag_star(
                mark,
                ctx.is_active,
                palette,
                state.row_height(),
                on_row_click(index, RowClick::Tag),
            ));
            if let Some(rename) = state
                .rename
                .as_ref()
                .filter(|rename| rename.path == entry.path)
            {
                return rename_row(
                    RenameRow {
                        entry,
                        draft: &rename.draft,
                        error: rename.error.as_deref(),
                        busy: rename.busy,
                        height: state.row_height(),
                        columns: layout,
                        special: special_folder_of(ctx.special_folders, &entry.path)
                            .filter(|_| entry.is_dir),
                        on_input: PanelMessage::RenameInput,
                        on_submit: PanelMessage::RenameSubmit,
                    },
                    palette,
                );
            }
            // A button takes only the left button, so the right press reaches
            // the area around it.
            mouse_area(file_row(
                entry,
                mark,
                ctx.is_active,
                palette,
                |click| on_row_click(index, click),
                row,
                RowExtras {
                    columns: layout,
                    special: special_folder_of(ctx.special_folders, &entry.path)
                        .filter(|_| entry.is_dir),
                    third_line: None,
                    has_note: ctx.noted.contains(&entry.path),
                    git: git_cell_of(state, &entry.name),
                    size_progress: ctx
                        .size_run
                        .filter(|_| entry.is_dir && !entry.is_parent)
                        .map_or(SizeProgress::Idle, |run| {
                            run.progress_of(&state.path, &entry.path)
                        }),
                    hermes: hermes_cell.map(|cell| cell(entry)),
                    compare: compare_marks.map_or(CompareCell::Off, |marks| {
                        marks
                            .get(&entry.name)
                            .map_or(CompareCell::Blank, |found| CompareCell::Mark(*found))
                    }),
                    hit_scope: state
                        .hit_list
                        .as_ref()
                        .map(|list| (list.scope.as_path(), state.row_height())),
                },
            ))
            .on_right_press(PanelMessage::RowContext { side, index })
            .into()
        });

    // The wheel belongs to the panel under the pointer. The rows are drawn from
    // the window `scroll_offset` selects, not by a `scrollable`, so the panel
    // has to ask for the wheel itself.
    mouse_area(panel_frame(
        column![
            column_header(
                palette,
                layout,
                Some(state.sort),
                trailing,
                ctx.header_drag,
                Some(HeaderEvents {
                    on_press: Box::new(move |column| PanelMessage::HeaderPressed { side, column }),
                    on_resize: Box::new(move |column| PanelMessage::HeaderResizePressed {
                        side,
                        column
                    }),
                    on_context: PanelMessage::HeaderContext(side),
                }),
            ),
            Column::with_children(rows).height(Length::Fill).clip(true),
            panel_footer(footer_text(&panel_totals(state), lang), palette),
        ]
        .extend(git_legend_of(state, lang, palette)),
        palette,
    ))
    .on_scroll(move |delta| PanelMessage::PanelScrolled { side, delta })
    .on_enter(PanelMessage::PanelEntered(side))
    .on_exit(PanelMessage::PanelExited(side))
    .into()
}

/// What the Git column of a row says about git.
fn git_cell_of(state: &PanelState, name: &std::ffi::OsString) -> GitCell {
    GitCell {
        mark: state.git.mark_of(name),
    }
}

/// The line under the panel that explains its Git column, if it has one.
fn git_legend_of<'a>(
    state: &PanelState,
    lang: Language,
    palette: &Palette,
) -> Option<Element<'a, PanelMessage>> {
    if state.git.in_repository() {
        Some(git_legend(
            lang.text(Msg::GitLegendStates).to_owned(),
            false,
            palette,
        ))
    } else if state.git.holds_repositories() {
        Some(git_legend(
            lang.text(Msg::GitLegendRepositories).to_owned(),
            true,
            palette,
        ))
    } else {
        None
    }
}
