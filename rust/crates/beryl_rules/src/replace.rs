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
//! ([`crate::replacement::check`]).
//!
//! [CR 614.1]: https://yawgatog.com/resources/magic-rules/#R6141
//! [CR 614.6]: https://yawgatog.com/resources/magic-rules/#R6146

use beryl_card::prompt::Purpose;
use beryl_card::{ChosenKind, Prompt};
use beryl_kernel::chosen::ChosenValue;

use crate::bitboard::Bitboard;
use crate::card::{
    Ability, AbilityIx, AbilityKind, ObjectAbility, Replaced, Replacement, Rewrite, Types,
};
use crate::decision::{Candidates, Choice, Decision, DecisionKind, OPTIONS, Opt};
use crate::event::{Entry, ReplacementKind, Sink};
use crate::filter::Evaluate;
use crate::fixed::{Fixed, Full};
use crate::game::Game;
use crate::ids::Seat;
use crate::layers;
use crate::layout::{Layout, Queue};
use crate::life;
use crate::profile::Stops;
use crate::state::{AbilityMask, CounterKind, MoveReplacement, ObjectRef, State, Zone};
use crate::table::{Capability, GameTable};
use crate::{chosen, interpret};
use beryl_kernel::copiable::additions::Addition;
use beryl_kernel::query::Copiable;

mod damage;
mod destruction;
pub(crate) use damage::{Damage, damage};
pub(crate) use destruction::destruction;

mod draws;
mod enchant;

pub(crate) use draws::{DrawReplacement, draw, dredge, dredges};
pub(crate) use enchant::{Enchants, enchants};

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
    /// ([CR 608.3a]): "If the object that's resolving has no targets, it becomes a permanent
    /// and enters the battlefield". Only that resolution moves objects for
    /// [`crate::event::Reason::Resolve`], so a card put onto the battlefield by an effect
    /// "wasn't cast", whichever zone it left. It is not in [`Entry`], which a batch of moves
    /// holds 128 of.
    ///
    /// [CR 608.3a]: https://yawgatog.com/resources/magic-rules/#R6083a
    pub cast: bool,
}

/// Applies the replacement effects of a move to `proposal`: where the object goes
/// ([`destination`]), then, for a permanent that enters, how it enters ([`entering`]). A
/// value the permanent chooses as it enters is asked through `sink`'s record of answers, and
/// kept in `sink` until the permanent has entered ([`Sink::chosen`]).
#[inline]
pub(crate) fn moving<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut proposal: Move<L>,
) -> Result<Move<L>, Full> {
    proposal.to = destination(s, t, &proposal)?;
    if proposal.to == Zone::Battlefield {
        proposal.entry = entering(s, t, sink, proposal.slot, proposal.entry)?;
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

/// The most replacements one move can meet: the scratch lists of [`destination`] hold this
/// many.
const MOST: usize = 16;

/// The zone the object of `proposal`, about to be put into its `to`, goes to after the
/// replacement effects that apply to it, read while the layer results are current.
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
/// applied by the instruction that asked for the move, before it got here. No replacement of a
/// move changes control, copies or turns a face (CR 616.1b to 616.1d), so every candidate here
/// is of [CR 616.1e]: "Any of the applicable replacement and/or prevention effects may be
/// chosen." Where two candidates differ, they are applied in the order they are found, which
/// no card of a game makes happen yet.
///
/// With no replacement of a move on the battlefield or on the object, it is one AND and an
/// empty search of the Effects store.
///
/// [`Full`] past [`MOST`] replacements.
///
/// [CR 616.1]: https://yawgatog.com/resources/magic-rules/#R6161
/// [CR 616.1f]: https://yawgatog.com/resources/magic-rules/#R6161f
/// [CR 616.1a]: https://yawgatog.com/resources/magic-rules/#R6161a
/// [CR 616.1e]: https://yawgatog.com/resources/magic-rules/#R6161e
#[inline]
fn destination<L: Layout>(s: &State<L>, t: &GameTable, proposal: &Move<L>) -> Result<Zone, Full> {
    let statics = s
        .replacing()
        .is_some_and(|r| r.replaces(ReplacementKind::Move));
    if !statics
        && s.move_replacements(s.object_ref(proposal.slot))
            .next()
            .is_none()
    {
        return Ok(proposal.to);
    }
    replaced_destination(s, t, proposal, statics)
}

/// [`destination`] once some replacement of a move may apply: the loop of CR 616.1.
#[inline(never)]
fn replaced_destination<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    proposal: &Move<L>,
    statics: bool,
) -> Result<Zone, Full> {
    let (slot, mut to) = (proposal.slot, proposal.to);
    let object = s.object_ref(slot);
    let mut applied: Fixed<Option<Source<L::Slot>>, MOST> = Fixed::new();
    loop {
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
                entering_destinations(s, t, proposal, &mut add)?;
            }
        }
        let Some(&Some((_, instead))) = found.as_slice().first() else {
            return Ok(to);
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
        to = instead;
    }
}

