//! Token creation proposed before the tokens are made, and the replacement effects that
//! modify how many tokens an effect creates ([CR 616.1]).
//!
//! Implements: none. Applies the parent module's replacement rules to token creation.
//!
//! [CR 616.1]: https://yawgatog.com/resources/magic-rules/#R6161

use beryl_card::Prompt;
use beryl_card::prompt::Purpose;

use crate::card::{Ability, AbilityKind, ObjectAbility, Replaced, Replacement, Rewrite};
use crate::decision::{Decision, DecisionKind, OPTIONS, Opt};
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::replacement::event::{ReplacementKind, Sink};
use crate::state::State;
use crate::table::GameTable;

/// Token creation proposed before the tokens are made: the proposal that replacement effects
/// of token creation rewrite ([`tokens`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Tokens {
    pub controller: Seat,
    pub count: u8,
    /// True when an effect creates the tokens ("If an effect would create one or more
    /// tokens...").
    pub by_effect: bool,
}

/// Applies the replacement effects of token creation to `proposal` and returns the number of
/// tokens to create.
///
/// Gated on [`ReplacementKind::Tokens`]: while no permanent on the battlefield replaces token
/// creation, the proposal's count is returned after one test of [`State::replacing`].
///
/// [`Full`] if the rewritten number does not fit in a `u8`, if the layout's scratch lists
/// fill, or if a choice of order stops the run ([`super::answer`]).
#[inline]
pub(crate) fn tokens<L: Layout>(game: &mut Game<'_, L>, proposal: Tokens) -> Result<u8, Full> {
    if !game
        .state()
        .replacing()
        .is_some_and(|r| r.replaces(ReplacementKind::Tokens))
    {
        return Ok(proposal.count);
    }
    if !proposal.by_effect || proposal.count == 0 {
        return Ok(proposal.count);
    }
    replaced(game, proposal)
}

/// What a replacement effect of a number (tokens created, counters put) does to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Change {
    /// Multiplies the count by N.
    Multiply(u8),
    /// Adds N to the count.
    Plus(u8),
}

impl Change {
    /// `n` rewritten, or [`Full`] when the result does not fit in a `u8`.
    pub(super) fn apply(self, n: u8) -> Result<u8, Full> {
        match self {
            Change::Multiply(m) => n.checked_mul(m).ok_or(Full),
            Change::Plus(m) => n.checked_add(m).ok_or(Full),
        }
    }

    /// The change a rewrite of a number makes, if it is one.
    pub(super) fn of(rewrite: Rewrite) -> Option<Change> {
        match rewrite {
            Rewrite::Multiply(n) => Some(Change::Multiply(n)),
            Rewrite::Plus(n) => Some(Change::Plus(n)),
            Rewrite::None
            | Rewrite::ToZone(_)
            | Rewrite::EntersTapped
            | Rewrite::EntersPrepared
            | Rewrite::EntersTappedUnlessFewLands(_)
            | Rewrite::EntersTappedUnlessYouControl(..)
            | Rewrite::EntersTappedUnlessPayLife(_)
            | Rewrite::EntersWithCounters(..)
            | Rewrite::MaySkipTurnToUntap
            | Rewrite::Instead
            | Rewrite::Dredge(_)
            | Rewrite::Choose(_)
            | Rewrite::EnterAsCopy { .. } => None,
        }
    }
}

/// A static ability of a permanent that applies to the event now: its permanent, its place
/// among that permanent's abilities (the identity [CR 614.5] tracks as having applied), and
/// how it rewrites the number.
///
/// [CR 614.5]: https://yawgatog.com/resources/magic-rules/#R6145
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct Candidate<S> {
    pub slot: S,
    pub ability: ObjectAbility,
    pub change: Change,
}

/// The scratch lists in [`replaced`] hold this many effects in `Small` and grow in `Large`.
pub(super) const MOST: usize = 16;

pub(super) type Candidates<L> = <L as Layout>::Queue<Option<Candidate<<L as Layout>::Slot>>, MOST>;

/// The replacements that have applied to one event ([CR 614.5]).
///
/// [CR 614.5]: https://yawgatog.com/resources/magic-rules/#R6145
pub(super) type Applied<L> =
    <L as Layout>::Queue<Option<(<L as Layout>::Slot, ObjectAbility)>, MOST>;

