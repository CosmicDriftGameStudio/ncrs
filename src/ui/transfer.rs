//! The two masks of a copy or move: the confirmation before it starts, and the
//! progress while it runs.

use std::path::Path;

use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, scrollable, text, Column, Row};
use iced::{alignment, Element, Font, Length, Size};

use super::dialog as dialog_ui;
use super::format;
use super::frame::{self, DialogSize, Frame};
use super::kit::{self, Tone};
use super::theme::{self, font_size, spacing};
use crate::i18n::{Language, Msg};
use crate::messages::{Message, TransferKind, TransferKindJob as _};
use crate::palette::Palette;
use crate::transfer_dialog::{
    Policy, Refusal, TransferDialog, TransferDialogMessage, FIRST_NAMES, LISTED_NAMES,
};

const SIZE: DialogSize = DialogSize::Medium;

/// Lets the app focus the target field when the dialog opens.
pub const TARGET_FIELD_ID: &str = "transfer-target";

/// Shown in place of a number that is still being counted.
const COUNTING: &str = "…";

/// The width the policy label takes, so the control starts at one place.
const LABEL_WIDTH: f32 = 140.0;

/// The height of the list of all names, which scrolls inside the dialog.
const LIST_HEIGHT: f32 = 160.0;

fn send(message: TransferDialogMessage) -> Message {
    Message::TransferDialog(message)
}

fn title(kind: TransferKind, lang: Language) -> &'static str {
    lang.text(match kind {
        TransferKind::Copy => Msg::TransferTitleCopy,
        TransferKind::Move => Msg::TransferTitleMove,
    })
}

pub fn refusal_text(refusal: Refusal, lang: Language) -> &'static str {
    lang.text(match refusal {
        Refusal::Missing => Msg::TransferErrorMissing,
        Refusal::NotAFolder => Msg::TransferErrorNotFolder,
        Refusal::SameFolder => Msg::TransferErrorSameFolder,
        Refusal::InsideSource => Msg::TransferErrorInside,
        Refusal::Unreadable => Msg::TransferErrorUnreadable,
        Refusal::IntoArchive => Msg::TransferErrorIntoArchive,
        Refusal::ArchiveInArchive => Msg::TransferErrorArchiveNested,
        Refusal::ServerToArchive => Msg::TransferErrorServerToArchive,
        Refusal::MoveFromArchive => Msg::TransferErrorMoveFromArchive,
        Refusal::MoveToServer | Refusal::MoveFromServer => Msg::TransferErrorMoveRemote,
        Refusal::ServerToServer => Msg::TransferErrorServerToServer,
        Refusal::BadAddress => Msg::ErrorRemoteUrlInvalid,
    })
}

fn policy_text(policy: Policy, lang: Language) -> &'static str {
    lang.text(match policy {
        Policy::Ask => Msg::TransferPolicyAsk,
        Policy::Overwrite => Msg::TransferPolicyOverwrite,
        Policy::Skip => Msg::TransferPolicySkip,
    })
}

fn mono_name<'a>(name: String, palette: &Palette) -> Element<'a, Message> {
    text(name)
        .font(Font::MONOSPACE)
        .size(font_size::FILE)
        .color(palette.text)
        .wrapping(Wrapping::None)
        .into()
}

fn plain_dim<'a>(content: String, palette: &Palette) -> Element<'a, Message> {
    text(content)
        .size(font_size::BODY)
        .color(palette.dim_text)
        .wrapping(Wrapping::None)
        .into()
}

/// "{count} entries · {size}" under the title, the size an ellipsis until the
/// scan has it.
fn summary(dialog: &TransferDialog, lang: Language) -> String {
    let missing = if dialog.counts_unknown() {
        "–"
    } else {
        COUNTING
    };
    let size = dialog
        .scan()
        .map_or_else(|| missing.to_string(), |scan| format::size(scan.bytes));
    let count = dialog.sources().len();
    let template = lang.text(if count == 1 {
        Msg::TransferSummaryOne
    } else {
        Msg::TransferSummary
    });
    template
        .replace("{count}", &count.to_string())
        .replace("{size}", &size)
}

