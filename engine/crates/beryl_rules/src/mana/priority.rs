//! Mana abilities activated on their own at priority, rather than by a payment's plan.
//!
//! Implements: none. Offers the activations of CR 605.3, which `mana::pay` implements.

use crate::bitboard::Bitboard;
use crate::card::{ChosenKind, CostAction, ManaColor, ObjectAbility};
use crate::decision::{Candidates, OPTIONS, Opt};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::replacement::event::Sink;
use crate::state::{Pending, State};
use crate::table::{Capability, GameTable, is_instant_mana, is_sacrificed};
use beryl_kernel::chosen::ChosenValue;
use beryl_kernel::reference::Permanent;

use super::KINDS;
use super::pay::{activate, activate_consuming};
use super::remembered;
use super::solver::lowest_kind;
use super::sources::{allowed, alternative_ability, can_tap, controlled, is_alternative, source};

/// The standalone mana activations offered at priority, under manual payment or to the player
/// [`crate::Profile::taps_mana`] names
/// (see [CR 605.3a](https://yawgatog.com/resources/magic-rules/#R6053a)): "A player may activate
/// an activated mana ability whenever they have priority". Otherwise tapping ahead of a
/// payment is dominated by tapping during it (§10), so nothing is offered.
///
/// One per kind of mana each untapped source makes, of each set of interchangeable objects
/// (§7.2), not of each class of sources: a player taps the permanent they mean, an Island
/// rather than the Mox Sapphire that makes the same mana. A list these would fill takes as
/// many as fit: tapping ahead is never needed, as a payment taps for itself.
pub(crate) fn standalone_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    out: &mut L::Queue<Opt<L>, OPTIONS>,
) {
    let sources: Bitboard<L::Slot> = controlled(s, player)
        .filter(|&(slot, d)| source(s, t, player, slot, d).is_some())
        .map(|(slot, _)| slot)
        .collect();
    let sources = crate::engine::collapse(s, sources);
    add_activations(s, t, player, out, sources);
}

/// The mana abilities `player` may activate only while they have priority
/// ([`is_instant_mana`]), one option for each, for each set of interchangeable permanents
/// (§7.2). The mana solver never activates such an ability, so it is offered here whatever
/// the profile: "Activate only as an instant" lets it be activated at priority alone
/// (see [CR 117.1a]): "A player may cast an instant spell any time they have priority." A game
/// whose cards have none pays one test of the table.
///
/// [CR 117.1a]: https://yawgatog.com/resources/magic-rules/#R1171a
pub(crate) fn instant_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    out: &mut L::Queue<Opt<L>, OPTIONS>,
) -> Result<(), Full> {
    if !t.can(Capability::InstantMana) {
        return Ok(());
    }
    list_instant_options(s, t, player, out)
}

/// [`instant_options`] in a game whose cards have such an ability. Out of line.
#[inline(never)]
fn list_instant_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    out: &mut L::Queue<Opt<L>, OPTIONS>,
) -> Result<(), Full> {
    let instant = |slot: L::Slot| {
        let words = s.words_of(slot);
        s.abilities(t, slot).filter(move |&(ability, a)| {
            matches!(ability, ObjectAbility::Own(_) | ObjectAbility::Merged(..))
                && is_instant_mana(a)
                && (!a.cost().does(CostAction::Tap) || can_tap(s, player, slot, s.derived(slot)))
                && allowed(s, t, a, player, &words)
        })
    };
    let objects: Bitboard<L::Slot> = (s.battlefield().iter().copied())
        .filter(|&slot| s.controller(slot) == player && instant(slot).next().is_some())
        .collect();
    Candidates::<L>::extend_options(out, crate::engine::collapse(s, objects), |out, slot| {
        for (ability, _) in instant(slot) {
            out.push(Opt::Activate { slot, ability })?;
        }
        Ok(())
    })
}

