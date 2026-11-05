//! Casting spells and activating abilities as an explicit machine (architecture §7.4): announce,
//! face, alternative and additional costs, X, targets, total cost, payment plan, payment.
//!
//! [CR 601.2](https://yawgatog.com/resources/magic-rules/#R6012): "To cast a spell is to take
//! it from where it is (usually the hand), put it on the stack, and pay its costs, so that it
//! will eventually resolve and have its effect. ... If a player is unable to comply with the
//! requirements of a step listed below while performing that step, the casting of the spell
//! is illegal; the game returns to the moment before the casting of that spell was proposed".
//!
//! Spells that cannot be paid for or targeted are never offered, and every step offers only
//! answers that leave the rest completable, so the machine never has to rewind.

use crate::bitboard::Bitboard;
use crate::card::{
    Ability, AbilityIx, AbilityKind, CastingOption, Face, FaceAbility, Keywords, ManaCost, Rule,
    TargetKind, TargetSpec, Timing, Types,
};
use crate::decision::{Candidates, Decision, DecisionKind, Opt};
use crate::engine::collapse;
use crate::event::{self, Event, Reason, Sink};
use crate::filter::{Filter, Whose};
use crate::fixed::{Fixed, Full};
use crate::ids::{CardIx, Seat, Slot};
use crate::mana;
use crate::profile::Payment;
use crate::state::{
    CastPermission, CastStep, Casting, CounterKind, ObjectFlags, ObjectRef, Pending, StackEntry,
    State, Target, TurnStep, Zone,
};
use crate::table::GameTable;

/// The face of an object as it is now.
fn face_of(s: &State, t: &GameTable, slot: Slot) -> &'static Face {
    let o = s.object(slot);
    &t.def(o.card).faces[usize::from(o.face)]
}

/// The ability a stack entry resolves: an ability of its card's face, or a spell's spell
/// ability. A permanent spell with no targets has none.
pub(crate) fn entry_ability(t: &GameTable, entry: &StackEntry) -> Option<&'static Ability> {
    let face = &t.def(entry.card).faces[usize::from(entry.face)];
    match entry.ability {
        None => face.spell_ability(),
        Some(ability) => Some(face.ability(ability)),
    }
}

/// The ability of a stack entry that is an ability, by its card, face and index.
fn entry_ability_of(t: &GameTable, entry: &StackEntry) -> &'static Ability {
    entry_ability(t, entry).expect("an ability's entry")
}

/// The card, face and ability a stack entry records for activating ability `ability` of the
/// permanent in `slot`: one of its own, or one granted to it, which is printed on the
/// granting card's front face (§9).
fn activated(s: &State, slot: Slot, ability: AbilityIx) -> Option<(CardIx, u8, FaceAbility)> {
    let o = s.object(slot);
    match ability {
        AbilityIx::Own(i) => Some((o.card, o.face, FaceAbility::Own(i))),
        AbilityIx::Granted(k) => {
            if o.zone != Zone::Battlefield {
                return None;
            }
            let grant = s.derived(slot).granted[usize::from(k)]?;
            Some((grant.card, 0, FaceAbility::Grant(grant.grant)))
        }
    }
}

/// Whether `player` may cast a sorcery now
/// ([CR 307.1](https://yawgatog.com/resources/magic-rules/#R3071)): "A player who has
/// priority may cast a sorcery card from their hand during a main phase of their turn when the
/// stack is empty."
pub(crate) fn sorcery_timing(s: &State, player: Seat) -> bool {
    player == s.turn.active
        && matches!(
            s.turn.step,
            TurnStep::PrecombatMain | TurnStep::PostcombatMain
        )
        && s.stack.is_empty()
}

/// Whether a permanent on the battlefield has a static ability with this rule.
fn has_rule(s: &State, t: &GameTable, slot: Slot, rule: Rule) -> bool {
    face_of(s, t, slot)
        .abilities
        .iter()
        .any(|a| a.kind == AbilityKind::Static && a.rule == rule)
}

/// Whether an opponent's permanent limits `player` to casting spells at sorcery speed, as
/// Teferi, Time Raveler does. A prohibition beats a permission
/// ([CR 101.2](https://yawgatog.com/resources/magic-rules/#R1012)): "When a rule or effect
/// allows or directs something to happen, and another effect states that it can't happen,
/// the "can't" effect takes precedence."
fn limited_to_sorcery_speed(s: &State, t: &GameTable, player: Seat) -> bool {
    s.battlefield.iter().any(|&slot| {
        s.controller(slot) == player.opponent()
            && has_rule(s, t, slot, Rule::OpponentsCastAsSorceries)
    })
}

/// Whether `player` may cast this face at instant speed: an instant
/// ([CR 117.1a](https://yawgatog.com/resources/magic-rules/#R1171a)), a face with flash
/// ([CR 702.8a](https://yawgatog.com/resources/magic-rules/#R7028a)), or a sorcery while an
/// effect lets them cast sorceries as though they had flash.
fn instant_speed(s: &State, face: &Face, player: Seat) -> bool {
    face.types.intersects(Types::INSTANT)
        || face.keywords.contains(Keywords::FLASH)
        || (face.types.intersects(Types::SORCERY) && s.player(player).sorceries_as_flash)
}

/// The permission that lets `player` cast the card in `slot`, if one does.
fn permission(s: &State, player: Seat, slot: Slot) -> Option<CastPermission> {
    s.permissions
        .iter()
        .copied()
        .find(|p| p.object.slot == slot && p.player == player && s.is_current(p.object))
}

/// Where a card is cast from, and what that changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum From {
    /// At priority, from a hand or under a permission.
    Priority,
    /// During a resolution that offers it (CR 608.2g), for free or not.
    Resolution { free: bool },
}

