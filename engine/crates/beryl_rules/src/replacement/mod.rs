//! Replacement effects ([CR 614.1]): "Such effects watch for a particular event that would
//! happen and completely or partially replace that event with a different event."
//!
//! Each replaceable happening is decided here before the state changes: where an object that
//! is about to move goes and how a permanent enters ([`moving`]), and what happens instead of a
//! card drawn ([`draw`]). The caller then performs what is decided, so the
//! event that was replaced never happens and nothing triggers on it ([CR 614.6]): "If an event
//! is replaced, it never happens. A modified event occurs instead, which may in turn trigger
//! abilities." A creature exiled instead of being put into a graveyard did not die.
//!
//! The replacements come from three places: an entry of the Effects store on the object
//! ([`MoveReplacement`]), a static ability of a permanent ([`Replaced`]), and the entering
//! object's own abilities. The card table holds only replacements this module has code for
//! ([`beryl_kernel::replacement::check`]).
//!
//! Implements: CR 614.1, CR 614.5, CR 614.6, CR 616.1
//!
//! [CR 614.1]: https://yawgatog.com/resources/magic-rules/#R6141
//! [CR 614.6]: https://yawgatog.com/resources/magic-rules/#R6146

use crate::card::{Ability, AbilityKind, ObjectAbility, Replaced, Replacement, Rewrite};
use crate::decision::{Choice, Decision};
use crate::filter::Evaluate;
use crate::fixed::{Fixed, Full};
use crate::interpret;
use crate::layout::Layout;
use crate::profile::Stops;
use crate::replacement::event::{Entry, ReplacementKind, Sink};
use crate::state::{MoveReplacement, State, Zone};
use crate::table::{Capability, GameTable};
use beryl_kernel::reference::Spell;

mod damage;
mod destruction;
pub(crate) use damage::{Damage, damage};
pub(crate) use destruction::destruction;

mod draws;
mod enchant;
mod entering;
pub mod event;
pub(crate) mod protection;
pub(crate) mod turns;

pub(crate) use draws::{DrawReplacement, draw, dredge, dredges};
pub(crate) use enchant::{Enchants, enchants};
pub(crate) use entering::{
    chosen_to_enter, controls_at_least, few_other_lands, life_to_enter, pay_life_to_enter,
};
use entering::{entering, entering_copy, entering_destinations, entering_values, selected_copy};

/// A move proposed for one object of a batch, before the state changes: the proposal the
/// replacement effects of a move rewrite ([`moving`]). Where in a library it goes and why it
/// moves are the same for the whole batch, and are kept by the batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Move<L: Layout> {
    pub slot: L::Slot,
    pub from: Zone,
    pub to: Zone,
    /// How it enters, if it is put onto the battlefield.
    pub entry: Entry<L>,
    /// It moves as its spell resolves: a permanent spell becoming a permanent
    /// (see [CR 608.3a]): "If the object that's resolving has no targets, it becomes a permanent
    /// and enters the battlefield". Only that resolution moves objects for
    /// [`crate::replacement::event::Reason::Resolve`], so a card put onto the battlefield by an effect
    /// "wasn't cast", whichever zone it left. It is not in [`Entry`], which a batch of moves
    /// holds 128 of.
    ///
    /// [CR 608.3a]: https://yawgatog.com/resources/magic-rules/#R6083a
    pub cast: bool,
}

/// Applies the replacement effects of a move to `proposal`. If any effect could redirect
/// the move, [`moving_with_replacements`] checks it against the chosen copy before deciding
/// where the object goes. A permanent that still enters gets its own remaining entry
/// replacements ([`entering`]). The answers are read through `sink` so a paused move can
/// reach the same choices on replay.
#[inline]
pub(crate) fn moving<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut proposal: Move<L>,
) -> Result<Move<L>, Full> {
    let statics = s
        .replacing()
        .is_some_and(|r| r.replaces(ReplacementKind::Move));
    if statics
        || s.move_replacements(s.object_ref(proposal.slot))
            .next()
            .is_some()
    {
        return moving_with_replacements(s, t, sink, proposal, statics);
    }
    if proposal.to == Zone::Battlefield {
        let copied = entering_copy(s, t, sink, &proposal)?;
        proposal.entry = entering(s, t, sink, proposal.slot, proposal.entry, copied)?;
    }
    Ok(proposal)
}

/// A replacement effect that applies to a move, by where it comes from: what [CR 614.5] counts
/// as having applied. "A replacement effect doesn't invoke itself repeatedly; it gets only one
/// opportunity to affect an event or any modified events that may replace that event."
///
/// [CR 614.5]: https://yawgatog.com/resources/magic-rules/#R6145
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source<S> {
    /// An entry of the Effects store on the moving object.
    Effect(MoveReplacement),
    /// A static ability of a permanent, which may be one of a card under it.
    Ability(S, ObjectAbility),
}

