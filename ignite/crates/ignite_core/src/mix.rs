//! Chooses a provider at reservation time from fleet load and the asking machine's offers.

use std::cmp::Ordering;

use crate::fleet::ProviderSetting;
use crate::plan_usage::{Hold, model_for_tier};
use crate::protocol::hub::ProviderOffer;

#[derive(Clone, Debug, PartialEq)]
pub struct ProviderLoad {
    pub name: String,
    pub setting: ProviderSetting,
    pub in_progress: u32,
    pub holds: Vec<Hold>,
}

/// Pick the least loaded provider per unit of share. A provider with a share of zero is off.
/// If none fits, explain each provider's blocker.
pub fn choose<'a>(
    loads: &'a [ProviderLoad],
    offered: &[ProviderOffer],
    tier: Option<&str>,
    pinned: Option<&str>,
) -> Result<&'a str, String> {
    let mut best = None;
    let mut reasons = Vec::new();
    for load in loads {
        let offer = offered.iter().find(|offer| offer.name == load.name);
        match blocker(load, offer, tier, pinned) {
            Some(reason) => reasons.push(reason),
            None if best.is_none_or(|other| compare(load, other).is_lt()) => best = Some(load),
            None => {}
        }
    }
    best.map(|load: &ProviderLoad| load.name.as_str())
        .ok_or_else(|| {
            if reasons.is_empty() {
                "no provider has a CLI on this machine".to_owned()
            } else {
                reasons.sort_unstable();
                reasons.join("; ")
            }
        })
}

fn compare(first: &ProviderLoad, second: &ProviderLoad) -> Ordering {
    let first_share = first.setting.share;
    let second_share = second.setting.share;
    // Compare ratios exactly by cross multiplying. A product of two u32s fits in u64.
    let first_weighted = u64::from(first.in_progress) * u64::from(second_share);
    let second_weighted = u64::from(second.in_progress) * u64::from(first_share);
    first_weighted
        .cmp(&second_weighted)
        .then_with(|| second_share.cmp(&first_share))
        .then_with(|| first.name.cmp(&second.name))
}

fn blocker(
    load: &ProviderLoad,
    offer: Option<&ProviderOffer>,
    tier: Option<&str>,
    pinned: Option<&str>,
) -> Option<String> {
    let name = &load.name;
    if pinned.is_some_and(|pin| !pin.eq_ignore_ascii_case(name)) {
        return Some(format!("{name} is excluded by the provider pin"));
    }
    if !load.setting.takes_work() {
        return Some(format!("{name} is {}", load.setting.state()));
    }
    let Some(offer) = offer else {
        return Some(format!("{name} has no CLI on this machine"));
    };
    if let Some(tier) = tier
        && !offer.tiers.iter().any(|offered| offered == tier)
    {
        return Some(format!("{name} does not run tier {tier}"));
    }
    let model = model_for_tier(name, tier, &offer.model_ids, offer.default_model.as_deref());
    load.holds
        .iter()
        .find(|hold| hold.holds_model(model))
        .map(|hold| match &hold.resets_at {
            Some(reset) => format!("{name} is held until {reset} ({})", hold.label),
            None => format!("{name} is on a usage hold ({})", hold.describe()),
        })
}
