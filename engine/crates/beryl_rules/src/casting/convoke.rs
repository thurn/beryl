//! Convoke ([CR 702.51a]) and improvise ([CR 702.126a]): tapping creatures or artifacts as a
//! spell is cast, each rather than pay one mana of its total cost. The parts of a spell's cost
//! that these keywords add, which a face has, and whether it can be paid; `parts` pays them.
//!
//! Implements: CR 702.51a, CR 702.51b, CR 702.126a, CR 702.126b
//!
//! [CR 702.51a]: https://yawgatog.com/resources/magic-rules/#R70251a
//! [CR 702.126a]: https://yawgatog.com/resources/magic-rules/#R702126a
//!
//! [CR 702.51b](https://yawgatog.com/resources/magic-rules/#R70251b): "The convoke ability
//! isn't an additional or alternative cost and applies only after the total cost of the
//! spell with convoke is determined." [CR 702.126b] says the same of improvise. So the
//! objects are chosen once the total cost is locked in, X included, as delve's cards are.
//!
//! [CR 702.126b]: https://yawgatog.com/resources/magic-rules/#R702126b

use crate::bitboard::Bitboard;
use crate::card::{
    Chosen, CostAction, CostPart, Face, FaceIx, Keywords, ManaColor, ManaCost, Measure, Types,
};
use crate::decision::{Candidates, OPTIONS, Opt};
use crate::engine::collapse;
use crate::filter::Filter;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::mana::{self, Funds};
use crate::replacement::event::{self, Sink};
use crate::state::{Casting, StackEntry, StackItem, State, Zone};
use crate::table::GameTable;

use super::parts::candidates;

/// What convoke has its spell's caster do rather than pay mana ([CR 702.51a]): "For each
/// colored mana in this spell's total cost, you may tap an untapped creature of that color
/// you control rather than pay that mana. For each generic mana in this spell's total cost,
/// you may tap an untapped creature you control rather than pay that mana." A part of no cost
/// printed on a card, which the casting steps name by [`CONVOKE_PART`].
///
/// [CR 702.51a]: https://yawgatog.com/resources/magic-rules/#R70251a
pub(super) static CONVOKE: CostPart = CostPart::Chosen(
    CostAction::Tap,
    Chosen {
        zone: Zone::Battlefield,
        filter: Filter::Types(Types::CREATURE),
        other: false,
        measure: Measure::ForMana,
    },
);

/// The index the casting steps give [`CONVOKE`] among a spell's parts, past any it prints and
/// beside delve's.
pub(super) const CONVOKE_PART: u8 = u8::MAX - 1;

/// What improvise has its spell's caster do rather than pay generic mana ([CR 702.126a]):
/// "For each generic mana in this spell's total cost, you may tap an untapped artifact you
/// control rather than pay that mana." Named by [`IMPROVISE_PART`].
///
/// [CR 702.126a]: https://yawgatog.com/resources/magic-rules/#R702126a
pub(super) static IMPROVISE: CostPart = CostPart::Chosen(
    CostAction::Tap,
    Chosen {
        zone: Zone::Battlefield,
        filter: Filter::Types(Types::ARTIFACT),
        other: false,
        measure: Measure::ForGeneric(u8::MAX),
    },
);

/// The index the casting steps give [`IMPROVISE`] among a spell's parts.
pub(super) const IMPROVISE_PART: u8 = u8::MAX - 2;

/// The part, by its index, that convoke or improvise adds to the cost of a spell of `face`,
/// if it has one of them. Each functions only while its spell is on the stack
/// ([CR 702.51a], [CR 702.126a]), where an effect that grants or removes abilities reaches no
/// spell this engine plays, so the face's keywords answer. A card table holds no face with
/// both, nor with delve beside one (`beryl_kernel`'s `payable`).
///
/// [CR 702.51a]: https://yawgatog.com/resources/magic-rules/#R70251a
/// [CR 702.126a]: https://yawgatog.com/resources/magic-rules/#R702126a
pub(super) fn part_of(face: &Face) -> Option<u8> {
    if face.keywords.contains(Keywords::CONVOKE) {
        Some(CONVOKE_PART)
    } else if face.keywords.contains(Keywords::IMPROVISE) {
        Some(IMPROVISE_PART)
    } else {
        None
    }
}