/// The bitmasks over a face's casting options that may be chosen together: any of the
/// additional costs, and at most one alternative cost
/// ([CR 118.9a](https://yawgatog.com/resources/magic-rules/#R1189a)): "Only one alternative
/// cost can be applied to any one spell as it's being cast." Casting without paying the mana
/// cost is itself an alternative cost, so `free` excludes the face's own.
fn option_masks(face: &Face, free: bool) -> impl Iterator<Item = u8> + '_ {
    let n = face.casting.len();
    debug_assert!(n <= 8);
    (0..(1u16 << n)).map(|m| m as u8).filter(move |&m| {
        let alternatives = face
            .casting
            .iter()
            .enumerate()
            .filter(|&(i, o)| m & (1 << i) != 0 && o.is_alternative())
            .count();
        alternatives <= usize::from(!free)
    })
}

/// The chosen casting options of a mask.
fn chosen(face: &Face, costs: u8) -> impl Iterator<Item = CastingOption> + '_ {
    face.casting
        .iter()
        .enumerate()
        .filter(move |&(i, _)| costs & (1 << i) != 0)
        .map(|(_, &o)| o)
}

/// Whether X is announced for this cast, and the least value it may take.
///
/// [CR 107.3b](https://yawgatog.com/resources/magic-rules/#R1073b): an effect that lets a
/// player cast a spell "while paying neither its mana cost nor an alternative cost that
/// includes X, then the only legal choice for X is 0."
fn x_range_start(face: &Face, costs: u8, free: bool) -> Option<u8> {
    let alternative = chosen(face, costs).any(CastingOption::is_alternative);
    let in_mana_cost = face.cost.x > 0 && !free && !alternative;
    let mut min = None;
    for o in chosen(face, costs) {
        if o.cost().x > 0 {
            let least = match o {
                CastingOption::Kicker { min_x, .. } => min_x,
                _ => 0,
            };
            min = Some(min.map_or(least, |m: u8| m.max(least)));
        }
    }
    if in_mana_cost {
        min = Some(min.unwrap_or(0));
    }
    min
}

/// A spell's total cost ([CR 601.2f](https://yawgatog.com/resources/magic-rules/#R6012f)):
/// "The total cost is the mana cost or alternative cost (as determined in rule 601.2b), plus
/// all additional costs and cost increases, and minus all cost reductions." A permission's
/// increase, such as Elite Spellbinder's {2}, is the engine's cost-modifier hook (§6.3).
fn spell_cost(face: &Face, costs: u8, x: u8, extra: u8, free: bool) -> ManaCost {
    let mut cost = if free { ManaCost::NONE } else { face.cost };
    for o in chosen(face, costs) {
        if o.is_alternative() {
            cost = o.cost();
        }
    }
    for o in chosen(face, costs).filter(|o| !o.is_alternative()) {
        cost = cost.plus(o.cost());
    }
    cost.plus(ManaCost::generic(extra)).with_x(x)
}

/// The objects or players a target spec can choose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TargetSet {
    Objects(Bitboard),
    /// A bit per player.
    Players(u8),
}

fn player_matches(filter: Filter, player: Seat, you: Seat) -> bool {
    match filter {
        Filter::Any => true,
        Filter::Whose(Whose::You) => player == you,
        Filter::Whose(Whose::Opponent) => player == you.opponent(),
        _ => unreachable!("a player filter is Any or Whose"),
    }
}

/// The spells on the stack, as objects.
fn spells(s: &State) -> Bitboard {
    s.stack
        .iter()
        .filter_map(StackEntry::spell)
        .map(|r| r.slot)
        .collect()
}

fn in_zone(s: &State, zone: Zone) -> Bitboard {
    s.zone(Seat::P0, zone) | s.zone(Seat::P1, zone)
}

/// The objects or players a target spec can choose now, for a spell or ability controlled by
/// `you`. `itself` is the spell being cast, if one is, which is an illegal target for itself
/// ([CR 115.5](https://yawgatog.com/resources/magic-rules/#R1155)): "A spell or ability on the
/// stack is an illegal target for itself." Before it is cast it is not yet on the stack, but
/// it will have left the zone it is in when its targets are chosen. `x` is the value of X of
/// the spell or ability. `source` is an ability's source, which "another target" can't
/// choose.
fn candidates(
    s: &State,
    t: &GameTable,
    spec: &TargetSpec,
    you: Seat,
    itself: Option<Slot>,
    x: u8,
    source: Option<Slot>,
) -> TargetSet {
    let mut within = match spec.kind {
        TargetKind::Player => {
            let players = Seat::BOTH
                .into_iter()
                .filter(|&p| player_matches(spec.filter, p, you))
                .fold(0, |m, p| m | (1 << p.get()));
            return TargetSet::Players(players);
        }
        TargetKind::Permanent => in_zone(s, Zone::Battlefield),
        TargetKind::Spell => spells(s),
        TargetKind::SpellOrPermanent => spells(s) | in_zone(s, Zone::Battlefield),
        TargetKind::GraveyardCard => in_zone(s, Zone::Graveyard),
    };
    if let Some(itself) = itself {
        within.remove(itself);
    }
    if let Some(source) = source.filter(|_| spec.other) {
        within.remove(source);
    }
    TargetSet::Objects(spec.filter.select_x(s, t, within, you, x))
}

impl TargetSet {
    fn count(self) -> u32 {
        match self {
            TargetSet::Objects(b) => b.count(),
            TargetSet::Players(p) => p.count_ones(),
        }
    }
}

