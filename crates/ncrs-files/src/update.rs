//! What a panel message does to the panels, and what it asks the app for.

use std::path::{Path, PathBuf};

use iced::Task;
use ncrs_core::{FileEntry, PanelSide, TransferKind};
use ncrs_fs::remote::RemotePath;
use ncrs_fs::{SortColumn, SortKey};
use ncrs_i18n::Language;
use ncrs_ui_kit::column_header::{column_spans, ColumnSpan};

use crate::action::Action;
use crate::ctx::Ctx;
use crate::header_drag::{self, HeaderPhase, HeaderPress};
use crate::header_menu::HeaderMenu;
use crate::message::PanelMessage;
use crate::panel::PanelState;
use crate::panel_drag::{self, PanelDrag};
use crate::state::Panels;

/// Which way Alt+Left / Alt+Right walks a panel's history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryDirection {
    Back,
    Forward,
}

/// The load that shows what another program changed in the folder of `side`.
/// It is the ordinary reload: cursor, tags, sort and scroll stay, a tag whose
/// file is gone drops out. An open rename input is not touched by it.
fn reload_after_change_on_disk(panels: &Panels, side: PanelSide) -> Option<Action> {
    let panel = panels.panel(side);
    if panel.is_virtual() || panel.is_hit_list() {
        return None;
    }
    Some(Action::Load {
        side,
        path: panel.path.clone(),
        select: panel
            .selected_entry()
            .map(|entry| entry.name.to_string_lossy().into_owned()),
    })
}

