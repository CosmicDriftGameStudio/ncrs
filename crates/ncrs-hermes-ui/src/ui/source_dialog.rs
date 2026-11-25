//! The dialog of a source, and of the one-off filing of a folder. Both are the
//! same draft; the once dialog does not watch and saves nothing, it shows the
//! plan.

use std::path::Path;

use iced::widget::{Column, Row};
use iced::{alignment, Element, Size};
use ncrs_config::config::Filing;
use ncrs_config::palette::Palette;
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_intake::{LocationTransfer, OnNoText, OnSeveralTargets};
use ncrs_ui_kit::frame::{DialogSize, Frame, KeyHints};
use ncrs_ui_kit::format::shown_path;
use ncrs_ui_kit::hermes_kit::{self as page_kit, TargetCheck, GAP_S, GAP_XS};
use ncrs_ui_kit::key_intercept::key_intercept_pressed;
use ncrs_ui_kit::kit::{self, Check, Tone};

use crate::message::Message;
use crate::sorting_page::SortingMessage;
use crate::source_dialog::{SourceDialog, SourceDialogKind, SourceField, SourceProblem};
use crate::target_dialog::{target_name, TargetsMessage};

pub const FOLDER_FIELD_ID: &str = "source-dialog-folder";
pub const INCLUDE_FIELD_ID: &str = "source-dialog-include";
pub const EXCLUDE_FIELD_ID: &str = "source-dialog-exclude";

fn send(message: SortingMessage) -> Message {
    Message::Sorting(message)
}

pub fn view<'a>(
    dialog: &SourceDialog,
    filing: &Filing,
    home: &Path,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let problems = dialog.problems(filing);
    let once = dialog.kind == SourceDialogKind::Once;
    let submit = send(SortingMessage::Save);
    let mut body = Column::new().spacing(GAP_S);

    let folder_field = Row::new()
        .spacing(GAP_S)
        .align_y(alignment::Vertical::Center)
        .push(noticed(
            SourceField::Folder,
            page_kit::text_field(
                "",
                &dialog.folder,
                FOLDER_FIELD_ID,
                Some(|typed| send(SortingMessage::FolderText(typed))),
                submit.clone(),
                palette,
            ),
        ))
        .push(kit::button(
            lang.text(Msg::SettingsChoose),
            Some(send(SortingMessage::PickFolder)),
            Tone::Secondary,
            palette,
        ));
    body = body.push(page_kit::form_row(
        lang.text(Msg::SortingFieldFolder),
        folder_field.into(),
        problem_line(&problems, lang, palette, |problem| {
            matches!(
                problem,
                SourceProblem::FolderMissing | SourceProblem::InsideTarget
            )
        }),
        palette,
    ));
    let glob_problem = |at: SourceField| {
        problem_line(&problems, lang, palette, move |problem| {
            matches!(problem, SourceProblem::Glob { field, .. } if *field == at)
        })
    };
    body = body.push(page_kit::form_row(
        lang.text(Msg::SortingFieldInclude),
        noticed(
            SourceField::Include,
            page_kit::mono_text_field(
                lang.text(Msg::SortingIncludePlaceholder),
                &dialog.include,
                INCLUDE_FIELD_ID,
                Some(|typed| send(SortingMessage::Include(typed))),
                submit.clone(),
                palette,
            ),
        ),
        glob_problem(SourceField::Include),
        palette,
    ));
    body = body.push(page_kit::form_row(
        lang.text(Msg::SortingFieldExclude),
        noticed(
            SourceField::Exclude,
            page_kit::mono_text_field(
                "",
                &dialog.exclude,
                EXCLUDE_FIELD_ID,
                Some(|typed| send(SortingMessage::Exclude(typed))),
                submit,
                palette,
            ),
        ),
        glob_problem(SourceField::Exclude),
        palette,
    ));
    body = body.push(page_kit::form_row(
        lang.text(Msg::SortingFieldTargets),
        targets_list(dialog, filing, home, lang, palette),
        problem_line(&problems, lang, palette, |problem| {
            matches!(problem, SourceProblem::NoTarget)
        }),
        palette,
    ));
    body = body.push(page_kit::form_row(
        lang.text(Msg::SortingFieldWhen),
        kit::focusable_segmented(
            &[
                (lang.text(Msg::SortingWhenWatch), send(SortingMessage::SetWatch(true))),
                (lang.text(Msg::SortingWhenOnce), send(SortingMessage::SetWatch(false))),
            ],
            usize::from(!dialog.watch),
            !once,
            dialog.focus == SourceField::When,
            palette,
        ),
        (dialog.watch && dialog.replacing.is_none()).then(|| {
            kit::paragraph(lang.text(Msg::SortingHintExistingStay).to_owned(), palette)
        }),
        palette,
    ));
    body = body.push(choice_row(
        lang.text(Msg::SortingFieldFiles),
        [
            (Msg::SortingFilesCopy, SortingMessage::SetTransfer(LocationTransfer::Copy)),
            (Msg::SortingFilesMove, SortingMessage::SetTransfer(LocationTransfer::Move)),
        ],
        usize::from(dialog.transfer == LocationTransfer::Move),
        dialog.focus == SourceField::Files,
        lang,
        palette,
    ));
    body = body.push(choice_row(
        lang.text(Msg::SortingFieldNoText),
        [
            (Msg::SortingNoTextAsk, SortingMessage::SetNoText(OnNoText::Ask)),
            (Msg::SortingNoTextSkip, SortingMessage::SetNoText(OnNoText::Skip)),
        ],
        usize::from(dialog.no_text == OnNoText::Skip),
        dialog.focus == SourceField::NoText,
        lang,
        palette,
    ));
    body = body.push(choice_row(
        lang.text(Msg::SortingFieldSeveral),
        [
            (Msg::SortingSeveralAsk, SortingMessage::SetSeveral(OnSeveralTargets::Ask)),
            (
                Msg::SortingSeveralCopyAll,
                SortingMessage::SetSeveral(OnSeveralTargets::CopyToAll),
            ),
        ],
        usize::from(dialog.several == OnSeveralTargets::CopyToAll),
        dialog.focus == SourceField::Several,
        lang,
        palette,
    ));

    let cancel = kit::button(
        lang.text(Msg::DialogCancel),
        Some(send(SortingMessage::Cancel)),
        Tone::Secondary,
        palette,
    );
    let ready = dialog.can_be_saved(filing);
    let (title, action) = if once {
        (
            Msg::SortingOnceTitle,
            kit::button(
                lang.text(Msg::SortingShowPlan),
                ready.then(|| send(SortingMessage::ShowPlan)),
                Tone::Primary,
                palette,
            ),
        )
    } else {
        (
            Msg::SortingSourceTitle,
            kit::button(
                lang.text(Msg::SavedQueriesSave),
                ready.then(|| send(SortingMessage::Save)),
                Tone::Primary,
                palette,
            ),
        )
    };
    let frame = Frame::new(lang.text(title), body, DialogSize::Wide, window, lang, palette);
    let folder = dialog.path(home);
    let subject = if once {
        folder_below_its_parent(&folder)
    } else {
        folder_name(&folder)
    };
    let frame = match subject {
        Some(folder) => frame.subtitle(folder),
        None => frame,
    };
    frame
        .buttons(vec![cancel, action])
        .on_close(send(SortingMessage::Cancel))
        .hints(KeyHints {
            enter: true,
            tab: true,
        })
        .into_element()
}