/// The most replacements one move can meet: [`moving_with_replacements`] holds this many.
const MOST: usize = 16;

/// Applies the replacements that can redirect the move while the layer results are current.
///
/// Applying them follows [CR 616.1]: "If two or more replacement and/or prevention effects are
/// attempting to modify the way an event affects an object or player, the affected object's
/// controller (or its owner if it has no controller) or the affected player chooses one to
/// apply". The effects that apply and have not applied are collected, one is applied, and
/// they are collected again ([CR 616.1f]): "Once the chosen effect has been applied, this
/// process is repeated (taking into account only replacement or prevention effects that would
/// now be applicable) until there are no more left to apply."
///
/// Effects that would put the object into the same zone are one candidate, applied together:
/// which of two Rest in Peace exiles a card makes no difference. A self-replacement effect
/// ([CR 616.1a]), such as "counter it; if it is countered this way, exile it instead", was
/// applied by the instruction that asked for the move, before it got here. An entry copy is
/// chosen before the first collection for a move to the battlefield ([CR 616.1c]): "If any
/// of the replacement and/or prevention effects would cause an object to become a copy of
/// another object as it enters the battlefield, one of them must be chosen." No replacement
/// changes control or turns a face (CR 616.1b and 616.1d). Apply the remaining candidates
/// under [CR 616.1e]: "Any of the applicable replacement and/or prevention effects may be chosen."
/// Where two candidates differ, they are applied in the order they are found, which no card
/// of a game makes happen yet.
///
/// [`Full`] past [`MOST`] replacements.
///
/// [CR 616.1]: https://yawgatog.com/resources/magic-rules/#R6161
/// [CR 616.1f]: https://yawgatog.com/resources/magic-rules/#R6161f
/// [CR 616.1a]: https://yawgatog.com/resources/magic-rules/#R6161a
/// [CR 616.1c]: https://yawgatog.com/resources/magic-rules/#R6161c
/// [CR 616.1e]: https://yawgatog.com/resources/magic-rules/#R6161e
#[inline(never)]
fn moving_with_replacements<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut proposal: Move<L>,
    statics: bool,
) -> Result<Move<L>, Full> {
    let slot = proposal.slot;
    let object = s.object_ref(slot);
    let mut applied: Fixed<Option<Source<L::Slot>>, MOST> = Fixed::new();
    let mut checked_copy = false;
    let mut copied = None;
    loop {
        let to = proposal.to;
        if to == Zone::Battlefield && !checked_copy {
            copied = entering_copy(s, t, sink, &proposal)?;
            checked_copy = true;
        }
        let mut found: Fixed<Option<(Source<L::Slot>, Zone)>, MOST> = Fixed::new();
        let mut add = |source, instead| {
            if applied.as_slice().contains(&Some(source)) {
                Ok(())
            } else {
                found.push(Some((source, instead)))
            }
        };
        for r in s.move_replacements(object) {
            if let Some(instead) = effect_destination(r, to) {
                add(Source::Effect(r), instead)?;
            }
        }
        if statics {
            for (permanent, ability, instead) in static_destinations(s, t, slot, to) {
                add(Source::Ability(permanent, ability), instead)?;
            }
            if to == Zone::Battlefield {
                entering_destinations(s, t, &proposal, copied, &mut add)?;
            }
        }
        let Some(&Some((_, instead))) = found.as_slice().first() else {
            break;
        };
        debug_assert!(
            found.iter().flatten().all(|&(_, zone)| zone == instead),
            "the affected player would choose between replacements putting it into different zones"
        );
        for &(source, zone) in found.iter().flatten() {
            if zone == instead {
                applied.push(Some(source))?;
            }
        }
        proposal.to = instead;
    }
    if proposal.to == Zone::Battlefield {
        proposal.entry = entering(s, t, sink, proposal.slot, proposal.entry, copied)?;
    }
    Ok(proposal)
}

/// Where an entry of the Effects store puts an object that would be put into `to`, if it
/// applies: "exile it instead" of a graveyard, or a flashback cost's, which exiles the spell
/// wherever else it would go (see [CR 702.34a]): "If the flashback cost was paid, exile this card
/// instead of putting it anywhere else any time it would leave the stack."
///
/// [CR 702.34a]: https://yawgatog.com/resources/magic-rules/#R70234a
fn effect_destination(r: MoveReplacement, to: Zone) -> Option<Zone> {
    let applies = match r {
        MoveReplacement::ExileInsteadOfGraveyard => to == Zone::Graveyard,
        MoveReplacement::Flashback => to != Zone::Exile,
    };
    applies.then_some(Zone::Exile)
}