/// Where an entry of the Effects store puts an object that would be put into `to`, if it
/// applies: "exile it instead" of a graveyard, or a flashback cost's, which exiles the spell
/// wherever else it would go ([CR 702.34a]): "If the flashback cost was paid, exile this card
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
/// ([CR 702.140e]): "A mutated permanent has all abilities of each card and token that
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
    s.battlefield()
        .iter()
        .filter(move |&&p| may_replace(s, t, p, ReplacementKind::Move))
        .flat_map(move |&p| {
            let controller = s.controller(p);
            let words = s.words_of(p);
            let put_into = move |a: &Ability, r: &Replacement| match *r {
                Replacement {
                    replaces: Replaced::PutInto(objects, zone),
                    rewrite: Rewrite::ToZone(instead),
                } if a.kind() == AbilityKind::Replacement && zone == to => Some((objects, instead)),
                _ => None,
            };
            statics(s, t, p, put_into).filter_map(move |(ability, a, (objects, instead))| {
                if !objects.matches(s, t, slot, controller, &words) {
                    return None;
                }
                crate::coverage::hit(a);
                Some((p, ability, instead))
            })
        })
}

/// Offers to `add` each static ability of a permanent that replaces the object of `proposal`
/// entering the battlefield as its `entry` says, with the zone it goes to instead, in the order
/// of the battlefield: "If a nontoken creature would enter and it wasn't cast, exile it
/// instead." The filter is read for the permanent's controller, of the object as it would
/// exist on the battlefield ([CR 614.12]): "To determine which replacement effects apply and
/// how they apply, check the characteristics of the permanent as it would exist on the
/// battlefield". An enchantment that another effect would make a creature there is a
/// creature. A permanent entering is not yet on the battlefield, so its own such ability does
/// not replace its entering, nor that of anything entering with it.
///
/// The state as it would be with the object entered is made only for an ability left to ask
/// ([`layers::entered`]); [`Full`] if making it, or `add`, ran out of room.
///
/// [CR 614.12]: https://yawgatog.com/resources/magic-rules/#R61412
fn entering_destinations<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    proposal: &Move<L>,
    mut add: impl FnMut(Source<L::Slot>, Zone) -> Result<(), Full>,
) -> Result<(), Full> {
    let Move {
        slot, entry, cast, ..
    } = *proposal;
    for &p in s.battlefield() {
        if !may_replace(s, t, p, ReplacementKind::Move) {
            continue;
        }
        let controller = s.controller(p);
        let words = s.words_of(p);
        let enters = |a: &Ability, r: &Replacement| match *r {
            Replacement {
                replaces: Replaced::Enters { objects, not_cast },
                rewrite: Rewrite::ToZone(instead),
            } if a.kind() == AbilityKind::Replacement && !(not_cast && cast) => {
                Some((objects, instead))
            }
            _ => None,
        };
        for (ability, a, (objects, instead)) in statics(s, t, p, enters) {
            let passes = layers::entered(s, t, slot, entry, None, |as_it_would_exist| {
                objects.matches(as_it_would_exist, t, slot, controller, &words)
            })?;
            if !passes {
                continue;
            }
            crate::coverage::hit(a);
            add(Source::Ability(p, ability), instead)?;
        }
    }
    Ok(())
}

