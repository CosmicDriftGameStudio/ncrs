//! The line shown while Hermes works: the dots move, and a read counts files.

use ncrs_filing::chat_work::ChatWork;
use ncrs_i18n::Msg;

use crate::ctx::Ctx;
use crate::state::{Entry, State};
use crate::texts::{fill, folder_name, number};

const DOTS: usize = 3;

/// The line with the dots after it that belong to the tick.
fn with_dots(line: &str, tick: usize) -> String {
    let base = line.trim_end_matches(['.', '…', ' ']);
    format!("{base} {}", ".".repeat(tick % DOTS + 1))
}

fn reading_line(state: &State, ctx: &Ctx) -> Option<String> {
    let Some(ChatWork::Read { folder, progress }) = state.pending.as_ref() else {
        return None;
    };
    let shown = folder_name(ctx, folder);
    let total = progress.total();
    Some(if total == 0 {
        fill(
            ctx.lang,
            Msg::HermesChatWorkingReadingFound,
            &[("folder", &shown), ("found", &number(progress.found()))],
        )
    } else {
        fill(
            ctx.lang,
            Msg::HermesChatWorkingReadingProgress,
            &[
                ("folder", &shown),
                ("done", &number(progress.done())),
                ("total", &number(total)),
            ],
        )
    })
}

/// A beat: the last working line gets its next dots, and a read its counts.
pub fn tick(state: &mut State, ctx: &Ctx) {
    state.tick = state.tick.wrapping_add(1);
    let reading = reading_line(state, ctx);
    let tick = state.tick;
    let Some(Entry::Working(line)) = state
        .thread
        .iter_mut()
        .rev()
        .find(|entry| matches!(entry, Entry::Working(_)))
    else {
        return;
    };
    let base = reading.as_deref().unwrap_or(line);
    *line = with_dots(base, tick);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dots_move_and_replace_the_ellipsis() {
        assert_eq!(with_dots("Ich lese …", 0), "Ich lese .");
        assert_eq!(with_dots("Ich lese .", 1), "Ich lese ..");
        assert_eq!(with_dots("Ich lese ..", 2), "Ich lese ...");
        assert_eq!(with_dots("Ich lese ...", 3), "Ich lese .");
    }
}