/// The replacement abilities the permanent in `p` has now ([`State::abilities`]) that `pick`
/// accepts, each with what `pick` read of its replacement, its place among the permanent's
/// abilities and the ability, in the order of the permanent's abilities. `pick` is asked first
/// and the ability's condition only of one it accepts ([`interpret::static_condition_holds`]),
/// so the cheap tests go in `pick`.
///
/// Those of the face that is up come first, then those of the cards under a merged permanent
/// (see [CR 702.140e]): "A mutated permanent has all abilities of each card and token that
/// represents it." The card table refuses a card that grants a replacement effect
/// (`TableError::Granted`), so none is granted.
///
/// [CR 702.140e]: https://yawgatog.com/resources/magic-rules/#R702140e
pub(crate) fn statics<'a, L: Layout, T>(
    s: &'a State<L>,
    t: &'a GameTable,
    p: L::Slot,
    mut pick: impl FnMut(&'static Ability, &'static Replacement) -> Option<T> + 'a,
) -> impl Iterator<Item = (ObjectAbility, &'static Ability, T)> + 'a {
    s.abilities(t, p).filter_map(move |(ability, a)| {
        if matches!(ability, ObjectAbility::Granted(_)) {
            return None;
        }
        let picked = pick(a, a.replacement()?)?;
        interpret::static_condition_holds(s, t, p, ability, a).then_some((ability, a, picked))
    })
}

/// Whether the permanent in `p` may have a replacement ability of `kind`: one of the face that
/// is up, or, of a merged permanent, of a card under it ([`statics`]). The faces under it are
/// read only in a game with a card that merges.
#[inline]
pub(crate) fn may_replace<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    p: L::Slot,
    kind: ReplacementKind,
) -> bool {
    s.definition(p).may_replace(t).replaces(kind)
        || (t.can(Capability::Merges) && merged_may_replace(s, t, p, kind))
}

/// [`may_replace`] for the cards under a merged permanent. Out of line: only a game with a
/// card that merges comes here.
#[cold]
#[inline(never)]
fn merged_may_replace<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    p: L::Slot,
    kind: ReplacementKind,
) -> bool {
    s.merged_faces_now(t, p).any(|(card, face)| {
        crate::definition::Definition::<L>::of_face(card, face)
            .may_replace(t)
            .replaces(kind)
    })
}

/// The replacement abilities of `kind` that the permanents on the battlefield have now and
/// `pick` accepts ([`statics`]), in the order of the battlefield, each with its permanent,
/// its place among that permanent's abilities, the ability and what `pick` read of it. `pick`
/// is given the permanent's slot, and is asked only of a permanent that may have such an
/// ability ([`may_replace`]).
pub(crate) fn replacements<'a, L: Layout, T: 'a>(
    s: &'a State<L>,
    t: &'a GameTable,
    kind: ReplacementKind,
    pick: impl Fn(L::Slot, &'static Ability, &'static Replacement) -> Option<T> + Copy + 'a,
) -> impl Iterator<Item = (L::Slot, ObjectAbility, &'static Ability, T)> + 'a {
    s.battlefield()
        .iter()
        .copied()
        .filter(move |&p| may_replace(s, t, p, kind))
        .flat_map(move |p| {
            statics(s, t, p, move |a, r| pick(p, a, r))
                .map(move |(ability, a, picked)| (p, ability, a, picked))
        })
}

/// The static abilities of permanents that replace the object in `slot` being put into `to`,
/// each with the zone it goes to instead, in the order of the battlefield: "If a card or token
/// would be put into a graveyard from anywhere, exile it instead." The filter is read for the
/// permanent's controller.
fn static_destinations<'a, L: Layout>(
    s: &'a State<L>,
    t: &'a GameTable,
    slot: L::Slot,
    to: Zone,
) -> impl Iterator<Item = (L::Slot, ObjectAbility, Zone)> + 'a {
    let put_into = move |_, a: &Ability, r: &Replacement| match *r {
        Replacement {
            replaces: Replaced::PutInto(objects, zone),
            rewrite: Rewrite::ToZone(instead),
        } if a.kind() == AbilityKind::Replacement && zone == to => Some((objects, instead)),
        _ => None,
    };
    replacements(s, t, ReplacementKind::Move, put_into).filter_map(
        move |(p, ability, a, (objects, instead))| {
            if !objects.matches(s, t, slot, s.controller(p), &s.words_of(p)) {
                return None;
            }
            crate::coverage::hit(a);
            Some((p, ability, instead))
        },
    )
}

