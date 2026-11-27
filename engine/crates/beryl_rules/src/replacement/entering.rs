//! Replacement effects that modify how a permanent enters the battlefield ([CR 614.12]): "Some
//! replacement effects modify how a permanent enters the battlefield. ... To determine which
//! replacement effects apply and how they apply, check the characteristics of the permanent as
//! it would exist on the battlefield".
//!
//! An entering object meets them in this order, each read for it as it would exist there: the
//! choice of a permanent to enter as a copy of ([`copy_to_enter`]), the static abilities of
//! permanents already on the battlefield that send it elsewhere ([`entering_destinations`]),
//! then its own abilities that change how it enters ([`entering`]). Whatever puts it onto the
//! battlefield asks the life an "unless you pay N life" ability wants ([`life_to_enter`]) and
//! the kind of value it chooses ([`chosen_to_enter`]) before the move.
//!
//! Implements: CR 614.12
//!
//! [CR 614.12]: https://yawgatog.com/resources/magic-rules/#R61412

use beryl_card::prompt::Purpose;
use beryl_card::{ChosenKind, Prompt};
use beryl_kernel::chosen::ChosenValue;
use beryl_kernel::copiable::additions::Addition;
use beryl_kernel::query::Copiable;

use super::{Move, Source, answer, replacements};
use crate::bitboard::Bitboard;
use crate::card::{Ability, AbilityIx, AbilityKind, Replaced, Replacement, Rewrite, Types};
use crate::continuous::layers;
use crate::decision::{Candidates, Decision, DecisionKind, OPTIONS, Opt};
use crate::filter::Evaluate;
use crate::fixed::Full;
use crate::game::{Game, life};
use crate::ids::Seat;
use crate::interpret::catalog;
use crate::layout::{Layout, Queue};
use crate::profile::Stops;
use crate::replacement::event::{Entry, Preparation, ReplacementKind, Sink};
use crate::state::{AbilityMask, CounterKind, State, Zone};
use crate::table::{Capability, GameTable};

/// Offers to `add` each static ability of a permanent that replaces the object of `proposal`
/// entering the battlefield as its `entry` says and with the chosen copy applied, with the zone
/// it goes to instead, in battlefield order: "If a nontoken creature would enter and it
/// wasn't cast, exile it instead." The filter uses the permanent's controller and the
/// entering object's characteristics as it would exist on the battlefield ([CR 614.12]):
/// "To determine which replacement effects apply and how they apply, check the characteristics
/// of the permanent as it would exist on the battlefield". An enchantment that another effect
/// would make a creature there is a creature. The replacement comes from a permanent already
/// on the battlefield. A permanent entering cannot use such an ability on itself or another
/// object entering with it.
///
/// The state as it would be with the object entered is made only for an ability left to ask
/// ([`layers::entered`]); [`Full`] if making it, or `add`, ran out of room.
///
/// [CR 614.12]: https://yawgatog.com/resources/magic-rules/#R61412
pub(super) fn entering_destinations<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    proposal: &Move<L>,
    copied: Option<Copiable<L>>,
    mut add: impl FnMut(Source<L::Slot>, Zone) -> Result<(), Full>,
) -> Result<(), Full> {
    let Move {
        slot, entry, cast, ..
    } = *proposal;
    let enters = |_, a: &Ability, r: &Replacement| match *r {
        Replacement {
            replaces: Replaced::Enters { objects, not_cast },
            rewrite: Rewrite::ToZone(instead),
        } if a.kind() == AbilityKind::Replacement && !(not_cast && cast) => {
            Some((objects, instead))
        }
        _ => None,
    };
    for (p, ability, a, (objects, instead)) in replacements(s, t, ReplacementKind::Move, enters) {
        let (controller, words) = (s.controller(p), s.words_of(p));
        let passes = layers::entered(s, t, slot, entry, copied, |as_it_would_exist| {
            objects.matches(as_it_would_exist, t, slot, controller, &words)
        })?;
        if !passes {
            continue;
        }
        crate::coverage::hit(a);
        add(Source::Ability(p, ability), instead)?;
    }
    Ok(())
}

