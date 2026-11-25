//! The menus behind Pull and Push, and the question before a force push.

use ncrs_git::{ForcePushPlan, PullStrategy};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::kit::{paragraph, Tone};
use ncrs_ui_kit::popup_menu::{MenuRow, PopupEntry};

use super::overlay::dialog_frame;
use crate::actions::ForceBlock;
use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::sync_menu::{SyncAction, SyncKind, SyncMenu};

fn label_of(action: SyncAction) -> Msg {
    match action {
        SyncAction::Pull(PullStrategy::Merge) => Msg::GitViewPullMenuMerge,
        SyncAction::Pull(PullStrategy::Rebase) => Msg::GitViewPullMenuRebase,
        SyncAction::Pull(PullStrategy::FastForwardOnly) => Msg::GitViewPullMenuFastForward,
        SyncAction::Push => Msg::GitViewPushMenuPush,
        SyncAction::ForcePush => Msg::GitViewPushMenuForceWithLease,
    }
}

/// The rows of the open menu: the letter of each entry at the right, the
/// default of the Pull button marked.
pub fn rows(
    menu: &SyncMenu,
    blocked: Option<ForceBlock>,
    ctx: ViewCtx<'_>,
) -> Vec<MenuRow<Message>> {
    let mut rows = Vec::new();
    for (index, action) in menu.kind.entries().iter().copied().enumerate() {
        if action == SyncAction::ForcePush && menu.kind == SyncKind::Push {
            rows.push(MenuRow::Separator);
        }
        let letter = action.letter().map(|letter| letter.to_ascii_uppercase().to_string());
        let is_default = action == SyncAction::Pull(ctx.pull_default);
        let reason = match (action, blocked) {
            (SyncAction::ForcePush, Some(ForceBlock::Detached)) => Some(Msg::GitViewForceBlockedDetached),
            (SyncAction::ForcePush, Some(ForceBlock::NoUpstream)) => {
                Some(Msg::GitViewForceBlockedNoUpstream)
            }
            _ => None,
        };
        let key = match (is_default, letter) {
            _ if reason.is_some() => reason.map(|why| ctx.lang.text(why).to_owned()),
            (true, Some(letter)) => Some(format!(
                "{} \u{b7} {letter}",
                ctx.lang.text(Msg::GitViewMenuDefaultMark)
            )),
            (true, None) => Some(ctx.lang.text(Msg::GitViewMenuDefaultMark).to_owned()),
            (false, letter) => letter,
        };
        rows.push(MenuRow::Entry(PopupEntry {
            label: ctx.lang.text(label_of(action)).to_owned(),
            key,
            is_selected: menu.cursor == index,
            on_press: reason.is_none().then_some(Message::SyncMenuPressed(action)),
            on_hover: Some(Message::SyncMenuCursor(index)),
        }));
    }
    rows
}

/// What the force push replaces, in words; Enter cancels, the button does it.
pub fn force_push_dialog<'a>(plan: &ForcePushPlan, ctx: ViewCtx<'a>) -> iced::Element<'a, Message> {
    let lang = ctx.lang;
    let names = [
        ("{remote}", plan.remote.as_str()),
        ("{target}", plan.target.as_str()),
        ("{branch}", plan.branch.as_str()),
    ];
    let mut text = if plan.lost == 0 {
        fill_template(lang.text(Msg::GitViewAskForcePushTextNone), &names)
    } else {
        let count = plan.lost.to_string();
        fill_template(
            lang.text(Msg::GitViewAskForcePushText),
            &[names[0], names[1], names[2], ("{count}", &count)],
        )
    };
    for title in &plan.lost_titles {
        text.push_str("\n\u{2022} ");
        text.push_str(title);
    }
    dialog_frame(
        &fill_template(lang.text(Msg::GitViewAskForcePush), &names),
        paragraph(text, ctx.palette),
        lang.text(Msg::GitViewAskForcePushConfirm),
        Some(Message::ConfirmDialog),
        Tone::Danger,
        ctx,
    )
}
