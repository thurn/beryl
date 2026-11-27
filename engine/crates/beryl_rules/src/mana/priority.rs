//! Mana abilities activated on their own at priority, rather than by a payment's plan.
//!
//! Implements: none. Offers the activations of CR 605.3, which `mana::pay` implements.

use crate::bitboard::Bitboard;
use crate::card::{
    Ability, AbilityKind, Amount, ChosenKind, CostAction, CostPart, ManaColor, ManaSet,
    ObjectAbility, Timing,
};
use crate::decision::{Candidates, OPTIONS, Opt};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::replacement::event::{self, Sink};
use crate::state::{Pending, State};
use crate::table::{Capability, GameTable, is_instant_mana, is_sacrificed};
use beryl_kernel::chosen::ChosenValue;
use beryl_kernel::reference::Permanent;

use super::KINDS;
use super::pay::{activate, activate_consuming};
use super::remembered;
use super::solver::lowest_kind;
use super::sources::{allowed, alternative_ability, can_tap, controlled, is_alternative, source};

/// Whether `a` is a `{T}`-only multi-term or counted mana ability (`Ability::mana_of`), offered
/// as `Opt::Activate` at priority when manual tapping is active.
fn is_tap_multi_mana(a: &Ability) -> bool {
    a.kind() == AbilityKind::Mana
        && a.produces() == ManaSet::NONE
        && a.timing() != Timing::AsInstant
        && a.spend().is_none()
        && a.cost().mana.is_zero()
        && matches!(a.cost().parts(), [CostPart::This(CostAction::Tap)])
}

fn is_tap_multi_mana_now<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    a: &Ability,
    words: &crate::card::Words,
) -> bool {
    is_tap_multi_mana(a)
        && allowed(s, t, a, player, words)
        && (!crate::table::is_could_produce(a)
            || super::changes::could_produce_of(s, t, player, a, words) == 0)
}

fn has_tap_multi_mana<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
    d: &crate::state::Derived<L>,
) -> bool {
    if !can_tap(s, player, slot, d) {
        return false;
    }
    let words = s.words_of(slot);
    s.abilities(t, slot)
        .any(|(_, a)| is_tap_multi_mana_now(s, t, player, a, &words))
}

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
    let mana_changes = t.can(Capability::ManaChanges) && s.has_mana_changes();
    let rich = t.can_either(Capability::PricedMana, Capability::ManaChanges);
    let sources: Bitboard<L::Slot> = controlled(s, player)
        .filter(|&(slot, d)| {
            source(s, t, player, slot, d).is_some()
                || (mana_changes && super::changes::standalone_kinds(s, t, player, slot, d) != 0)
                || (rich && has_tap_multi_mana(s, t, player, slot, d))
        })
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
        s.abilities(t, slot).filter(move |&(ability, a)| {
            matches!(
                ability,
                ObjectAbility::Own(_) | ObjectAbility::Merged(..) | ObjectAbility::Granted(_)
            ) && is_instant_mana(a)
                && (!a.cost().does(CostAction::Tap) || can_tap(s, player, slot, s.derived(slot)))
                && allowed(s, t, a, player, &s.ability_words(slot, ability))
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
    let printed = s
        .abilities(t, slot)
        .find(|&(a, _)| a == ability)
        .map(|(_, a)| a)
        .expect("an ability offered");
    // See CR 117.3c: "If a player has priority when they ... activate an ability ..., that player
    // receives priority afterward."
    s.reset_passes();
    if printed.produces() == ManaSet::NONE {
        return activate_multi_mana(s, t, sink, permanent, printed);
    }
    let (kinds, consume) = (printed.produces().colors, is_sacrificed(printed));
    if kinds.is_power_of_two() {
        let color = ManaColor::ALL[lowest_kind(kinds)];
        return activate_consuming(s, t, sink, permanent, color, consume);
    }
    s.ask(Pending::ManaColor { slot, ability });
    Ok(())
}

/// Activates a `{T}`-only multi-term mana ability (`Ability::mana_of`) at priority, applying
/// any live mana changes and asking `Pending::ManaType` if `Added::TypeProduced` triggered on a
/// tap that produced more than one type of mana.
#[cold]
#[inline(never)]
fn activate_multi_mana<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    permanent: Permanent<L>,
    ability: &'static Ability,
) -> Result<(), Full> {
    let slot = permanent.slot();
    let player = s.controller(slot);
    let has_changes = t.can(Capability::ManaChanges) && s.has_mana_changes();
    let changes = if has_changes {
        super::changes::matching(s, t, slot)?
    } else {
        super::changes::MatchingChanges::default()
    };
    event::set_tapped(s, t, sink, permanent, true)?;
    crate::coverage::hit(ability);
    let source = Some(permanent.into());
    let words = s.words_of(slot);
    let mut produced_mask = 0u8;
    for term in ability.product() {
        let amount = if let Amount::Fixed(n) = term.amount {
            n
        } else {
            crate::interpret::amount_of(s, t, slot, term.amount)?
        };
        if amount == 0 {
            continue;
        }
        let kinds = match term.kinds {
            beryl_card::Kinds::Fixed(bits) => bits,
            beryl_card::Kinds::CouldProduce(filter) => {
                super::changes::could_produce(s, t, player, filter, &words)
            }
        };
        if kinds == 0 {
            continue;
        }
        let scaled = amount.checked_mul(changes.multiplier).ok_or(Full)?;
        let kind = ManaColor::ALL[lowest_kind(kinds)];
        produced_mask |= 1 << kind.index();
        let added = s.add_mana(player, source, kind, scaled)?;
        sink.note_from(source, added);
    }
    if has_changes {
        super::changes::add_extra(s, sink, permanent, player, changes.fixed)?;
        if changes.type_produced > 0 && produced_mask != 0 {
            if produced_mask.is_power_of_two() {
                let kind = ManaColor::ALL[lowest_kind(produced_mask)];
                let added = s.add_mana(player, source, kind, changes.type_produced)?;
                sink.note_from(source, added);
            } else {
                s.ask(Pending::ManaType {
                    slot,
                    player,
                    kinds: produced_mask,
                    remaining: changes.type_produced,
                });
            }
        }
    }
    crate::continuous::layers::refresh(s, t, sink)
}

