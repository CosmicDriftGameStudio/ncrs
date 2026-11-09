//! The start dialog, the compare bar and the sync dialog. Pure views over
//! [`Compare`]; what the buttons do is decided by `update`.

use iced::widget::{column, container, row};
use iced::{Element, Padding, Size};
use ncrs_core::CompareMark;
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_ui_kit::compare_bar::{compare_bar, Tally};
use ncrs_ui_kit::compare_cell::{mark_color, mark_message};
use ncrs_ui_kit::format;
use ncrs_ui_kit::frame::{content_chars, DialogSize, Frame, KeyHints};
use ncrs_ui_kit::kit::{self, dim, error_line, paragraph, Check, Columns, ListView, Tone};
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::theme::spacing;

use crate::core::{can_delete_in, Direction, SyncRow};
use crate::message::Message;
use crate::state::{Compare, Folders, SyncDialog};
use crate::update::{COPY_ROWS, DELETE_ROWS};

/// The width of the Left and Right labels of the start dialog.
const LABEL_WIDTH: f32 = 56.0;

/// Where the note of the Content choice starts: under the label, not the box.
const NOTE_INDENT: f32 = 22.0;

/// The dialog on top, if there is one: the sync dialog before the start dialog.
pub fn dialog<'a>(
    compare: &'a Compare,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Option<Element<'a, Message>> {
    if let Some(sync) = compare.sync.as_ref() {
        return sync_dialog(compare, sync, window, lang, palette);
    }
    let folders = compare.start.as_ref()?;
    Some(start_dialog(compare, folders, window, lang, palette))
}

fn tick<'a>(
    label: &str,
    state: Check,
    on_toggle: Option<Message>,
    palette: &Palette,
) -> Element<'a, Message> {
    let text: Element<'a, Message> = if on_toggle.is_some() {
        paragraph(label.to_owned(), palette)
    } else {
        dim(label.to_owned(), palette)
    };
    row![kit::checkbox(state, on_toggle, palette), text]
        .spacing(spacing::XS)
        .into()
}

fn check(on: bool) -> Check {
    if on {
        Check::On
    } else {
        Check::Off
    }
}

fn start_dialog<'a>(
    compare: &Compare,
    folders: &Folders,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let choices = compare.choices;
    let path_row = |label: Msg, path: &std::path::Path| -> Element<'a, Message> {
        row![
            container(dim(lang.text(label).to_owned(), palette)).width(LABEL_WIDTH),
            paragraph(path.display().to_string(), palette),
        ]
        .into()
    };
    let content_note = container(dim(lang.text(Msg::CompareContentNote).to_owned(), palette))
        .padding(Padding::ZERO.left(NOTE_INDENT));
    let body = column![
        column![
            path_row(Msg::CompareLeft, &folders.left),
            path_row(Msg::CompareRight, &folders.right),
        ]
        .spacing(spacing::XS),
        column![
            dim(lang.text(Msg::CompareCriteria).to_owned(), palette),
            tick(lang.text(Msg::ColumnName), Check::On, None, palette),
            tick(
                lang.text(Msg::ColumnSize),
                check(choices.size),
                Some(Message::ToggleSize),
                palette,
            ),
            tick(
                lang.text(Msg::CompareCriterionDate),
                check(choices.date),
                Some(Message::ToggleDate),
                palette,
            ),
            column![
                tick(
                    lang.text(Msg::CompareCriterionContent),
                    check(choices.content),
                    Some(Message::ToggleContent),
                    palette,
                ),
                content_note,
            ]
            .spacing(spacing::XS),
        ]
        .spacing(spacing::LINE_GAP),
        tick(
            lang.text(Msg::CompareSubfolders),
            check(choices.subfolders),
            Some(Message::ToggleSubfolders),
            palette,
        ),
    ]
    .spacing(spacing::M);
    Frame::new(
        lang.text(Msg::ActionCompareFolders),
        body,
        DialogSize::Medium,
        window,
        lang,
        palette,
    )
    .buttons(vec![
        kit::button(
            lang.text(Msg::DialogCancel),
            Some(Message::Cancel),
            Tone::Secondary,
            palette,
        ),
        kit::enter_button(
            lang.text(Msg::CompareStart),
            Some(Message::Confirm),
            Tone::Primary,
            palette,
        ),
    ])
    .on_close(Message::Cancel)
    .hints(KeyHints {
        enter: true,
        tab: false,
    })
    .into_element()
}