/// Applies the permanent's remaining entry replacements (CR 614.12). These effects
/// "check the characteristics of the permanent as it would exist on the battlefield",
/// including any copy already chosen for the move ([`copy_to_enter`]).
/// One it would not have there does not apply ([`layers::lost_entering`]): under an effect
/// that makes nonbasic lands Mountains, a land that would enter tapped unless its controller
/// paid life enters untapped, and no life is paid. Each changes only how it enters and none
/// conflicts with another, so each applies in turn. A choice one requires is made here, before
/// the permanent enters ([CR 614.12a]): "If a replacement effect that modifies how a permanent
/// enters the battlefield requires a choice, that choice is made before the permanent enters
/// the battlefield." Then the planeswalker's intrinsic one
/// (see [CR 306.5b]): "A planeswalker has the intrinsic ability 'This permanent enters with a
/// number of loyalty counters on it equal to its printed loyalty number.'"
///
/// [CR 306.5b]: https://yawgatog.com/resources/magic-rules/#R3065b
/// [CR 614.12a]: https://yawgatog.com/resources/magic-rules/#R61412a
#[allow(
    clippy::disallowed_methods,
    reason = "reads the face the permanent enters with (see CR 614.12)"
)]
pub(super) fn entering<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
    mut entry: Entry<L>,
    copied: Option<Copiable<L>>,
) -> Result<Entry<L>, Full> {
    let controller = entry.controller.unwrap_or(s.object(slot).owner());
    let face = entering_values(s, t, slot, entry, copied).face(t);
    // Found at the first replacement of entering: most permanents have none.
    let mut lost = None;
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
            None => *lost.insert(layers::lost_entering(s, t, slot, entry, face, copied)?),
        };
        if lost.contains(AbilityIx::new(n)) {
            continue;
        }
        match replacement.rewrite {
            Rewrite::EntersTapped => entry.tapped = true,
            // See CR 723.3a: "Some spells and abilities cause a permanent with a prepare
            // spell to become prepared or state that a permanent enters prepared."
            Rewrite::EntersPrepared => entry.preparation = Preparation::Prepared,
            Rewrite::EntersTappedUnlessFewLands(n) => {
                if !few_other_lands(s, controller, n) {
                    entry.tapped = true;
                }
            }
            Rewrite::EntersTappedUnlessYouControl(n, filter) => {
                if !controls_at_least(s, t, controller, n, filter) {
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
            | Rewrite::MaySkipTurnToUntap
            | Rewrite::Multiply(_)
            | Rewrite::Plus(_) => continue,
        }
        crate::coverage::hit(ability);
    }
    // Life is paid only to an ability that applied.
    entry.pay_life = pays_life;
    if let Some(loyalty) = face.loyalty {
        entry.counters = Some((CounterKind::Loyalty, loyalty));
    }
    if t.can(Capability::Saga) {
        crate::saga::entering(s, t, slot, &mut entry, copied)?;
    }
    if s.replacing()
        .is_some_and(|r| r.replaces(ReplacementKind::Counters))
    {
        entry = counters_entering(s, t, sink, slot, entry, copied, controller)?;
    }
    if let Some(values) = copied {
        sink.copies.push((slot, values))?;
        entry.face = values.record(t).1;
    }
    Ok(entry)
}