/// Whether `spell`, about to move to `to`, is exiled instead by an entry of the Effects store
/// on it ([`effect_destination`]), for an instruction that takes a spell off the stack and
/// must know where its card goes before it moves.
pub(crate) fn exiled_instead<L: Layout>(s: &State<L>, spell: Spell<L>, to: Zone) -> bool {
    s.move_replacements(spell.into())
        .any(|r| effect_destination(r, to).is_some())
}

/// Whether a replacement effect's choice can come up ([CR 616.1]): an optional replacement
/// may apply during the next resolution or step, a replacement of draws whose instructions
/// may ask a question, or two replacements of damage whose order changes the result. The
/// engine runs each resolution and step that may meet such a choice on a copy of the state,
/// so the choice can pause it ([`crate::asking`]). A run that does not run
/// on the copy cannot pause, and takes the first candidate of any choice it meets ([`answer`]),
/// which declines an optional replacement.
///
/// It holds in every game with a card that has dredge, wherever that card is. Dredge works
/// from the graveyard as the draw happens
/// (see [CR 702.52a]), and the same run can put a dredge card there before it draws, as an
/// ability that mills two cards and then draws one does. A replacement of draws by
/// instructions ([`ReplacementKind::DrawInstructions`]) may ask too ("discards a card"), and
/// the drawing player chooses the order of it and another replacement of the draw ([`draw`]),
/// so this holds in every game with a card that has one: such a card may enter the battlefield
/// during the run. So does a game with a card whose static ability replaces damage
/// ([`ReplacementKind::Damage`]): the player dealt the damage chooses its order against a
/// prevention shield of an amount ([`damage`]). A game whose cards have none of these pays two
/// ANDs. Two replacements of a move that send an object to different zones would be a choice
/// too, but every card with such a replacement sends it to exile. Two replacements of the
/// same draw that give different players the win would also differ. With the current card
/// pool this cannot happen, and the engine takes the first candidate if it does.
///
/// [CR 616.1]: https://yawgatog.com/resources/magic-rules/#R6161
/// [CR 702.52a]: https://yawgatog.com/resources/magic-rules/#R70252a
#[inline]
pub(crate) fn may_ask(t: &GameTable) -> bool {
    let replaces = t.replaces_any();
    t.can(Capability::Dredge)
        || replaces.replaces(ReplacementKind::DrawInstructions)
        || replaces.replaces(ReplacementKind::Damage)
}

/// Whether a resolution may meet a choice it did not declare it asks: a replacement effect's
/// ([`may_ask`]), or a value a permanent chooses as it enters, which any resolution may put
/// onto the battlefield in a game with a card that chooses one
/// ([`Capability::ChoosesAsSpellResolves`]), or a mana ability's choice while a spell is
/// being cast in the resolution ([`Capability::ManaAbilityChoices`]). An Aura chooses what to
/// enchant as it enters by any means other than resolving as an Aura spell (see CR 303.4f: "If
/// an Aura is entering the battlefield under a player's control by any means other than by
/// resolving as an Aura spell, ... that player chooses what it will enchant"). So in a game
/// whose only such card is an Aura ([`Capability::ChoosesAsEnters`]), a resolution may ask
/// when its `ability`, `None` for a permanent spell, may put a permanent onto the battlefield.
/// A game with none of them pays the ANDs [`may_ask`] does and two more.
#[inline]
pub(crate) fn resolution_may_ask(t: &GameTable, ability: Option<&Ability>) -> bool {
    may_ask(t)
        || t.can_either(
            Capability::ChoosesAsSpellResolves,
            Capability::ManaAbilityChoices,
        )
        || (t.can(Capability::ChoosesAsEnters) && ability.is_some_and(Ability::may_enter_permanent))
}

/// The answer to a choice `d` asked below code that has no resolution to ask through: a
/// replacement effect's choice (CR 616.1), or a value a permanent chooses as it enters. One
/// answer is taken silently, except under [`Stops::Forced`]. Otherwise the run's record
/// answers it, or the run stops at it ([`crate::asking`]), returning [`Full`] for whoever
/// opened the run to tell apart. A run that cannot stop, which a choice no state could
/// foresee meets, takes the first candidate: declining, for an optional replacement.
pub(crate) fn answer<L: Layout>(
    s: &State<L>,
    sink: &mut Sink<L>,
    d: &Decision<L>,
) -> Result<Choice, Full> {
    debug_assert!(!d.is_empty(), "a choice with no candidate");
    if d.len() == 1 && s.profile().stops != Stops::Forced {
        return Ok(Choice(0));
    }
    let asking = &mut sink.asking;
    if let Some(c) = asking.recorded() {
        return Ok(c);
    }
    if asking.is_open() {
        return Err(asking.stop(d.clone()));
    }
    Ok(Choice(0))
}
