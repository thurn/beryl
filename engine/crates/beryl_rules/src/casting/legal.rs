//! Whether a player may begin to cast a spell (CR 601.3) or activate an ability (CR 602.5), and
//! so which casts and activations a priority decision offers.
//!
//! Implements: CR 305.9, CR 606.3, CR 606.6, CR 702.37a

use std::cell::{Cell, OnceCell};

use crate::bitboard::Bitboard;
use crate::card::{
    Ability, AbilityIx, AbilityKind, CastingOption, Castings, CostAction, CostPart, Face, FaceIx,
    Keywords, ManaCost, ObjectAbility, Timing, Types,
};
use crate::continuous::rule_change;
use crate::decision::{Candidates, OPTIONS, Opt, Refusal};
use crate::engine::collapse;
use crate::fixed::Full;
use crate::game::life;
use crate::ids::{Seat, SlotId};
use crate::layout::{Layout, Queue, Small};
use crate::mana::{self, Funds};
use crate::state::{RuleKind, State, Tracker, Zone, ZoneKey};
use crate::table::{Capability, GameTable};

use super::costs::{From, option_sets, spell_cost, x_range_start};
use super::delve;
use super::mana_payment;
use super::parts;
use super::targets::{Itself, targetable};
use super::timing::{activated, instant_speed, sorcery_timing};

mod copies;
mod preparation;
pub(super) mod special;
mod ways;

use ways::{Ways, cast_sources};

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

impl<L: Layout, const TARGETS: bool> Situation<L, TARGETS> {
    pub(super) fn of(player: Seat) -> Self {
        Situation {
            player,
            funds: OnceCell::new(),
            full: Cell::new(false),
        }
    }

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
    // a land. It can't be cast as a spell." see CR 712.8c: "Only the face that will be face up
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

/// Whether restricted mana the spell accepts pays `cost` for casting the card in `slot` as its
/// face `face`, where unrestricted mana does not: never in a game whose cards make no
/// restricted mana ([`mana::Paying`]).
#[cold]
#[inline(never)]
fn restricted_pays<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    funds: &Funds<L>,
    cost: ManaCost,
    slot: L::Slot,
    face: FaceIx,
) -> bool {
    let paying = mana::Paying::casting(s, t, slot, face);
    paying.reads_restrictions(t) && funds.pays_restricted(s, t, cost, Bitboard::EMPTY, paying)
}

/// Whether `cost` for casting the card in `slot` as its face `face` is paid some way other
/// than with unrestricted mana: with restricted mana the spell accepts, or with cards delve
/// exiles (see CR 702.66a). Never in a game whose cards make no restricted mana and have no
/// delve, which the caller tests first.
#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn pays_otherwise<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    funds: &Funds<L>,
    cost: ManaCost,
    player: Seat,
    slot: L::Slot,
    face: FaceIx,
    costs: Castings,
) -> bool {
    (t.can(Capability::RemembersMana) && restricted_pays(s, t, funds, cost, slot, face))
        || (t.can(Capability::Delve)
            && delve::delves(s.copiable(slot).turned(t, face).face(t))
            && delve::delve_pays(s, t, funds, player, slot, face, costs, cost))
}

/// Why `player` may not cast the card in `slot` with this face and these casting options:
/// the first check it fails, or none if they may. A timing refusal names no permanent here
/// ([`refusal`] does).
///
/// [CR 601.3](https://yawgatog.com/resources/magic-rules/#R6013): "A player can begin to cast
/// a spell only if a rule or effect allows that player to cast it and no rule or effect
/// prohibits that player from casting it."
/// See [CR 716.3a]: "When casting an adventurer card as an Adventure, only the alternative
/// characteristics are evaluated to see if it can be cast."
///
/// [CR 716.3a]: https://yawgatog.com/resources/magic-rules/#R7163a
#[expect(
    clippy::inline_always,
    reason = "with the escape casts a second caller, the compiler called it out of line from \
              the priority decision's casts, which cost the rollout benchmark 0.25%"
)]
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn refused<L: Layout, const TARGETS: bool>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L, TARGETS>,
    slot: L::Slot,
    face_index: FaceIx,
    costs: Castings,
    from: From,
    increase: u8,
) -> Option<Refusal<L>> {
    let player = now.player;
    let face = s.copiable(slot).turned(t, face_index).face(t);
    // A rule change that prohibits the cast (CR 601.3 above) is told as a timing refusal.
    if !in_time(s, t, now, face, from) || rule_change::cant_cast(s, t, player, face) {
        return Some(Refusal::Timing { by: None });
    }
    let free = matches!(from, From::Resolution { free: true });
    // The checks are pure, so their order changes nothing but their cost: the mana, found
    // once for the decision, first, and the targets, which read the battlefield, last.
    // [CR 118.3](https://yawgatog.com/resources/magic-rules/#R1183): "A player can't pay a
    // cost without having the necessary resources to pay it fully."
    let x = x_range_start(face, costs, free).unwrap_or(0);
    let cost = spell_cost(s, t, player, face, costs, x, increase, free);
    if !now.funds(s, t).is_some_and(|f| {
        f.pays(s, t, cost, Bitboard::EMPTY)
            || (t.can_either(Capability::RemembersMana, Capability::Delve)
                && pays_otherwise(s, t, f, cost, player, slot, face_index, costs))
    }) {
        return Some(Refusal::Mana);
    }
    // CR 601.2h: the player pays the total cost, including any parts funded by earlier parts.
    let payable = if t.can(Capability::SacrificeToCast) {
        mana_payment::whole_cost_payable(s, t, player, slot, face_index, face, costs, cost)
    } else {
        parts::spell_payable(s, t, player, slot, face, costs)
    };
    match payable {
        Ok(true) => {}
        Ok(false) => return Some(Refusal::Cost),
        Err(Full) => {
            now.full.set(true);
            return Some(Refusal::Cost);
        }
    }
    (TARGETS
        && !targetable(
            s,
            t,
            face.spell_ability_with(costs),
            player,
            Itself {
                spell: Some(slot),
                on_stack: false,
            },
            0,
            None,
        ))
    .then_some(Refusal::Target)
}

