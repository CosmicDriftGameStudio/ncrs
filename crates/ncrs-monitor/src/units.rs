//! Numbers as the page writes them: at most three digits, no unit for
//! plain bytes.

use std::time::Duration;

const UNITS: [char; 5] = ['K', 'M', 'G', 'T', 'P'];

/// `3.9K`, `88.1M`, `824G`; below 1000 just the number. The unit is chosen by
/// what is shown, so rounding never leaves `1024K` or four raw digits.
pub fn short_size(bytes: u64) -> String {
    let mut value = bytes as f64;
    let mut unit = None;
    for candidate in UNITS {
        if shown_value(value) < 1000.0 {
            break;
        }
        value /= 1024.0;
        unit = Some(candidate);
    }
    let Some(suffix) = unit else {
        return bytes.to_string();
    };
    let shown = shown_value(value);
    let decimals = usize::from(shown < 100.0);
    format!("{shown:.decimals$}{suffix}")
}

/// `value` rounded the way `short_size` writes it: one decimal below 100, none above.
fn shown_value(value: f64) -> f64 {
    let one_decimal = (value * 10.0).round() / 10.0;
    if one_decimal < 100.0 {
        one_decimal
    } else {
        value.round()
    }
}

/// Bytes a second in the unit the setting asks for; bits get a `b`.
pub fn short_rate(bytes_per_second: u64, bits: bool) -> String {
    if bits {
        format!("{}b", short_size(bytes_per_second.saturating_mul(8)))
    } else {
        short_size(bytes_per_second)
    }
}

/// `3.9K` with the unit of the setting, or `–` for no value.
pub fn rate_or_dash(bytes_per_second: Option<u64>, bits: bool) -> String {
    bytes_per_second.map_or_else(|| DASH.to_owned(), |rate| short_rate(rate, bits))
}

pub const DASH: &str = "\u{2013}";

/// `69.4%` for a meter, whose title does not say the unit.
pub fn percent_or_dash(percent: Option<f32>) -> String {
    percent.map_or_else(|| DASH.to_owned(), |value| format!("{value:.1}%"))
}

/// `69.4` for a table column whose header already says `%`.
pub fn one_decimal_or_dash(value: Option<f32>) -> String {
    value.map_or_else(|| DASH.to_owned(), |decimal| format!("{decimal:.1}"))
}

/// What passed between two readings of a counter, as a rate. `None` when no
/// time has passed.
pub fn rate_per_second(delta: u64, elapsed: Duration) -> Option<u64> {
    let seconds = elapsed.as_secs_f64();
    (seconds > 0.0).then(|| (delta as f64 / seconds).round() as u64)
}

/// `0:09`, `1:04`, `6:38:10`.
pub fn clock_time(total_seconds: u64) -> String {
    let (hours, minutes, seconds) = (
        total_seconds.div_euclid(3600),
        total_seconds.div_euclid(60) % 60,
        total_seconds % 60,
    );
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// `31 d 14:23` or `14:23`.
pub fn uptime_text(seconds: u64) -> String {
    let (days, hours, minutes) = (
        seconds.div_euclid(86_400),
        seconds.div_euclid(3600) % 24,
        seconds.div_euclid(60) % 60,
    );
    if days > 0 {
        format!("{days} d {hours}:{minutes:02}")
    } else {
        format!("{hours}:{minutes:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_keep_at_most_three_digits() {
        assert_eq!(short_size(0), "0");
        assert_eq!(short_size(512), "512");
        assert_eq!(short_size(4_000), "3.9K");
        assert_eq!(short_size(88 * 1024 * 1024 + 100_000), "88.1M");
        assert_eq!(short_size(824 * 1024 * 1024 * 1024), "824G");
    }

    #[test]
    fn the_unit_is_picked_by_the_rounded_value() {
        assert_eq!(short_size(999), "999");
        assert_eq!(short_size(1000), "1.0K");
        assert_eq!(short_size(1023), "1.0K");
        assert_eq!(short_size(1024), "1.0K");
        assert_eq!(short_size(102_349), "100K");
        assert_eq!(short_size(1_023 * 1024 + 900), "1.0M");
        assert_eq!(short_size(1_000 * 1024), "1.0M");
        assert_eq!(short_size(999 * 1024), "999K");
        assert_eq!(short_size(u64::MAX), "16384P");
    }

    #[test]
    fn bits_are_eight_times_the_bytes() {
        assert_eq!(short_rate(1024, true), "8.0Kb");
        assert_eq!(short_rate(1024, false), "1.0K");
    }

    #[test]
    fn a_rate_is_the_difference_over_the_time_that_really_passed() {
        assert_eq!(
            rate_per_second(3000, Duration::from_millis(1500)),
            Some(2000)
        );
        assert_eq!(rate_per_second(10, Duration::ZERO), None);
    }

    #[test]
    fn times_read_like_the_design() {
        assert_eq!(clock_time(9), "0:09");
        assert_eq!(clock_time(6 * 3600 + 38 * 60 + 10), "6:38:10");
        assert_eq!(uptime_text(31 * 86_400 + 14 * 3600 + 23 * 60), "31 d 14:23");
        assert_eq!(uptime_text(3600), "1:00");
    }
}
