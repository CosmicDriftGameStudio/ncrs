//! Where the documents of a proposed rule come from.

use ncrs_i18n::Msg;
use ncrs_ui_kit::hermes_kit::FactRow;

use crate::ctx::Ctx;
use crate::texts::{folder_name, text};

/// The row of the proposal that says which read folders the rule looks at.
/// Without read folders there is no row.
pub fn from_fact(ctx: &Ctx) -> Option<FactRow> {
    let value = match ctx.sources.as_slice() {
        [] => return None,
        [only] => folder_name(ctx, only),
        _ => text(ctx.lang, Msg::HermesChatProposalFromAll),
    };
    Some(FactRow {
        label: text(ctx.lang, Msg::HermesChatProposalFrom),
        value,
    })
}
