//! When the newest sample of a plan window reaches the provider's stop percentage, the hub
//! holds hand-offs until a lower sample arrives or the window resets. A window named for a
//! model family holds only hand-offs on models in that family.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::time::Timestamp;

/// The default stop percentage of a provider.
pub const DEFAULT_HOLD_PERCENT: u32 = 99;

/// One window of the plan whose newest sample holds hand-offs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hold {
    /// The window's label as `get_usage` reported it, such as "Weekly · all models".
    pub label: String,
    pub percent_used: f64,
    /// When the window resets, as `get_usage` reported it.
    pub resets_at: Option<String>,
    /// The model family the window counts, in lower case, or `None` for a window that counts
    /// every model.
    pub family: Option<String>,
}

impl Hold {
    /// Whether the window holds a hand-off on the model `model_id` (such as `claude-opus-5-5`).
    pub fn holds_model(&self, model_id: &str) -> bool {
        self.family
            .as_ref()
            .is_none_or(|family| model_id.to_lowercase().contains(family.as_str()))
    }

    /// The hold in a few words, such as "Weekly · all models at 99%, resets 2026-11-24T08:00:00Z".
    pub fn describe(&self) -> String {
        let mut text = format!("{} at {:.0}%", self.label, self.percent_used);
        if let Some(resets) = &self.resets_at {
            text.push_str(", resets ");
            text.push_str(resets);
        }
        text
    }
}

/// Find the actual model behind a ticket's tier for a usage-hold check. Older
/// offers lack model IDs, so use the tier name and Claude's historical defaults.
pub fn model_for_tier<'a>(
    provider: &str,
    tier: Option<&'a str>,
    model_ids: &'a BTreeMap<String, String>,
    default_model: Option<&'a str>,
) -> &'a str {
    if let Some(tier) = tier {
        let fallback = match (provider, tier) {
            ("claude", "light") => "sonnet",
            _ => tier,
        };
        model_ids.get(tier).map_or(fallback, String::as_str)
    } else {
        default_model.unwrap_or(if provider == "claude" { "sonnet" } else { "" })
    }
}

/// The hold of one sample of a window, if it holds at `now`: its percentage is at or above
/// `threshold` and its `resets_at` has not passed. A `resets_at` that cannot be read does not end
/// the hold, since a newer sample still can.
pub fn hold(
    label: &str,
    percent_used: Option<f64>,
    resets_at: Option<&str>,
    threshold: u32,
    now: Timestamp,
) -> Option<Hold> {
    let percent_used = percent_used?;
    if percent_used < f64::from(threshold) {
        return None;
    }
    if resets_at
        .and_then(parse_resets_at)
        .is_some_and(|at| at <= now)
    {
        return None;
    }
    Some(Hold {
        label: label.to_owned(),
        percent_used,
        resets_at: resets_at.map(str::to_owned),
        family: family(label),
    })
}

/// The model family a window's label names, in lower case: the first word after its `·`, unless
/// that part says "all". "Weekly · Fable" is `fable`; "Weekly · all models" and "5-hour" are
/// `None`.
pub fn family(label: &str) -> Option<String> {
    let (_, scope) = label.split_once('·')?;
    let word = scope.split_whitespace().next()?.to_lowercase();
    (word != "all").then_some(word)
}

/// A window's `resetsAt`: an ISO 8601 time ending in `Z` or in an offset such as `+00:00`, or
/// seconds or milliseconds since the Unix epoch.
pub fn parse_resets_at(text: &str) -> Option<Timestamp> {
    let text = text.trim().trim_matches('"');
    if let Ok(at) = Timestamp::parse(text) {
        return Some(at);
    }
    if let Ok(number) = text.parse::<f64>()
        && number.is_finite()
    {
        // Milliseconds pass 10^12 in 2001; seconds will not for thousands of years.
        let millis = if number > 1e12 {
            number
        } else {
            number * 1000.0
        };
        #[allow(clippy::cast_possible_truncation)]
        return Some(Timestamp::from_millis(millis as i64));
    }
    // An offset of `±HH:MM` after the time.
    let split = text.len().checked_sub(6)?;
    let (body, offset) = text.split_at_checked(split)?;
    let sign = match offset.as_bytes().first()? {
        b'+' => 1.0,
        b'-' => -1.0,
        _ => return None,
    };
    let (hours, minutes) = offset[1..].split_once(':')?;
    let offset_s =
        sign * (hours.parse::<f64>().ok()? * 3600.0 + minutes.parse::<f64>().ok()? * 60.0);
    let local = Timestamp::parse(&format!("{body}Z")).ok()?;
    Some(local.plus_seconds(-offset_s))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-11-20T12:00:00Z";

    fn now() -> Timestamp {
        Timestamp::parse(NOW).unwrap()
    }

    #[test]
    fn a_window_holds_at_the_threshold_until_it_resets() {
        let later = Some("2026-11-24T08:00:00Z");
        assert!(hold("5-hour", Some(98.9), later, 99, now()).is_none());
        let held = hold("5-hour", Some(99.0), later, 99, now()).unwrap();
        assert_eq!(held.family, None);
        assert!(
            hold(
                "5-hour",
                Some(100.0),
                Some("2026-11-20T11:59:59Z"),
                99,
                now()
            )
            .is_none()
        );
        assert!(hold("5-hour", Some(100.0), Some("soon"), 99, now()).is_some());
        assert!(hold("5-hour", None, later, 99, now()).is_none());
    }

    #[test]
    fn a_family_window_holds_only_its_family() {
        assert_eq!(family("Weekly · Fable"), Some("fable".to_owned()));
        assert_eq!(family("Weekly · all models"), None);
        assert_eq!(family("5-hour"), None);
        let fable = hold("Weekly · Fable", Some(99.5), None, 99, now()).unwrap();
        assert!(fable.holds_model("claude-fable-5-2"));
        assert!(!fable.holds_model("claude-opus-5-5"));
        let all = hold("Weekly · all models", Some(99.5), None, 99, now()).unwrap();
        assert!(all.holds_model("claude-opus-5-5"));
    }

    #[test]
    fn resets_at_reads_offsets_and_epochs() {
        let at = now();
        assert_eq!(parse_resets_at("2026-11-20T04:00:00-08:00"), Some(at));
        assert_eq!(parse_resets_at("2026-11-20T12:00:00+00:00"), Some(at));
        let seconds = at.millis() / 1000;
        assert_eq!(parse_resets_at(&seconds.to_string()), Some(at));
        assert_eq!(parse_resets_at(&at.millis().to_string()), Some(at));
        assert_eq!(parse_resets_at("tomorrow"), None);
    }
}