/// The part at `index`, [`CONVOKE_PART`] or [`IMPROVISE_PART`].
pub(super) fn part(index: u8) -> Option<&'static CostPart> {
    match index {
        CONVOKE_PART => Some(&CONVOKE),
        IMPROVISE_PART => Some(&IMPROVISE),
        _ => None,
    }
}

/// The objects the part at `index` may tap for the spell whose card is in `slot`: the
/// untapped creatures, or artifacts, `player` controls. A creature that came under its
/// controller's control this turn may be tapped: it is tapped by the spell's cost, not by an
/// ability of its own with {T} (see CR 302.6).
pub(super) fn objects<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
    index: u8,
) -> Bitboard<L::Slot> {
    let Some(CostPart::Chosen(action, chosen)) = part(index) else {
        unreachable!("convoke's or improvise's part");
    };
    candidates(s, t, player, slot, *action, chosen)
}

/// The kinds of mana the object in `slot` pays when the part at `index` taps it, a bit per
/// [`crate::card::ManaColor`]: for convoke, the creature's colors as the layers give them
/// (none for a colorless creature, which pays generic mana only); for improvise, none.
pub(super) fn kinds<L: Layout>(s: &State<L>, index: u8, slot: L::Slot) -> u8 {
    if index == CONVOKE_PART {
        s.derived(slot).colors.0
    } else {
        0
    }
}

/// Whether `player` can pay `cost` for casting the card in `slot` as its face `face`, a spell
/// with convoke or improvise whose part is at `index`, tapping the objects that part may tap
/// for some of it and paying the rest with mana ([`Funds::pays_with_objects`]).
#[allow(clippy::too_many_arguments)]
pub(super) fn convoke_pays<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    funds: &Funds<L>,
    player: Seat,
    slot: L::Slot,
    face: FaceIx,
    index: u8,
    cost: ManaCost,
) -> bool {
    let objects = objects(s, t, player, slot, index);
    let paying = mana::Paying::casting(s, t, slot, face);
    funds.pays_with_objects(s, t, cost, Bitboard::EMPTY, paying, objects, |o| {
        kinds(s, index, o)
    })
}

/// Whether the caster of `c`, the spell at the top of the stack, can pay `cost` for it by
/// tapping the objects its convoke or improvise may tap for some of it: false for a spell with
/// neither, and for an ability.
#[allow(clippy::disallowed_methods, reason = "the face being cast")]
pub(super) fn entry_pays<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    c: &Casting<L>,
    cost: ManaCost,
) -> Result<bool, Full> {
    let entry = super::steps::top(s);
    let StackItem::Spell { card, face, .. } = entry.item else {
        return Ok(false);
    };
    let Some(index) = part_of(t.def(card).face(face)) else {
        return Ok(false);
    };
    let slot = entry.object().expect("a spell's card").slot;
    let funds = Funds::of(s, t, c.player)?;
    Ok(convoke_pays(
        s, t, &funds, c.player, slot, face, index, cost,
    ))
}

/// Whether `chosen` is improvise's, which taps for generic mana as a waterbend cost does but is
/// asked with its own prompt.
pub(super) fn improvises(action: CostAction, chosen: &Chosen) -> bool {
    CostPart::Chosen(action, *chosen) == IMPROVISE
}

/// How much of `cost`, what is left to pay, the part at `index` may pay with objects: its
/// generic mana, and for convoke its colored symbols too. Never a colorless symbol ({C}).
pub(super) fn payable_count(index: u8, cost: ManaCost) -> u8 {
    let colored: u8 = cost.symbols[..5].iter().sum();
    if index == CONVOKE_PART {
        cost.generic.saturating_add(colored)
    } else {
        cost.generic
    }
}

/// The colors `cost` still has a symbol of, a bit per [`ManaColor`] from white to green.
fn colors_left(cost: ManaCost) -> u8 {
    (0..5)
        .filter(|&c| cost.symbols[c] > 0)
        .fold(0, |m, c| m | (1 << c))
}

