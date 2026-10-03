//! What the AI costs this month, and the monthly ceiling the psychologist sets.
//!
//! Only numbers are kept: tokens per model per month, from the `usage` the API returns. No text,
//! no case, no time of day. The cost is an estimate from the list prices below (the bill is in
//! the provider's console); a model without a known price is counted in tokens only.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::dates;

/// Settings key prefix for one month's numbers: `usage/2026-10`.
const MONTH_PREFIX: &str = "usage/";
/// Settings key of the monthly ceiling, in whole dollars. Absent = no ceiling.
pub(crate) const CAP_KEY: &str = "monthly_cap_usd";

/// Tokens one model used in one month.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Tokens {
    #[serde(default)]
    pub requests: u32,
    #[serde(default)]
    pub input: u32,
    #[serde(default)]
    pub output: u32,
    #[serde(default)]
    pub cache_read: u32,
    #[serde(default)]
    pub cache_write: u32,
}

impl Tokens {
    /// The `usage` block of one answer (Messages API shape).
    pub(crate) fn from_answer(answer: &Value) -> Self {
        let n = |k: &str| {
            answer["usage"][k]
                .as_u64()
                .map_or(0, |v| u32::try_from(v).unwrap_or(u32::MAX))
        };
        Self {
            requests: 1,
            input: n("input_tokens"),
            output: n("output_tokens"),
            cache_read: n("cache_read_input_tokens"),
            cache_write: n("cache_creation_input_tokens"),
        }
    }

    fn add(&mut self, other: Self) {
        self.requests = self.requests.saturating_add(other.requests);
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.cache_write = self.cache_write.saturating_add(other.cache_write);
    }

    /// Estimated cost in hundredths of a cent (1/10,000 dollar), or `None` without a price.
    fn cost_ten_thousandths(&self, model: &str) -> Option<u64> {
        let (input, output, cache_read) = price(model)?;
        // Prices are dollars per million tokens, here in 1/10,000 dollar: × 10,000 / 1,000,000.
        let part = |tokens: u32, per_million: f64| f64::from(tokens) * per_million / 100.0;
        let total = part(self.input, input)
            + part(self.output, output)
            + part(self.cache_read, cache_read)
            // Writing to the cache costs 1.25 × the input price.
            + part(self.cache_write, input * 1.25);
        // Rounded up: an estimate of a ceiling must not undercount.
        Some(total.ceil().max(0.0) as u64)
    }
}

/// List prices, dollars per million tokens: (input, output, cache read).
fn price(model: &str) -> Option<(f64, f64, f64)> {
    Some(match model {
        "claude-opus-5-5" => (4.0, 20.0, 0.20),
        "claude-opus-5" | "claude-opus-4-8" => (5.0, 25.0, 0.50),
        "claude-sonnet-5-5" | "claude-sonnet-5" => (2.0, 10.0, 0.20),
        _ => return None,
    })
}

pub(crate) fn month_key(year: i32, month: u32) -> String {
    format!("{MONTH_PREFIX}{year:04}-{month:02}")
}

pub(crate) fn this_month_key() -> String {
    let (y, m, _) = dates::today();
    month_key(y, m)
}

/// One month's numbers, by model, as stored in settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Month(pub BTreeMap<String, Tokens>);

impl Month {
    pub(crate) fn parse(stored: Option<&str>) -> Self {
        stored
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default()
    }

    pub(crate) fn add(&mut self, model: &str, tokens: Tokens) {
        self.0.entry(model.to_owned()).or_default().add(tokens);
    }

    /// Estimated dollars × 10,000 for the models with a known price.
    fn cost_ten_thousandths(&self) -> u64 {
        self.0
            .iter()
            .filter_map(|(m, t)| t.cost_ten_thousandths(m))
            .sum()
    }

    /// Whether this month has reached a ceiling of `cap_usd` dollars.
    pub(crate) fn reached(&self, cap_usd: u32) -> bool {
        self.cost_ten_thousandths() >= u64::from(cap_usd) * 10_000
    }

    pub(crate) fn view(&self, month: &str, cap_usd: Option<u32>) -> UsageSummary {
        let mut total = Tokens::default();
        let mut unpriced = Vec::new();
        for (model, t) in &self.0 {
            total.add(*t);
            if price(model).is_none() {
                unpriced.push(model.clone());
            }
        }
        let cents = self.cost_ten_thousandths().div_ceil(100);
        UsageSummary {
            month: month.trim_start_matches(MONTH_PREFIX).to_owned(),
            requests: total.requests,
            input_tokens: total
                .input
                .saturating_add(total.cache_read)
                .saturating_add(total.cache_write),
            output_tokens: total.output,
            estimated_cents: u32::try_from(cents).unwrap_or(u32::MAX),
            cap_usd,
            unpriced_models: unpriced,
        }
    }
}

/// This month's use of the AI, for settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UsageSummary {
    /// `2026-10`.
    pub month: String,
    pub requests: u32,
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Estimated cost in cents (US dollars), from list prices.
    pub estimated_cents: u32,
    /// The ceiling she set, in dollars. Sending stops when it is reached.
    pub cap_usd: Option<u32>,
    /// Models used this month whose price is not known here (counted in tokens only).
    pub unpriced_models: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tokens_are_read_from_the_answer_and_priced() {
        let t = Tokens::from_answer(&json!({"usage": {
            "input_tokens": 1_000_000, "output_tokens": 100_000,
            "cache_read_input_tokens": 1_000_000, "cache_creation_input_tokens": 0
        }}));
        assert_eq!(t.requests, 1);
        // $5 + $2.50 + $0.50 = $8.00
        assert_eq!(t.cost_ten_thousandths("claude-opus-5"), Some(80_000));
        assert_eq!(t.cost_ten_thousandths("some-other-model"), None);
        // An answer without usage counts as a request with no tokens.
        assert_eq!(Tokens::from_answer(&json!({})).input, 0);
    }

    #[test]
    fn a_month_adds_up_and_reaches_its_ceiling() {
        let mut m = Month::parse(None);
        let big = Tokens {
            requests: 1,
            input: 2_000_000,
            ..Tokens::default()
        };
        m.add("claude-sonnet-5", big);
        m.add("claude-sonnet-5", big);
        m.add("mistral-large-2411", big);
        // 4M input tokens on Sonnet at $2 = $8; the unpriced model counts in tokens only.
        assert!(m.reached(8));
        assert!(!m.reached(9));
        let v = m.view(&month_key(2026, 10), Some(9));
        assert_eq!(v.month, "2026-10");
        assert_eq!(v.requests, 3);
        assert_eq!(v.estimated_cents, 800);
        assert_eq!(v.unpriced_models, vec!["mistral-large-2411".to_owned()]);
        let stored = serde_json::to_string(&m).unwrap();
        assert_eq!(Month::parse(Some(&stored)), m);
        assert_eq!(Month::parse(Some("not json")), Month::default());
    }
}