/// Keeps in `most` whichever of it and `r` got further through [`refused`]'s checks, which
/// run in the order of `progress`.
fn keep_furthest<L: Layout>(most: &mut Option<Refusal<L>>, r: Refusal<L>) {
    let progress = |r| match r {
        Refusal::Timing { .. } | Refusal::LandPlayed => 0,
        Refusal::Mana => 1,
        Refusal::Cost => 2,
        Refusal::Target => 3,
    };
    if most.is_none_or(|m| progress(r) > progress(m)) {
        *most = Some(r);
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
        return if s.prepare_spells_for(player).contains(slot) {
            preparation::refusal(s, t, &now, slot)
        } else {
            Ok(land)
        };
    };
    let ways = Ways::of(s, t, player, slot, From::Priority);
    let mut most = land;
    for f in def.face_ixs() {
        let face = def.face(f);
        if !face.playable() || face.types.intersects(Types::LAND) {
            continue;
        }
        let ways = if ways.on_adventure && f == FaceIx::BACK {
            ways.back(s, player, slot)
        } else {
            ways
        };
        for costs in ways.option_sets(face) {
            // A cast for escape is under no permission's terms.
            let increase = if costs.escapes() { 0 } else { ways.increase };
            let Some(r) = refused(s, t, &now, slot, f, costs, From::Priority, increase) else {
                return Ok(None);
            };
            keep_furthest(&mut most, r);
        }
    }
    for (face, costs) in special::casts(s, t, player, slot, def, ways) {
        let Some(r) = refused(s, t, &now, slot, face, costs, From::Priority, ways.increase) else {
            return Ok(None);
        };
        keep_furthest(&mut most, r);
    }
    // Only sorcery speed, where a face could be cast at instant speed but for the limit.
    if most == Some(Refusal::Timing { by: None }) {
        let instant = (def.face_ixs().map(|f| def.face(f)))
            .find(|face| face.playable() && instant_speed(s, t, face, player));
        if let Some(face) = instant {
            let limit = RuleKind::CastOnlyAsSorcery;
            most = Some(Refusal::Timing {
                by: rule_change::maker(s, t, limit, player, face)
                    .and_then(|slot| s.permanent_at(slot)),
            });
        }
    }
    now.complete()?;
    Ok(most)
}

/// Appends the spells `player` can cast now, by card, face and casting options.
pub(super) fn cast_options<L: Layout, const TARGETS: bool, const SEARCH: bool>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L, TARGETS>,
    cards: Bitboard<L::Slot>,
    from: From,
    out: &mut L::Queue<Opt<L>, OPTIONS>,
) -> Result<(), Full> {
    let player = now.player;
    // Cards no face of which may be cast now are left out before collapsing, which saves
    // comparing them: whether one may depends only on its card, which interchangeable cards
    // share, so the cards kept are the same.
    let special = !SEARCH && t.can_either(Capability::Morph, Capability::Split);
    let cards = cards.filter(|slot| {
        copies::castable_def(s, t, slot, from).is_some_and(|def| {
            def.face_ixs().any(|f| {
                let face = def.face(f);
                face.playable()
                    && (!SEARCH || face.permits_cast_while_searching())
                    && in_time(s, t, now, face, from)
            }) || (special && special::in_time(s, t, now, slot, def, from))
        })
    });
    // Each card cast during a search leaves its own place in the library.
    let cards = if SEARCH { cards } else { collapse(s, cards) };
    Candidates::<L>::extend_options(out, cards, |out, slot| {
        let Some(def) = copies::castable_def(s, t, slot, from) else {
            return Ok(());
        };
        let ways = Ways::of(s, t, player, slot, from);
        for f in def.face_ixs() {
            let face = def.face(f);
            if !face.playable() || SEARCH && !face.permits_cast_while_searching() {
                continue;
            }
            let ways = if ways.on_adventure && f == FaceIx::BACK {
                ways.back(s, player, slot)
            } else {
                ways
            };
            if ways.plain {
                for costs in option_sets(face, ways.alternative) {
                    if ways.permitted(s, t, player, face, costs)
                        && refused(s, t, now, slot, f, costs, from, ways.increase).is_none()
                    {
                        out.push(Opt::Cast {
                            slot,
                            face: f,
                            costs,
                        })?;
                    }
                }
            }
            if ways.escape {
                escape_options(s, t, now, slot, f, from, out)?;
            }
        }
        if special {
            special::options(s, t, now, slot, def, ways, from, out)?;
        }
        Ok(())
    })
}