/// Applies the permanent's own replacement effects that modify how it enters (CR 614.12):
/// first one that has it enter as a copy, then the others, found against the copy
/// ([`copy_to_enter`]). Then
/// they "check the characteristics of the permanent as it would exist on the battlefield".
/// One it would not have there does not apply ([`layers::lost_entering`]): under an effect
/// that makes nonbasic lands Mountains, a land that would enter tapped unless its controller
/// paid life enters untapped, and no life is paid. Each changes only how it enters and none
/// conflicts with another, so each applies in turn. A choice one requires is made here, before
/// the permanent enters ([CR 614.12a]): "If a replacement effect that modifies how a permanent
/// enters the battlefield requires a choice, that choice is made before the permanent enters
/// the battlefield." Then the planeswalker's intrinsic one
/// ([CR 306.5b]): "A planeswalker has the intrinsic ability 'This permanent enters with a
/// number of loyalty counters on it equal to its printed loyalty number.'"
///
/// [CR 306.5b]: https://yawgatog.com/resources/magic-rules/#R3065b
/// [CR 614.12a]: https://yawgatog.com/resources/magic-rules/#R61412a
fn entering<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
    mut entry: Entry<L>,
) -> Result<Entry<L>, Full> {
    let o = s.object(slot);
    let controller = entry.controller.unwrap_or(o.owner());
    let mut face = s.copiable(slot).turned(entry.face).face(t);
    // Found at the first replacement of entering: most permanents have none.
    let mut lost = None;
    if t.can(Capability::ChoosesAsEnters)
        && let Some(copy) = copy_to_enter(s, t, sink, slot, controller, entry)?
    {
        sink.copies.push((slot, copy))?;
        entry.face = copy.record(t).1;
        face = copy.face(t);
        // What the copy would lose is not found yet: no card of the pool removes abilities
        // from a permanent as it enters.
        lost = Some(L::AbilityMask::default());
    }
    let mut pays_life = false;
    for (ability, n) in face.abilities.iter().zip(0..) {
        let Some(replacement) = ability.replacement() else {
            continue;
        };
        if replacement.replaces != Replaced::ThisEnters {
            continue;
        }
        let lost = match lost {
            Some(lost) => lost,
            None => *lost.insert(layers::lost_entering(s, t, slot, entry, face)?),
        };
        if lost.contains(AbilityIx::new(n)) {
            continue;
        }
        match replacement.rewrite {
            Rewrite::EntersTapped => entry.tapped = true,
            Rewrite::EntersTappedUnlessFewLands(n) => {
                // Not yet on the battlefield, so every land counted is another land.
                let lands = s
                    .battlefield()
                    .iter()
                    .filter(|&&p| {
                        let d = s.derived(p);
                        d.controller == controller && d.types.contains(Types::LAND)
                    })
                    .count();
                if lands > usize::from(n) {
                    entry.tapped = true;
                }
            }
            Rewrite::EntersTappedUnlessPayLife(_) => {
                pays_life = entry.pay_life;
                if !entry.pay_life {
                    entry.tapped = true;
                }
            }
            // CR 614.1c: "Effects that read '[This permanent] enters with . . . ,' ... are
            // replacement effects."
            Rewrite::EntersWithCounters(kind, n) => entry.counters = Some((kind, n)),
            Rewrite::Choose(kind) => {
                let value = choose_value(s, t, sink, slot, controller, kind)?;
                sink.chosen.push((slot, value))?;
            }
            // `EnterAsCopy` is applied first ([`copy_to_enter`]); a copied one finds the event
            // already modified by a copy effect, which it is one of (CR 614.5). The card table
            // holds no other rewrite of entering.
            Rewrite::EnterAsCopy { .. }
            | Rewrite::None
            | Rewrite::ToZone(_)
            | Rewrite::Instead
            | Rewrite::Dredge(_)
            | Rewrite::Multiply(_) => continue,
        }
        crate::coverage::hit(ability);
    }
    // Life is paid only to an ability that applied.
    entry.pay_life = pays_life;
    if let Some(loyalty) = face.loyalty {
        entry.counters = Some((CounterKind::Loyalty, loyalty));
    }
    Ok(entry)
}

