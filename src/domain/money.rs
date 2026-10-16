use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Money {
    minor_units: u64,
    currency: String,
}

impl Money {
    const DOLLARS: &'static str = "USD";
    const MINOR_UNITS_PER_UNIT: u64 = 100;

    pub fn minor_units(minor_units: u64, currency: &str) -> Self {
        Self {
            minor_units,
            currency: currency.to_string(),
        }
    }

    pub fn major_units(amount: f64, currency: &str) -> Self {
        let minor_units = (amount * Self::MINOR_UNITS_PER_UNIT as f64)
            .round()
            .max(0.0);
        Self::minor_units(minor_units as u64, currency)
    }
}

impl fmt::Display for Money {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let units = self.minor_units / Self::MINOR_UNITS_PER_UNIT;
        let cents = self.minor_units % Self::MINOR_UNITS_PER_UNIT;
        if self.currency == Self::DOLLARS {
            write!(formatter, "${units}.{cents:02}")
        } else {
            write!(formatter, "{units}.{cents:02} {}", self.currency)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dollars_show_with_a_dollar_sign_and_cents() {
        assert_eq!(Money::minor_units(320, "USD").to_string(), "$3.20");
    }

    #[test]
    fn a_major_amount_rounds_to_the_nearest_minor_unit() {
        assert_eq!(Money::major_units(1.713516, "USD").to_string(), "$1.71");
    }

    #[test]
    fn a_negative_major_amount_is_zero() {
        assert_eq!(Money::major_units(-2.5, "USD").to_string(), "$0.00");
    }

    #[test]
    fn another_currency_shows_its_code_after_the_amount() {
        assert_eq!(Money::minor_units(7005, "CAD").to_string(), "70.05 CAD");
    }
}
