//! What holds at a moment's first live decision.

use beryl_engine::card::{CardDef, Purpose};
use beryl_session::protocol::{Candidate as Offer, PlayerView, Side, StackItem, Status, ZoneName};
use serde_json::json;

use super::definition::activated;
use crate::fixture::Condition;

/// The conditions that hold of `view` at its first live decision, in the fixture conditions'
/// words: the turn, the decision, the stack, and the card's and its tokens' places. Each is
/// checked to hold, so the vocabulary and this stay in step.
pub fn snapshot(view: &PlayerView, def: &'static CardDef) -> Vec<String> {
    let mut out = Vec::new();
    let name_of = |slot: u16| {
        view.objects
            .iter()
            .find(|o| o.slot == slot)
            .and_then(|o| o.name.clone())
    };
    let protocol = |v: serde_json::Value| v.as_str().map(str::to_owned).unwrap_or_default();
    out.push(format!("turn {}", view.turn.number));
    out.push(
        if view.turn.active == Side::You {
            "your turn"
        } else {
            "their turn"
        }
        .to_owned(),
    );
    out.push(format!("step {}", protocol(json!(view.turn.step))));
    if let Status::Deciding { decision } = &view.status {
        if !decision.halt {
            out.push(format!("decision {}", protocol(json!(decision.kind))));
        }
        // A priority's kind says it all.
        let prompt = &decision.prompt;
        if !prompt.is(Purpose::Priority) {
            let part = match protocol(json!(prompt.part)).as_str() {
                "pick" => String::new(),
                part => format!(" {part}"),
            };
            out.push(format!("prompt {}{part}", prompt.purpose));
        }
        if json!(decision.kind) != json!("priority") {
            out.push(format!("{} candidates", decision.candidates.len()));
        }
        match &decision.resolving {
            Some(StackItem::Spell { slot, .. }) => {
                if let Some(n) = name_of(*slot) {
                    out.push(format!("resolving {n}"));
                }
            }
            Some(StackItem::Ability { name, .. }) => out.push(format!("resolving {name}")),
            None => {}
        }
        let offered: Vec<u8> = decision
            .candidates
            .iter()
            .filter_map(|c| match c.option {
                Offer::Activate {
                    slot,
                    ability,
                    granted: false,
                } if name_of(slot).as_deref() == Some(def.name) => Some(ability),
                _ => None,
            })
            .collect();
        let mine_on_battlefield = view.objects.iter().any(|o| {
            o.zone == ZoneName::Battlefield
                && o.controller == Side::You
                && o.name.as_deref() == Some(def.name)
        });
        if json!(decision.kind) == json!("priority") && mine_on_battlefield {
            for (k, _) in activated(def.front()) {
                if offered.contains(&k) {
                    out.push(format!("offers {} ability {k}", def.name));
                } else {
                    out.push(format!("not offers {} ability {k}", def.name));
                }
            }
        }
    }
    places(view, def, &mut out);
    let castable = format!("castable {}", def.name);
    let ways = format!("ways to play {}", def.name);
    for flag in [
        castable.as_str(),
        ways.as_str(),
        "attacking",
        "blocking",
        "castable from exile",
        "castable from the graveyard",
        "mana floating",
        "stolen permanent",
        "known card in the opponent's hand",
        "X on the stack",
        "emblems a side",
        "exiled beneath a permanent",
        "game over",
    ] {
        if Condition::parse(flag).is_ok_and(|c| c.holds(view)) {
            out.push(flag.to_owned());
        }
    }
    debug_assert!(
        out.iter()
            .all(|c| Condition::parse(c).is_ok_and(|c| c.holds(view))),
        "a recorded condition does not hold: {out:?}"
    );
    out
}

/// The stack's top, and where the card and its tokens are.
fn places(view: &PlayerView, def: &'static CardDef, out: &mut Vec<String>) {
    let name_of = |slot: u16| {
        view.objects
            .iter()
            .find(|o| o.slot == slot)
            .and_then(|o| o.name.clone())
    };
    match view.stack.last() {
        Some(StackItem::Spell { slot, .. }) => {
            if let Some(n) = name_of(*slot) {
                out.push(format!("stack {n}"));
            }
        }
        Some(StackItem::Ability { name, .. }) => out.push(format!("stack {name}")),
        None => {}
    }
    let names = std::iter::once(def.name).chain(def.tokens.iter().map(|t| t.name));
    for name in names {
        let count = |side: Side| {
            view.objects
                .iter()
                .filter(|o| {
                    o.zone == ZoneName::Battlefield
                        && o.controller == side
                        && o.name.as_deref() == Some(name)
                })
                .count()
        };
        out.push(format!("controls {} {name}", count(Side::You)));
        let theirs = count(Side::Opponent);
        if theirs > 0 {
            out.push(format!("they control {theirs} {name}"));
        }
    }
    let tapped = view.objects.iter().filter(|o| {
        o.zone == ZoneName::Battlefield
            && o.controller == Side::You
            && o.name.as_deref() == Some(def.name)
    });
    let tapped: Vec<bool> = tapped.map(|o| o.tapped).collect();
    if tapped.iter().any(|&t| t) {
        out.push(format!("tapped {}", def.name));
    } else if !tapped.is_empty() {
        out.push(format!("not tapped {}", def.name));
    }
    if let Some(host) = view
        .objects
        .iter()
        .find(|o| o.zone == ZoneName::Battlefield && o.name.as_deref() == Some(def.name))
    {
        for c in &host.counters {
            let kind = serde_json::to_value(c.kind).unwrap_or_default();
            out.push(format!(
                "{} has {} {}",
                def.name,
                c.count,
                kind.as_str().unwrap_or("")
            ));
        }
        let n = view
            .objects
            .iter()
            .filter(|o| o.zone == ZoneName::Exile && o.exiled_with == Some(host.slot))
            .count();
        if n > 0 {
            out.push(format!("{n} exiled with {}", def.name));
        }
    }
}