/// The answers while creatures pay for a spell with convoke, `cost` being what is left to pay
/// ([CR 702.51a]): "For each colored mana in this spell's total cost, you may tap an untapped
/// creature of that color you control rather than pay that mana. For each generic mana ...,
/// you may tap an untapped creature you control rather than pay that mana."
///
/// A creature is offered while it can pay part of what is left and the rest stays payable
/// ([`Funds::pays_with_objects`]). It pays a symbol of its color when exactly one of its
/// colors has a symbol left, and generic mana when none has, as [`Opt::Object`]. When two or
/// more of its colors have a symbol left it is offered once for each, as [`Opt::Mana`] of
/// that color. Paying a colored symbol first never loses a payment: any mana that would have
/// paid it can pay the generic mana instead. The end ([`Opt::Done`]) is offered once mana can
/// pay the rest. Interchangeable creatures are one answer (§7.2). Empty when there is no
/// choice to make.
///
/// [CR 702.51a]: https://yawgatog.com/resources/magic-rules/#R70251a
pub(super) fn options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    c: &Casting<L>,
    entry: &StackEntry<L>,
    cost: ManaCost,
) -> Result<L::Queue<Opt<L>, OPTIONS>, Full> {
    let mut out: L::Queue<Opt<L>, OPTIONS> = Default::default();
    let itself = entry.object().expect("a spell's card").slot;
    let able = objects(s, t, c.player, itself, CONVOKE_PART);
    let funds = Funds::of(s, t, c.player)?;
    let paying = mana::Paying::entry(s, t, entry);
    let kinds = |o: L::Slot| kinds(s, CONVOKE_PART, o);
    let left = colors_left(cost);
    let rest_pays = |slot: L::Slot, fewer: ManaCost| {
        funds.pays_with_objects(s, t, fewer, Bitboard::single(slot), paying, able, kinds)
    };
    let mut picks: L::Queue<Opt<L>, OPTIONS> = Default::default();
    Candidates::<L>::extend_options(&mut picks, collapse(s, able), |picks, slot| {
        let colors = kinds(slot) & left;
        if colors.count_ones() < 2 {
            let mut fewer = cost;
            if colors == 0 {
                if fewer.generic == 0 {
                    return Ok(());
                }
                fewer.generic -= 1;
            } else {
                fewer.symbols[usize::try_from(colors.trailing_zeros()).expect("a color")] -= 1;
            }
            if rest_pays(slot, fewer) {
                picks.push(Opt::Object(slot))?;
            }
            return Ok(());
        }
        for color in (0..5).filter(|&k| colors & (1 << k) != 0) {
            let mut fewer = cost;
            fewer.symbols[color] -= 1;
            if rest_pays(slot, fewer) {
                picks.push(Opt::Mana {
                    slot,
                    color: ManaColor::ALL[color],
                })?;
            }
        }
        Ok(())
    })?;
    if picks.is_empty() {
        return Ok(out);
    }
    if mana::payable(s, t, c.player, cost, Bitboard::EMPTY, paying)? {
        out.push(Opt::Done)?;
    }
    for &o in picks.iter() {
        out.push(o)?;
    }
    Ok(out)
}

/// Taps the creature in `slot` for convoke and records what it paid of `c`'s cost: a symbol
/// of `color` when it was offered for that color ([`Opt::Mana`]), else a symbol of its one
/// color with a symbol left, else one generic mana ([`options`] says why).
pub(super) fn pay_with<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    c: &mut Casting<L>,
    slot: L::Slot,
    color: Option<ManaColor>,
) -> Result<(), Full> {
    let cost = super::costs::total_cost(s, t, super::steps::top(s), c);
    let colors = kinds(s, CONVOKE_PART, slot) & colors_left(cost);
    let paid = color.map(ManaColor::index).or_else(|| {
        (colors != 0).then(|| usize::try_from(colors.trailing_zeros()).expect("a color"))
    });
    match paid {
        Some(color) => c.pay_color(color)?,
        None => c.substituted += 1,
    }
    match s.permanent_at(slot) {
        Some(permanent) => event::set_tapped(s, t, sink, permanent, true),
        None => Ok(()),
    }
}

/// Whether `player` can pay `cost` for casting the card in `slot`, in their hand, as its face
/// `face`, tapping objects for its convoke or improvise ([`convoke_pays`]): `None` for a face
/// with neither. The payment reference checks this against every set of objects it could tap.
#[allow(
    clippy::disallowed_methods,
    reason = "the face a reference cast pays for"
)]
pub(crate) fn card_pays<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
    face: FaceIx,
    cost: ManaCost,
) -> Result<Option<bool>, Full> {
    let Some(index) = part_of(s.card_def(t, slot).expect("a card").face(face)) else {
        return Ok(None);
    };
    let funds = Funds::of(s, t, player)?;
    Ok(Some(convoke_pays(
        s, t, &funds, player, slot, face, index, cost,
    )))
}
