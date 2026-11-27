//! Whether a player may begin to activate an ability (CR 602.5), and so which activations a
//! priority decision offers.
//!
//! Implements: CR 606.3, CR 606.6

use crate::bitboard::Bitboard;
use crate::card::{
    Ability, AbilityKind, CostAction, CostPart, Keywords, ObjectAbility, Timing, Types,
};
use crate::continuous::rule_change;
use crate::decision::{Candidates, OPTIONS, Opt};
use crate::engine::collapse;
use crate::fixed::Full;
use crate::game::life;
use crate::ids::{Seat, SlotId};
use crate::layout::{Layout, Queue};
use crate::mana;
use crate::state::{State, Tracker, Zone, ZoneKey};
use crate::table::{Capability, GameTable};

use super::super::parts;
use super::super::targets::{Itself, targetable};
use super::super::timing::{activated, sorcery_timing};
use super::Situation;
use super::others::{granted, has_others};

/// The permanents that pay for an ability with themselves, and so cannot also tap for its
/// mana.
pub(in crate::casting) fn spent_by_cost<S: SlotId>(slot: S, a: &Ability) -> Bitboard<S> {
    let mut b = Bitboard::EMPTY;
    if a.cost().does(CostAction::Tap) || a.cost().does(CostAction::Sacrifice) {
        b.insert(slot);
    }
    b
}

/// A level bar can only be activated from the preceding level (see CR 717.2a).
#[cold]
fn at_previous_level<L: Layout>(s: &State<L>, slot: L::Slot, a: &Ability) -> bool {
    if let beryl_card::Designation::LevelUp(level) = a.designation() {
        s.class_level(slot) == level - 1
    } else {
        true
    }
}