/// Applies `message` to the panels. The actions are for the app to run, in
/// order, right after.
pub fn update(
    panels: &mut Panels,
    message: PanelMessage,
    ctx: &Ctx<'_>,
) -> (Task<PanelMessage>, Vec<Action>) {
    let rows = ctx.visible_rows;
    let mut actions = Vec::new();
    let mut task = Task::none();
    match message {
        PanelMessage::DirectoryLoaded {
            side,
            request_id,
            path,
            mut entries,
            error,
            sorted_by,
            select,
            archive,
        } => {
            let sorted_by_claim = fill_cached_folder_sizes(
                panels,
                side,
                &path,
                archive.is_none(),
                &mut entries,
                sorted_by,
            );
            let panel = panels.panel_mut(side);
            let mut left_archive = None;
            let mut left_server = None;
            let mut changed_meanwhile = false;
            if panel.request_id == request_id {
                let before = panel.remote();
                left_archive = panel
                    .archive
                    .take()
                    .filter(|old| Some(old) != archive.as_ref());
                panel.archive = archive;
                let previous = panel.path.clone();
                panel.apply_sorted_listing(
                    path,
                    entries,
                    error,
                    select.as_deref(),
                    rows,
                    sorted_by_claim,
                );
                panel.record_arrival(previous, request_id);
                left_server = before.filter(|server| panel.remote().as_ref() != Some(server));
                changed_meanwhile = std::mem::take(&mut panel.changed_while_loading);
                actions.push(Action::CloseMenuOfReloaded(side));
            }
            if let Some(old) = left_archive {
                actions.push(Action::DropViewCopies(old));
            }
            if let Some(server) = left_server {
                actions.push(Action::DropViewCopies(server.root_path()));
            }
            actions.push(Action::StopFolderSizeRunIfLeft(side));
            panels.drop_rename_of_missing_entry();
            if changed_meanwhile {
                actions.extend(reload_after_change_on_disk(panels, side));
            }
        }

        PanelMessage::FolderChangedOnDisk(side) => {
            let panel = panels.panel_mut(side);
            if panel.loading {
                // The read in flight may have started before the change.
                panel.changed_while_loading = true;
            } else {
                actions.extend(reload_after_change_on_disk(panels, side));
            }
        }

        PanelMessage::MoveSelection(delta) => {
            actions.push(Action::ClearMessages);
            panels.active_panel_mut().move_selection(delta, rows);
        }
        PanelMessage::PageUp => {
            let page = panels.visible_rows_of(panels.active, rows);
            panels
                .active_panel_mut()
                .move_selection(-(page as isize), rows);
        }
        PanelMessage::PageDown => {
            let page = panels.visible_rows_of(panels.active, rows);
            panels
                .active_panel_mut()
                .move_selection(page as isize, rows);
        }
        PanelMessage::SelectFirst => {
            panels.active_panel_mut().select(0, rows);
        }
        PanelMessage::SelectLast => {
            panels.active_panel_mut().select_last(rows);
        }

        PanelMessage::OpenSelected => actions.extend(open_selected(panels)),
        PanelMessage::GoUp => actions.extend(go_up(panels, panels.active)),
        PanelMessage::TypeAhead(typed) => jump_to_typed(panels, ctx, &typed),
        PanelMessage::HistoryBack => {
            actions.extend(step_history(panels, ctx, HistoryDirection::Back));
        }
        PanelMessage::HistoryForward => {
            actions.extend(step_history(panels, ctx, HistoryDirection::Forward));
        }

        // --- selection ---
        PanelMessage::ToggleTag => tag_and_move(panels, ctx, 1),
        PanelMessage::TagMove(delta) => tag_and_move(panels, ctx, delta),
        PanelMessage::TagAll => {
            // Split borrow: the set is filled while the entries are read.
            let panel = panels.active_panel_mut();
            let mut selection = std::mem::take(&mut panel.selection);
            selection.tag_all(&panel.entries);
            panel.selection = selection;
        }
        PanelMessage::ClearTags => {
            panels.active_panel_mut().selection.clear();
        }

        // The overlays do not capture the wheel, so the panel behind one
        // would otherwise scroll under a dialog.
        PanelMessage::PanelScrolled { side, delta } => {
            if !ctx.overlay_is_open {
                panels.panel_mut(side).scroll(delta, rows);
            }
        }
        PanelMessage::SwitchPanel => {
            panels.active = panels.active.other();
        }
        PanelMessage::ActivatePanel(side) => {
            panels.active = side;
            actions.push(Action::ShowPanels);
        }
        PanelMessage::SortBy { .. } | PanelMessage::SortActive(_) => {
            update_sort(panels, ctx, message, &mut actions);
        }
        // A click in the tag column toggles the tag; anywhere else it moves
        // the cursor. Without this, tagging is keyboard-only, and the
        // column of stars the view draws cannot be clicked at all.
        // A Ctrl+click is a right click: it replaces an open menu.
        PanelMessage::RowClicked { side, index, .. } if ctx.is_context_click => {
            actions.push(Action::OpenContextMenu {
                side,
                index,
                pointer: ctx.pointer,
            });
        }
        PanelMessage::RowClicked { .. } if ctx.context_menu_is_open => {
            // A click beside the menu only closes it.
            actions.push(Action::CloseContextMenu);
        }
        PanelMessage::RowClicked {
            side,
            index,
            on_tag,
        } => {
            panels.active = side;
            // Cmd is also the move key of a drag on macOS, so a Cmd press
            // tags only once it turns out not to be a drag.
            let toggle_on_release = !on_tag && ctx.modifiers.command();
            let panel = panels.panel_mut(side);
            panel.select(index, rows);
            if on_tag {
                toggle_tag(panel, index);
            }
            arm_panel_drag(
                panels,
                ctx,
                side,
                on_tag,
                toggle_on_release.then_some(index),
                &mut actions,
            );
        }
        PanelMessage::RowContext { side, index } => {
            actions.push(Action::ContextMenuAt { side, index });
        }
        PanelMessage::PanelEntered(side) => panels.hovered = Some(side),
        PanelMessage::PanelExited(side) => {
            if panels.hovered == Some(side) {
                panels.hovered = None;
            }
        }
        PanelMessage::PanelDragMoved(position) => {
            if let Some(press) = panels.header_press.as_mut() {
                let spans = header_spans(ctx, press.side);
                press.moved_to(position, &spans, ctx.window_size.width);
            } else if let Some(drag) = panels.drag.as_mut() {
                drag.moved_to(position);
                #[cfg(any(target_os = "macos", target_os = "windows"))]
                if drag.is_dragging() && panel_drag::is_outside(position, ctx.window_size) {
                    actions.push(Action::StartDragOut);
                }
            }
        }
        PanelMessage::PanelDragReleased if panels.header_press.is_some() => {
            finish_header_press(panels, ctx, &mut actions);
        }
        PanelMessage::PanelDragReleased => finish_panel_drag(panels, ctx, &mut actions),
        PanelMessage::PanelDragCancelled => {
            panels.drag = None;
            panels.header_press = None;
            actions.push(Action::DragArmed(false));
        }

        PanelMessage::HeaderPressed { side, .. } if ctx.is_context_click => {
            open_header_menu(panels, ctx, side, &mut actions);
        }
        PanelMessage::HeaderPressed { .. } if ctx.context_menu_is_open => {
            actions.push(Action::CloseContextMenu);
        }
        PanelMessage::HeaderPressed { side, column } => {
            panels.header_menu = None;
            panels.header_press = Some(HeaderPress::pressed(side, column, ctx.pointer));
            actions.push(Action::DragArmed(true));
        }
        PanelMessage::HeaderResizePressed { side, column } => {
            panels.header_menu = None;
            let width = ctx
                .columns
                .layout(side, Language::default())
                .width_of(column);
            panels.header_press = Some(HeaderPress::resizing(side, column, ctx.pointer, width));
            actions.push(Action::DragArmed(true));
        }
        PanelMessage::HeaderContext(side) => open_header_menu(panels, ctx, side, &mut actions),
        PanelMessage::HeaderMenuToggleColumn(column) => {
            if let Some(menu) = panels.header_menu {
                actions.push(Action::ColumnsChanged {
                    side: menu.side,
                    columns: header_drag::toggled(ctx.columns.of(menu.side), column),
                });
            }
        }
        PanelMessage::HeaderMenuSameInBoth => {
            actions.push(Action::SameColumnsInBoth(!ctx.columns.same_in_both));
        }
        PanelMessage::HeaderMenuConfigure => {
            panels.header_menu = None;
            actions.push(Action::OpenColumnSettings);
        }
        PanelMessage::HeaderMenuRestore => {
            panels.header_menu = None;
            actions.push(Action::RestoreColumnDefaults);
        }
        PanelMessage::HeaderMenuClose => panels.header_menu = None,

        PanelMessage::RowActivated { side, index } => {
            activate_row(panels, ctx, side, index, &mut actions);
        }

        PanelMessage::RenameStart => task = panels.begin_rename(),
        PanelMessage::RenameInput(typed) => panels.type_rename(typed),
        PanelMessage::RenameSubmit => actions.extend(panels.confirm_rename()),
    }
    (task, actions)
}

