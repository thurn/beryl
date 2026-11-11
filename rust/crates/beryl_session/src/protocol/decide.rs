//! Building the open decision, its refusals and what passing leads to.

use beryl_engine::card::Purpose;
use beryl_engine::layout::Queue;
use beryl_engine::state::{StackEntry, TurnStep};
use beryl_engine::{
    Candidates, Decision, DecisionKind, GameTable, Layout, NextStop, Observation, Opt, Refusal,
    SlotId, Zone, ZoneKey, can_declare_blockers, next_step_stop, next_stop, refusal,
};

use super::ask::ask_view;
use super::{
    AskView, Candidate, CandidateView, DecisionId, DecisionView, Kind, PassTo, RefusalView,
    Refused, StopsSetting, candidate, name, stack_item,
};

pub(super) fn decision_view<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    id: DecisionId,
    d: &Decision<L>,
    resolving: Option<&StackEntry<L>>,
    stops: StopsSetting,
) -> DecisionView {
    let candidates = match &d.candidates {
        Candidates::Objects(b) => b
            .iter()
            .map(|slot| CandidateView {
                label: name(t, obs, slot).unwrap_or("a hidden card").to_owned(),
                option: Candidate::Object { slot: slot.num() },
                payments: Vec::new(),
                blocks: Vec::new(),
                nth: None,
                targets: Vec::new(),
            })
            .collect(),
        Candidates::Options(opts) => opts.iter().map(|&o| candidate(t, obs, o)).collect(),
    };
    DecisionView {
        waterbend: None,
        self_removal_targets: None,
        damage_default: None,
        id,
        kind: d.kind.into(),
        candidates,
        resolving: resolving.map(|e| stack_item(t, obs, e)),
        ask: ask_view(obs, d),
        pass_to: (d.kind == DecisionKind::Priority).then(|| pass_to(obs, stops).into()),
        pass_on: if d.kind == DecisionKind::Priority {
            pass_on(obs, stops).map(Into::into)
        } else {
            None
        },
        source: decision_source(obs, d, resolving),
        halt: false,
        cancellable: false,
        undo: 0,
        undo_depth: 0,
        refused: if d.kind == DecisionKind::Priority {
            refused(t, obs)
        } else {
            Vec::new()
        },
    }
}

/// The cards in the viewer's hand they may neither cast nor play as a land at their priority
/// ([`DecisionView::refused`]).
pub(super) fn refused<L: Layout>(t: &GameTable, obs: Observation<'_, L>) -> Vec<Refused> {
    obs.zone(ZoneKey::Of(obs.viewer(), Zone::Hand))
        .iter()
        .filter_map(|slot| {
            let reason = match refusal(obs, t, slot)? {
                Refusal::Timing { by } => RefusalView::Timing {
                    by: by.map(SlotId::num),
                },
                Refusal::LandPlayed => RefusalView::LandPlayed,
                Refusal::Mana => RefusalView::Mana,
                Refusal::Cost => RefusalView::Cost,
                Refusal::Target => RefusalView::Target,
            };
            Some(Refused {
                slot: slot.num(),
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
        waterbend: None,
        self_removal_targets: None,
        damage_default: None,
        id,
        kind: Kind::Priority,
        candidates: vec![CandidateView {
            label: "Pass".to_owned(),
            option: Candidate::Pass,
            payments: Vec::new(),
            blocks: Vec::new(),
            nth: None,
            targets: Vec::new(),
        }],
        resolving: None,
        ask: AskView::Plain,
        pass_to: Some(pass_to(obs, stops).into()),
        pass_on: None,
        source: None,
        halt: true,
        cancellable: false,
        undo: 0,
        undo_depth: 0,
        refused: refused(t, obs),
    }
}

/// [`DecisionView::pass_to`]: the engine's [`next_stop`], less the declare blockers step for a
/// defender with nothing to block with under the smart stops ([`skip_idle_blocks`]).
pub(super) fn pass_to<L: Layout>(obs: Observation<'_, L>, stops: StopsSetting) -> NextStop {
    skip_idle_blocks(
        obs,
        stops,
        next_stop(obs, stops.stops(), stops.step_stops()),
    )
}

/// [`DecisionView::pass_on`]: where the viewer's next stop in a later step is, when passing
/// would stop them again in this one, for the stack resolving or for combat damage.
pub(crate) fn pass_on<L: Layout>(obs: Observation<'_, L>, stops: StopsSetting) -> Option<NextStop> {
    match pass_to(obs, stops) {
        NextStop::Resolve | NextStop::CombatDamage => {
            let later = next_step_stop(obs, stops.step_stops(), obs.turn().step);
            Some(skip_idle_blocks(obs, stops, later))
        }
        NextStop::Step(_) | NextStop::NextTurn => None,
    }
}

/// `next` less the declare blockers step for a defender with nothing to block with under the
/// smart stops. The session passes them through it, as no creature blocks ([`crate::Game`]),
/// on to their next stop after it: by default the end step, as the postcombat main phase is
/// the active player's.
fn skip_idle_blocks<L: Layout>(
    obs: Observation<'_, L>,
    stops: StopsSetting,
    next: NextStop,
) -> NextStop {
    match next {
        NextStop::Step(TurnStep::DeclareBlockers)
            if !stops.full_control
                && obs.turn().active != obs.viewer()
                && !can_declare_blockers(obs) =>
        {
            next_step_stop(obs, stops.step_stops(), TurnStep::DeclareBlockers)
        }
        next => next,
    }
}

impl From<NextStop> for PassTo {
    fn from(next: NextStop) -> PassTo {
        match next {
            NextStop::Resolve => PassTo::Resolve,
            NextStop::CombatDamage => PassTo::CombatDamage,
            NextStop::Step(step) => PassTo::Step { step: step.into() },
            NextStop::NextTurn => PassTo::NextTurn,
        }
    }
}

/// [`DecisionView::source`].
fn decision_source<L: Layout>(
    obs: Observation<'_, L>,
    d: &Decision<L>,
    resolving: Option<&StackEntry<L>>,
) -> Option<u16> {
    let slot = match (d.kind, d.prompt.purpose) {
        // The land entering asks.
        (_, Purpose::PayLife) => d.about.first(),
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
                Opt::Lethal(blocker) => obs.object(blocker).combat,
                _ => None,
            })
        }
        _ => resolving
            .or_else(|| obs.stack().last())
            .and_then(|e| e.object)
            .map(|r| r.slot),
    };
    slot.map(SlotId::num)
}