/// The portions of the name, size and mark columns of a sync list.
const PORTIONS: [u16; 3] = [6, 1, 2];

fn list_rows<'a>(
    deletions: bool,
    rows: &[SyncRow],
    columns: &Columns<'_>,
    offset: usize,
    visible: usize,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    rows.iter()
        .skip(offset)
        .take(visible)
        .map(|shown| {
            let name = if shown.is_dir {
                format!("/{}", shown.name.to_string_lossy())
            } else {
                shown.name.to_string_lossy().into_owned()
            };
            let size = if shown.is_dir {
                String::new()
            } else {
                format::size(shown.size)
            };
            let name_color = if shown.is_dir {
                palette.directory
            } else {
                palette.text
            };
            let mark = if deletions {
                ""
            } else {
                lang.text(mark_message(shown.mark))
            };
            columns.row_colored(&[
                (name.as_str(), name_color),
                (size.as_str(), palette.dim_text),
                (mark, mark_color(palette, shown.mark)),
            ])
        })
        .collect()
}

fn sync_list<'a>(
    rows: &[SyncRow],
    window: ncrs_ui_kit::list_window::ListWindow,
    visible: usize,
    deletions: bool,
    columns: &Columns<'_>,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let offset = window.offset(rows.len(), visible);
    let header = columns.header(
        &[
            lang.text(Msg::ColumnName),
            lang.text(Msg::ColumnSize),
            if deletions {
                ""
            } else {
                lang.text(Msg::ColumnCompare)
            },
        ],
        palette,
    );
    kit::scroll_list(
        header,
        list_rows(deletions, rows, columns, offset, visible, lang, palette),
        ListView {
            len: rows.len(),
            visible,
            offset,
        },
        move |delta| Message::SyncScrolled { deletions, delta },
        palette,
    )
}

fn sync_dialog<'a>(
    compare: &'a Compare,
    sync: &SyncDialog,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Option<Element<'a, Message>> {
    let (direction, plan) = compare.plan()?;
    let folders = &compare.run.as_ref()?.folders;
    let (source, target) = (
        folders.of(direction.source()),
        folders.of(direction.target()),
    );
    let (target_text, source_text) = (target.display().to_string(), source.display().to_string());
    let columns = Columns {
        portions: &PORTIONS,
        line_chars: content_chars(DialogSize::Large, window, 0.0),
    };
    let intro = fill_template(
        lang.text(Msg::CompareSyncIntro),
        &[
            ("{count}", &plan.copies.len().to_string()),
            ("{source}", &source_text),
        ],
    );
    let mut body = column![
        dim(intro, palette),
        sync_list(
            &plan.copies,
            sync.copy_window,
            COPY_ROWS,
            false,
            &columns,
            lang,
            palette,
        ),
    ]
    .spacing(spacing::M);
    if can_delete_in(target) {
        let label = match direction {
            Direction::ToRight => Msg::CompareSyncDeleteRight,
            Direction::ToLeft => Msg::CompareSyncDeleteLeft,
        };
        body = body.push(tick(
            lang.text(label),
            check(sync.delete),
            Some(Message::ToggleDelete),
            palette,
        ));
    } else {
        body = body.push(dim(
            lang.text(Msg::CompareSyncDeleteUnavailable).to_owned(),
            palette,
        ));
    }
    if !plan.deletions.is_empty() {
        let heading = fill_template(
            lang.text(Msg::CompareSyncDeleteHeading),
            &[
                ("{count}", &plan.deletions.len().to_string()),
                ("{path}", &target_text),
            ],
        );
        body = body.push(error_line(heading, palette)).push(sync_list(
            &plan.deletions,
            sync.delete_window,
            DELETE_ROWS,
            true,
            &columns,
            lang,
            palette,
        ));
    }
    let (copies, deletions) = (
        plan.copies.len().to_string(),
        plan.deletions.len().to_string(),
    );
    let (label, tone) = if plan.deletions.is_empty() {
        (
            fill_template(
                lang.text(Msg::CompareSyncCopyOnly),
                &[("{copies}", &copies)],
            ),
            Tone::Primary,
        )
    } else {
        (
            fill_template(
                lang.text(Msg::CompareSyncCopyAndDelete),
                &[("{copies}", &copies), ("{deletions}", &deletions)],
            ),
            Tone::Danger,
        )
    };
    let nothing_to_do = plan.copies.is_empty() && plan.deletions.is_empty();
    let title = fill_template(
        lang.text(Msg::CompareSyncTitle),
        &[(
            "{path}",
            &format::ellipsize_middle(
                &target_text,
                content_chars(DialogSize::Large, window, 0.0).saturating_sub(24),
            ),
        )],
    );
    Some(
        Frame::new(title, body, DialogSize::Large, window, lang, palette)
            .buttons(vec![
                kit::button(
                    lang.text(Msg::DialogCancel),
                    Some(Message::Cancel),
                    Tone::Secondary,
                    palette,
                ),
                kit::enter_button(
                    &label,
                    (!nothing_to_do).then_some(Message::Confirm),
                    tone,
                    palette,
                ),
            ])
            .on_close(Message::Cancel)
            .hints(KeyHints {
                enter: true,
                tab: false,
            })
            .into_element(),
    )
}