/// Puts the values of this session into a fresh local listing. The returned
/// sort claim is dropped when a value moved a folder in a size-sorted
/// panel, so that the listing is sorted again.
fn fill_cached_folder_sizes(
    panels: &Panels,
    side: PanelSide,
    path: &Path,
    outside_an_archive: bool,
    entries: &mut [FileEntry],
    sorted_by: SortKey,
) -> Option<SortKey> {
    let cached = panels.folder_sizes.get(path);
    let Some(known) = cached.filter(|_| outside_an_archive && RemotePath::parse(path).is_none())
    else {
        return Some(sorted_by);
    };
    let mut filled = false;
    for entry in entries.iter_mut().filter(|entry| entry.is_dir) {
        if let Some(size) = known.get(&entry.name) {
            entry.folder_size = Some(*size);
            filled = true;
        }
    }
    let resort = filled && panels.panel(side).sort.column == SortColumn::Size;
    (!resort).then_some(sorted_by)
}

fn update_sort(
    panels: &mut Panels,
    ctx: &Ctx<'_>,
    message: PanelMessage,
    actions: &mut Vec<Action>,
) {
    let rows = ctx.visible_rows;
    match message {
        // A click beside an open menu only closes it.
        PanelMessage::SortBy { .. } if ctx.context_menu_is_open => {
            actions.push(Action::CloseContextMenu);
        }
        PanelMessage::SortBy { side, column } => {
            panels.active = side;
            panels.panel_mut(side).request_sort(column, rows);
        }
        PanelMessage::SortActive(column) => panels.active_panel_mut().request_sort(column, rows),
        _ => {}
    }
}

