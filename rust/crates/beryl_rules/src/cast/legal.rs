//! Whether a player may begin to cast a spell (CR 601.3) or activate an ability (CR 602.5), and
//! so which casts and activations a priority decision offers.

use std::cell::{Cell, OnceCell};

use crate::bitboard::Bitboard;
use crate::card::{
    Ability, AbilityKind, Castings, CostAction, Face, FaceIx, ObjectAbility, Timing, Types,
};
use crate::casting;
use crate::decision::{Opt, Refusal};
use crate::engine::collapse;
use crate::fixed::Full;
use crate::ids::{Seat, SlotId};
use crate::layout::{Layout, Queue, Small, Table};
use crate::mana::Funds;
use crate::rule_change;
use crate::state::{RuleKind, State, Tracker, Zone, ZoneKey};
use crate::table::GameTable;

use super::costs::{From, option_sets, spell_cost, x_range_start};
use super::parts;
use super::targets::targetable;
use super::timing::{activated, instant_speed, sorcery_timing};

/// What every check of one player's options reads, found once for all of them: the mana they
/// could pay with and whether an opponent limits them to sorcery speed, each found at the first
/// check that needs it.
///
/// `TARGETS` is whether a spell or ability needs a legal target to be offered. It is false only
/// for [`payable_responses`]. It is a const parameter rather than a field so that the checks
/// behind every priority decision are compiled separately from the search's copy: with a
/// second caller, the compiler stopped inlining [`activation_options`] into the decision
/// builder that rollouts run, which cost the rollout benchmark about 0.3%.
pub(super) struct Situation<L: Layout = Small, const TARGETS: bool = true> {
    player: Seat,
    /// None once found if more classes of sources than [`Funds`] holds: a stall for whatever
    /// needed them.
    funds: OnceCell<Option<Funds<L>>>,
    full: Cell<bool>,
}

impl<L: Layout> Situation<L> {
    pub(super) fn of(player: Seat) -> Situation<L> {
        Situation {
            player,
            funds: OnceCell::new(),
            full: Cell::new(false),
        }
    }
}

impl<L: Layout> Situation<L, false> {
    /// The situation of `player` when a spell or ability with no legal target yet still counts.
    fn ignoring_targets(player: Seat) -> Situation<L, false> {
        Situation {
            player,
            funds: OnceCell::new(),
            full: Cell::new(false),
        }
    }
}

impl<L: Layout, const TARGETS: bool> Situation<L, TARGETS> {
    /// The player's funds. Most priority decisions offer nothing that costs mana, as an
    /// opponent's turn with no instant in hand, so the source scan waits until a check needs it.
    fn funds(&self, s: &State<L>, t: &GameTable) -> Option<&Funds<L>> {
        let funds = self
            .funds
            .get_or_init(|| Funds::of(s, t, self.player).ok())
            .as_ref();
        if funds.is_none() {
            self.full.set(true);
        }
        funds
    }

    /// `Full` if a check needed more classes of sources than [`Funds`] holds, so that its answer was
    /// not found: the options listed stand only if this is `Ok`.
    pub(super) fn complete(&self) -> Result<(), Full> {
        if self.full.get() { Err(Full) } else { Ok(()) }
    }
}

/// Whether a face may be cast now, as far as its card types and the timing rules say: what
/// [`refused`] checks first, which depends only on the face, so on the card.
#[inline]
fn in_time<L: Layout, const TARGETS: bool>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L, TARGETS>,
    face: &Face,
    from: From,
) -> bool {
    // CR 305.9: "If an object is both a land and another card type, it can be played only as
    // a land. It can't be cast as a spell." CR 712.8c: "Only the face that will be face up
    // on the stack is evaluated to determine if it can be cast."
    if face.types.intersects(Types::LAND) {
        return false;
    }
    match from {
        // Sorcery timing first: it is three compares, lets any spell be cast, and is how
        // most spells are, so the rule changes are read only outside it.
        From::Priority => {
            sorcery_timing(s, now.player)
                || (instant_speed(s, t, face, now.player)
                    && !rule_change::cast_only_as_sorcery(s, t, now.player, face))
        }
        // During a resolution the stack is never empty, so a sorcery-speed limit forbids it.
        From::Resolution { .. } => !rule_change::cast_only_as_sorcery(s, t, now.player, face),
    }
}