/// A card with morph may be cast face down. [CR 702.37a] says "Morph is a static ability
/// that functions in any zone from which you could play the card it’s on".
///
/// [CR 702.37a]: https://yawgatog.com/resources/magic-rules/#R70237a
fn has_morph(face: &Face) -> bool {
    face.casting
        .iter()
        .any(|o| matches!(o, CastingOption::Morph(_)))
}

/// Appends the casts of face `face` of the card in `slot` with the escape an effect grants it
/// ([`Castings::ESCAPE`]), with each set of the face's own options that pays none of its
/// alternative costs. Out of line: only a card that has escape comes here.
#[cold]
#[inline(never)]
fn escape_options<L: Layout, const TARGETS: bool>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L, TARGETS>,
    slot: L::Slot,
    face: FaceIx,
    from: From,
    out: &mut L::Queue<Opt<L>, OPTIONS>,
) -> Result<(), Full> {
    let def_face = s.copiable(slot).turned(t, face).face(t);
    for costs in option_sets(def_face, true).map(Castings::with_escape) {
        if refused(s, t, now, slot, face, costs, from, 0).is_none() {
            out.push(Opt::Cast { slot, face, costs })?;
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
fn activatable<L: Layout, const TARGETS: bool>(
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
            let nothing = mana::Paying::NOTHING;
            parts::payable_with_substitutes(s, t, funds, cost, most, objects, spent, nothing)
        }
        None => funds.pays(s, t, cost, spent),
    });
    paid && (!TARGETS || targetable(s, t, Some(a), player, Itself::default(), 0, Some(slot)))
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
fn activation_options<L: Layout, const TARGETS: bool>(
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

/// The abilities other objects granted the object in `slot`, and those of the cards under it
/// if it is a merged permanent (see [CR 702.140e]), if `any` card of the game grants one or merges
/// ([`Capability::Grants`], [`Capability::Merges`]).
///
/// [CR 702.140e]: https://yawgatog.com/resources/magic-rules/#R702140e
fn granted<'a, L: Layout>(
    s: &'a State<L>,
    t: &'a GameTable,
    slot: L::Slot,
    any: bool,
) -> impl Iterator<Item = ObjectAbility> + 'a {
    any.then(|| s.abilities(t, slot))
        .into_iter()
        .flatten()
        .map(|(ability, _)| ability)
        .filter(|ability| {
            matches!(
                ability,
                ObjectAbility::Granted(_) | ObjectAbility::Merged(..)
            )
        })
}

/// Whether the object in `slot` has an ability other objects granted it, or one of a card
/// under it as a merged permanent. Out of line: only a game with a card that grants an
/// ability or merges asks.
#[inline(never)]
fn has_others<L: Layout>(s: &State<L>, slot: L::Slot) -> bool {
    s.has(slot, ObjectAbility::Granted(0))
        || s.has(slot, ObjectAbility::Merged(0, AbilityIx::new(0)))
}

/// Appends the priority actions casting and activation add: spells and abilities.
pub(crate) fn priority_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    out: &mut L::Queue<Opt<L>, OPTIONS>,
) -> Result<(), Full> {
    let now = Situation::of(player);
    cast_options::<L, true, false>(s, t, &now, cast_sources(s, t, player), From::Priority, out)?;
    activation_options(s, t, &now, out)?;
    if t.can(Capability::Preparation) {
        preparation::casts(s, t, &now, out)?;
    }
    now.complete()?;
    Ok(())
}

/// Spells and abilities whose costs `player` could pay at instant speed on another
/// player's turn. A response's target may only appear after the other player acts.
///
/// Under see [CR 117.1a], "A player may cast an instant spell any time they have priority."
/// Under [CR 118.3], "A player can't pay a cost without having the necessary resources
/// to pay it fully."
///
/// [CR 117.1a]: https://yawgatog.com/resources/magic-rules/#R1171a
/// [CR 118.3]: https://yawgatog.com/resources/magic-rules/#R1183
pub fn payable_responses<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
) -> Result<L::Queue<Opt<L>, OPTIONS>, Full> {
    debug_assert_ne!(s.turn().active(), player);
    // A spell or ability with no legal target yet still counts.
    let now = Situation::<L, false>::of(player);
    let mut out = Default::default();
    cast_options::<L, false, false>(
        s,
        t,
        &now,
        cast_sources(s, t, player),
        From::Priority,
        &mut out,
    )?;
    activation_options(s, t, &now, &mut out)?;
    if t.can(Capability::Preparation) {
        preparation::casts(s, t, &now, &mut out)?;
    }
    now.complete()?;
    Ok(out)
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