/// The counters `entry` enters with, as the replacement effects of counters leave them
/// ([`super::entering_counters`]). Each is put by an effect: one a replacement effect of the
/// permanent gives it, a planeswalker's printed loyalty, and a Saga's first lore counter. The
/// last two come from intrinsic abilities that create replacement effects (see [CR 306.5b]): "A
/// planeswalker has the intrinsic ability 'This permanent enters with a number of loyalty
/// counters on it equal to its printed loyalty number.' This ability creates a replacement
/// effect (see rule 614.1c)." And (see [CR 715.3a]): "Each Saga without read ahead has the
/// intrinsic ability 'This Saga enters with a lore counter on it.' This ability creates a
/// replacement effect (see rule 614.1c)." The rulings of a replacement limited to effects
/// double a planeswalker's loyalty, and `XMage` and Forge double a Saga's first lore counter the
/// same way.
///
/// [CR 306.5b]: https://yawgatog.com/resources/magic-rules/#R3065b
/// [CR 715.3a]: https://yawgatog.com/resources/magic-rules/#R7153a
#[cold]
#[inline(never)]
fn counters_entering<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
    mut entry: Entry<L>,
    copied: Option<Copiable<L>>,
    controller: Seat,
) -> Result<Entry<L>, Full> {
    let entering = super::Entering {
        entry,
        copied,
        controller,
    };
    if let Some((kind, amount)) = entry.counters {
        let proposal = super::Counters {
            on: slot,
            kind,
            amount,
            by_effect: true,
        };
        let n = super::entering_counters(s, t, sink, proposal, entering)?;
        entry.counters = Some((kind, n));
    }
    if entry.saga_lore > 0 {
        let proposal = super::Counters {
            on: slot,
            kind: CounterKind::Lore,
            amount: entry.saga_lore,
            by_effect: true,
        };
        entry.saga_lore = super::entering_counters(s, t, sink, proposal, entering)?;
    }
    Ok(entry)
}

/// The copy choice is possible only in games whose card table has one.
#[inline]
pub(super) fn entering_copy<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    proposal: &Move<L>,
) -> Result<Option<Copiable<L>>, Full> {
    if !t.can(Capability::ChoosesAsEnters) {
        return Ok(None);
    }
    copy_to_enter(s, t, sink, proposal.slot, proposal.entry)
}

/// The copiable values the object in `slot` enters as a copy of, if it has a replacement that
/// may have it enter as one and its prospective controller chooses a permanent
/// ([`Rewrite::EnterAsCopy`]): those of the chosen permanent, with their own exceptions, and
/// the exception of the ability after them (see [CR 707.9b]): "The final set of values for that
/// characteristic becomes part of the copiable values of the copy." So a copy of a copy takes
/// what the copied permanent copied (see [CR 707.3]): "Objects that copy the object will use the
/// new copiable values."
///
/// The choice comes before the other replacements of entering ([CR 616.1c]): "If any of the
/// replacement and/or prevention effects would cause an object to become a copy of another
/// object as it enters the battlefield, one of them must be chosen." It is a choice made before
/// the permanent enters ([CR 614.12a]), among the permanents on the battlefield the filter
/// accepts, read for that controller. Objects entering in the same batch are still off the
/// battlefield. The permanent is chosen without a target, so hexproof and protection do not
/// stop it. If the controller declines, remaining replacements see the object's uncopied
/// characteristics.
///
/// [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b
/// [CR 707.3]: https://yawgatog.com/resources/magic-rules/#R7073
/// [CR 616.1c]: https://yawgatog.com/resources/magic-rules/#R6161c
/// [CR 614.12a]: https://yawgatog.com/resources/magic-rules/#R61412a
#[cold]
#[inline(never)]
#[allow(
    clippy::disallowed_methods,
    reason = "reads the face the permanent enters with (see CR 614.12)"
)]
fn copy_to_enter<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
    entry: Entry<L>,
) -> Result<Option<Copiable<L>>, Full> {
    let values = entering_values(s, t, slot, entry, None);
    let face = values.face(t);
    let Some((n, ability, of, except)) =
        (0u8..)
            .zip(face.abilities)
            .find_map(|(n, a)| match a.replacement()?.rewrite {
                Rewrite::EnterAsCopy { of, except } => Some((n, a, of, except)),
                Rewrite::None
                | Rewrite::ToZone(_)
                | Rewrite::EntersTapped
                | Rewrite::EntersPrepared
                | Rewrite::EntersTappedUnlessFewLands(_)
                | Rewrite::EntersTappedUnlessYouControl(..)
                | Rewrite::EntersTappedUnlessPayLife(_)
                | Rewrite::EntersWithCounters(..)
                | Rewrite::Instead
                | Rewrite::Dredge(_)
                | Rewrite::Choose(_)
                | Rewrite::MaySkipTurnToUntap
                | Rewrite::Multiply(_)
                | Rewrite::Plus(_) => None,
            })
    else {
        return Ok(None);
    };
    if layers::lost_entering(s, t, slot, entry, face, None)?.contains(AbilityIx::new(n)) {
        return Ok(None);
    }
    let player = entry.controller.unwrap_or(s.object(slot).owner());
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
    let choice = answer(s, sink, &d).inspect_err(|_| {
        if s.profile().stops == Stops::Forced {
            sink.asking.mark_enter_as_copy();
        }
    })?;
    let Some(Opt::Object(copied)) = d.candidates.option(choice) else {
        return Ok(None);
    };
    crate::coverage::hit(ability);
    let mut copy = s.copiable(copied).recorded(t).with_flipped_status(t, false);
    if let Some(except) = except {
        let (card, face) = values.record(t);
        copy = copy.with_addition(Addition::new(card, face, except))?;
    }
    Ok(Some(copy))
}