/// Toggles the tag on the cursor row and moves by `delta`, as NC does. The
/// `..` entry cannot be tagged but the cursor still moves past it.
fn tag_and_move(panels: &mut Panels, ctx: &Ctx<'_>, delta: isize) {
    let rows = ctx.visible_rows;
    let panel = panels.active_panel_mut();
    if let Some(entry) = panel.selected_entry() {
        if !entry.is_parent {
            let path = entry.path.clone();
            panel.selection.toggle(&path);
        }
    }
    panel.move_selection(delta, rows);
}

/// Moves the cursor to the first entry named like what has been typed.
/// Nothing happens under an overlay or when no entry matches.
fn jump_to_typed(panels: &mut Panels, ctx: &Ctx<'_>, typed: &str) {
    if ctx.overlay_is_open {
        return;
    }
    let rows = ctx.visible_rows;
    let now = (ctx.clock)();
    let prefix = panels.type_ahead.extend(typed, now).to_lowercase();
    let panel = panels.active_panel_mut();
    let found = panel.entries.iter().position(|entry| {
        !entry.is_parent
            && entry
                .name
                .to_string_lossy()
                .to_lowercase()
                .starts_with(&prefix)
    });
    if let Some(index) = found {
        panel.select(index, rows);
    }
}

/// Alt+Left / Alt+Right on the active panel. A folder that is gone since
/// opens its nearest existing parent, and the arrival does not count as a
/// visit, so Forward survives it. A step is ignored while the one before it
/// is still loading.
fn step_history(panels: &mut Panels, ctx: &Ctx<'_>, direction: HistoryDirection) -> Vec<Action> {
    let side = panels.active;
    let panel = panels.panel(side);
    // The path only changes when the listing arrives; a second step before
    // that would take the old folder as the current one again.
    if panel.loading && panel.history_step_request == Some(panel.request_id) {
        return Vec::new();
    }
    let current = panel.path.clone();
    let history = &mut panels.panel_mut(side).history;
    let stepped = match direction {
        HistoryDirection::Back => history.step_back(current.clone()),
        HistoryDirection::Forward => history.step_forward(current.clone()),
    };
    let Some(target) = stepped else {
        return Vec::new();
    };
    let (resolved, select) = if let Some(place) = RemotePath::parse(&target) {
        // Whether a folder on a server is still there is found out by
        // opening it; going up puts the cursor on the folder just left.
        let left = RemotePath::parse(&current)
            .filter(|from| from.parent().as_ref() == Some(&place))
            .and_then(|from| from.file_name().map(str::to_owned));
        (target, left)
    } else {
        let resolved = (ctx.nearest_existing_directory)(&target).unwrap_or(target);
        // Going up to a parent puts the cursor on the folder just left.
        let select = current
            .strip_prefix(&resolved)
            .ok()
            .and_then(|below| below.components().next())
            .map(|first| first.as_os_str().to_string_lossy().into_owned());
        (resolved, select)
    };
    vec![
        Action::ClearMessages,
        Action::Load {
            side,
            path: resolved,
            select,
        },
        Action::MarkHistoryStep(side),
    ]
}

/// Enter: up, into a folder, or into an archive file. An archive inside an
/// archive is refused before this runs.
fn open_selected(panels: &Panels) -> Option<Action> {
    let side = panels.active;
    let panel = panels.active_panel();
    let entry = panel.selected_entry()?;
    if entry.is_parent {
        return go_up(panels, side);
    }
    let opens_as_folder =
        entry.is_dir || (!panel.is_virtual() && ncrs_fs::archive::is_archive_name(&entry.name));
    if !opens_as_folder {
        // Placeholder for file actions (view / edit / open with …).
        return None;
    }
    let path = entry.path.clone();
    Some(Action::Load {
        side,
        path,
        select: None,
    })
}

/// Navigates `side` to its parent directory, re-selecting the directory
/// we came from.
pub fn go_up(panels: &Panels, side: PanelSide) -> Option<Action> {
    if panels.panel(side).is_hit_list() {
        return None;
    }
    let current = &panels.panel(side).path;
    // A server path is cut as text: `Path::parent` would answer `sftp:` at
    // the root and does not know the root of a server.
    if let Some(place) = RemotePath::parse(current) {
        let above = place.parent()?;
        let came_from = place.file_name().map(str::to_owned);
        return Some(Action::Load {
            side,
            path: above.to_path_buf(),
            select: came_from,
        });
    }
    let parent = current.parent().map(PathBuf::from)?;
    let came_from = current
        .file_name()
        .map(|n| n.to_string_lossy().into_owned());
    Some(Action::Load {
        side,
        path: parent,
        select: came_from,
    })
}