/// The copiable values the object in `slot` enters as a copy of, if it has a replacement that
/// may have it enter as one and its controller `player` chooses a permanent
/// ([`Rewrite::EnterAsCopy`]): those of the chosen permanent, with their own exceptions, and
/// the exception of the ability after them ([CR 707.9b]): "The final set of values for that
/// characteristic becomes part of the copiable values of the copy." So a copy of a copy takes
/// what the copied permanent copied ([CR 707.3]): "Objects that copy the object will use the
/// new copiable values."
///
/// The choice comes before the other replacements of entering ([CR 616.1c]): "If any of the
/// replacement and/or prevention effects would cause an object to become a copy of another
/// object as it enters the battlefield, one of them must be chosen." It is a choice made before
/// the permanent enters ([CR 614.12a]), among the permanents on the battlefield the filter
/// accepts, read for `player`: none of a batch entering with it, which are not there yet. The
/// permanent is not targeted, so hexproof and protection do not stop it. Declining is choosing
/// none, and the object enters as itself.
///
/// [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b
/// [CR 707.3]: https://yawgatog.com/resources/magic-rules/#R7073
/// [CR 616.1c]: https://yawgatog.com/resources/magic-rules/#R6161c
/// [CR 614.12a]: https://yawgatog.com/resources/magic-rules/#R61412a
#[cold]
#[inline(never)]
fn copy_to_enter<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
    player: Seat,
    entry: Entry<L>,
) -> Result<Option<Copiable<L>>, Full> {
    let values = s.copiable(slot).turned(entry.face);
    let face = values.face(t);
    let Some((n, ability, of, except)) =
        (0u8..)
            .zip(face.abilities)
            .find_map(|(n, a)| match a.replacement()?.rewrite {
                Rewrite::EnterAsCopy { of, except } => Some((n, a, of, except)),
                Rewrite::None
                | Rewrite::ToZone(_)
                | Rewrite::EntersTapped
                | Rewrite::EntersTappedUnlessFewLands(_)
                | Rewrite::EntersTappedUnlessPayLife(_)
                | Rewrite::EntersWithCounters(..)
                | Rewrite::Instead
                | Rewrite::Dredge(_)
                | Rewrite::Choose(_)
                | Rewrite::Multiply(_) => None,
            })
    else {
        return Ok(None);
    };
    if layers::lost_entering(s, t, slot, entry, face)?.contains(AbilityIx::new(n)) {
        return Ok(None);
    }
    let words = crate::card::Words::NONE;
    let mut options = <L::Queue<Opt<L>, OPTIONS>>::default();
    options.push(Opt::Done)?;
    let eligible = s
        .battlefield()
        .iter()
        .copied()
        .filter(|&p| p != slot && of.matches(s, t, p, player, &words))
        .collect();
    Candidates::<L>::extend_options(&mut options, eligible, |out, p| out.push(Opt::Object(p)))?;
    let d = Decision::options(
        player,
        DecisionKind::ChooseUpTo,
        options,
        Prompt::new(Purpose::Choose).most(1),
    );
    let Some(Opt::Object(copied)) = d.candidates.option(answer(s, sink, &d)?) else {
        return Ok(None);
    };
    crate::coverage::hit(ability);
    let mut copy = s.copiable(copied).recorded(t);
    if let Some(except) = except {
        let (card, face) = values.record(t);
        copy = copy.with_addition(Addition::new(card, face, except))?;
    }
    Ok(Some(copy))
}