/// Whether every required target can be chosen
/// ([CR 115.3](https://yawgatog.com/resources/magic-rules/#R1153)): "The same target can't be
/// chosen multiple times for any one instance of the word "target" on a spell or ability."
fn targetable(
    s: &State,
    t: &GameTable,
    ability: Option<&Ability>,
    you: Seat,
    itself: Option<Slot>,
    x: u8,
    source: Option<Slot>,
) -> bool {
    ability.is_none_or(|a| {
        a.targets.iter().all(|spec| {
            spec.optional
                || candidates(s, t, spec, you, itself, x, source).count() >= u32::from(spec.count)
        })
    })
}

/// The slot of an entry's ability source; a spell has none.
fn source_of(entry: &StackEntry) -> Option<Slot> {
    entry.object.filter(|_| !entry.is_spell()).map(|r| r.slot)
}

/// Whether a chosen target is still legal as its spell or ability resolves
/// ([CR 608.2b](https://yawgatog.com/resources/magic-rules/#R6082b)): "A target that's no
/// longer in the zone it was in when it was targeted is illegal. Other changes to the game
/// state may cause a target to no longer be legal". A zone change gives the object a new
/// incarnation, so a stale reference is illegal.
pub(crate) fn still_legal(
    s: &State,
    t: &GameTable,
    entry: &StackEntry,
    ability: &Ability,
    target: Target,
) -> bool {
    let spec = &ability.targets[usize::from(target.spec)];
    let you = entry.controller;
    let Some(r) = target.as_object() else {
        return target
            .as_player()
            .is_some_and(|p| player_matches(spec.filter, p, you));
    };
    if !s.is_current(r) {
        return false;
    }
    let itself = entry.spell().map(|r| r.slot);
    match candidates(s, t, spec, you, itself, entry.x, source_of(entry)) {
        TargetSet::Objects(b) => b.contains(r.slot),
        TargetSet::Players(_) => false,
    }
}

/// Whether `player` may cast the card in `slot` with this face and these casting options.
///
/// [CR 601.3](https://yawgatog.com/resources/magic-rules/#R6013): "A player can begin to cast
/// a spell only if a rule or effect allows that player to cast it and no rule or effect
/// prohibits that player from casting it."
fn castable(
    s: &State,
    t: &GameTable,
    player: Seat,
    slot: Slot,
    face_index: u8,
    costs: u8,
    from: From,
) -> Result<bool, Full> {
    let face = &t.def(s.object(slot).card).faces[usize::from(face_index)];
    // CR 305.9: "If an object is both a land and another card type, it can be played only as
    // a land. It can't be cast as a spell." CR 712.11c: "Only the face that will be face up
    // on the stack is evaluated to determine if it can be cast."
    if face.types.intersects(Types::LAND) {
        return Ok(false);
    }
    let (extra, free) = match from {
        From::Priority => {
            let extra = permission(s, player, slot).map_or(0, |p| p.extra);
            if (!instant_speed(s, face, player) || limited_to_sorcery_speed(s, t, player))
                && !sorcery_timing(s, player)
            {
                return Ok(false);
            }
            (extra, false)
        }
        From::Resolution { free } => {
            // During a resolution the stack is never empty, so a sorcery-speed limit forbids
            // it.
            if limited_to_sorcery_speed(s, t, player) {
                return Ok(false);
            }
            (0, free)
        }
    };
    if !targetable(s, t, face.spell_ability(), player, Some(slot), 0, None) {
        return Ok(false);
    }
    let x = x_range_start(face, costs, free).unwrap_or(0);
    mana::payable(
        s,
        t,
        player,
        spell_cost(face, costs, x, extra, free),
        Bitboard::EMPTY,
    )
}

/// The cards `player` can cast at priority: their hand and the cards a permission names.
fn cast_sources(s: &State, player: Seat) -> Bitboard {
    let mut cards = s.zone(player, Zone::Hand);
    for p in &s.permissions {
        if p.player == player && s.is_current(p.object) {
            cards.insert(p.object.slot);
        }
    }
    cards
}