/// Adds one mana of the type chosen in answer to [`Pending::ManaType`], and asks again if more
/// `Added::TypeProduced` triggers remain.
pub(crate) fn choose_mana_type<L: Layout>(
    s: &mut State<L>,
    sink: &mut Sink<L>,
    slot: L::Slot,
    player: Seat,
    kinds: u8,
    remaining: u8,
    opt: Option<Opt<L>>,
) -> Result<(), Full> {
    let color = match opt {
        Some(Opt::Mana { color, .. }) => color,
        other => {
            let ChosenValue::Color(colors) =
                crate::interpret::catalog::value(ChosenKind::Color, other)
            else {
                unreachable!("a mana type among the produced types");
            };
            ManaColor::ALL[lowest_kind(colors.0)]
        }
    };
    let source = s.permanent_at(slot).map(Into::into);
    let added = s.add_mana(player, source, color, 1)?;
    sink.note_from(source, added);
    if remaining > 1 {
        s.ask(Pending::ManaType {
            slot,
            player,
            kinds,
            remaining: remaining - 1,
        });
    }
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
        || (t.can_either(Capability::PricedMana, Capability::ManaChanges)
            && s.abilities(t, slot)
                .any(|(a, printed)| a == ability && is_tap_multi_mana(printed)))
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
    let remembers = t.can(Capability::RemembersMana);
    let alternatives = t.can(Capability::SacrificeAlternative);
    let mana_changes = t.can(Capability::ManaChanges) && s.has_mana_changes();
    let rich = t.can_either(Capability::PricedMana, Capability::ManaChanges);
    // Tapping ahead is optional, so a full list keeps the options it already holds.
    let _full = Candidates::<L>::extend_options(out, sources, |out, slot| {
        let d = s.derived(slot);
        // Its restricted mana too: the mana remembers what it may be spent on.
        let more = if remembers && s.definition(slot).facts(t).has_restricted_mana {
            remembered::restricted_of_slot(s, t, player, slot, d).map_or(0, |r| r.kinds)
        } else {
            0
        };
        let extra_kinds = if mana_changes {
            super::changes::standalone_kinds(s, t, player, slot, d)
        } else {
            0
        };
        if let Some(kinds) = source(s, t, player, slot, d)
            .map(|(kinds, ..)| kinds | more)
            .or(((more | extra_kinds) != 0).then_some(more | extra_kinds))
        {
            for kind in (0..KINDS).filter(|&k| kinds & (1 << k) != 0) {
                let option = Opt::Mana {
                    slot,
                    color: ManaColor::ALL[kind],
                };
                out.push(option)?;
            }
        }
        if rich && can_tap(s, player, slot, d) {
            let words = s.words_of(slot);
            for (ability, a) in s.abilities(t, slot) {
                if is_tap_multi_mana_now(s, t, player, a, &words) {
                    out.push(Opt::Activate { slot, ability })?;
                }
            }
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