/// Why `player` may not cast the card in `slot` with this face and these casting options:
/// the first check it fails, or none if they may. A timing refusal names no permanent here
/// ([`refusal`] does).
///
/// [CR 601.3](https://yawgatog.com/resources/magic-rules/#R6013): "A player can begin to cast
/// a spell only if a rule or effect allows that player to cast it and no rule or effect
/// prohibits that player from casting it."
fn refused<L: Layout, const TARGETS: bool>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L, TARGETS>,
    slot: L::Slot,
    face_index: FaceIx,
    costs: Castings,
    from: From,
) -> Option<Refusal<L>> {
    let player = now.player;
    let face = s.copiable(slot).turned(face_index).face(t);
    if !in_time(s, t, now, face, from) {
        return Some(Refusal::Timing { by: None });
    }
    let (increase, free) = match from {
        From::Priority => (
            casting::permission(s, player, slot).map_or(0, |p| p.terms.increase),
            false,
        ),
        From::Resolution { free } => (0, free),
    };
    // The checks are pure, so their order changes nothing but their cost: the mana, found
    // once for the decision, first, and the targets, which read the battlefield, last.
    // [CR 118.3](https://yawgatog.com/resources/magic-rules/#R1183): "A player can't pay a
    // cost without having the necessary resources to pay it fully."
    let x = x_range_start(face, costs, free).unwrap_or(0);
    let cost = spell_cost(face, costs, x, increase, free);
    if !now
        .funds(s, t)
        .is_some_and(|f| f.pays(s, t, cost, Bitboard::EMPTY))
    {
        return Some(Refusal::Mana);
    }
    // CR 601.2h: the player pays the total cost, so the objects its parts take must be there.
    if !parts::spell_payable(s, t, player, slot, face, costs) {
        return Some(Refusal::Cost);
    }
    (TARGETS
        && !targetable(
            s,
            t,
            face.spell_ability_with(costs),
            player,
            Some(slot),
            0,
            None,
        ))
    .then_some(Refusal::Target)
}

/// How far a refusal's way of casting got through [`refused`]'s checks, which run in this
/// order.
fn progress<L: Layout>(r: Refusal<L>) -> u8 {
    match r {
        Refusal::Timing { .. } | Refusal::LandPlayed => 0,
        Refusal::Mana => 1,
        Refusal::Cost => 2,
        Refusal::Target => 3,
    }
}

/// Why `player` may not cast the card in `slot` at priority now: the refusal of the way of
/// casting it, by face and casting options, that passes the most checks, or `land`, why they
/// may not play it as a land, if that passes as many. None if one way is legal, or if the
/// card has no face to cast and `land` is none.
pub(crate) fn refusal<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
    land: Option<Refusal<L>>,
) -> Result<Option<Refusal<L>>, Full> {
    let now = Situation::of(player);
    let Some(def) = s.card_def(t, slot) else {
        return Ok(land);
    };
    let flashback = casting::permission(s, player, slot).is_some_and(|p| p.terms.is_flashback());
    let mut most = land;
    for f in def.face_ixs() {
        let face = def.face(f);
        if face.transformed || face.types.intersects(Types::LAND) {
            continue;
        }
        for costs in option_sets(face, flashback) {
            let Some(r) = refused(s, t, &now, slot, f, costs, From::Priority) else {
                return Ok(None);
            };
            if most.is_none_or(|m| progress(r) > progress(m)) {
                most = Some(r);
            }
        }
    }
    // Only sorcery speed, where a face could be cast at instant speed but for the limit.
    if most == Some(Refusal::Timing { by: None }) {
        let instant = (def.face_ixs().map(|f| def.face(f)))
            .find(|face| !face.transformed && instant_speed(s, t, face, player));
        if let Some(face) = instant {
            let limit = RuleKind::CastOnlyAsSorcery;
            most = Some(Refusal::Timing {
                by: rule_change::maker(s, t, limit, player, face),
            });
        }
    }
    now.complete()?;
    Ok(most)
}

/// The cards `player` can cast at priority: their hand and the cards a permission names.
fn cast_sources<L: Layout>(s: &State<L>, player: Seat) -> Bitboard<L::Slot> {
    let mut cards = s.zone(ZoneKey::Of(player, Zone::Hand));
    for p in s.cast_permissions() {
        if p.terms.player == player {
            cards.insert(p.card.slot);
        }
    }
    cards
}