/// A double click on a row: the row's Enter, unless it is a context click
/// or a context menu is open.
fn activate_row(
    panels: &mut Panels,
    ctx: &Ctx<'_>,
    side: PanelSide,
    index: usize,
    actions: &mut Vec<Action>,
) {
    if ctx.is_context_click || ctx.context_menu_is_open {
        return;
    }
    panels.active = side;
    let rows = ctx.visible_rows;
    panels.panel_mut(side).select(index, rows);
    actions.push(Action::OpenSelected);
}

/// `..` is not a thing to copy, so it is not taggable — the same rule the
/// keyboard path uses.
fn toggle_tag(panel: &mut PanelState, index: usize) {
    if let Some(entry) = panel.entries.get(index) {
        if !entry.is_parent {
            let path = entry.path.clone();
            panel.selection.toggle(&path);
        }
    }
}

/// A press on the star only tags, so it never starts a drag.
fn arm_panel_drag(
    panels: &mut Panels,
    ctx: &Ctx<'_>,
    side: PanelSide,
    on_tag: bool,
    toggle_on_release: Option<usize>,
    actions: &mut Vec<Action>,
) {
    if on_tag {
        return;
    }
    panels.drag = Some(PanelDrag::new(side, ctx.pointer, toggle_on_release));
    actions.push(Action::DragArmed(true));
}

/// The shown columns of `side` as they lie in the panel, for hit-testing a
/// dragged title.
fn header_spans(ctx: &Ctx<'_>, side: PanelSide) -> Vec<ColumnSpan> {
    column_spans(
        ctx.columns.layout(side, Language::default()),
        header_drag::panel_width(ctx.window_size.width),
        ctx.has_hermes_column,
    )
}

fn open_header_menu(
    panels: &mut Panels,
    ctx: &Ctx<'_>,
    side: PanelSide,
    actions: &mut Vec<Action>,
) {
    panels.header_menu = Some(HeaderMenu::at(side, ctx.pointer, ctx.window_size));
    if ctx.context_menu_is_open {
        actions.push(Action::CloseContextMenu);
    }
}

/// The button came up on a title: a click sorts, a drag moved or resized.
fn finish_header_press(panels: &mut Panels, ctx: &Ctx<'_>, actions: &mut Vec<Action>) {
    actions.push(Action::DragArmed(false));
    let Some(press) = panels.header_press.take() else {
        return;
    };
    match press.phase {
        HeaderPhase::Pressed => {
            if let Some(column) = press.column.sort_column() {
                update_sort(
                    panels,
                    ctx,
                    PanelMessage::SortBy {
                        side: press.side,
                        column,
                    },
                    actions,
                );
            }
        }
        HeaderPhase::Reordering { before, .. } => {
            let current = ctx.columns.of(press.side);
            let columns = header_drag::reordered(current, press.column, before);
            if columns != current {
                actions.push(Action::ColumnsChanged {
                    side: press.side,
                    columns,
                });
            }
        }
        HeaderPhase::Resizing { start_width, width } => {
            if (width - start_width).abs() >= 1.0 {
                actions.push(Action::ColumnWidth {
                    column: press.column,
                    width: width.round() as u16,
                });
            }
        }
    }
}

/// Dropping on the other panel asks the F5 or F6 question for it.
fn finish_panel_drag(panels: &mut Panels, ctx: &Ctx<'_>, actions: &mut Vec<Action>) {
    actions.push(Action::DragArmed(false));
    let Some(drag) = panels.drag.take() else {
        return;
    };
    if let (false, Some(index)) = (drag.is_dragging(), drag.toggle_on_release()) {
        toggle_tag(panels.panel_mut(drag.source()), index);
    }
    if !drag.is_dragging() || ctx.overlay_is_open || panels.hovered != Some(drag.source().other()) {
        return;
    }
    panels.active = drag.source();
    let kind = if panel_drag::move_modifier_held(ctx.modifiers) {
        TransferKind::Move
    } else {
        TransferKind::Copy
    };
    actions.push(Action::OpenTransferDialog(kind));
}