/// The values an entering permanent chose to copy, before it reaches the battlefield
/// ([`copy_to_enter`]).
pub(super) fn selected_copy<L: Layout>(sink: &Sink<L>, slot: L::Slot) -> Option<Copiable<L>> {
    sink.copies
        .iter()
        .find_map(|&(copier, values)| (copier == slot).then_some(values))
}

/// The copiable values the object in `slot` enters with as `entry` says: those of the
/// permanent it chose to copy (`copied`, [`copy_to_enter`]), or else its own with the face
/// `entry` turns up.
pub(super) fn entering_values<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    entry: Entry<L>,
    copied: Option<Copiable<L>>,
) -> Copiable<L> {
    copied.unwrap_or_else(|| s.copiable(slot).turned(t, entry.face))
}

/// What `find` reads of an entering replacement of the object in `slot`, entering as `entry`
/// says, if its face has one: none when the object would not have that ability on the
/// battlefield ([`layers::lost_entering`], CR 614.12).
///
/// [`Full`] if finding what it would have ran out of room.
#[allow(
    clippy::disallowed_methods,
    reason = "reads the face the permanent enters with (see CR 614.12)"
)]
fn kept_entering<L: Layout, T>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    entry: Entry<L>,
    find: impl FnOnce(&crate::card::Face) -> Option<(AbilityIx, T)>,
) -> Result<Option<T>, Full> {
    let face = entering_values(s, t, slot, entry, None).face(t);
    let Some((ability, found)) = find(face) else {
        return Ok(None);
    };
    let lost = layers::lost_entering(s, t, slot, entry, face, None)?;
    Ok((!lost.contains(ability)).then_some(found))
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
            | Rewrite::EntersPrepared
            | Rewrite::EntersTappedUnlessFewLands(_)
            | Rewrite::EntersTappedUnlessYouControl(..)
            | Rewrite::EntersWithCounters(..)
            | Rewrite::Instead
            | Rewrite::Dredge(_)
            | Rewrite::Choose(_)
            | Rewrite::MaySkipTurnToUntap
            | Rewrite::Multiply(_)
            | Rewrite::Plus(_)
            | Rewrite::EnterAsCopy { .. } => None,
        })
}