/// Whether `player` may activate `ability` now, after selecting sources they can activate.
///
/// [CR 602.5](https://yawgatog.com/resources/magic-rules/#R6025): "A player can't begin to
/// activate an ability that's prohibited from being activated."
#[allow(
    clippy::disallowed_methods,
    reason = "reads an activated ability by its place on the face `activated` names"
)]
pub(super) fn activatable<L: Layout, const TARGETS: bool>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L, TARGETS>,
    slot: L::Slot,
    ability: ObjectAbility,
) -> bool {
    let player = now.player;
    let o = s.object(slot);
    let Some((source, index)) = activated(s, t, slot, ability) else {
        return false;
    };
    let a = source.face(t).ability(index);
    if a.kind() != AbilityKind::Activated {
        return false;
    }
    // See CR 113.6b: "An ability that states which zones it functions in functions only from
    // those zones."
    if !a.zones().contains(o.zone()) {
        return false;
    }
    if a.cost().does(CostAction::Tap) {
        // CR 118.3: "a permanent that's already tapped can't be tapped to pay a cost". CR
        // 602.5a: "A creature's activated ability with the tap symbol ... in its activation
        // cost can't be activated unless the creature has been under its controller's control
        // since the start of their most recent turn."
        if o.is_tapped()
            || (s.derived(slot).types.intersects(Types::CREATURE)
                && o.is_summoning_sick()
                && !s.derived(slot).keywords.contains(Keywords::HASTE))
        {
            return false;
        }
    }
    // CR 118.3: "A player can't pay a cost without having the necessary resources to pay it
    // fully."
    if a.cost().chooses() && !ability_payable(s, t, player, slot, a.cost().parts()) {
        return false;
    }
    if a.cost().pays_life_or_chooses() && !life_payable(s, t, player, a.cost().parts()) {
        return false;
    }
    if matches!(a.cost().removes(), Some(kind) if s.counters_of(slot, kind) == 0) {
        return false;
    }
    let timely = if let Some(loyalty) = a.cost().loyalty_change() {
        // CR 606.6: "A loyalty ability with a negative loyalty cost ... can't be activated
        // unless the permanent has at least that many loyalty counters on it."
        let payable = i16::from(o.counters().loyalty) + i16::from(loyalty) >= 0;
        // CR 606.3: "A player may activate a loyalty ability of a permanent they control any
        // time they have priority and the stack is empty during a main phase of their turn,
        // but only if no player has previously activated a loyalty ability of that permanent
        // that turn." The timing first, so that most decisions read no tracker. A rule
        // change may put "any time they could cast an instant" where the rule has the main
        // phase.
        payable
            && (sorcery_timing(s, player) || rule_change::loyalty_as_instant(s, t, slot))
            && !s.tracked(Tracker::LoyaltyActivated, slot)
    } else {
        // CR 602.5d: "Activated abilities that read 'Activate only as a sorcery' mean the
        // player must follow the timing rules for casting a sorcery spell".
        match a.timing() {
            Timing::Sorcery => {
                sorcery_timing(s, player)
                    && (!t.can(Capability::DesignatedAbilities) || at_previous_level(s, slot, a))
            }
            // CR 602.5: "A player can't begin to activate an ability that's prohibited
            // from being activated." The restriction follows the player taking the turn.
            Timing::YourTurn => s.turn().active() == player,
            Timing::Instant | Timing::AsInstant => true,
        }
    };
    if !timely {
        return false;
    }
    // The mana before the targets, which read the battlefield: both checks are pure.
    let mut cost = a.cost().mana.with_x(0);
    if rule_change::spends_as_any_color(s, t, player, slot) && o.on_battlefield() {
        cost = cost.as_any_color();
    }
    let spent = spent_by_cost(slot, a);
    let substitute = if a.cost().chooses() {
        parts::substitute(a.cost().parts())
    } else {
        None
    };
    let paid = now.funds(s, t).is_some_and(|funds| match substitute {
        Some((_, action, chosen, most)) => {
            let objects = parts::candidates(s, t, player, slot, action, chosen);
            let nothing = mana::Paying::NOTHING;
            parts::payable_with_substitutes(s, t, funds, cost, most, objects, spent, nothing)
        }
        None => funds.pays_for(s, t, cost, spent, mana::Paying::NOTHING),
    });
    paid && (!TARGETS || targetable(s, t, Some(a), player, Itself::default(), 0, Some(slot)))
}

/// Whether activating `ability` of the object in `slot` now would do nothing, as its effect
/// says ([`Ability::needs`]): its cost would be paid and its effect find nothing to act on.
/// The activation is still legal (CR 602.2);
/// [`Profile::hides_pointless`](crate::profile::Profile) says who isn't offered it.
#[allow(
    clippy::disallowed_methods,
    reason = "reads an activated ability by its place on the face `activated` names"
)]
pub fn pointless<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    ability: ObjectAbility,
) -> bool {
    let Some((source, index)) = activated(s, t, slot, ability) else {
        return false;
    };
    (source.face(t).ability(index).needs())
        .is_some_and(|objects| crate::interpret::names_nothing(s, t, slot, objects))
}

