//! Building the open decision, its refusals and what passing leads to.

use beryl_engine::card::Purpose;
use beryl_engine::layout::Queue;
use beryl_engine::state::{StackEntry, TurnStep};
use beryl_engine::{
    Candidates, Decision, DecisionKind, GameTable, Layout, NextStop, Observation, Opt, Refusal,
    SlotId, Zone, ZoneKey, can_declare_blockers, next_step_stop, next_stop, refusal,
};

use super::ObjectId;
use super::{
    Candidate, CandidateView, DecisionId, DecisionView, Kind, PassTo, RefusalView, Refused,
    StackItem, StopsSetting, candidate, catalog_label, name, side, stack_item,
};
use crate::prompt::{halt_prompt, prompt_view};

pub(super) fn decision_view<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    id: DecisionId,
    d: &Decision<L>,
    resolving: Option<&StackEntry<L>>,
    stops: StopsSetting,
) -> DecisionView {
    let candidates = match &d.candidates {
        Candidates::Objects(_) => (d.candidates.objects())
            .map(|slot| CandidateView {
                label: name(t, obs, slot).unwrap_or("a hidden card").to_owned(),
                option: Candidate::Object {
                    slot: ObjectId(slot.num()),
                },
                payments: Vec::new(),
                blocks: Vec::new(),
                nth: None,
                text: None,
                targets: Vec::new(),
            })
            .collect(),
        Candidates::Options(opts) => (opts.iter())
            .map(|&o| {
                let mut view = candidate(t, obs, o);
                if let Some(label) = catalog_label(t, d, o) {
                    view.label = label;
                }
                view
            })
            .collect(),
    };
    let item = resolving.map(|e| stack_item(t, obs, e));
    // What asks, by name: the ability resolving, or the spell.
    let asker = match (&item, resolving) {
        (Some(StackItem::Ability { name, .. }), _) => Some(name.as_str()),
        (Some(StackItem::Spell { .. }), Some(e)) => e.object().and_then(|r| name(t, obs, r.slot)),
        _ => None,
    };
    DecisionView {
        id,
        kind: d.kind.into(),
        candidates,
        prompt: Box::new(prompt_view(t, obs, d, asker)),
        resolving: item,
        pass_to: (d.kind == DecisionKind::Priority).then(|| wire_pass_to(pass_to(t, obs, stops))),
        pass_on: if d.kind == DecisionKind::Priority {
            pass_on(t, obs, stops).map(wire_pass_to)
        } else {
            None
        },
        source: decision_source(obs, d, resolving),
        halt: false,
        cancellable: false,
        undo: 0,
        undo_depth: 0,
        // The viewer's own hand says nothing of a priority they hold for another player.
        refused: if d.kind == DecisionKind::Priority && d.player == obs.viewer() {
            refused(t, obs)
        } else {
            Vec::new()
        },
        acting_for: (d.player != obs.viewer()).then(|| side(obs, d.player)),
    }
}

/// The cards in the viewer's hand they may neither cast nor play as a land at their priority
/// ([`DecisionView::refused`]).
pub(super) fn refused<L: Layout>(t: &GameTable, obs: Observation<'_, L>) -> Vec<Refused> {
    obs.in_candidate_order(obs.zone(ZoneKey::Of(obs.viewer(), Zone::Hand)))
        .filter_map(|slot| {
            let reason = match refusal(obs, t, slot)? {
                Refusal::Timing { by } => RefusalView::Timing {
                    by: by.map(|slot| ObjectId(slot.num())),
                },
                Refusal::LandPlayed => RefusalView::LandPlayed,
                Refusal::Mana => RefusalView::Mana,
                Refusal::Cost => RefusalView::Cost,
                Refusal::Target => RefusalView::Target,
            };
            Some(Refused {
                slot: ObjectId(slot.num()),
                reason,
            })
        })
        .collect()
}