/// The numbers of a large selection, in the box of the kit.
fn figures<'a>(
    dialog: &TransferDialog,
    line_chars: usize,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let scan = dialog.scan();
    // Nothing is counted on a server: the figures are not coming.
    let missing = if dialog.counts_unknown() {
        "–"
    } else {
        COUNTING
    };
    let counted = |value: Option<String>| value.unwrap_or_else(|| missing.to_string());
    let mut numbers = vec![
        (
            lang.text(Msg::TransferFigureFiles),
            counted(scan.map(|found| found.files.to_string())),
        ),
        (
            lang.text(Msg::TransferFigureFolders),
            counted(scan.map(|found| found.folders.to_string())),
        ),
        (
            lang.text(Msg::TransferFigureSize),
            counted(scan.map(|found| format::size(found.bytes))),
        ),
    ];
    if let Some(found) = scan.filter(|found| found.unreadable > 0) {
        numbers.push((
            lang.text(Msg::TransferFigureUnreadable),
            found.unreadable.to_string(),
        ));
    }
    let names: Vec<String> = dialog.names().take(FIRST_NAMES).collect();
    let left_out = dialog.sources().len().saturating_sub(names.len());
    kit::count_summary(
        &numbers,
        Some(first_names(&names, left_out, line_chars, lang, palette)),
        dialog.list_open(),
        send(TransferDialogMessage::ToggleList),
        lang.text(Msg::TransferShowList),
        lang.text(Msg::TransferHideList),
        palette,
    )
}

/// "a, b, c and 9 997 more": the names in the monospace font, the words around
/// them where the language puts them.
fn first_names<'a>(
    names: &[String],
    left_out: usize,
    line_chars: usize,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let template = lang.text(Msg::TransferFirstNames);
    let (head, tail) = template.split_once("{names}").unwrap_or(("", template));
    let count = left_out.to_string();
    let before = head.replace("{count}", &count);
    let after = tail.replace("{count}", &count);
    let room = line_chars.saturating_sub(before.chars().count() + after.chars().count());
    let joined = format::ellipsize_end(&names.join(", "), room);
    Row::with_children([
        plain_dim(before, palette),
        mono_name(joined, palette),
        plain_dim(after, palette),
    ])
    .into()
}

/// Every selected name, in a box that scrolls.
fn name_list<'a>(
    dialog: &TransferDialog,
    line_chars: usize,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut lines: Column<'a, Message> = Column::new().spacing(spacing::XS / 2.0);
    for name in dialog.names().take(LISTED_NAMES) {
        lines = lines.push(mono_name(
            format::ellipsize_middle(&name, line_chars),
            palette,
        ));
    }
    let left_out = dialog.sources().len().saturating_sub(LISTED_NAMES);
    if left_out > 0 {
        lines = lines.push(plain_dim(
            lang.text(Msg::TransferListMore)
                .replace("{count}", &left_out.to_string()),
            palette,
        ));
    }
    container(scrollable(container(lines).padding([spacing::S, spacing::M])).height(LIST_HEIGHT))
        .width(Length::Fill)
        .style(theme::outlined(palette))
        .into()
}

/// A short selection: all the names, one under the other.
fn names_directly<'a>(
    dialog: &TransferDialog,
    line_chars: usize,
    palette: &Palette,
) -> Element<'a, Message> {
    let lines = Column::with_children(dialog.names().map(|name| {
        mono_name(
            format::ellipsize_middle(&name, line_chars.saturating_sub(4)),
            palette,
        )
    }))
    .spacing(spacing::XS / 2.0);
    container(lines)
        .padding([spacing::S, spacing::M])
        .width(Length::Fill)
        .style(theme::outlined(palette))
        .into()
}

fn conflict_warning<'a>(count: usize, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let text = if count == 1 {
        lang.text(Msg::TransferConflictsOne).to_string()
    } else {
        lang.text(Msg::TransferConflicts)
            .replace("{count}", &count.to_string())
    };
    kit::warning_box(text, palette)
}

