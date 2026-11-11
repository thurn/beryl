//! What the spec page lists: the cards, generating none, and one card's moments.

use std::collections::HashMap;

use beryl_session::bugs;
use serde_json::{Value, json};

use super::{Spec, reviews};

/// The cards as `GET /specs` lists them: each with when it was reviewed and its open reports.
pub fn cards() -> Value {
    let reviewed = reviews::load(&reviews::path());
    let reports = bugs::open_by_card(&bugs::dir());
    let moment_reports = bugs::open_by_moment(&bugs::dir());
    super::cards()
        .iter()
        .map(|s| {
            let prefix = format!("{}/", s.slug);
            json!({
                "card": s.card,
                "slug": s.slug,
                "reviewed": reviewed.get(s.card),
                "reports": reports.get(&s.slug).copied().unwrap_or(0),
                "momentReports": moment_reports
                    .iter()
                    .filter(|(key, _)| key.starts_with(&prefix))
                    .collect::<HashMap<_, _>>(),
            })
        })
        .collect()
}

/// One card's moments as `GET /specs/<card>` lists them, generating that card alone, with the
/// candidates its generation dropped: a card none of whose candidates reached a live decision
/// has no moments, and its page says why from these.
pub fn moments(spec: &Spec) -> Value {
    let moments: Vec<Value> = spec
        .moments()
        .iter()
        .map(|m| {
            json!({
                "name": m.name,
                "title": m.title,
                // The scenario it comes from, empty for the card's own moments.
                "group": m.group,
                // Whether it plays actions out once shown, which a replay plays again.
                "plays": !m.play.is_empty(),
                "replay": spec.replay_from_hand(m.group),
            })
        })
        .collect();
    let dropped: Vec<String> = spec
        .dropped()
        .iter()
        .map(|(name, why)| format!("{name}: {why}"))
        .collect();
    json!({ "moments": moments, "dropped": dropped })
}
