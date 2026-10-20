//! Budgets ([draft-hardt-aauth-budgets]): an amount a mission may spend, and
//! what one request costs.
//!
//! [draft-hardt-aauth-budgets]: https://github.com/dickhardt/AAuth

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

/// `amount / 10^decimals` in `unit`: `{"amount": 300000, "unit": "USD",
/// "decimals": 2}` is $3,000.00.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Budget {
    /// In the unit's smallest denomination at `decimals`.
    pub amount: u64,
    /// An ISO 4217 currency code, or any unit both sides agree on.
    pub unit: String,
    /// How many of `amount`'s digits are after the decimal point.
    pub decimals: u32,
}

/// Larger amounts lose precision in a JSON number other implementations read.
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

impl Budget {
    /// A budget of `amount` in `unit` at `decimals`.
    pub fn new(amount: u64, unit: impl Into<String>, decimals: u32) -> Self {
        Self {
            amount,
            unit: unit.into(),
            decimals,
        }
    }

    /// A budget from a JSON claim: `None` when absent or null, and an error
    /// for anything that is not `{amount, unit, decimals}` with non-negative
    /// integers no larger than 2^53 - 1.
    ///
    /// ```
    /// # use aauth_protocol::Budget;
    /// let claim = serde_json::json!({ "amount": 300000, "unit": "USD", "decimals": 2 });
    /// assert_eq!(Budget::from_value(Some(&claim))?, Some(Budget::new(300_000, "USD", 2)));
    /// assert_eq!(Budget::from_value(None)?, None);
    /// assert!(Budget::from_value(Some(&serde_json::json!("$3000"))).is_err());
    /// # Ok::<(), aauth_protocol::Error>(())
    /// ```
    pub fn from_value(value: Option<&Value>) -> Result<Option<Self>> {
        let Some(value) = value.filter(|v| !v.is_null()) else {
            return Ok(None);
        };
        let invalid = || Error::Invalid("budget is {amount, unit, decimals}".into());
        let amount = value
            .get("amount")
            .and_then(Value::as_u64)
            .filter(|a| *a <= MAX_SAFE_INTEGER)
            .ok_or_else(invalid)?;
        let unit = value
            .get("unit")
            .and_then(Value::as_str)
            .ok_or_else(invalid)?;
        let decimals = value
            .get("decimals")
            .and_then(Value::as_u64)
            .and_then(|d| u32::try_from(d).ok())
            .ok_or_else(invalid)?;
        Ok(Some(Self::new(amount, unit, decimals)))
    }

    /// Whether two budgets count in the same unit at the same scale, so that
    /// one can be spent from the other.
    pub fn same_scale(&self, other: &Budget) -> bool {
        self.unit == other.unit && self.decimals == other.decimals
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn budgets_parse_strictly() {
        for bad in [
            json!({"amount": -1, "unit": "USD", "decimals": 2}),
            json!({"amount": 1.5, "unit": "USD", "decimals": 2}),
            json!({"amount": 1, "decimals": 2}),
            json!({"amount": 1, "unit": "USD", "decimals": -2}),
            json!({"amount": 9_007_199_254_740_992u64, "unit": "USD", "decimals": 2}),
            json!("100 USD"),
        ] {
            assert!(Budget::from_value(Some(&bad)).is_err(), "{bad}");
        }
        assert!(Budget::from_value(Some(&Value::Null)).unwrap().is_none());
    }

    #[test]
    fn only_the_same_unit_and_scale_compare() {
        let usd = Budget::new(1, "USD", 2);
        assert!(usd.same_scale(&Budget::new(5, "USD", 2)));
        assert!(!usd.same_scale(&Budget::new(5, "USD", 0)));
        assert!(!usd.same_scale(&Budget::new(5, "EUR", 2)));
    }
}