/// [`tokens`] once some permanent replaces token creation: the loop of [CR 616.1]: "If two or
/// more replacement and/or prevention effects are attempting to modify the way an event affects
/// an object or player, the affected object's controller (or its owner if it has no controller)
/// or the affected player chooses one to apply". Each replacement applies at most once
/// ([CR 614.5]): "A replacement effect doesn't invoke itself repeatedly; it gets only one
/// opportunity to affect an event or any modified events that may replace that event." Once
/// chosen, the process repeats ([CR 616.1f]): "Once the chosen effect has been applied, this
/// process is repeated (taking into account only replacement or prevention effects that would
/// now be applicable) until there are no more left to apply." None of these is a
/// self-replacement effect or changes control, copies or turns a face (CR 616.1a to 616.1d),
/// so any may be chosen ([CR 616.1e]): "Any of the applicable replacement and/or prevention
/// effects may be chosen."
///
/// [CR 614.5]: https://yawgatog.com/resources/magic-rules/#R6145
/// [CR 616.1]: https://yawgatog.com/resources/magic-rules/#R6161
/// [CR 616.1e]: https://yawgatog.com/resources/magic-rules/#R6161e
/// [CR 616.1f]: https://yawgatog.com/resources/magic-rules/#R6161f
#[cold]
#[inline(never)]
fn replaced<L: Layout>(game: &mut Game<'_, L>, mut p: Tokens) -> Result<u8, Full> {
    let mut applied = Applied::<L>::default();
    while p.count > 0 {
        let found = candidates(game.state(), game.table(), p, applied.as_slice())?;
        let Some(c) = choose(game.state, game.sink, p.controller, found.as_slice())? else {
            break;
        };
        applied.push(Some((c.slot, c.ability)))?;
        p.count = c.change.apply(p.count)?;
    }
    Ok(p.count)
}

/// The static abilities that apply to `p` now and are not among `applied`, in the order of
/// the battlefield.
fn candidates<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    p: Tokens,
    applied: &[Option<(L::Slot, ObjectAbility)>],
) -> Result<Candidates<L>, Full> {
    let mut found = Candidates::<L>::default();
    let pick = |_, a: &Ability, r: &Replacement| match *r {
        Replacement {
            replaces: Replaced::Tokens { players },
            rewrite,
        } if a.kind() == AbilityKind::Replacement => Some((players, Change::of(rewrite)?)),
        _ => None,
    };
    for (slot, ability, a, (players, change)) in
        super::replacements(s, t, ReplacementKind::Tokens, pick)
    {
        if !crate::filter::is_for(&players, p.controller, s.controller(slot)) {
            continue;
        }
        if applied.contains(&Some((slot, ability))) {
            continue;
        }
        crate::coverage::hit(a);
        found.push(Some(Candidate {
            slot,
            ability,
            change,
        }))?;
    }
    Ok(found)
}

/// The effect of `found` to apply next, or none when none applies.
///
/// Multipliers alone commute with each other, and additions alone commute with each other, so
/// when every remaining effect is of the same kind any order produces the same count and the
/// first is taken without asking. When both a multiplier and an addition remain, the order
/// changes the final count, so `chooser` (the player who would control the tokens, or who
/// controls the permanent the counters are put on) chooses among one representative of each
/// distinct rewrite.
pub(super) fn choose<L: Layout>(
    s: &State<L>,
    sink: &mut Sink<L>,
    chooser: Seat,
    found: &[Option<Candidate<L::Slot>>],
) -> Result<Option<Candidate<L::Slot>>, Full> {
    let found = || found.iter().flatten().copied();
    let has_mul = found().any(|c| matches!(c.change, Change::Multiply(_)));
    let has_plus = found().any(|c| matches!(c.change, Change::Plus(_)));
    if !has_mul || !has_plus {
        return Ok(found().next());
    }
    let mut reps = Candidates::<L>::default();
    let mut options = <L::Queue<Opt<L>, OPTIONS>>::default();
    for c in found() {
        let same = reps
            .as_slice()
            .iter()
            .flatten()
            .any(|r| r.change == c.change || r.slot == c.slot);
        if !same {
            reps.push(Some(c))?;
            options.push(Opt::Object(c.slot))?;
        }
    }
    if options.len() <= 1 {
        return Ok(reps.as_slice().iter().flatten().copied().next());
    }
    let d = Decision::options(
        chooser,
        DecisionKind::ChooseObject,
        options,
        Prompt::new(Purpose::Replace),
    );
    let c = super::answer(s, sink, &d)?;
    let chosen = d.candidates.option(c);
    Ok(reps
        .as_slice()
        .iter()
        .flatten()
        .copied()
        .find(|r| chosen == Some(Opt::Object(r.slot)))
        .or_else(|| reps.as_slice().iter().flatten().copied().next()))
}