/// Appends the abilities `player` can activate now. see CR 117.1b: "A player may activate an
/// activated ability any time they have priority."
#[expect(
    clippy::inline_always,
    reason = "keeps the activation scan in the priority decision's hot path"
)]
#[inline(always)]
pub(super) fn activation_options<L: Layout, const TARGETS: bool>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L, TARGETS>,
    out: &mut L::Queue<Opt<L>, OPTIONS>,
) -> Result<(), Full> {
    let player = now.player;
    // Activations that would do nothing, left out for the player the profile hides them from,
    // where a card of the game has one.
    let prunes = TARGETS && s.profile().hides_pointless[player.index()] && t.needy();
    // CR 602.2: "Only an object's controller (or its owner, if it doesn't have a controller)
    // can activate its activated ability unless the object specifically says otherwise."
    let mut objects = s
        .in_zone(Zone::Battlefield)
        .filter(|slot| s.controller(slot) == player)
        | s.zone(ZoneKey::Of(player, Zone::Hand));
    if t.can(Capability::ActivatesFromGraveyardOrExile) {
        objects |= graveyard_and_exile(s, player);
    }
    // Most objects have no activated ability; leaving them out before collapsing saves
    // comparing them. Whether one has is part of what collapsing compares (its card, face
    // and granted abilities), so the result is the same.
    let facts = |slot: L::Slot| s.definition(slot).facts(t);
    // Abilities that are not those of the face up: granted ones, and those of the cards under
    // a merged permanent, in a game with a card that grants one or merges.
    let others = t.can_either(Capability::Grants, Capability::Merges);
    let objects = objects.filter(|slot| facts(slot).activates() || (others && has_others(s, slot)));
    // CR 602.5: "A player can't begin to activate an ability that's prohibited from being
    // activated." Asked once of the player, and of the objects only while such a rule change
    // is in force for them.
    let prohibited = rule_change::prohibitions(s, t, player);
    let objects = if prohibited.is_empty() {
        objects
    } else {
        rule_change::unprohibited(s, t, prohibited, player, objects)
    };
    Candidates::<L>::extend_options(out, collapse(s, objects), |out, slot| {
        // Of its own abilities, only the activated ones can be activated (see CR 602.1), so the
        // rest are not checked; granted ones and those of the cards under a merged permanent
        // are checked whatever they are. One it has lost
        // can't be activated (see CR 613.1f): asked once of a permanent that has them all, as
        // most have.
        let lost = !s.has_every_ability(slot);
        let abilities = facts(slot)
            .activated()
            .map(ObjectAbility::Own)
            .chain(granted(s, t, slot, others));
        for ability in abilities {
            if lost && !s.has(slot, ability) {
                continue;
            }
            if activatable(s, t, now, slot, ability) && !(prunes && pointless(s, t, slot, ability))
            {
                out.push(Opt::Activate { slot, ability })?;
            }
        }
        Ok(())
    })
}

/// The cards `player` owns in the graveyard and exile. Only a game with an ability
/// that works in one of these zones needs to look there (see [CR 113.6b]):
/// "An ability that states which zones it functions in functions only from those zones."
///
/// [CR 113.6b]: https://yawgatog.com/resources/magic-rules/#R1136b
#[cold]
#[inline(never)]
fn graveyard_and_exile<L: Layout>(s: &State<L>, player: Seat) -> Bitboard<L::Slot> {
    s.zone(ZoneKey::Of(player, Zone::Graveyard)) | s.zone(ZoneKey::Of(player, Zone::Exile))
}

/// Whether `player` has the objects to pay every part of the cost of an ability of the object
/// in `slot` that is paid with objects.
fn ability_payable<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
    parts: &'static [CostPart],
) -> bool {
    parts.iter().all(|part| match part {
        CostPart::Chosen(action, chosen) => parts::payable(s, t, player, slot, *action, chosen),
        CostPart::This(_)
        | CostPart::Loyalty(_)
        | CostPart::Life(_)
        | CostPart::OneOf(_)
        | CostPart::DiscardHand => true,
    })
}

/// Whether `player` can pay every payment of life among `parts`, as [`life::can_pay`]
/// answers for each (see [CR 119.4](https://yawgatog.com/resources/magic-rules/#R1194)): "If a
/// cost or effect allows a player to pay an amount of life greater than 0, the player may do
/// so only if their life total is greater than or equal to the amount of the payment."
fn life_payable<L: Layout>(s: &State<L>, t: &GameTable, player: Seat, parts: &[CostPart]) -> bool {
    parts.iter().all(|part| match part {
        CostPart::Life(n) => life::can_pay(s, t, player, *n),
        CostPart::This(_)
        | CostPart::Chosen(..)
        | CostPart::Loyalty(_)
        | CostPart::OneOf(_)
        | CostPart::DiscardHand => true,
    })
}
