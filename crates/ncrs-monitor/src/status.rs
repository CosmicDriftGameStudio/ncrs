//! The short text of the badge in the status bar of the main window.

use ncrs_config::system_monitor::{StatusStyle, StatusValue};

use crate::snapshot::Snapshot;
use crate::state::State;
use crate::units::{short_rate, DASH};

const FULL_BAR: char = '\u{2588}';

const BARS: [char; 8] = [
    '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}', '\u{2588}',
];

/// Loopback carries no traffic that left the computer.
fn is_loopback(name: &str) -> bool {
    matches!(name, "lo" | "lo0") || name.starts_with("Loopback")
}

fn bar(percent: f32) -> char {
    let step = (percent.clamp(0.0, 100.0) / 100.0 * (BARS.len() as f32 - 1.0)).round() as usize;
    BARS.get(step).copied().unwrap_or(FULL_BAR)
}

fn level(percent: Option<f32>, style: StatusStyle) -> String {
    match (percent, style) {
        (None, _) => DASH.to_owned(),
        (Some(value), StatusStyle::Number) => format!("{value:.0}%"),
        (Some(value), StatusStyle::Symbol) => bar(value).to_string(),
    }
}

fn network_text(snapshot: &Snapshot, bits: bool) -> String {
    let sum = |rate: fn(&crate::snapshot::NetworkRow) -> Option<u64>| {
        snapshot
            .networks
            .iter()
            .filter(|network| !is_loopback(&network.name))
            .filter_map(rate)
            .reduce(u64::saturating_add)
    };
    let part = |arrow: char, rate: Option<u64>| {
        format!(
            "{arrow}{}",
            rate.map_or_else(
                || DASH.to_owned(),
                |per_second| short_rate(per_second, bits)
            )
        )
    };
    format!(
        "{} {}",
        part('\u{2193}', sum(|network| network.received_per_second)),
        part('\u{2191}', sum(|network| network.transmitted_per_second)),
    )
}

/// `None` when no value is selected. Network and load are always numbers.
pub fn status_text(state: &State) -> Option<String> {
    let settings = state.settings();
    let values = settings.badge_values();
    if values.is_empty() {
        return None;
    }
    let snapshot = state.snapshot();
    let bits = settings.network_unit == ncrs_config::system_monitor::NetworkUnit::Bits;
    let parts: Vec<String> = values
        .into_iter()
        .map(|value| match value {
            StatusValue::Cpu => {
                let percent = snapshot
                    .and_then(|s| s.cpu.as_ref())
                    .and_then(|cpu| cpu.percent);
                format!("CPU {}", level(percent, settings.status_style))
            }
            StatusValue::Memory => {
                let percent = snapshot.and_then(|s| s.memory).map(|usage| usage.percent());
                format!("MEM {}", level(percent, settings.status_style))
            }
            StatusValue::Swap => {
                let percent = snapshot.and_then(|s| s.swap).map(|usage| usage.percent());
                format!("SWAP {}", level(percent, settings.status_style))
            }
            StatusValue::Load => {
                let one = snapshot
                    .and_then(|s| s.load)
                    .map(|load| format!("{:.2}", load.one));
                format!("LOAD {}", one.unwrap_or_else(|| DASH.to_owned()))
            }
            StatusValue::Network => snapshot.map_or_else(
                || format!("\u{2193}{DASH} \u{2191}{DASH}"),
                |measured| network_text(measured, bits),
            ),
        })
        .collect();
    Some(parts.join("  "))
}
