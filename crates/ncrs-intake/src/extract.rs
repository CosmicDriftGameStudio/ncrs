//! Reads a number, a date and an amount out of document text with the regex
//! a rule carries. The text is only ever searched, never acted on.

use chrono::NaiveDate;
use ncrs_index::{compile_bounded_regex, Amount, Currency};
use regex::{Captures, Regex};

use crate::sanitise::sanitise_value;

/// A pattern that is invalid or over the size limit is `None`: a rule that
/// cannot be compiled simply does not match.
pub fn compile(source: &str) -> Option<Regex> {
    compile_bounded_regex(source).ok()
}

/// Capture group 1 if the pattern has one that took part, else the whole match.
fn value_of<'text>(captures: &Captures<'text>) -> &'text str {
    captures
        .get(1)
        .or_else(|| captures.get(0))
        .map_or("", |found| found.as_str())
}

/// The first match `read` accepts. The matches are declared after the regex
/// so they are dropped before it.
fn first_match<T>(
    source: &str,
    text: &str,
    read: impl FnMut(Captures<'_>) -> Option<T>,
) -> Option<T> {
    let regex = compile(source)?;
    let mut matches = regex.captures_iter(text);
    matches.find_map(read)
}

/// The first match that is not empty after sanitising.
pub fn extract_number(source: &str, text: &str) -> Option<String> {
    first_match(source, text, |captures| sanitise_value(value_of(&captures)))
}

/// The first match that reads as a date.
pub fn extract_date(source: &str, text: &str) -> Option<NaiveDate> {
    first_match(source, text, |captures| parse_date(value_of(&captures)))
}

/// The first match that reads as an amount with a currency. The currency is a
/// named group `currency`, else a sign or code inside the match. The number
/// is capture group 1, so a `currency` group must come after it.
pub fn extract_amount(source: &str, text: &str) -> Option<Amount> {
    first_match(source, text, |captures| {
        let named = captures
            .name("currency")
            .and_then(|found| parse_currency(found.as_str()));
        let beside = || {
            let whole = captures.get(0)?;
            strip_currency(whole.as_str()).1
        };
        parse_amount(value_of(&captures), named.or_else(beside))
    })
}

/// `dd.mm.yyyy`, `yyyy-mm-dd` or `dd/mm/yyyy`; `None` for anything else and
/// for a day that does not exist.
pub fn parse_date(text: &str) -> Option<NaiveDate> {
    let value = text.trim();
    let (separator, year_first) = if value.contains('.') {
        ('.', false)
    } else if value.contains('-') {
        ('-', true)
    } else if value.contains('/') {
        ('/', false)
    } else {
        return None;
    };
    let mut parts = value.split(separator);
    let (Some(first), Some(second), Some(third), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    let (year, month, day) = if year_first {
        (first, second, third)
    } else {
        (third, second, first)
    };
    if year.len() != 4 {
        return None;
    }
    NaiveDate::from_ymd_opt(
        i32::try_from(ascii_number(year, 4)?).ok()?,
        ascii_number(month, 2)?,
        ascii_number(day, 2)?,
    )
}

fn ascii_number(part: &str, max_digits: usize) -> Option<u32> {
    if part.is_empty() || part.len() > max_digits || !part.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    part.parse().ok()
}

/// Codes recognised inside a match without a `currency` group. A short list
/// on purpose: any three capitals ("VAT", "NET") would otherwise read as one.
const CODES_IN_TEXT: [&str; 12] = [
    "EUR", "USD", "GBP", "CHF", "SEK", "NOK", "DKK", "PLN", "CZK", "JPY", "CAD", "AUD",
];

fn sign_currency(character: char) -> Option<Currency> {
    match character {
        '€' => Currency::new("EUR"),
        '$' => Currency::new("USD"),
        '£' => Currency::new("GBP"),
        _ => None,
    }
}

/// Any ISO code (`EUR`, in either case) or one of the signs `€`, `$`, `£`.
pub fn parse_currency(text: &str) -> Option<Currency> {
    let token = text.trim();
    let mut characters = token.chars();
    match (characters.next(), characters.next()) {
        (Some(sign), None) => sign_currency(sign),
        _ => Currency::new(&token.to_ascii_uppercase()),
    }
}

/// The value without any currency sign or code, and the currency it named.
fn strip_currency(value: &str) -> (String, Option<Currency>) {
    let mut currency = None;
    let without_signs: String = value
        .chars()
        .filter(|&character| {
            let sign = sign_currency(character);
            let is_sign = sign.is_some();
            if sign.is_some() {
                currency = sign;
            }
            !is_sign
        })
        .collect();
    let rest = without_signs
        .split_whitespace()
        .filter(|token| {
            let code = CODES_IN_TEXT
                .contains(token)
                .then(|| Currency::new(token))
                .flatten();
            let is_code = code.is_some();
            if code.is_some() {
                currency = code;
            }
            !is_code
        })
        .collect::<String>();
    (rest, currency)
}

/// "1.234,56", "1,234.56", "12.50" or "12" into minor units. The currency is
/// the one written in `value`, else `fallback`; with neither the amount is
/// `None`. One separator followed by exactly three digits is a thousands
/// separator ("1.234" is 1234), by one or two digits a decimal point.
pub fn parse_amount(value: &str, fallback: Option<Currency>) -> Option<Amount> {
    let (number, written) = strip_currency(value);
    let currency = written.or(fallback)?;
    let (whole, fraction) = split_number(&number)?;
    let fraction_minor = match fraction.len() {
        0 => 0,
        1 => ascii_number(fraction, 1)? * 10,
        _ => ascii_number(fraction, 2)?,
    };
    let minor_units = whole
        .parse::<i64>()
        .ok()?
        .checked_mul(100)?
        .checked_add(i64::from(fraction_minor))?;
    Some(Amount {
        minor_units,
        currency,
    })
}

fn is_separator(character: char) -> bool {
    character == '.' || character == ','
}

/// The digits before and after the decimal point; the whole part without its
/// thousands separators.
fn split_number(number: &str) -> Option<(String, &str)> {
    if !number
        .chars()
        .all(|character| character.is_ascii_digit() || is_separator(character))
    {
        return None;
    }
    let decimal_at = match (number.rfind('.'), number.rfind(',')) {
        (Some(dot), Some(comma)) => Some(dot.max(comma)),
        (Some(only), None) | (None, Some(only)) => {
            let repeated = number.chars().filter(|&c| is_separator(c)).count() > 1;
            let after = number.get(only + 1..)?;
            (!repeated && after.len() != 3).then_some(only)
        }
        (None, None) => None,
    };
    let (whole_text, fraction) = match decimal_at {
        Some(at) => {
            let (before, from) = number.split_at_checked(at)?;
            (before, from.get(1..)?)
        }
        None => (number, ""),
    };
    let mut groups = whole_text.split(is_separator);
    let first = groups.next()?;
    let grouped = whole_text.len() != first.len();
    let well_formed = !first.is_empty()
        && first.len() <= if grouped { 3 } else { usize::MAX }
        && groups.all(|group| group.len() == 3);
    let digits_only = whole_text
        .chars()
        .filter(|&c| !is_separator(c))
        .all(|c| c.is_ascii_digit());
    let fraction_ok = fraction.len() <= 2 && fraction.bytes().all(|b| b.is_ascii_digit());
    (well_formed && digits_only && fraction_ok)
        .then(|| (whole_text.replace(is_separator, ""), fraction))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    fn amount(value: &str, fallback: Option<&str>) -> Option<(i64, String)> {
        parse_amount(value, fallback.and_then(parse_currency))
            .map(|found| (found.minor_units, found.currency.as_str().to_owned()))
    }

    #[test]
    fn dates_in_the_three_formats_are_read() {
        let expected = NaiveDate::from_ymd_opt(2026, 3, 1);
        assert_eq!(parse_date("01.03.2026"), expected);
        assert_eq!(parse_date("2026-03-01"), expected);
        assert_eq!(parse_date("1/3/2026"), expected);
        assert_eq!(parse_date(" 1.3.2026 "), expected);
    }

    #[test]
    fn things_that_are_not_dates_are_refused() {
        for value in [
            "31.02.2026",
            "2026.03.01",
            "01.03.26",
            "1.2.3.2026",
            "",
            "a.b.cccc",
            "01.03.20260",
        ] {
            assert_eq!(parse_date(value), None, "{value:?}");
        }
    }

    #[test]
    fn amounts_in_either_notation_are_read_with_their_currency() {
        let eur = Some((123_456, "EUR".to_owned()));
        assert_eq!(amount("1.234,56 €", None), eur);
        assert_eq!(amount("EUR 1,234.56", None), eur);
        assert_eq!(amount("1234.56", Some("EUR")), eur);
        assert_eq!(amount("$12.50", None), Some((1250, "USD".to_owned())));
        assert_eq!(amount("12 £", None), Some((1200, "GBP".to_owned())));
        assert_eq!(amount("12.5 CHF", None), Some((1250, "CHF".to_owned())));
        assert_eq!(amount("1.234 €", None), Some((123_400, "EUR".to_owned())));
        assert_eq!(
            amount("1.234.567 €", None),
            Some((123_456_700, "EUR".to_owned()))
        );
    }

    #[test]
    fn an_amount_without_currency_or_with_a_malformed_number_is_refused() {
        assert_eq!(amount("12,50", None), None);
        for value in [
            "12,345.6.7 €",
            "1,23,4 €",
            "abc €",
            ",50 €",
            "99999999999999999999 €",
        ] {
            assert_eq!(amount(value, None), None, "{value:?}");
        }
    }

    #[test]
    fn the_currency_group_wins_over_a_sign_next_to_the_number() {
        let found = extract_amount(r"([\d,.]+) (?P<currency>[A-Z]{3})", "Total 5,00 USD").unwrap();
        assert_eq!((found.minor_units, found.currency.as_str()), (500, "USD"));
        let beside = extract_amount(r"Total: \$?([\d.]+)", "Total: $7.25").unwrap();
        assert_eq!((beside.minor_units, beside.currency.as_str()), (725, "USD"));
    }

    #[test]
    fn a_pattern_over_the_size_limit_is_not_compiled() {
        assert!(compile("(a{1000}){1000}{1000}").is_none());
        assert!(extract_number("(", "text").is_none());
    }

    #[test]
    fn the_whole_match_is_used_without_a_group() {
        assert_eq!(
            extract_number(r"\d{4}", "No 4711 here").as_deref(),
            Some("4711")
        );
    }
}