/// Appends the spells `player` can cast now, by card, face and casting options.
pub(super) fn cast_options<L: Layout, const TARGETS: bool>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L, TARGETS>,
    cards: Bitboard<L::Slot>,
    from: From,
    out: &mut L::Queue<Opt<L>, 64>,
) -> Result<(), Full> {
    let player = now.player;
    let free = matches!(from, From::Resolution { free: true });
    // Cards no face of which may be cast now are left out before collapsing, which saves
    // comparing them: whether one may depends only on its card, which interchangeable cards
    // share, so the cards kept are the same.
    let cards = cards.filter(|slot| {
        s.card_def(t, slot).is_some_and(|def| {
            def.face_ixs().any(|f| {
                let face = def.face(f);
                !face.transformed && in_time(s, t, now, face, from)
            })
        })
    });
    // Candidates are listed in the candidate order, which a `Choice` counts in.
    let with_def = |slot| Some((slot, s.card_def(t, slot)?));
    for (slot, def) in s
        .in_candidate_order(collapse(s, cards))
        .filter_map(with_def)
    {
        let flashback = from == From::Priority
            && casting::permission(s, player, slot).is_some_and(|p| p.terms.is_flashback());
        for f in def.face_ixs() {
            let face = def.face(f);
            if face.transformed {
                continue;
            }
            for costs in option_sets(face, free || flashback) {
                if refused(s, t, now, slot, f, costs, from).is_none() {
                    out.push(Opt::Cast {
                        slot,
                        face: f,
                        costs,
                    })?;
                }
            }
        }
    }
    Ok(())
}

/// The permanents that pay for an ability with themselves, and so cannot also tap for its
/// mana.
pub(super) fn spent_by_cost<S: SlotId>(slot: S, a: &Ability) -> Bitboard<S> {
    let mut b = Bitboard::EMPTY;
    if a.cost().does(CostAction::Tap) || a.cost().does(CostAction::Sacrifice) {
        b.insert(slot);
    }
    b
}

/// Whether `player` may activate ability `ability` of the object in `slot` now.
///
/// [CR 602.5](https://yawgatog.com/resources/magic-rules/#R6025): "A player can't begin to
/// activate an ability that's prohibited from being activated."
fn activatable<L: Layout, const TARGETS: bool>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L, TARGETS>,
    slot: L::Slot,
    ability: ObjectAbility,
) -> bool {
    let player = now.player;
    let o = s.object(slot);
    let Some((source, index)) = activated(s, slot, ability) else {
        return false;
    };
    let a = source.face(t).ability(index);
    if a.kind() != AbilityKind::Activated {
        return false;
    }
    // CR 602.2: "Only an object's controller (or its owner, if it doesn't have a controller)
    // can activate its activated ability". CR 702.29a: cycling "functions only while the card
    // with cycling is in a player's hand."
    let in_place = if a.cost().does(CostAction::Discard) {
        o.zone() == Zone::Hand && o.owner() == player
    } else {
        o.zone() == Zone::Battlefield && s.controller(slot) == player
    };
    if !in_place {
        return false;
    }
    if a.cost().does(CostAction::Tap) {
        // CR 118.3: "a permanent that's already tapped can't be tapped to pay a cost". CR
        // 602.5a: "A creature's activated ability with the tap symbol ... in its activation
        // cost can't be activated unless the creature has been under its controller's control
        // since the start of their most recent turn."
        if o.is_tapped()
            || (s.derived(slot).types.intersects(Types::CREATURE) && o.is_summoning_sick())
        {
            return false;
        }
    }
    // CR 118.3: "A player can't pay a cost without having the necessary resources to pay it
    // fully."
    if a.cost().chooses() && !parts::ability_payable(s, t, player, slot, a.cost().parts()) {
        return false;
    }
    if a.cost().pays_life_or_chooses() && !parts::life_payable(s, player, a.cost().parts()) {
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
        a.timing() != Timing::Sorcery || sorcery_timing(s, player)
    };
    if !timely {
        return false;
    }
    // The mana before the targets, which read the battlefield: both checks are pure.
    let cost = a.cost().mana.with_x(0);
    let spent = spent_by_cost(slot, a);
    let substitute = if a.cost().chooses() {
        parts::substitute(a.cost().parts())
    } else {
        None
    };
    let paid = now.funds(s, t).is_some_and(|funds| match substitute {
        Some((_, action, chosen, most)) => {
            let objects = parts::candidates(s, t, player, slot, action, chosen);
            parts::payable_with_substitutes(s, t, funds, cost, most, objects, spent)
        }
        None => funds.pays(s, t, cost, spent),
    });
    paid && (!TARGETS || targetable(s, t, Some(a), player, None, 0, Some(slot)))
}