/// The values an entering permanent chose to copy, before it reaches the battlefield
/// ([`copy_to_enter`]).
fn selected_copy<L: Layout>(sink: &Sink<L>, slot: L::Slot) -> Option<Copiable<L>> {
    sink.copies
        .iter()
        .find_map(|&(copier, values)| (copier == slot).then_some(values))
}

/// The life an "unless you pay N life" replacement of the entering object asks for, if it has
/// one ([`Rewrite::EntersTappedUnlessPayLife`]), with the index of its ability.
fn life_asked(face: &crate::card::Face) -> Option<(AbilityIx, u8)> {
    (0u8..)
        .zip(face.abilities)
        .find_map(|(n, a)| match a.replacement()?.rewrite {
            Rewrite::EntersTappedUnlessPayLife(life) => Some((AbilityIx::new(n), life)),
            Rewrite::None
            | Rewrite::ToZone(_)
            | Rewrite::EntersTapped
            | Rewrite::EntersTappedUnlessFewLands(_)
            | Rewrite::EntersWithCounters(..)
            | Rewrite::Instead
            | Rewrite::Dredge(_)
            | Rewrite::Choose(_)
            | Rewrite::Multiply(_)
            | Rewrite::EnterAsCopy { .. } => None,
        })
}

/// Pays the life an "unless you pay N life" entering replacement asks for
/// ([CR 119.4]): "If a player pays life, the payment is subtracted from their life total; in
/// other words, the player loses that much life."
///
/// [CR 119.4]: https://yawgatog.com/resources/magic-rules/#R1194
pub(crate) fn pay_life_to_enter<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
    entry: Entry<L>,
) -> Result<(), Full> {
    let o = s.object(slot);
    let copied = selected_copy(sink, slot);
    let values = copied.unwrap_or_else(|| s.copiable(slot).turned(entry.face));
    let player = entry.controller.unwrap_or(o.owner());
    if let Some((ability, n)) = life_asked(values.face(t))
        && (copied.is_some() || s.has_ability(slot, ability))
    {
        // The life is lost to the permanent's own replacement effect ([CR 614.1c]: "Effects
        // that read ... "As [this permanent] enters . . . ," ... are replacement effects."),
        // not to whatever puts the permanent onto the battlefield.
        //
        // [CR 614.1c]: https://yawgatog.com/resources/magic-rules/#R6141c
        sink.enter_source(|| s.object_ref(slot));
        let lost = life::lose(&mut Game::new(s, t, sink), player, n);
        sink.leave();
        lost?;
    }
    Ok(())
}

/// The life an entering replacement of the object in `slot` asks for as it enters as
/// `entry` says, if any: none when the object would not have that ability on the
/// battlefield ([`layers::lost_entering`], CR 614.12).
///
/// [`Full`] if finding what it would have ran out of room.
pub(crate) fn life_to_enter<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    entry: Entry<L>,
) -> Result<Option<u8>, Full> {
    let face = s.copiable(slot).turned(entry.face).face(t);
    let Some((ability, life)) = life_asked(face) else {
        return Ok(None);
    };
    let lost = layers::lost_entering(s, t, slot, entry, face)?;
    Ok((!lost.contains(ability)).then_some(life))
}

/// Whether `object`, about to move to `to`, is exiled instead by an entry of the Effects store
/// on it ([`effect_destination`]), for an instruction that takes a spell off the stack and
/// must know where its card goes before it moves.
pub(crate) fn exiled_instead<L: Layout>(s: &State<L>, object: ObjectRef<L>, to: Zone) -> bool {
    s.move_replacements(object)
        .any(|r| effect_destination(r, to).is_some())
}