/// Appends the spells `player` can cast now, by card, face and casting options.
fn cast_options(
    s: &State,
    t: &GameTable,
    player: Seat,
    cards: Bitboard,
    from: From,
    out: &mut Fixed<Opt, 64>,
) -> Result<(), Full> {
    let free = matches!(from, From::Resolution { free: true });
    for slot in collapse(s, cards).iter() {
        let def = t.def(s.object(slot).card);
        for (f, face) in def.faces.iter().enumerate() {
            if face.transformed {
                continue;
            }
            for costs in option_masks(face, free) {
                if castable(s, t, player, slot, f as u8, costs, from)? {
                    out.push(Opt::Cast {
                        slot,
                        face: f as u8,
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
fn spent_by_cost(slot: Slot, a: &Ability) -> Bitboard {
    let mut b = Bitboard::EMPTY;
    if a.cost.tap || a.cost.sacrifice_self {
        b.insert(slot);
    }
    b
}

/// The other untapped creatures `player` controls, which may crew the Vehicle in `vehicle`
/// (CR 702.122a). Summoning sickness doesn't stop them: tapping them is a cost of the
/// Vehicle's ability, not an ability of theirs with {T} (CR 302.6).
fn crew_candidates(s: &State, player: Seat, vehicle: Slot) -> Bitboard {
    s.battlefield()
        .iter()
        .copied()
        .filter(|&slot| {
            slot != vehicle
                && s.controller(slot) == player
                && s.derived(slot).types.contains(Types::CREATURE)
                && !s.object(slot).flags.has(ObjectFlags::TAPPED)
        })
        .collect()
}

/// A creature's power counted toward a crew cost; less than 0 counts as 0.
fn power_of(s: &State, slot: Slot) -> u8 {
    u8::try_from(s.derived(slot).power.max(0)).unwrap_or(0)
}

/// The total power of the creatures that could crew the Vehicle in `vehicle`.
fn crew_power(s: &State, player: Seat, vehicle: Slot) -> u32 {
    crew_candidates(s, player, vehicle)
        .iter()
        .map(|slot| u32::from(power_of(s, slot)))
        .sum()
}

/// Whether `player` may activate ability `ability` of the object in `slot` now.
///
/// [CR 602.5](https://yawgatog.com/resources/magic-rules/#R6025): "A player can't begin to
/// activate an ability that's prohibited from being activated."
fn activatable(
    s: &State,
    t: &GameTable,
    player: Seat,
    slot: Slot,
    ability: AbilityIx,
) -> Result<bool, Full> {
    let o = s.object(slot);
    let Some((card, face, index)) = activated(s, slot, ability) else {
        return Ok(false);
    };
    let a = t.def(card).faces[usize::from(face)].ability(index);
    if a.kind != AbilityKind::Activated {
        return Ok(false);
    }
    // CR 602.2: "Only an object's controller (or its owner, if it doesn't have a controller)
    // can activate its activated ability". CR 702.29a: cycling "functions only while the card
    // with cycling is in a player's hand."
    let in_place = if a.cost.discard_self {
        o.zone == Zone::Hand && o.owner == player
    } else {
        o.zone == Zone::Battlefield && s.controller(slot) == player
    };
    if !in_place {
        return Ok(false);
    }
    if a.cost.tap {
        // CR 118.3: "a permanent that's already tapped can't be tapped to pay a cost". CR
        // 602.5a: "A creature's activated ability with the tap symbol ... in its activation
        // cost can't be activated unless the creature has been under its controller's control
        // since the start of their most recent turn."
        if o.flags.has(ObjectFlags::TAPPED)
            || (s.derived(slot).types.intersects(Types::CREATURE)
                && o.flags.has(ObjectFlags::SUMMONING_SICK))
        {
            return Ok(false);
        }
    }
    if a.cost.crew > 0 && crew_power(s, player, slot) < u32::from(a.cost.crew) {
        return Ok(false);
    }
    // CR 118.3: "A player can't pay a cost without having the necessary resources to pay it
    // fully."
    if a.cost.remove.is_some_and(|kind| o.counters.get(kind) == 0) {
        return Ok(false);
    }
    let sorcery_speed = if let Some(loyalty) = a.cost.loyalty {
        // CR 606.3: "A player may activate a loyalty ability of a permanent they control any
        // time they have priority and the stack is empty during a main phase of their turn,
        // but only if no player has previously activated a loyalty ability of that permanent
        // that turn."
        if o.flags.has(ObjectFlags::LOYALTY_ACTIVATED) {
            return Ok(false);
        }
        // CR 606.6: "A loyalty ability with a negative loyalty cost ... can't be activated
        // unless the permanent has at least that many loyalty counters on it."
        if i16::from(o.counters.loyalty) + i16::from(loyalty) < 0 {
            return Ok(false);
        }
        !(o.flags.has(ObjectFlags::ENTERED_THIS_TURN)
            && has_rule(s, t, slot, Rule::InstantLoyaltyTheTurnItEntered))
    } else {
        // CR 602.5d: "Activated abilities that read 'Activate only as a sorcery' mean the
        // player must follow the timing rules for casting a sorcery spell".
        a.timing == Timing::Sorcery
    };
    if sorcery_speed && !sorcery_timing(s, player) {
        return Ok(false);
    }
    if !targetable(s, t, Some(a), player, None, 0, Some(slot)) {
        return Ok(false);
    }
    let cost = a.cost.mana.with_x(0);
    let spent = spent_by_cost(slot, a);
    if a.cost.waterbend > 0 {
        return waterbend_payable(s, t, player, cost, a.cost.waterbend, spent);
    }
    mana::payable(s, t, player, cost, spent)
}

/// Whether `player` can pay `cost`, tapping artifacts and creatures for up to `bendable` of
/// its generic mana (CR 701.67a), with none of `exclude`. Tapping one that makes no mana now
/// pays as much as it ever can, so those pay first; a mana source pays at least as much
/// tapped for mana, so it never needs to.
fn waterbend_payable(
    s: &State,
    t: &GameTable,
    player: Seat,
    mut cost: ManaCost,
    bendable: u8,
    mut exclude: Bitboard,
) -> Result<bool, Full> {
    let free = benders(s, player) & !exclude & !mana::sources(s, t, player, exclude)?;
    for slot in free.iter().take(usize::from(bendable.min(cost.generic))) {
        cost.generic -= 1;
        exclude.insert(slot);
    }
    mana::payable(s, t, player, cost, exclude)
}

/// The untapped artifacts and creatures `player` controls, which may each pay for one
/// generic mana of a waterbend cost (CR 701.67a). Summoning sickness doesn't stop them:
/// tapping them is a cost of the ability, not an ability of theirs with {T} (CR 302.6).
fn benders(s: &State, player: Seat) -> Bitboard {
    s.battlefield()
        .iter()
        .copied()
        .filter(|&slot| {
            s.controller(slot) == player
                && s.derived(slot)
                    .types
                    .intersects(Types::ARTIFACT.union(Types::CREATURE))
                && !s.object(slot).flags.has(ObjectFlags::TAPPED)
        })
        .collect()
}

/// Appends the abilities `player` can activate now. CR 117.1b: "A player may activate an
/// activated ability any time they have priority."
fn activation_options(
    s: &State,
    t: &GameTable,
    player: Seat,
    out: &mut Fixed<Opt, 64>,
) -> Result<(), Full> {
    let mine = s.zone(Seat::P0, Zone::Battlefield) | s.zone(Seat::P1, Zone::Battlefield);
    let objects = mine.iter().filter(|&slot| s.controller(slot) == player);
    let objects: Bitboard = objects.collect::<Bitboard>() | s.zone(player, Zone::Hand);
    for slot in collapse(s, objects).iter() {
        let own = face_of(s, t, slot).abilities.len() as u8;
        let granted = if s.object(slot).zone == Zone::Battlefield {
            s.derived(slot).granted.len() as u8
        } else {
            0
        };
        let abilities = (0..own)
            .map(AbilityIx::Own)
            .chain((0..granted).map(AbilityIx::Granted));
        for ability in abilities {
            if activatable(s, t, player, slot, ability)? {
                out.push(Opt::Activate { slot, ability })?;
            }
        }
    }
    Ok(())
}

/// Appends the priority actions casting and activation add: spells, abilities, and under
/// manual payment, standalone mana abilities.
pub(crate) fn priority_options(
    s: &State,
    t: &GameTable,
    player: Seat,
    out: &mut Fixed<Opt, 64>,
) -> Result<(), Full> {
    cast_options(s, t, player, cast_sources(s, player), From::Priority, out)?;
    activation_options(s, t, player, out)?;
    if s.profile.payment == Payment::Manual {
        mana::standalone_options(s, t, player, out)?;
    }
    Ok(())
}

/// Begins casting a spell: moves it to the stack and starts the machine.
///
/// [CR 601.2a](https://yawgatog.com/resources/magic-rules/#R6012a): "To propose the casting of
/// a spell, a player first moves that card ... from where it is to the stack. It becomes the
/// topmost object on the stack. ... and that player becomes its controller."
///
/// [CR 712.11b](https://yawgatog.com/resources/magic-rules/#R71211b): "A player casting a modal
/// double-faced card ... chooses which face they are casting before putting it onto the
/// stack."
pub(crate) fn begin_cast(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    player: Seat,
    option: Opt,
    resolving: Option<bool>,
) -> Result<(), Full> {
    let Opt::Cast { slot, face, costs } = option else {
        unreachable!("a cast option");
    };
    let permission = permission(s, player, slot);
    event::move_one(s, t, sink, slot, Zone::Stack, Reason::Cast)?;
    let card = s.object(slot).card;
    let o = s.object_mut(slot);
    o.face = face;
    o.base_controller = player;
    s.stack.push(StackEntry {
        object: Some(s.object_ref(slot)),
        controller: player,
        ability: None,
        card,
        face,
        costs,
        exile: permission.is_some_and(|p| p.exile),
        ..StackEntry::default()
    })?;
    let free = resolving.unwrap_or(false);
    let def_face = &t.def(card).faces[usize::from(face)];
    let c = Casting {
        player,
        step: if x_range_start(def_face, costs, free).is_some() {
            CastStep::X
        } else {
            CastStep::TargetCount
        },
        spec: 0,
        left: 0,
        extra: permission.map_or(0, |p| p.extra),
        free,
        resolving: resolving.is_some(),
        remaining: ManaCost::NONE,
        bent: 0,
        triggered: false,
    };
    advance(s, t, sink, c)
}

/// Begins activating an ability: puts it on the stack and starts the machine.
///
/// [CR 602.2a](https://yawgatog.com/resources/magic-rules/#R6022a): "That ability is created
/// on the stack as an object that's not a card. It becomes the topmost object on the stack.
/// ... Its controller is the player who activated the ability."
pub(crate) fn begin_activate(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    player: Seat,
    slot: Slot,
    ability: AbilityIx,
) -> Result<(), Full> {
    let (card, face, index) = activated(s, slot, ability).expect("an ability offered");
    let a = t.def(card).faces[usize::from(face)].ability(index);
    if a.is_loyalty() {
        s.object_mut(slot)
            .flags
            .set(ObjectFlags::LOYALTY_ACTIVATED, true);
    }
    s.stack.push(StackEntry {
        object: Some(s.object_ref(slot)),
        controller: player,
        ability: Some(index),
        card,
        face,
        ..StackEntry::default()
    })?;
    let c = Casting {
        player,
        step: if a.cost.mana.x > 0 {
            CastStep::X
        } else {
            CastStep::TargetCount
        },
        spec: 0,
        left: 0,
        extra: 0,
        free: false,
        resolving: false,
        remaining: ManaCost::NONE,
        bent: 0,
        triggered: false,
    };
    advance(s, t, sink, c)
}

/// Chooses the targets of the triggered ability just put on the stack, if it has any.
///
/// [CR 603.3d](https://yawgatog.com/resources/magic-rules/#R6033d): "The remainder of the
/// process for putting a triggered ability on the stack is identical to the process for
/// casting a spell listed in rules 601.2c-d. If a choice is required when the triggered
/// ability goes on the stack but no legal choices can be made for it, ... the ability is
/// simply removed from the stack."
pub(crate) fn target_trigger(s: &mut State, t: &GameTable, sink: &mut Sink) -> Result<(), Full> {
    let entry = top(s);
    let Some(ability) = entry_ability(t, &entry) else {
        return Ok(());
    };
    if ability.targets.is_empty() {
        return Ok(());
    }
    if !targetable(
        s,
        t,
        Some(ability),
        entry.controller,
        None,
        entry.x,
        source_of(&entry),
    ) {
        s.stack.pop();
        return Ok(());
    }
    let c = Casting {
        player: entry.controller,
        step: CastStep::TargetCount,
        spec: 0,
        left: 0,
        extra: 0,
        free: false,
        resolving: false,
        remaining: ManaCost::NONE,
        bent: 0,
        triggered: true,
    };
    advance(s, t, sink, c)
}

fn top(s: &State) -> StackEntry {
    *s.stack
        .as_slice()
        .last()
        .expect("the spell or ability being cast")
}

fn top_mut(s: &mut State) -> &mut StackEntry {
    s.stack
        .as_mut_slice()
        .last_mut()
        .expect("the spell or ability being cast")
}

/// The target specs of the spell or ability being cast.
fn specs(t: &GameTable, entry: &StackEntry) -> &'static [TargetSpec] {
    entry_ability(t, entry).map_or(&[], |a| a.targets)
}

/// The total mana cost of the spell or ability being cast, once X is announced.
fn total_cost(t: &GameTable, entry: &StackEntry, c: &Casting) -> ManaCost {
    let face = &t.def(entry.card).faces[usize::from(entry.face)];
    match entry.ability {
        None => spell_cost(face, entry.costs, entry.x, c.extra, c.free),
        Some(ability) => {
            let mut cost = face.ability(ability).cost.mana.with_x(entry.x);
            cost.generic -= c.bent;
            cost
        }
    }
}

/// What may be spent paying for the entry being cast, before its other costs are paid: all
/// but a source that pays with itself.
fn spent_by_entry(t: &GameTable, entry: &StackEntry) -> Bitboard {
    match source_of(entry) {
        Some(source) => spent_by_cost(source, entry_ability_of(t, entry)),
        None => Bitboard::EMPTY,
    }
}

/// Moves the machine on to its next question, or finishes.
fn advance(s: &mut State, t: &GameTable, sink: &mut Sink, mut c: Casting) -> Result<(), Full> {
    loop {
        match c.step {
            CastStep::X
            | CastStep::Plan
            | CastStep::Manual
            | CastStep::Crew
            | CastStep::Waterbend => break,
            CastStep::TargetCount | CastStep::Target => {
                let entry = top(s);
                let specs = specs(t, &entry);
                let Some(spec) = specs.get(usize::from(c.spec)) else {
                    if c.triggered {
                        s.pending = Pending::None;
                        return Ok(());
                    }
                    let crew = entry_ability(t, &entry).map_or(0, |a| a.cost.crew);
                    if crew == 0 {
                        return begin_payment(s, t, sink, c);
                    }
                    c.step = CastStep::Crew;
                    c.left = crew;
                    break;
                };
                if c.step == CastStep::TargetCount {
                    // CR 601.2c: "If the spell has a variable number of targets, the player
                    // announces how many targets they will choose before they announce those
                    // targets."
                    if spec.optional {
                        break;
                    }
                    c.left = spec.count;
                    c.step = CastStep::Target;
                }
                if c.left > 0 {
                    break;
                }
                c.spec += 1;
                c.step = CastStep::TargetCount;
            }
        }
    }
    s.pending = Pending::Casting(c);
    Ok(())
}

/// Pays the costs that are not mana, then turns to the mana.
///
/// [CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h): "The player pays the
/// total cost. First, they pay all costs that don't involve random elements or moving objects
/// from the library to a public zone, in any order." The solver never taps a source that pays
/// with itself (`spent_by_entry`), so paying these first changes nothing.
fn begin_payment(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    mut c: Casting,
) -> Result<(), Full> {
    let entry = top(s);
    if !entry.is_spell() {
        let a = entry_ability_of(t, &entry);
        let source = entry.object.expect("an activated ability's source");
        if a.cost.tap {
            event::set_tapped(s, t, sink, source, true)?;
        }
        if let Some(n) = a.cost.loyalty {
            // CR 606.4: "The cost to activate a loyalty ability of a permanent is to put on or
            // remove from that permanent a certain number of loyalty counters".
            let amount = n.unsigned_abs();
            if n > 0 {
                event::add_counters(s, t, sink, source, CounterKind::Loyalty, amount)?;
            } else if n < 0 {
                event::remove_counters(s, t, sink, source, CounterKind::Loyalty, amount)?;
            }
        }
        if a.cost.sacrifice_self {
            event::move_one(s, t, sink, source.slot, Zone::Graveyard, Reason::Sacrifice)?;
        }
        if a.cost.discard_self {
            event::move_one(s, t, sink, source.slot, Zone::Graveyard, Reason::Discard)?;
        }
        if let Some(kind) = a.cost.remove {
            event::remove_counters(s, t, sink, source, kind, 1)?;
        }
        if a.cost.waterbend > 0 {
            c.left = a.cost.waterbend.min(total_cost(t, &entry, &c).generic);
            if c.left > 0 && !waterbend_options(s, t, &c)?.is_empty() {
                c.step = CastStep::Waterbend;
                s.pending = Pending::Casting(c);
                return Ok(());
            }
        }
    }
    pay_mana(s, t, sink, c)
}

/// The answers of a waterbend step (CR 701.67a): tap one more artifact or creature, while the
/// rest stays payable, or pay what is left with mana, when it can be. Interchangeable ones are
/// one answer (§7.2). Empty when no artifact or creature can help.
fn waterbend_options(s: &State, t: &GameTable, c: &Casting) -> Result<Fixed<Opt, 64>, Full> {
    let mut out = Fixed::new();
    let entry = top(s);
    let cost = total_cost(t, &entry, c);
    let mut objects = Fixed::<Opt, 64>::new();
    if c.left > 0 {
        let mut fewer = cost;
        fewer.generic -= 1;
        for slot in collapse(s, benders(s, c.player)).iter() {
            let rest = waterbend_payable(s, t, c.player, fewer, c.left - 1, Bitboard::single(slot));
            if rest? {
                objects.push(Opt::Object(slot))?;
            }
        }
    }
    if objects.is_empty() {
        return Ok(out);
    }
    if mana::payable(s, t, c.player, cost, Bitboard::EMPTY)? {
        out.push(Opt::Done)?;
    }
    for &o in &objects {
        out.push(o)?;
    }
    Ok(out)
}

/// Pays the mana of the total cost: by plan, or one source at a time under manual payment.
fn pay_mana(s: &mut State, t: &GameTable, sink: &mut Sink, mut c: Casting) -> Result<(), Full> {
    let entry = top(s);
    let cost = total_cost(t, &entry, &c);
    match s.profile.payment {
        Payment::Manual => {
            c.remaining = cost;
            if cost.is_zero() {
                return finish(s, t, sink, c);
            }
            c.step = CastStep::Manual;
            s.pending = Pending::Casting(c);
            Ok(())
        }
        payment => {
            let plans = mana::plans(
                s,
                t,
                c.player,
                cost,
                Bitboard::EMPTY,
                payment == Payment::Greedy,
            )?;
            debug_assert!(!plans.is_empty(), "a spell offered is payable");
            if plans.len() > 1 {
                c.step = CastStep::Plan;
                s.pending = Pending::Casting(c);
                return Ok(());
            }
            if let Some(plan) = plans.as_slice().first() {
                mana::pay(s, t, sink, c.player, cost, Bitboard::EMPTY, plan)?;
            }
            finish(s, t, sink, c)
        }
    }
}

/// The spell becomes cast, or the ability activated.
///
/// [CR 601.2i](https://yawgatog.com/resources/magic-rules/#R6012i): "Any abilities that
/// trigger when a spell is cast or put onto the stack trigger at this time. If the spell's
/// controller had priority before casting it, they get priority." CR 608.2g: after a spell
/// cast during a resolution, "no player receives priority"; the resolution it was cast in is
/// over by now, so the active player receives priority (CR 117.3b).
fn finish(s: &mut State, t: &GameTable, sink: &mut Sink, c: Casting) -> Result<(), Full> {
    s.pending = Pending::None;
    s.turn.priority = if c.resolving { s.turn.active } else { c.player };
    s.turn.passes = 0;
    let entry = top(s);
    if let Some(spell) = entry.spell() {
        event::settle(
            s,
            t,
            sink,
            Event::Cast {
                spell,
                controller: entry.controller,
            },
        )?;
    }
    Ok(())
}

/// The X values the payment can still cover, from the least allowed.
fn x_options(s: &State, t: &GameTable, c: &Casting) -> Result<Fixed<Opt, 64>, Full> {
    let entry = top(s);
    let face = &t.def(entry.card).faces[usize::from(entry.face)];
    let least = if entry.is_spell() {
        x_range_start(face, entry.costs, c.free).unwrap_or(0)
    } else {
        0
    };
    let exclude = spent_by_entry(t, &entry);
    let mut out = Fixed::new();
    for x in least..(least.saturating_add(64)) {
        let mut probe = entry;
        probe.x = x;
        if !mana::payable(s, t, c.player, total_cost(t, &probe, c), exclude)? {
            break;
        }
        out.push(Opt::Number(x))?;
    }
    Ok(out)
}

/// The objects or players the current target spec can still choose: not one already chosen
/// for it.
fn target_options(s: &State, t: &GameTable, c: &Casting) -> TargetSet {
    let entry = top(s);
    let spec = &specs(t, &entry)[usize::from(c.spec)];
    let itself = entry.spell().map(|r| r.slot);
    match candidates(s, t, spec, c.player, itself, entry.x, source_of(&entry)) {
        TargetSet::Objects(mut b) => {
            for target in entry.targets.iter().filter(|x| x.spec == c.spec) {
                if let Some(r) = target.as_object() {
                    b.remove(r.slot);
                }
            }
            TargetSet::Objects(b)
        }
        TargetSet::Players(mut p) => {
            for target in entry.targets.iter().filter(|x| x.spec == c.spec) {
                if let Some(player) = target.as_player() {
                    p &= !(1 << player.get());
                }
            }
            TargetSet::Players(p)
        }
    }
}

/// The open question of the machine.
pub(crate) fn decision(s: &State, t: &GameTable, c: &Casting) -> Result<Decision, Full> {
    let options = |kind: DecisionKind, list: Fixed<Opt, 64>| Decision {
        player: c.player,
        kind,
        candidates: Candidates::Options(list),
    };
    Ok(match c.step {
        CastStep::X => options(DecisionKind::X, x_options(s, t, c)?),
        CastStep::TargetCount => {
            let entry = top(s);
            let spec = &specs(t, &entry)[usize::from(c.spec)];
            let most = target_options(s, t, c).count().min(u32::from(spec.count));
            let mut list = Fixed::new();
            for n in 0..=most {
                list.push(Opt::Number(n as u8))?;
            }
            options(DecisionKind::TargetCount, list)
        }
        CastStep::Target => match target_options(s, t, c) {
            TargetSet::Objects(b) => Decision {
                player: c.player,
                kind: DecisionKind::Target,
                candidates: Candidates::Objects(collapse(s, b)),
            },
            TargetSet::Players(p) => {
                let mut list = Fixed::new();
                for player in Seat::BOTH.into_iter().filter(|q| p & (1 << q.get()) != 0) {
                    list.push(Opt::Player(player))?;
                }
                options(DecisionKind::Target, list)
            }
        },
        CastStep::Plan => {
            let entry = top(s);
            let plans = mana::plans(
                s,
                t,
                c.player,
                total_cost(t, &entry, c),
                Bitboard::EMPTY,
                false,
            )?;
            let mut list = Fixed::new();
            for i in 0..plans.len() {
                list.push(Opt::Plan(i as u8))?;
            }
            options(DecisionKind::PaymentPlan, list)
        }
        CastStep::Crew => Decision {
            player: c.player,
            kind: DecisionKind::Crew,
            candidates: Candidates::Objects(collapse(
                s,
                crew_candidates(s, c.player, source_of(&top(s)).expect("a Vehicle to crew")),
            )),
        },
        CastStep::Manual => options(
            DecisionKind::ManaPayment,
            mana::manual_options(s, t, c.player, c.remaining, Bitboard::EMPTY)?,
        ),
        CastStep::Waterbend => options(DecisionKind::Waterbend, waterbend_options(s, t, c)?),
    })
}

/// Applies an answer to the machine's question: an option, or an object for a target.
pub(crate) fn perform(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    mut c: Casting,
    option: Option<Opt>,
    object: Option<Slot>,
) -> Result<(), Full> {
    match c.step {
        CastStep::X => {
            let Some(Opt::Number(x)) = option else {
                unreachable!("an X answer");
            };
            top_mut(s).x = x;
            c.step = CastStep::TargetCount;
        }
        CastStep::TargetCount => {
            let Some(Opt::Number(n)) = option else {
                unreachable!("a target count");
            };
            c.left = n;
            c.step = CastStep::Target;
        }
        CastStep::Target => {
            let target = match (option, object) {
                (Some(Opt::Player(player)), _) => Target::player(c.spec, player),
                (_, Some(slot)) => Target::object(c.spec, s.object_ref(slot)),
                _ => unreachable!("a target"),
            };
            top_mut(s).targets.push(target)?;
            c.left -= 1;
        }
        CastStep::Plan => {
            let Some(Opt::Plan(i)) = option else {
                unreachable!("a payment plan");
            };
            let entry = top(s);
            let cost = total_cost(t, &entry, &c);
            let plans = mana::plans(s, t, c.player, cost, Bitboard::EMPTY, false)?;
            mana::pay(
                s,
                t,
                sink,
                c.player,
                cost,
                Bitboard::EMPTY,
                &plans.as_slice()[usize::from(i)],
            )?;
            return finish(s, t, sink, c);
        }
        CastStep::Crew => {
            // CR 702.122b: "A creature 'crews a Vehicle' when it's tapped to pay the cost to
            // activate a Vehicle's crew ability."
            let slot = object.expect("a creature to crew with");
            let power = power_of(s, slot);
            event::set_tapped(s, t, sink, s.object_ref(slot), true)?;
            c.left = c.left.saturating_sub(power);
            if c.left == 0 {
                return begin_payment(s, t, sink, c);
            }
        }
        CastStep::Waterbend => {
            // CR 701.67a: "you may tap an untapped artifact or creature you control rather
            // than pay that mana."
            let Some(Opt::Object(slot)) = option else {
                return pay_mana(s, t, sink, c);
            };
            event::set_tapped(s, t, sink, s.object_ref(slot), true)?;
            c.bent += 1;
            c.left -= 1;
            if waterbend_options(s, t, &c)?.is_empty() {
                return pay_mana(s, t, sink, c);
            }
            s.pending = Pending::Casting(c);
            return Ok(());
        }
        CastStep::Manual => {
            let option = option.expect("a manual payment");
            c.remaining = mana::manual_pay(s, t, sink, c.player, c.remaining, option)?;
            if c.remaining.is_zero() {
                return finish(s, t, sink, c);
            }
        }
    }
    advance(s, t, sink, c)
}

/// The options of a resolution's offer to cast a card (CR 608.2g): not casting it, then each
/// way of casting it.
pub(crate) fn offer_options(
    s: &State,
    t: &GameTable,
    object: ObjectRef,
    player: Seat,
    free: bool,
) -> Result<Fixed<Opt, 64>, Full> {
    let mut out = Fixed::new();
    out.push(Opt::Pass)?;
    if s.is_current(object) {
        let mut cards = Bitboard::EMPTY;
        cards.insert(object.slot);
        cast_options(s, t, player, cards, From::Resolution { free }, &mut out)?;
    }
    Ok(out)
}

/// Removes a spell from the stack to its owner's graveyard, or to exile when `exile` or its
/// entry says so. Returns whether it was there.
pub(crate) fn counter(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    spell: ObjectRef,
    exile: bool,
) -> Result<bool, Full> {
    let Some(entry) = take_spell(s, spell) else {
        return Ok(false);
    };
    leave_stack(s, t, sink, &entry, exile, Reason::Counter)?;
    Ok(true)
}

/// Removes a spell's entry from the stack, if it is there, for an effect that moves the
/// spell's card elsewhere.
pub(crate) fn take_spell(s: &mut State, spell: ObjectRef) -> Option<StackEntry> {
    let i = s.stack.iter().position(|e| e.spell() == Some(spell))?;
    Some(s.stack.remove(i))
}

/// Puts a spell that leaves the stack without becoming a permanent into its owner's
/// graveyard, or exile when `exile` or its entry says so.
pub(crate) fn leave_stack(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    entry: &StackEntry,
    exile: bool,
    reason: Reason,
) -> Result<(), Full> {
    let to = if exile || entry.exile {
        Zone::Exile
    } else {
        Zone::Graveyard
    };
    let spell = entry.spell().expect("a spell leaving the stack");
    event::move_one(s, t, sink, spell.slot, to, reason)
}

/// The objects a pending reference names: stack targets and cast permissions. They are never
/// interchangeable with another object (§7.2).
pub(crate) fn referenced(s: &State) -> Bitboard {
    let mut b = Bitboard::EMPTY;
    for e in &s.stack {
        for target in &e.targets {
            if let Some(r) = target.as_object() {
                b.insert(r.slot);
            }
        }
    }
    for p in &s.permissions {
        b.insert(p.object.slot);
    }
    b
}
