//! The state changes of the Hermes pages. What reaches outside them comes back
//! as actions. Each page has its own entry; the app routes the messages.

mod quick_fix;
mod reorganisation;
mod review_list;

use iced::Task;
use ncrs_filing::replan_work::ReplanScope;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::{Message, QuickFixMessage, ReorganisationPlanMessage, ReviewListMessage};
use crate::state::Hermes;

pub use self::review_list::{request_review_preview, run_review_load};

pub type Outcome = (Task<Message>, Vec<Action>);

/// The pages and the menu of the Hermes state count, as they are now.
fn blocking_overlay_is_open(hermes: &Hermes, ctx: &Ctx<'_>) -> bool {
    ctx.overlay_apart_from_hermes_open || hermes.page_is_open() || hermes.quick_fix.is_some()
}

pub fn update_review_list(
    hermes: &mut Hermes,
    message: ReviewListMessage,
    ctx: &Ctx<'_>,
) -> Outcome {
    let mut actions = Vec::new();
    let task = review_list::update(hermes, message, ctx, &mut actions);
    (task, actions)
}

pub fn update_quick_fix(hermes: &mut Hermes, message: QuickFixMessage, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = quick_fix::update(hermes, message, ctx, &mut actions);
    (task, actions)
}

pub fn update_reorganisation(
    hermes: &mut Hermes,
    message: ReorganisationPlanMessage,
    ctx: &Ctx<'_>,
) -> Outcome {
    let mut actions = Vec::new();
    let task = reorganisation::update(hermes, message, ctx, &mut actions);
    (task, actions)
}

pub fn open_review_list(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = review_list::open(hermes, ctx, &mut actions);
    (task, actions)
}

pub fn start_replan(hermes: &mut Hermes, scope: ReplanScope, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = review_list::start_replan(hermes, scope, ctx, &mut actions);
    (task, actions)
}

pub fn open_reorganisation(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = reorganisation::open(hermes, ctx, &mut actions);
    (task, actions)
}

pub use self::reorganisation::replan_if_open as replan_reorganisation_if_open;
