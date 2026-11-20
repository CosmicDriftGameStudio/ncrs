//! UI tests: what the user actually does, driven through `iced_test`.
//!
//! `iced`'s own harness. `Simulator` renders a view headless and can click,
//! type, tap keys and take snapshots; `Emulator` runs the real application for
//! end-to-end tests.
//!
//! This covers what the state tests cannot: that a click lands where it should,
//! that typed text arrives, and that the layout is the shape it should be. The
//! state tests in `app.rs` cover the routing decisions; both are needed,
//! because correct `update` behind a button nobody can click is not a feature.
//!
//! These tests run on every operating system in CI, but the pixel verdict of the
//! snapshot comparisons only counts on macOS (see `.github/workflows/ci.yml`).
//! The reference images in `tests/snapshots/` were produced there, and font
//! rasterisation differs per operating system: a reference made on macOS does
//! not match Windows output. Elsewhere `ReferenceSnapshot` reports a match
//! without comparing, and the tests that only make sense with a real
//! comparison ("this view differs from that reference") are compiled for macOS
//! only.
//!
//! Two limits worth knowing, both from iced 0.14:
//!
//! - `iced_selector` finds widgets by `widget::Id`, and `Button` has no `id`
//!   method. So widgets are addressed by the position the layout reports for
//!   them rather than by name.
//! - `Snapshot` exposes no bytes, only a comparison against a file. The
//!   reference is written on the first run.
mod own_server;

use iced_test::Simulator;
use ncrs_ui_kit::snapshot_support::ReferenceSnapshot as _;

use crate::app::App;
use crate::filing::{FilingMessage, FilingModule, KeyProblem, Phase, SecretText};
use crate::i18n::Language;
use crate::messages::Message;
use crate::messages::PanelMessage;

/// Which panel the pointer is over is bookkeeping, not something the user did.
fn is_action(message: &Message) -> bool {
    !matches!(
        message,
        Message::Panel(PanelMessage::PanelEntered(_))
            | Message::Panel(PanelMessage::PanelExited(_))
            | Message::FocusPageThen(_, None)
    )
}

/// A click on a control of a page the app fills first moves the keyboard
/// focus to its row; this is what the control itself sends.
fn without_focus(message: Message) -> Message {
    match message {
        Message::FocusPageThen(_, Some(inner)) => *inner,
        other => other,
    }
}

fn simulator(app: &App) -> Simulator<'_, Message, iced::Theme> {
    Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1200.0, 760.0),
        app.view(),
    )
}

/// Renders like the real window, whose default font is the app's. The Hermes
/// pages set weights on top of it, so their snapshots need the same family.
fn app_simulator(app: &App) -> Simulator<'_, Message, iced::Theme> {
    Simulator::with_size(
        iced::Settings {
            default_font: ncrs_ui_kit::APP_FONT,
            ..iced::Settings::default()
        },
        iced::Size::new(1200.0, 760.0),
        app.view(),
    )
}

/// The outer bounds of the dialog frame in the view of `app`. Test-only like
/// the modules below, where a failing lookup is the signal.
fn frame_bounds(app: &App) -> iced::Rectangle {
    simulator(app)
        .find(iced::widget::Id::new(crate::ui::frame::FRAME_ID))
        .expect("the dialog frame")
        .bounds()
}

/// A confirm button that repeats the title of its dialog: plain text would
/// find the title, which comes first.
fn button_repeating_the_title(
    label: &str,
) -> impl FnMut(iced_selector::Candidate<'_>) -> Option<iced_selector::Target> + Send + '_ {
    let mut title_passed = false;
    move |candidate| {
        if !matches!(&candidate, iced_selector::Candidate::Text { content, .. } if *content == label)
        {
            return None;
        }
        if !title_passed {
            title_passed = true;
            return None;
        }
        Some(iced_selector::Target::from(candidate))
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod tests;

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod tagging;

#[cfg(test)]
mod conflict_overlay;

#[cfg(test)]
mod archive_panel;

#[cfg(test)]
mod transfer_overlay;

#[cfg(test)]
mod delete_overlay;

#[cfg(test)]
mod volume_overlay;

#[cfg(test)]
mod context_menu_overlay;

#[cfg(test)]
mod status_notice;

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod search_toggle_keys;

#[cfg(test)]
mod filing_cards;

#[cfg(test)]
mod filing_status_page;

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod command_palette;

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod theme_fetch_card;

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
pub mod search_filters_card;

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod saved_queries_card;

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod tags_card;

#[cfg(test)]
mod rules_card;

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod title_bar;

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod ocr_models_card;

#[cfg(test)]
mod about_card;

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod ai_model_card;

#[cfg(test)]
mod settings_tab;

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod command_line_view_tests;

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod key_bindings_dialog;

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod connect_dialog;

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod servers_page;

#[cfg(test)]
mod hit_list_snapshots;

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod quick_fix_menu_actions;

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod settings_index_page;

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod rename_row;
