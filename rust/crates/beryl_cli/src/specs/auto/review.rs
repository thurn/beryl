//! A card's review checkpoints: the playable setups that expose each of its interactions.

use beryl_engine::card::{CardDef, Purpose};
use beryl_scenario::{Action, Scenario};
use beryl_session::protocol::{
    Candidate as Offer, Kind as Decision, PlayerView, Side, StackItem, Status,
};
use serde_json::json;

use super::super::{Moment, View};
use super::definition::from_definition;
use super::scenarios::{from_scenario, heading};
use super::snapshot::snapshot;
use super::{Candidate, Kind, leak, moment, timeless};

/// A playable setup and the interaction it exposes to a reviewer.
pub struct Checkpoint {
    pub moment: Moment,
    pub first: View,
    pub roles: [String; 2],
    pub actions: Vec<String>,
    pub review_casts: &'static [Action],
}

pub struct ReviewSet {
    pub examples: Vec<Checkpoint>,
    /// Scenarios whose setup never yielded a review checkpoint.
    pub unreached: Vec<&'static str>,
}

pub fn checkpoints(def: &'static CardDef, scenarios: &'static [Scenario]) -> ReviewSet {
    let mut candidates = from_definition(def);
    for scenario in scenarios {
        from_scenario(def, scenario, &mut candidates);
    }
    // Prefer the choice itself to arriving at it through an earlier decision.
    candidates.sort_by_key(|c| match c.kind {
        Kind::Ask => 0,
        Kind::Play | Kind::Probe | Kind::Ways | Kind::Bounced => 1,
        _ => 2,
    });
    let mut seen = Vec::new();
    let mut groups = Vec::new();
    let mut result = Vec::new();
    for c in candidates {
        if matches!(c.kind, Kind::Flip(_) | Kind::FollowOn)
            || c.kind == Kind::Whole && groups.contains(&c.group)
        {
            continue;
        }
        let m = moment(&c, c.title.clone(), None);
        let Ok((views, countered)) = super::super::review_position(&m) else {
            continue;
        };
        let Some((key, checkpoint)) = review_example(def, &c, m, &views, countered.is_some())
        else {
            continue;
        };
        groups.push(c.group);
        if !seen.contains(&key) {
            seen.push(key);
            let mut checkpoint = checkpoint;
            if let Some(card) = countered
                && c.kind == Kind::Play
                && checkpoint.moment.title == "Spell countered"
            {
                use_cast_before_counter(def, &mut checkpoint, &card);
            }
            result.push(checkpoint);
        }
    }
    ReviewSet {
        examples: result,
        unreached: scenarios
            .iter()
            .map(|s| heading(def, s))
            .filter(|name| !groups.contains(name))
            .collect(),
    }
}

/// Start the counter example before the original spell, so either cast can be tried.
fn use_cast_before_counter(def: &CardDef, checkpoint: &mut Checkpoint, countered: &str) {
    let m = &checkpoint.moment;
    let [
        Action::Cast {
            card,
            player: answerer,
            ..
        },
    ] = m.play
    else {
        return;
    };
    if *card != def.name {
        return;
    }
    let Some((
        index,
        &Action::Cast {
            player: caster,
            card,
            ..
        },
    )) = m.board.actions.iter().enumerate().rev().find(|(_, a)| {
        matches!(a, Action::Cast { card, player, .. }
            if *card == countered && player != answerer)
    })
    else {
        return;
    };
    // A recorded cast step can advance a cast, while separate scripted choices still
    // belong to the full replay.
    if index + 1 != m.board.actions.len() {
        return;
    }
    let first = View::ALL[caster.index()];
    let mut before = Moment::new(
        m.name,
        "Cast into a counter",
        m.board.actions(&m.board.actions[..index]),
    );
    before.group = m.group;
    let Ok((views, _)) = super::super::review_position(&before) else {
        return;
    };
    let view = &views[caster.index()];
    let Status::Deciding { decision } = &view.status else {
        return;
    };
    if !decision.candidates.iter().any(|c| {
        matches!(c.option, Offer::Cast { slot, .. } if view.objects.iter().any(|o|
            o.slot == slot && o.name.as_deref() == Some(card)))
    }) {
        return;
    }
    checkpoint.review_casts = leak(vec![m.board.actions[index], m.play[0]]);
    checkpoint.moment = before;
    checkpoint.first = first;
    checkpoint.roles[caster.index()] = format!("{card} caster");
    checkpoint.roles[answerer.index()] = format!("{} caster", def.name);
    checkpoint.actions = decision
        .candidates
        .iter()
        .map(|c| c.label.clone())
        .collect();
}