/// The bar under the favourites bar while a comparison is running or done.
pub fn bar<'a>(
    compare: &Compare,
    lang: Language,
    palette: &Palette,
) -> Option<Element<'a, Message>> {
    let run = compare.run.as_ref()?;
    let figure = |count: usize, words: Msg, mark: CompareMark| Tally {
        text: format!("{count} {}", lang.text(words)),
        color: mark_color(palette, mark),
    };
    let (tallies, left_copies, right_copies) = match run.result.as_ref() {
        Some(compared) => {
            let tally = compared.tally();
            (
                vec![
                    figure(
                        tally.only_left,
                        Msg::CompareTallyOnlyLeft,
                        CompareMark::OnlyHere,
                    ),
                    figure(
                        tally.newer_left,
                        Msg::CompareTallyNewerLeft,
                        CompareMark::NewerHere,
                    ),
                    figure(
                        tally.only_right,
                        Msg::CompareTallyOnlyRight,
                        CompareMark::OnlyHere,
                    ),
                    figure(
                        tally.newer_right,
                        Msg::CompareTallyNewerRight,
                        CompareMark::NewerHere,
                    ),
                    figure(
                        tally.different,
                        Msg::CompareTallyDifferent,
                        CompareMark::Different,
                    ),
                    figure(tally.same, Msg::CompareTallySame, CompareMark::Same),
                ],
                crate::core::sync_plan(compared, Direction::ToRight, false)
                    .copies
                    .len(),
                crate::core::sync_plan(compared, Direction::ToLeft, false)
                    .copies
                    .len(),
            )
        }
        None => (
            vec![Tally {
                text: lang.text(Msg::CompareRunning).to_owned(),
                color: palette.dim_text,
            }],
            0,
            0,
        ),
    };
    let done = run.result.is_some() && compare.after_sync.is_none();
    let sync_button = |label: Msg, count: usize, direction: Direction| {
        let text = fill_template(lang.text(label), &[("{count}", &count.to_string())]);
        kit::button(
            &text,
            (done && count > 0).then_some(Message::OpenSync(direction)),
            Tone::Secondary,
            palette,
        )
    };
    let controls = vec![
        row![
            kit::checkbox(
                check(run.only_differences),
                Some(Message::ToggleOnlyDifferences),
                palette,
            ),
            paragraph(lang.text(Msg::CompareOnlyDifferences).to_owned(), palette),
        ]
        .spacing(spacing::XS)
        .into(),
        sync_button(Msg::CompareSyncRight, left_copies, Direction::ToRight),
        sync_button(Msg::CompareSyncLeft, right_copies, Direction::ToLeft),
        kit::button(
            lang.text(Msg::CompareAgain),
            done.then_some(Message::Again),
            Tone::Secondary,
            palette,
        ),
        kit::button(
            lang.text(Msg::DialogDone),
            Some(Message::Done),
            Tone::Primary,
            palette,
        ),
    ];
    Some(compare_bar(
        lang.text(Msg::CompareBarTitle),
        tallies,
        controls,
        palette,
    ))
}
