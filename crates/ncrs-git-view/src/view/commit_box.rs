//! The commit box of the local changes: title with its counter, the longer
//! text, "amend", "push afterwards" and the commit button.

use iced::keyboard::key::Physical;
use iced::keyboard::{Key, Modifiers};
use iced::widget::{column, container, row, Space};
use iced::{alignment, Element, Length};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::key_intercept::key_intercept_pressed;
use ncrs_ui_kit::kit::{
    counted_text_field, focused, key_button, labelled_checkbox, text_area_hinted, Check, Tone,
};
use ncrs_ui_kit::page_text::page_line;
use ncrs_ui_kit::theme::{font_size, spacing};

use crate::actions::{LocalChanges, WipFocus};
use crate::ctx::ViewCtx;
use crate::keys::commit_field_key;
use crate::message::Message;
use crate::shortcuts::Shortcut;

pub const TITLE_ID: &str = "git-commit-title";
pub const BODY_ID: &str = "git-commit-body";
/// The length after which a title is shown in the warning color; it is not enforced.
pub const TITLE_LIMIT: usize = 72;
/// Height of the field for the longer commit text.
const BODY_HEIGHT: f32 = 84.0;

/// `has_keys`: the pane has the keyboard, so the stop of the Tab ring shows its focus.
pub fn view<'a>(local: &'a LocalChanges, has_keys: bool, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    let keys = ctx.keys;
    let staged = local.staged.len();
    let can_commit = staged > 0 || local.amend;
    let label = match staged {
        1 => lang.text(Msg::GitViewCommitOne).to_owned(),
        _ => fill_template(
            lang.text(Msg::GitViewCommitMany),
            &[("{count}", &staged.to_string())],
        ),
    };
    let length = local.title.chars().count();
    let counter = fill_template(
        lang.text(Msg::GitViewCommitCounter),
        &[
            ("{count}", &length.to_string()),
            ("{limit}", &TITLE_LIMIT.to_string()),
        ],
    );
    let ring = |stop: WipFocus| has_keys && local.focus == stop;
    let resolve = move |key: &Key, _physical: Physical, modifiers: Modifiers| {
        commit_field_key(keys, key, modifiers)
    };
    let mut form = column![
        key_intercept_pressed(
            focused(
                counted_text_field(
                    lang.text(Msg::GitViewCommitTitlePlaceholder),
                    &local.title,
                    (counter, length > TITLE_LIMIT),
                    TITLE_ID,
                    Some(Message::CommitTitleChanged),
                    Message::Commit,
                    palette,
                ),
                ring(WipFocus::Title),
                palette,
            ),
            resolve,
            || Message::WipFieldPressed(WipFocus::Title),
        ),
        focused(
            key_intercept_pressed(
                container(text_area_hinted(
                    &local.body,
                    BODY_ID,
                    lang.text(Msg::GitViewCommitBodyLabel),
                    Message::CommitBodyEdited,
                    palette,
                ))
                .height(BODY_HEIGHT),
                resolve,
                || Message::WipFieldPressed(WipFocus::Body),
            ),
            ring(WipFocus::Body),
            palette,
        ),
        row![
            focused(
                labelled_checkbox(
                    if local.amend { Check::On } else { Check::Off },
                    Some(Message::ToggleAmend),
                    Some(lang.text(Msg::GitViewCommitAmend)),
                    palette,
                ),
                ring(WipFocus::Amend),
                palette,
            ),
            focused(
                labelled_checkbox(
                    if local.push_after {
                        Check::On
                    } else {
                        Check::Off
                    },
                    Some(Message::TogglePushAfter),
                    Some(lang.text(Msg::GitViewCommitPushAfter)),
                    palette,
                ),
                ring(WipFocus::PushAfter),
                palette,
            ),
        ]
        .spacing(spacing::L)
        .align_y(alignment::Vertical::Center),
        row![
            Space::new().width(Length::Fill),
            focused(
                key_button(
                    &label,
                    &Shortcut::Commit.label(keys),
                    can_commit.then_some(Message::Commit),
                    Tone::Primary,
                    palette
                ),
                ring(WipFocus::Commit),
                palette,
            ),
        ],
    ]
    .spacing(spacing::M)
    .padding(spacing::M);
    if !can_commit {
        form = form.push(page_line(
            lang.text(Msg::GitViewNothingToCommitText),
            font_size::SMALL,
            palette.dim_text,
        ));
    }
    form.into()
}