/// Whether a replacement effect's choice can come up ([CR 616.1]): an optional replacement
/// may apply during the next resolution or step, a replacement of draws whose instructions
/// may ask a question, or two replacements of damage whose order changes the result. The engine runs each resolution and step that may meet such a choice on
/// a copy of the state, so the choice can pause it ([`crate::asking`]). A run that does not run
/// on the copy cannot pause, and takes the first candidate of any choice it meets ([`answer`]),
/// which declines an optional replacement.
///
/// It holds in every game with a card that has dredge, wherever that card is. Dredge works
/// from the graveyard as the draw happens
/// ([CR 702.52a]), and the same run can put a dredge card there before it draws, as an
/// ability that mills two cards and then draws one does. A replacement of draws by
/// instructions ([`ReplacementKind::DrawInstructions`]) may ask too ("discards a card"), and
/// the drawing player chooses the order of it and another replacement of the draw ([`draw`]),
/// so this holds in every game with a card that has one: such a card may enter the battlefield
/// during the run. So does a game with a card whose static ability replaces damage
/// ([`ReplacementKind::Damage`]): the player dealt the damage chooses its order against a
/// prevention shield of an amount ([`damage`]). A game whose cards have none of these pays two
/// ANDs. Two replacements of a move
/// that would put an object into different zones would be a choice too, but no card yet puts
/// one anywhere but exile, so [`destination`] has nothing to choose between. Two replacements
/// of a draw that each end the game, of permanents of different players, would differ too: no
/// card of the pool makes that happen, and the choice then takes its first candidate.
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
/// ([`Capability::ChoosesAsEnters`]). A game with none of them pays the ANDs [`may_ask`]
/// does and one more.
#[inline]
pub(crate) fn resolution_may_ask(t: &GameTable) -> bool {
    may_ask(t) || t.can(Capability::ChoosesAsEnters)
}

/// The value of `kind` that the permanent in `slot` chooses as it enters under `player`'s
/// control ([`Rewrite::Choose`]), asked through the record of answers ([`answer`]).
fn choose_value<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
    player: Seat,
    kind: ChosenKind,
) -> Result<ChosenValue<L>, Full> {
    let d = chosen::decision(t, player, kind, Bitboard::single(slot))?;
    let c = answer(s, sink, &d)?;
    Ok(chosen::value(kind, d.candidates.option(c)))
}

/// The kind of value the face `face` chooses as it enters, if it chooses one
/// ([`Rewrite::Choose`]), with the index of its ability.
fn chooses(face: &crate::card::Face) -> Option<(AbilityIx, ChosenKind)> {
    (0u8..)
        .zip(face.abilities)
        .find_map(|(n, a)| match a.replacement()?.rewrite {
            Rewrite::Choose(kind) => Some((AbilityIx::new(n), kind)),
            Rewrite::None
            | Rewrite::ToZone(_)
            | Rewrite::EntersTapped
            | Rewrite::EntersTappedUnlessFewLands(_)
            | Rewrite::EntersTappedUnlessPayLife(_)
            | Rewrite::EntersWithCounters(..)
            | Rewrite::Instead
            | Rewrite::Dredge(_)
            | Rewrite::Multiply(_)
            | Rewrite::EnterAsCopy { .. } => None,
        })
}

/// The kind of value the object in `slot` chooses as it enters as `entry` says, if any: none
/// when it would not have that ability on the battlefield ([`layers::lost_entering`],
/// CR 614.12).
///
/// [`Full`] if finding what it would have ran out of room.
pub(crate) fn chosen_to_enter<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    entry: Entry<L>,
) -> Result<Option<ChosenKind>, Full> {
    let face = s.copiable(slot).turned(entry.face).face(t);
    let Some((ability, kind)) = chooses(face) else {
        return Ok(None);
    };
    let lost = layers::lost_entering(s, t, slot, entry, face)?;
    Ok((!lost.contains(ability)).then_some(kind))
}

/// The answer to a choice `d` asked below code that has no resolution to ask through: a
/// replacement effect's choice (CR 616.1), or a value a permanent chooses as it enters. One
/// answer is taken silently, except under [`Stops::Forced`]. Otherwise the run's record
/// answers it, or the run stops at it ([`crate::asking`]), returning [`Full`] for whoever
/// opened the run to tell apart. A run that cannot stop, which a choice no state could
/// foresee meets, takes the first candidate: declining, for an optional replacement.
fn answer<L: Layout>(s: &State<L>, sink: &mut Sink<L>, d: &Decision<L>) -> Result<Choice, Full> {
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
