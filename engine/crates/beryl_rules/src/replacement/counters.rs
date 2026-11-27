//! Counters proposed before they are put on a permanent, and the replacement effects that
//! modify how many are put ([CR 616.1]).
//!
//! Implements: none. Applies the parent module's replacement rules to counters being put.
//!
//! [CR 616.1]: https://yawgatog.com/resources/magic-rules/#R6161

use beryl_kernel::query::Copiable;

use super::tokens::{Applied, Candidate, Candidates, Change, choose};
use beryl_card::Filter;

use crate::card::{Ability, AbilityKind, Replaced, Replacement};
use crate::continuous::layers;
use crate::filter::Evaluate;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::replacement::event::{Entry, ReplacementKind, Sink};
use crate::state::{CounterKind, State};
use crate::table::GameTable;

/// Counters proposed before they are put on a permanent: the proposal that replacement effects
/// of counters rewrite ([`counters`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Counters<L: Layout> {
    /// The permanent they are put on, or the object that is entering the battlefield with them.
    pub on: L::Slot,
    pub kind: CounterKind,
    pub amount: u8,
    /// True when an effect puts them ("If an effect would put one or more counters..."), and
    /// false for a cost (a loyalty ability's) or a turn-based action (a Saga's lore counter in
    /// its controller's precombat main phase). An event does not say what caused it, so each
    /// place that puts counters says which it is.
    pub by_effect: bool,
}

/// The object entering the battlefield that a proposal of [`entering_counters`] is about,
/// which the replacements' filters are asked of as it would exist on the battlefield.
#[derive(Clone, Copy)]
pub(crate) struct Entering<L: Layout> {
    pub entry: Entry<L>,
    pub copied: Option<Copiable<L>>,
    /// The player who will control it.
    pub controller: Seat,
}

/// Applies the replacement effects of counters to `proposal`, about a permanent on the
/// battlefield, and returns the number of counters to put on it.
///
/// Gated on [`ReplacementKind::Counters`]: while no permanent on the battlefield replaces
/// counters being put, the proposal's amount is returned after one test of
/// [`State::replacing`].
///
/// [`Full`] if the rewritten number does not fit in a `u8`, if the layout's scratch lists
/// fill, or if a choice of order stops the run ([`super::answer`]).
#[inline]
pub(crate) fn counters<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    proposal: Counters<L>,
) -> Result<u8, Full> {
    if !s
        .replacing()
        .is_some_and(|r| r.replaces(ReplacementKind::Counters))
        || proposal.amount == 0
    {
        return Ok(proposal.amount);
    }
    replaced(s, t, sink, proposal, None)
}

/// [`counters`] for an object entering the battlefield with counters (see [CR 122.6]): "Some
/// spells and abilities refer to counters being put on an object. This refers to putting
/// counters on that object while it's on the battlefield and also to an object that's given
/// counters as it enters the battlefield." The filters are asked of the object as it would
/// exist on the battlefield, under the player who would control it (see [CR 614.12]): "To
/// determine which replacement effects apply and how they apply, check the characteristics of
/// the permanent as it would exist on the battlefield".
///
/// [CR 122.6]: https://yawgatog.com/resources/magic-rules/#R1226
/// [CR 614.12]: https://yawgatog.com/resources/magic-rules/#R61412
#[inline]
pub(crate) fn entering_counters<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    proposal: Counters<L>,
    entering: Entering<L>,
) -> Result<u8, Full> {
    if !s
        .replacing()
        .is_some_and(|r| r.replaces(ReplacementKind::Counters))
        || proposal.amount == 0
    {
        return Ok(proposal.amount);
    }
    replaced(s, t, sink, proposal, Some(entering))
}

/// [`counters`] once some permanent replaces counters being put: the loop of [CR 616.1]: "If
/// two or more replacement and/or prevention effects are attempting to modify the way an event
/// affects an object or player, the affected object's controller (or its owner if it has no
/// controller) or the affected player chooses one to apply". Each replacement applies at most
/// once ([CR 614.5]): "A replacement effect doesn't invoke itself repeatedly; it gets only one
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
fn replaced<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut p: Counters<L>,
    entering: Option<Entering<L>>,
) -> Result<u8, Full> {
    let chooser = match entering {
        Some(e) => e.controller,
        None => s.controller(p.on),
    };
    let mut applied = Applied::<L>::default();
    while p.amount > 0 {
        let found = candidates(s, t, p, entering, applied.as_slice())?;
        let Some(c) = choose(s, sink, chooser, found.as_slice())? else {
            break;
        };
        applied.push(Some((c.slot, c.ability)))?;
        p.amount = c.change.apply(p.amount)?;
    }
    Ok(p.amount)
}

/// The static abilities that apply to `p` now and are not among `applied`, in the order of
/// the battlefield.
fn candidates<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    p: Counters<L>,
    entering: Option<Entering<L>>,
    applied: &[Option<(L::Slot, crate::card::ObjectAbility)>],
) -> Result<Candidates<L>, Full> {
    let mut found = Candidates::<L>::default();
    let pick = move |_, a: &Ability, r: &Replacement| match *r {
        Replacement {
            replaces:
                Replaced::Counters {
                    objects,
                    kind,
                    only_effects,
                },
            rewrite,
        } if a.kind() == AbilityKind::Replacement
            && kind.is_none_or(|k| k == p.kind)
            && (p.by_effect || !only_effects) =>
        {
            Some((objects, Change::of(rewrite)?))
        }
        _ => None,
    };
    for (slot, ability, a, (objects, change)) in
        super::replacements(s, t, ReplacementKind::Counters, pick)
    {
        if applied.contains(&Some((slot, ability))) {
            continue;
        }
        if !accepts(s, t, p.on, slot, objects, entering)? {
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

/// Whether the filter of the replacement of the permanent in `source` accepts the object in
/// `on`, read for `source`'s controller: the permanent as it is, or as it would exist on the
/// battlefield when it is entering.
fn accepts<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    on: L::Slot,
    source: L::Slot,
    objects: &'static Filter,
    entering: Option<Entering<L>>,
) -> Result<bool, Full> {
    let (controller, words) = (s.controller(source), s.words_of(source));
    match entering {
        None => Ok(objects.matches(s, t, on, controller, &words)),
        Some(e) => {
            let entry = Entry {
                controller: Some(e.controller),
                ..e.entry
            };
            layers::entered(s, t, on, entry, e.copied, |as_it_would_exist| {
                objects.matches(as_it_would_exist, t, on, controller, &words)
            })
        }
    }
}