/// Activates the mana ability `ability` of `permanent`, one activated only as an
/// instant ([`is_instant_mana`]): at once if it makes one kind of mana, or once its
/// controller has chosen the color of "any one color" ([`Pending::ManaColor`]). That color
/// is chosen as the ability resolves, after its costs are paid, but here it is asked before
/// anything is paid: what paying shows changes none of the colors the player may choose.
pub(crate) fn begin_instant<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    permanent: Permanent<L>,
    ability: ObjectAbility,
) -> Result<(), Full> {
    let slot = permanent.slot();
    let (kinds, consume) = s
        .abilities(t, slot)
        .find(|&(a, _)| a == ability)
        .map(|(_, a)| (a.produces().colors, is_sacrificed(a)))
        .expect("an ability offered");
    // See CR 117.3c: "If a player has priority when they ... activate an ability ..., that player
    // receives priority afterward."
    s.reset_passes();
    if kinds.is_power_of_two() {
        let color = ManaColor::ALL[lowest_kind(kinds)];
        return activate_consuming(s, t, sink, permanent, color, consume);
    }
    s.ask(Pending::ManaColor { slot, ability });
    Ok(())
}

/// Activates the mana ability [`Pending::ManaColor`] asked about, of `permanent`, for the
/// color the answer `opt` names.
pub(crate) fn activate_chosen<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    permanent: Permanent<L>,
    opt: Option<Opt<L>>,
) -> Result<(), Full> {
    let ChosenValue::Color(colors) = crate::interpret::catalog::value(ChosenKind::Color, opt)
    else {
        unreachable!("a color among a color's candidates");
    };
    activate(s, t, sink, permanent, ManaColor::ALL[lowest_kind(colors.0)])
}

/// Whether `ability` of the permanent in `slot` is a mana ability that priority offers as an
/// option of its own, which [`begin_instant`] activates rather than the casting steps: one
/// activated only as an instant, or one that sacrifices a source that also makes mana for {T}
/// alone ([`is_alternative`]).
pub(crate) fn is_instant<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    ability: ObjectAbility,
) -> bool {
    (t.can(Capability::InstantMana)
        && s.abilities(t, slot)
            .any(|(a, printed)| a == ability && is_instant_mana(printed)))
        || is_alternative(s, t, slot, ability)
}

/// Exact sources the human can activate before paying Waterbend, even when a twin
/// would make the same mana. see [CR 601.2g](https://yawgatog.com/resources/magic-rules/#R6012g):
/// "the player then has a chance to activate mana abilities".
pub(crate) fn waterbend_mana_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    out: &mut L::Queue<Opt<L>, OPTIONS>,
) {
    let sources = controlled(s, player).map(|(slot, _)| slot).collect();
    add_activations(s, t, player, out, sources);
}

/// Whether the untapped source `player` controls in `slot` makes mana for {T} alone and also
/// has a mana ability that sacrifices it ([`alternative_ability`]): a Treasure, whose one mana
/// ability sacrifices it, has none beside it.
fn is_alternative_source<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
) -> bool {
    source(s, t, player, slot, s.derived(slot)).is_some_and(|(.., consumed)| !consumed)
        && alternative_ability(s, t, slot).is_some()
}

fn add_activations<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    out: &mut L::Queue<Opt<L>, OPTIONS>,
    sources: Bitboard<L::Slot>,
) {
    let restricted = remembered::restricted_sources(s, t, player).unwrap_or_default();
    let alternatives = t.can(Capability::SacrificeAlternative);
    // Tapping ahead is optional, so a full list keeps the options it already holds.
    let _full = Candidates::<L>::extend_options(out, sources, |out, slot| {
        // Its restricted mana too: the mana remembers what it may be spent on.
        let more = (restricted.iter().find(|r| r.slot == slot)).map_or(0, |r| r.kinds);
        let Some(kinds) = source(s, t, player, slot, s.derived(slot))
            .map(|(kinds, ..)| kinds | more)
            .or((more != 0).then_some(more))
        else {
            return Ok(());
        };
        for kind in (0..KINDS).filter(|&k| kinds & (1 << k) != 0) {
            let option = Opt::Mana {
                slot,
                color: ManaColor::ALL[kind],
            };
            out.push(option)?;
        }
        // A source's mana ability that sacrifices it, beside the ones above
        // ([`sacrifice_alternative`]): an option of its own, as each kind above activates
        // another.
        if alternatives && is_alternative_source(s, t, player, slot) {
            let ability = alternative_ability(s, t, slot).expect("a source's alternative");
            out.push(Opt::Activate { slot, ability })?;
        }
        Ok(())
    });
}