/// A halt ([`DecisionView::halt`]): priority with passing its one candidate.
pub(super) fn halt_view<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    id: DecisionId,
    stops: StopsSetting,
) -> DecisionView {
    DecisionView {
        id,
        kind: Kind::Priority,
        candidates: vec![CandidateView {
            label: "Pass".to_owned(),
            option: Candidate::Pass,
            payments: Vec::new(),
            blocks: Vec::new(),
            nth: None,
            text: None,
            targets: Vec::new(),
        }],
        resolving: None,
        prompt: Box::new(halt_prompt()),
        pass_to: Some(wire_pass_to(pass_to(t, obs, stops))),
        pass_on: None,
        source: None,
        halt: true,
        cancellable: false,
        undo: 0,
        undo_depth: 0,
        refused: refused(t, obs),
        acting_for: None,
    }
}

/// [`DecisionView::pass_to`]: the engine's [`next_stop`], less the declare blockers step for a
/// defender with nothing to block with under the smart stops ([`skip_idle_blocks`]).
pub(super) fn pass_to<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    stops: StopsSetting,
) -> NextStop {
    skip_idle_blocks(
        t,
        obs,
        stops,
        next_stop(t, obs, stops.stops(), stops.step_stops()),
    )
}

/// [`DecisionView::pass_on`]: where the viewer's next stop in a later step is, when passing
/// would stop them again in this one, for the stack resolving or for combat damage.
pub(crate) fn pass_on<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    stops: StopsSetting,
) -> Option<NextStop> {
    match pass_to(t, obs, stops) {
        NextStop::Resolve | NextStop::CombatDamage => {
            let later = next_step_stop(t, obs, stops.step_stops(), obs.turn().step());
            Some(skip_idle_blocks(t, obs, stops, later))
        }
        NextStop::Step(_) | NextStop::NextTurn => None,
    }
}

/// `next` less the declare blockers step for a defender with nothing to block with under the
/// smart stops. The session passes them through it, as no creature blocks ([`crate::Game`]),
/// on to their next stop after it: by default the end step, as the postcombat main phase is
/// the active player's.
fn skip_idle_blocks<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    stops: StopsSetting,
    next: NextStop,
) -> NextStop {
    match next {
        NextStop::Step(TurnStep::DeclareBlockers)
            if !stops.full_control
                && obs.turn().active() != obs.viewer()
                && !can_declare_blockers(obs, t) =>
        {
            next_step_stop(t, obs, stops.step_stops(), TurnStep::DeclareBlockers)
        }
        next => next,
    }
}

/// Where passing priority takes the viewer, as the wire names it.
fn wire_pass_to(next: NextStop) -> PassTo {
    match next {
        NextStop::Resolve => PassTo::Resolve,
        NextStop::CombatDamage => PassTo::CombatDamage,
        NextStop::Step(step) => PassTo::Step { step: step.into() },
        NextStop::NextTurn => PassTo::NextTurn,
    }
}

/// [`DecisionView::source`].
fn decision_source<L: Layout>(
    obs: Observation<'_, L>,
    d: &Decision<L>,
    resolving: Option<&StackEntry<L>>,
) -> Option<ObjectId> {
    let slot = match (d.kind, d.prompt.purpose) {
        // The land entering asks.
        (_, Purpose::PayLife) => d.about.sole(),
        (
            DecisionKind::Priority
            | DecisionKind::Attacker
            | DecisionKind::AttackTarget
            | DecisionKind::Blocker
            | DecisionKind::BlockTarget,
            _,
        ) => None,
        (DecisionKind::DamageDivision, _) => {
            // The blockers offered block the attacker being divided.
            let Candidates::Options(opts) = &d.candidates else {
                return None;
            };
            opts.iter().find_map(|&o| match o {
                Opt::Lethal(blocker) => obs.object(blocker).blocked_attackers().first(),
                _ => None,
            })
        }
        _ => resolving
            .or_else(|| obs.stack().last())
            .and_then(beryl_engine::state::StackEntry::object)
            .map(|r| r.slot),
    };
    slot.map(|slot| ObjectId(slot.num()))
}