/// The confirmation before F5 or F6 starts anything.
pub fn confirm<'a>(
    dialog: &'a TransferDialog,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let kind = dialog.kind();
    let line_chars = frame::content_chars(SIZE, window, 0.0);

    let kinds = [
        (
            format!("{} F5", title(TransferKind::Copy, lang)),
            send(TransferDialogMessage::SetKind(TransferKind::Copy)),
        ),
        (
            format!("{} F6", title(TransferKind::Move, lang)),
            send(TransferDialogMessage::SetKind(TransferKind::Move)),
        ),
    ];
    let kind_options: Vec<(&str, Message)> = kinds
        .iter()
        .map(|(label, message)| (label.as_str(), message.clone()))
        .collect();
    let switch = row![kit::segmented(
        &kind_options,
        match kind {
            TransferKind::Copy => 0,
            TransferKind::Move => 1,
        },
        true,
        palette,
    )];

    let mut content = column![switch].spacing(spacing::L);
    content = content.push(if dialog.names_directly() {
        names_directly(dialog, line_chars, palette)
    } else if dialog.list_open() {
        column![
            figures(dialog, line_chars, lang, palette),
            name_list(dialog, line_chars, lang, palette)
        ]
        .spacing(spacing::S)
        .into()
    } else {
        figures(dialog, line_chars, lang, palette)
    });
    if let Some(count) = dialog.conflicts().filter(|count| *count > 0) {
        content = content.push(conflict_warning(count, lang, palette));
    }
    let hint = dialog_ui::numbered_hint(
        lang.text(Msg::HintNumberedInTarget),
        dialog.numbered(),
        dialog.hit_scope(),
        lang,
        palette,
    );
    if !hint.is_empty() {
        content = content.push(Column::with_children(hint).spacing(spacing::LINE_GAP));
    }

    let label = lang.text(Msg::TransferTarget);
    let mut target = column![
        kit::dim(label, palette),
        kit::mono_text_field(
            label,
            dialog.target_text(),
            TARGET_FIELD_ID,
            Some(|typed| send(TransferDialogMessage::TargetInput(typed))),
            send(TransferDialogMessage::Confirm),
            palette,
        ),
    ]
    .spacing(spacing::XS);
    if let Some(refusal) = dialog.refusal() {
        target = target.push(kit::error_line(refusal_text(refusal, lang), palette));
    }
    content = content.push(target);

    let policy_labels: Vec<&str> = Policy::ALL
        .iter()
        .map(|policy| policy_text(*policy, lang))
        .collect();
    let policy_options: Vec<(&str, Message)> = Policy::ALL
        .iter()
        .zip(&policy_labels)
        .map(|(policy, text)| (*text, send(TransferDialogMessage::SetPolicy(*policy))))
        .collect();
    let selected_policy = Policy::ALL
        .iter()
        .position(|policy| *policy == dialog.policy())
        .unwrap_or_default();
    content = content.push(
        row![
            container(kit::dim(lang.text(Msg::TransferPolicy), palette)).width(LABEL_WIDTH),
            kit::segmented(&policy_options, selected_policy, true, palette),
        ]
        .spacing(spacing::L)
        .align_y(alignment::Vertical::Center),
    );

    Frame::new(title(kind, lang), content, SIZE, window, lang, palette)
        .subtitle(summary(dialog, lang))
        .footer_note(lang.text(Msg::TransferSwitchHint))
        .buttons(vec![
            kit::button(
                lang.text(Msg::DialogCancel),
                Some(send(TransferDialogMessage::Cancel)),
                Tone::Secondary,
                palette,
            ),
            kit::enter_button(
                title(kind, lang),
                Some(send(TransferDialogMessage::Confirm)),
                Tone::Primary,
                palette,
            ),
        ])
        .on_close(send(TransferDialogMessage::Cancel))
        .into_element()
}

/// What the progress mask shows of the running transfer.
pub struct Progress<'a> {
    pub kind: TransferKind,
    pub target: &'a Path,
    pub done: usize,
    pub total: usize,
}

/// The mask over a running copy or move. Closing it leaves the job running.
pub fn progress<'a>(
    shown: &Progress<'a>,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let line_chars = frame::content_chars(SIZE, window, 0.0);
    let target = format::ellipsize_middle(
        &shown.target.display().to_string(),
        line_chars.saturating_sub(24),
    );
    let counter = lang
        .text(Msg::JobProgress)
        .replace("{done}", &shown.done.to_string())
        .replace("{total}", &shown.total.to_string());
    let fraction = if shown.total == 0 {
        0.0
    } else {
        shown.done as f32 / shown.total as f32
    };
    let content = column![
        kit::dim(counter, palette),
        kit::progress_bar(fraction, palette)
    ]
    .spacing(spacing::LINE_GAP);

    Frame::new(
        lang.text(shown.kind.job_kind().label()),
        content,
        SIZE,
        window,
        lang,
        palette,
    )
    .subtitle(
        lang.text(Msg::TransferProgressTo)
            .replace("{target}", &target),
    )
    .footer_note(lang.text(Msg::TransferProgressHint))
    .buttons(vec![
        kit::button(
            lang.text(Msg::TransferBackground),
            Some(send(TransferDialogMessage::Background)),
            Tone::Secondary,
            palette,
        ),
        kit::button(
            lang.text(Msg::JobAbort),
            Some(Message::AbortJob),
            Tone::Secondary,
            palette,
        ),
    ])
    .on_close(send(TransferDialogMessage::Background))
    .into_element()
}