/// The field tells the dialog when the mouse goes into it.
fn noticed<'a>(field: SourceField, input: Element<'a, Message>) -> Element<'a, Message> {
    key_intercept_pressed(input, |_, _, _| None, move || send(SortingMessage::Focus(field)))
}

fn folder_name(folder: &Path) -> Option<String> {
    folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

/// "USB stick/Old": a folder filed one time is not in the list of the sources,
/// so its name alone says little.
fn folder_below_its_parent(folder: &Path) -> Option<String> {
    let name = folder_name(folder)?;
    let parent = folder.parent().and_then(folder_name);
    Some(match parent {
        Some(above) => format!("{above}{}{name}", std::path::MAIN_SEPARATOR),
        None => name,
    })
}

fn choice_row<'a>(
    label: &str,
    options: [(Msg, SortingMessage); 2],
    selected: usize,
    focused: bool,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let labelled: Vec<(&str, Message)> = options
        .into_iter()
        .map(|(text, message)| (lang.text(text), send(message)))
        .collect();
    page_kit::form_row(
        label,
        kit::focusable_segmented(&labelled, selected, true, focused, palette),
        None,
        palette,
    )
}

fn targets_list<'a>(
    dialog: &SourceDialog,
    filing: &Filing,
    home: &Path,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut list = Column::new().spacing(GAP_XS);
    for (at, setting) in filing.targets.iter().enumerate() {
        let target = &setting.target;
        let state = if dialog.targets.contains(&target.id) {
            Check::On
        } else {
            Check::Off
        };
        list = list.push(page_kit::target_check_row(
            TargetCheck {
                state,
                on_toggle: Some(send(SortingMessage::ToggleTarget(target.id.clone()))),
                index: at,
                number: &format!("T{}", at + 1),
                name: &target_name(target),
                path: &shown_path(&target.path, home),
                marked: dialog.focus == SourceField::Target(at),
            },
            palette,
        ));
    }
    if filing.targets.is_empty() {
        list = list.push(page_kit::hint(
            lang.text(Msg::SortingTargetNeeded).to_owned(),
            palette,
        ));
    }
    list.push(kit::button(
        lang.text(Msg::SortingAddTarget),
        Some(Message::Targets(TargetsMessage::Add)),
        Tone::filled_if(dialog.focus == SourceField::AddTarget),
        palette,
    ))
    .into()
}

fn problem_line<'a>(
    problems: &[SourceProblem],
    lang: Language,
    palette: &Palette,
    belongs: impl Fn(&SourceProblem) -> bool,
) -> Option<Element<'a, Message>> {
    let problem = problems.iter().find(|problem| belongs(problem))?;
    let text = match problem {
        SourceProblem::FolderMissing => lang.text(Msg::SortingProblemFolderMissing).to_owned(),
        SourceProblem::InsideTarget => lang.text(Msg::SortingProblemInsideTarget).to_owned(),
        SourceProblem::NoTarget => lang.text(Msg::SortingProblemNoTarget).to_owned(),
        SourceProblem::Glob { pattern, .. } => fill_template(
            lang.text(Msg::SortingGlobProblem),
            &[("{pattern}", pattern)],
        ),
    };
    Some(match problem {
        // Nothing is wrong with a draft that is just not finished.
        SourceProblem::NoTarget => page_kit::hint(text, palette),
        _ => page_kit::error_line(text, palette),
    })
}