fn review_example(
    def: &'static CardDef,
    c: &Candidate,
    mut moment: Moment,
    views: &[PlayerView; 2],
    countered: bool,
) -> Option<(serde_json::Value, Checkpoint)> {
    let owner = usize::from(!matches!(views[0].status, Status::Deciding { .. }));
    let view = &views[owner];
    let decision = match &view.status {
        Status::Deciding { decision } => Some(decision),
        _ => None,
    };
    let name_of = |slot| {
        view.objects
            .iter()
            .find(|o| o.slot == slot)
            .and_then(|o| o.name.as_deref())
    };
    let card_item = |item: &StackItem| match item {
        StackItem::Spell { slot, .. } => name_of(*slot) == Some(def.name),
        StackItem::Ability { name, .. } => name == def.name,
    };
    let on_stack = view.stack.iter().rev().find(|item| card_item(item));
    let source = decision
        .and_then(|d| d.resolving.as_ref())
        .filter(|item| card_item(item))
        .or(on_stack);
    let card_is_asking = decision.is_some_and(|d| {
        d.kind != Decision::Priority
            && (d.resolving.as_ref().is_some_and(card_item)
                || d.source.and_then(name_of) == Some(def.name))
    });
    if c.kind == Kind::Ask && !card_is_asking {
        return None;
    }
    let title = if card_is_asking {
        review_prompt(decision?)
    } else if countered && on_stack.is_none() {
        "Spell countered".to_owned()
    } else if on_stack.is_some() {
        format!("Respond to {}", def.name)
    } else if c.kind == Kind::Whole {
        "After resolution".to_owned()
    } else {
        c.title.clone()
    };
    let source_side = match source {
        Some(StackItem::Spell { slot, .. }) => view
            .objects
            .iter()
            .find(|o| o.slot == *slot)
            .map(|o| o.controller),
        Some(StackItem::Ability { controller, .. }) => Some(*controller),
        None => None,
    };
    let paying = decision.is_some_and(|d| d.prompt.is(Purpose::UnlessPays));
    let roles = std::array::from_fn(|seat| match (seat == owner, source_side) {
        (true, _) if paying => "Paying player".to_owned(),
        (true, Some(Side::You)) | (false, Some(Side::Opponent)) => {
            let role = if matches!(source, Some(StackItem::Spell { .. })) {
                "caster"
            } else {
                "controller"
            };
            format!("{} {role}", def.name)
        }
        _ => format!("Player {}", seat + 1),
    });
    let actions = decision.map_or_else(Vec::new, |d| {
        d.candidates
            .iter()
            .map(|c| match &c.option {
                // A yes and a no in the prompt's own words.
                Offer::Yes if !d.prompt.words.yes.is_empty() => d.prompt.words.yes.clone(),
                Offer::No if !d.prompt.words.no.is_empty() => d.prompt.words.no.clone(),
                _ => c.label.clone(),
            })
            .collect()
    });
    let key = if card_is_asking {
        review_choice_key(decision?, owner, source_side)
    } else if on_stack.is_some() || countered {
        json!([title, owner, source_side])
    } else {
        let snapshots = views.each_ref().map(|v| snapshot(v, def));
        json!([title, snapshots.each_ref().map(|r| timeless(r))])
    };
    moment.name = Box::leak(format!("review-{}", moment.name).into_boxed_str());
    moment.title = Box::leak(title.into_boxed_str());
    Some((
        key,
        Checkpoint {
            moment,
            first: View::ALL[owner],
            roles,
            actions,
            review_casts: &[],
        },
    ))
}

/// Deduplicate choices by their presentation and options, independent of the surrounding
/// board's card names and slot IDs.
fn review_choice_key(
    decision: &beryl_session::protocol::DecisionView,
    owner: usize,
    source: Option<Side>,
) -> serde_json::Value {
    let mut ask = json!(decision.prompt);
    if let Some(about) = ask.get_mut("about") {
        *about = json!(about.as_array().map(Vec::len));
    }
    let options: Vec<_> = decision
        .candidates
        .iter()
        .map(|c| {
            let payments: Vec<_> = c.payments.iter().map(|p| (p.x, p.taps.len())).collect();
            json!([json!(c.option)["kind"], payments, c.targets.len()])
        })
        .collect();
    json!(["choice", owner, source, decision.kind, ask, options])
}

fn review_prompt(d: &beryl_session::protocol::DecisionView) -> String {
    // The prompt's own title, or for one that has none its purpose.
    if !d.prompt.title.is_empty() {
        return d.prompt.title.clone();
    }
    let mut title = String::new();
    for (i, ch) in d.prompt.purpose.name().chars().enumerate() {
        match ch {
            _ if i == 0 => title.extend(ch.to_uppercase()),
            '-' => title.push(' '),
            _ => title.push(ch),
        }
    }
    title
}