/// Whether activating `ability` of the object in `slot` now would do nothing, as its effect
/// says ([`Ability::needs`]): its cost would be paid and its effect find nothing to act on.
/// The activation is still legal (CR 602.2);
/// [`Profile::hides_pointless`](crate::profile::Profile) says who isn't offered it.
pub fn pointless<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    ability: ObjectAbility,
) -> bool {
    let Some((source, index)) = activated(s, slot, ability) else {
        return false;
    };
    (source.face(t).ability(index).needs())
        .is_some_and(|objects| crate::interpret::names_nothing(s, t, slot, objects))
}

/// Appends the abilities `player` can activate now. CR 117.1b: "A player may activate an
/// activated ability any time they have priority."
fn activation_options<L: Layout, const TARGETS: bool>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L, TARGETS>,
    out: &mut L::Queue<Opt<L>, 64>,
) -> Result<(), Full> {
    let player = now.player;
    // Activations that would do nothing, left out for the player the profile hides them from,
    // where a card of the game has one.
    let prunes = TARGETS && s.profile().hides_pointless[player.index()] && t.needy();
    let objects = s
        .in_zone(Zone::Battlefield)
        .filter(|slot| s.controller(slot) == player)
        | s.zone(ZoneKey::Of(player, Zone::Hand));
    // Most objects have no activated ability; leaving them out before collapsing saves
    // comparing them. Whether one has is part of what collapsing compares (its card, face
    // and granted abilities), so the result is the same.
    let facts = |slot: L::Slot| s.copiable(slot).facts(t);
    let objects = objects.filter(|slot| {
        facts(slot).activates()
            || (s.object(slot).zone() == Zone::Battlefield && { s.derived(slot).granted }
                .iter()
                .any(Option::is_some))
    });
    // Candidates are listed in the candidate order, which a `Choice` counts in.
    for slot in s.in_candidate_order(collapse(s, objects)) {
        let granted = if s.object(slot).zone() == Zone::Battlefield {
            { s.derived(slot).granted }.len() as u8
        } else {
            0
        };
        // Of its own abilities, only the activated ones can be activated (CR 602.1), so the
        // rest are not checked; granted ones are checked whatever they are. One it has lost
        // can't be activated (CR 613.1f): asked once of a permanent that has them all, as
        // most have.
        let lost = !s.has_every_ability(slot);
        let abilities = facts(slot)
            .activated()
            .map(ObjectAbility::Own)
            .chain((0..granted).map(ObjectAbility::Granted));
        for ability in abilities {
            if lost && matches!(ability, ObjectAbility::Own(i) if !s.has_ability(slot, i)) {
                continue;
            }
            if activatable(s, t, now, slot, ability) && !(prunes && pointless(s, t, slot, ability))
            {
                out.push(Opt::Activate { slot, ability })?;
            }
        }
    }
    Ok(())
}

/// Appends the priority actions casting and activation add: spells and abilities.
pub(crate) fn priority_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    out: &mut L::Queue<Opt<L>, 64>,
) -> Result<(), Full> {
    let now = Situation::of(player);
    cast_options(s, t, &now, cast_sources(s, player), From::Priority, out)?;
    activation_options(s, t, &now, out)?;
    now.complete()?;
    Ok(())
}

/// Spells and abilities whose costs `player` could pay at instant speed on another
/// player's turn. A response's target may only appear after the other player acts.
///
/// Under [CR 117.1a], "A player may cast an instant spell any time they have priority."
/// Under [CR 118.3], "A player can't pay a cost without having the necessary resources
/// to pay it fully."
///
/// [CR 117.1a]: https://yawgatog.com/resources/magic-rules/#R1171a
/// [CR 118.3]: https://yawgatog.com/resources/magic-rules/#R1183
pub fn payable_responses<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
) -> Result<L::Queue<Opt<L>, 64>, Full> {
    debug_assert_ne!(s.turn().active(), player);
    let now = Situation::<L, false>::ignoring_targets(player);
    let mut out = Default::default();
    cast_options(
        s,
        t,
        &now,
        cast_sources(s, player),
        From::Priority,
        &mut out,
    )?;
    activation_options(s, t, &now, &mut out)?;
    now.complete()?;
    Ok(out)
}
