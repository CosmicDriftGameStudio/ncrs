//! An order for every area at once ("überall nach Jahr"): the same pattern
//! preview as "everywhere like this", reached from a typed sentence.

use std::sync::Arc;

use ncrs_ai::Order;
use ncrs_filing::chat_work::{ChatWork, FolderReading, PatternLevel};
use ncrs_i18n::Msg;

use crate::ctx::Ctx;
use crate::patterns;
use crate::state::{Everywhere, ReadFor, State, Step};
use crate::texts::text;
use crate::update::{fallback_chips, say, target_chips, work};

/// The folder levels an order stands for; `None` for a flat order, which
/// has no pattern to show.
fn levels_of(order: Option<Order>) -> Option<Vec<PatternLevel>> {
    match order {
        None => Some(Vec::new()),
        Some(Order::ByYear) => Some(vec![PatternLevel::Year]),
        Some(Order::ByMonth) => Some(vec![PatternLevel::Year, PatternLevel::Month]),
        Some(Order::BySender) => Some(vec![PatternLevel::Sender]),
        Some(Order::ByKind) => Some(vec![PatternLevel::Kind]),
        Some(Order::Flat) => None,
    }
}

pub fn start(state: &mut State, ctx: &Ctx, order: Option<Order>) {
    let Some(levels) = levels_of(order) else {
        state.step = Step::AfterChange;
        say(
            state,
            text(ctx.lang, Msg::HermesChatNotYetPropose),
            fallback_chips(ctx),
        );
        return;
    };
    let everywhere = Everywhere {
        levels,
        name: state.session.change_name.take(),
    };
    match state.session.learning.clone() {
        Some(reading) => preview(state, ctx, &reading, everywhere),
        None => {
            state.step = Step::ReadFolder;
            state.session.read_for = Some(ReadFor::Learn);
            state.session.everywhere = Some(everywhere);
            say(
                state,
                text(ctx.lang, Msg::HermesChatChangeNeedsReading),
                target_chips(state, ctx),
            );
        }
    }
}

/// A folder was read for an order that waited for it. False when none did.
pub fn after_read(state: &mut State, ctx: &Ctx, reading: &Arc<FolderReading>) -> bool {
    let Some(everywhere) = state.session.everywhere.take() else {
        return false;
    };
    if reading.files.is_empty() {
        return false;
    }
    state.session.learning = Some(Arc::clone(reading));
    preview(state, ctx, reading, everywhere);
    true
}

fn preview(state: &mut State, ctx: &Ctx, reading: &Arc<FolderReading>, everywhere: Everywhere) {
    state.session.change_word = patterns::levels_words(ctx, &everywhere.levels);
    work(
        state,
        ChatWork::PreviewPattern {
            reading: Arc::clone(reading),
            levels: everywhere.levels,
            name: everywhere.name,
        },
        None,
    );
}
