//! How an amount is written as text.

use super::Amount;

/// How many minor units (cents) make one unit of an amount.
pub const MINOR_UNITS_PER_UNIT: i64 = 100;

/// The amount as the field shows it: number and currency code.
pub fn amount_text(amount: &Amount) -> String {
    format!("{} {}", amount_digits(amount), amount.currency.as_str())
}

/// The number of an amount without its currency, with two decimals.
pub fn amount_digits(amount: &Amount) -> String {
    let magnitude = amount.minor_units.unsigned_abs();
    let unit = MINOR_UNITS_PER_UNIT.unsigned_abs();
    let sign = if amount.minor_units < 0 { "-" } else { "" };
    format!(
        "{sign}{}.{:02}",
        magnitude.div_euclid(unit),
        magnitude.rem_euclid(unit)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Currency;

    #[test]
    fn negative_amounts_keep_their_sign_and_two_decimals() {
        let currency = Currency::new("EUR").expect("a valid code");
        let amount = Amount {
            minor_units: -705,
            currency,
        };
        assert_eq!(amount_text(&amount), "-7.05 EUR");
        assert_eq!(amount_digits(&amount), "-7.05");
    }
}