/// Pays the life an "unless you pay N life" entering replacement asks for
/// (see [CR 119.4]): "If a player pays life, the payment is subtracted from their life total; in
/// other words, the player loses that much life."
///
/// [CR 119.4]: https://yawgatog.com/resources/magic-rules/#R1194
#[allow(
    clippy::disallowed_methods,
    reason = "reads the face the permanent enters with (see CR 614.12)"
)]
pub(crate) fn pay_life_to_enter<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
    entry: Entry<L>,
) -> Result<(), Full> {
    let copied = selected_copy(sink, slot);
    let values = entering_values(s, t, slot, entry, copied);
    let player = entry.controller.unwrap_or(s.object(slot).owner());
    if let Some((ability, n)) = life_asked(values.face(t))
        && (copied.is_some() || s.has_ability(slot, ability))
    {
        // The life is lost to the permanent's own replacement effect ([CR 614.1c]: "Effects
        // that read ... "As [this permanent] enters . . . ," ... are replacement effects."),
        // not to whatever puts the permanent onto the battlefield.
        //
        // [CR 614.1c]: https://yawgatog.com/resources/magic-rules/#R6141c
        sink.enter_source(|| s.object_ref(slot));
        let lost = life::lose(&mut Game::new(s, t, sink), player, i16::from(n));
        sink.leave();
        lost?;
    }
    Ok(())
}

/// The life an entering replacement of the object in `slot` asks for as it enters as
/// `entry` says, if any ([`kept_entering`]).
pub(crate) fn life_to_enter<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    entry: Entry<L>,
) -> Result<Option<u8>, Full> {
    kept_entering(s, t, slot, entry, life_asked)
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
    let d = catalog::decision(t, player, kind, Bitboard::single(slot))?;
    let c = answer(s, sink, &d)?;
    Ok(catalog::value(kind, d.candidates.option(c)))
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
            | Rewrite::EntersPrepared
            | Rewrite::EntersTappedUnlessFewLands(_)
            | Rewrite::EntersTappedUnlessYouControl(..)
            | Rewrite::EntersTappedUnlessPayLife(_)
            | Rewrite::EntersWithCounters(..)
            | Rewrite::Instead
            | Rewrite::Dredge(_)
            | Rewrite::MaySkipTurnToUntap
            | Rewrite::Multiply(_)
            | Rewrite::Plus(_)
            | Rewrite::EnterAsCopy { .. } => None,
        })
}

/// The kind of value the object in `slot` chooses as it enters as `entry` says, if any
/// ([`kept_entering`]).
pub(crate) fn chosen_to_enter<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    entry: Entry<L>,
) -> Result<Option<ChosenKind>, Full> {
    kept_entering(s, t, slot, entry, chooses)
}

/// Whether a land `controller` puts onto the battlefield finds them controlling `n` or fewer
/// other lands, for "enters tapped unless you control [n] or fewer other lands". It is not yet
/// on the battlefield, so every land counted is another land.
pub(crate) fn few_other_lands<L: Layout>(s: &State<L>, controller: Seat, n: u8) -> bool {
    let lands = s.battlefield().iter().filter(|&&p| {
        let d = s.derived(p);
        d.controller == controller && d.types.contains(Types::LAND)
    });
    lands.count() <= usize::from(n)
}

/// Whether `controller` controls `n` or more permanents that `filter` accepts, read for them,
/// for "enters tapped unless you control three or more other Islands". A land checks this as
/// it enters, so it counts only the permanents already on the battlefield. The rulings of
/// such lands say so: "As these lands are entering the battlefield, they check for lands that
/// are already on the battlefield. They won't see lands that are entering the battlefield at
/// the same time". The entering permanent is not yet there, so every permanent counted is
/// "other". Its own text changes do not apply to the filter until it is on the battlefield.
#[cold]
#[inline(never)]
pub(crate) fn controls_at_least<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    controller: Seat,
    n: u8,
    filter: &crate::filter::Filter,
) -> bool {
    let words = crate::card::Words::NONE;
    let matching = s.battlefield().iter().filter(|&&p| {
        s.derived(p).controller == controller && filter.matches(s, t, p, controller, &words)
    });
    matching.take(usize::from(n)).count() == usize::from(n)
}
