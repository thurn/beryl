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

use std::cell::{Cell, OnceCell};

use crate::bitboard::Bitboard;
use crate::card::{
    Ability, AbilityKind, CastingOption, Castings, Colors, Face, FaceAbility, FaceIx, Keywords,
    ManaCost, Needs, ObjectAbility, Part, Prompt, Purpose, Rule, TargetKind, TargetSpec, Timing,
    Types,
};
use crate::decision::{Candidates, Decision, DecisionKind, Opt, Refusal};
use crate::engine::{collapse, collapse_in_order};
use crate::event::{self, Event, Reason, Sink};
use crate::filter::{self, Evaluate, Filter};
use crate::fixed::Full;
use crate::ids::{CardIx, Seat, SlotId};
use crate::layout::{Layout, Queue, Small, Table, index};
use crate::mana::{self, Funds};
use crate::profile::Payment;
use crate::state::{
    CastPermission, CastStep, Casting, CounterKind, ObjectFlags, ObjectRef, Pending, StackEntry,
    State, Target, TurnStep, Zone, ZoneKey,
};
use crate::table::GameTable;

/// The ability a stack entry resolves: an ability of its card's face, or a spell's spell
/// ability, as the casting options paid for it make it (an overloaded spell's). A permanent
/// spell with no targets has none.
pub(crate) fn entry_ability<L: Layout>(
    t: &GameTable,
    entry: &StackEntry<L>,
) -> Option<&'static Ability> {
    let face = t.def(entry.card).face(entry.face);
    match entry.ability {
        None => face.spell_ability_with(entry.costs),
        Some(ability) => Some(face.ability(ability)),
    }
}

/// The ability of a stack entry that is an ability, by its card, face and index.
fn entry_ability_of<L: Layout>(t: &GameTable, entry: &StackEntry<L>) -> &'static Ability {
    entry_ability(t, entry).expect("an ability's entry")
}

/// The card, face and ability a stack entry records for activating ability `ability` of the
/// permanent in `slot`: one of its own, or one granted to it, which is printed on the
/// granting card's front face (§9).
fn activated<L: Layout>(
    s: &State<L>,
    slot: L::Slot,
    ability: ObjectAbility,
) -> Option<(CardIx, FaceIx, FaceAbility)> {
    let o = s.object(slot);
    match ability {
        ObjectAbility::Own(i) => Some((o.card, o.face, FaceAbility::Own(i))),
        ObjectAbility::Granted(k) => {
            if o.zone != Zone::Battlefield {
                return None;
            }
            let grant = s.derived(slot).granted[usize::from(k)]?;
            Some((grant.card, FaceIx::FRONT, FaceAbility::Grant(grant.grant)))
        }
    }
}

/// Whether `player` may cast a sorcery now
/// ([CR 307.1](https://yawgatog.com/resources/magic-rules/#R3071)): "A player who has
/// priority may cast a sorcery card from their hand during a main phase of their turn when the
/// stack is empty."
pub(crate) fn sorcery_timing<L: Layout>(s: &State<L>, player: Seat) -> bool {
    player == s.turn().active()
        && matches!(
            s.turn().step(),
            TurnStep::PrecombatMain | TurnStep::PostcombatMain
        )
        && s.stack().is_empty()
}

/// Whether a permanent on the battlefield has a static ability with this rule.
#[inline]
fn has_rule<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot, rule: Rule) -> bool {
    let o = s.object(slot);
    let has = t.may_have_rule(o.card, rule) && t.facts(o.card, o.face).has_rule(rule);
    #[cfg(feature = "coverage")]
    if has
        && let Some(a) = crate::trigger::face(s, t, slot)
            .abilities
            .iter()
            .find(|a| a.rule == rule)
    {
        crate::coverage::hit(a);
    }
    has
}

/// The first of an opponent's permanents that limits `player` to casting spells at sorcery
/// speed, as Teferi, Time Raveler does, if one does. A prohibition beats a permission
/// ([CR 101.2](https://yawgatog.com/resources/magic-rules/#R1012)): "When a rule or effect
/// allows or directs something to happen, and another effect states that it can't happen,
/// the "can't" effect takes precedence."
fn sorcery_speed_limiter<L: Layout>(s: &State<L>, t: &GameTable, player: Seat) -> Option<L::Slot> {
    // A permanent's derived characteristics, its controller among them, are at its place in
    // the battlefield list.
    s.permanents()
        .find(|&(slot, d)| {
            filter::is_an_opponent(player, d.controller)
                && has_rule(s, t, slot, Rule::OpponentsCastAsSorceries)
        })
        .map(|(slot, _)| slot)
}

/// Whether `player` may cast this face at instant speed: an instant
/// ([CR 117.1a](https://yawgatog.com/resources/magic-rules/#R1171a)), a face with flash
/// ([CR 702.8a](https://yawgatog.com/resources/magic-rules/#R7028a)), or a sorcery while an
/// effect lets them cast sorceries as though they had flash.
fn instant_speed<L: Layout>(s: &State<L>, face: &Face, player: Seat) -> bool {
    face.types.intersects(Types::INSTANT)
        || face.keywords.contains(Keywords::FLASH)
        || (face.types.intersects(Types::SORCERY) && s.player(player).sorceries_as_flash)
}

/// The permission that lets `player` cast the card in `slot`, if one does.
fn permission<L: Layout>(s: &State<L>, player: Seat, slot: L::Slot) -> Option<CastPermission<L>> {
    s.permissions()
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

/// The sets of a face's casting options that may be chosen together: any of the additional
/// costs, and at most one alternative cost
/// ([CR 118.9a](https://yawgatog.com/resources/magic-rules/#R1189a)): "Only one alternative
/// cost can be applied to any one spell as it's being cast." Casting without paying the mana
/// cost is itself an alternative cost, and so is a flashback cost, so `alternative` (one of
/// them is paid) excludes the face's own.
fn option_sets(face: &Face, alternative: bool) -> impl Iterator<Item = Castings> + '_ {
    Castings::subsets(face).filter(move |&costs| {
        let alternatives = costs.paid(face).filter(|o| o.is_alternative()).count();
        alternatives <= usize::from(!alternative)
    })
}

/// A cost paid by exiling cards ([CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h)):
/// "The player pays the total cost." No face has two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExileCost {
    /// A card of one of these colors from the caster's hand, as evoke's "Exile a blue card
    /// from your hand".
    Hand(Colors),
    /// This many cards from the caster's graveyard, as Abhorrent Oculus's "exile six cards
    /// from your graveyard".
    Graveyard(u8),
}

impl ExileCost {
    /// The cost that exiles cards of `face` cast with `costs`, if it has one.
    fn of(face: &Face, costs: Castings) -> Option<ExileCost> {
        let colors = face.exiles_with(costs);
        if colors != Colors::NONE {
            debug_assert_eq!(face.exile_from_graveyard, 0, "two costs that exile");
            return Some(ExileCost::Hand(colors));
        }
        (face.exile_from_graveyard > 0).then_some(ExileCost::Graveyard(face.exile_from_graveyard))
    }

    /// How many cards it exiles.
    fn count(self) -> u8 {
        match self {
            ExileCost::Hand(_) => 1,
            ExileCost::Graveyard(n) => n,
        }
    }

    /// Whether `player` has the cards to pay it, the spell in `itself` aside: its caster
    /// moves it to the stack before paying (CR 601.2a), so it can't pay for itself.
    fn payable<L: Layout>(
        self,
        s: &State<L>,
        t: &GameTable,
        player: Seat,
        itself: L::Slot,
    ) -> bool {
        match self {
            ExileCost::Hand(colors) => !hand_cards(s, t, player, colors, itself).is_empty(),
            ExileCost::Graveyard(n) => {
                let mut graveyard = s.zone(ZoneKey::Of(player, Zone::Graveyard));
                graveyard.remove(itself);
                graveyard
                    .iter()
                    .filter(|&slot| !s.object(slot).flags.has(ObjectFlags::TOKEN))
                    .count()
                    >= usize::from(n)
            }
        }
    }
}

/// The cards in `player`'s hand other than `itself` of one of `colors`.
fn hand_cards<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    colors: Colors,
    itself: L::Slot,
) -> Bitboard<L::Slot> {
    let mut hand = s.zone(ZoneKey::Of(player, Zone::Hand));
    hand.remove(itself);
    hand.iter()
        .filter(|&slot| t.def(s.object(slot).card).front().colors.intersects(colors))
        .collect()
}

/// The cards the exile step may choose next, one of each set of interchangeable ones (§7.2).
/// From a graveyard: the cards from position `c.spec` up that leave enough above them for the
/// `c.left - 1` still to choose after, the lowest of interchangeable ones, which leaves every
/// choice the others would. So every set of cards is one sequence of answers, and a graveyard
/// with no more cards than the cost exiles asks nothing.
fn exile_choices<L: Layout>(s: &State<L>, t: &GameTable, c: &Casting) -> Bitboard<L::Slot> {
    let entry = top(s);
    let face = t.def(entry.card).face(entry.face);
    let itself = entry.object.expect("a spell's card").slot;
    match ExileCost::of(face, entry.costs).expect("a cost that exiles") {
        ExileCost::Hand(colors) => collapse(s, hand_cards(s, t, c.player, colors, itself)),
        ExileCost::Graveyard(_) => {
            let graveyard = s.graveyard(c.player);
            // A sacrificed mana token remains here until state-based actions, but
            // [CR 111.6](https://yawgatog.com/resources/magic-rules/#R1116):
            // "A token isn't a card". Count and offer only cards for this cost.
            let card = |slot| !s.object(slot).flags.has(ObjectFlags::TOKEN);
            let last = graveyard
                .iter()
                .enumerate()
                .rev()
                .filter(|&(_, &slot)| card(slot))
                .nth(usize::from(c.left) - 1)
                .expect("enough cards to pay the cost")
                .0;
            collapse_in_order(
                s,
                graveyard[usize::from(c.spec)..=last]
                    .iter()
                    .copied()
                    .filter(|&slot| card(slot)),
            )
        }
    }
}

/// Whether X is announced for this cast, and the least value it may take.
///
/// [CR 107.3b](https://yawgatog.com/resources/magic-rules/#R1073b): an effect that lets a
/// player cast a spell "while paying neither its mana cost nor an alternative cost that
/// includes X, then the only legal choice for X is 0."
fn x_range_start(face: &Face, costs: Castings, free: bool) -> Option<u8> {
    let alternative = costs.paid(face).any(CastingOption::is_alternative);
    let in_mana_cost = face.cost.x > 0 && !free && !alternative;
    let mut min = None;
    for o in costs.paid(face) {
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
fn spell_cost(face: &Face, costs: Castings, x: u8, extra: u8, free: bool) -> ManaCost {
    let mut cost = if free { ManaCost::NONE } else { face.cost };
    for o in costs.paid(face) {
        if o.is_alternative() {
            cost = o.cost();
        }
    }
    for o in costs.paid(face).filter(|o| !o.is_alternative()) {
        cost = cost.plus(o.cost());
    }
    cost.plus(ManaCost::generic(extra)).with_x(x)
}

/// The objects or players a target spec can choose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TargetSet<L: Layout = Small> {
    Objects(Bitboard<L::Slot>),
    /// A bit per player.
    Players(u8),
}

fn player_matches(filter: Filter, player: Seat, you: Seat) -> bool {
    match filter {
        Filter::Any => true,
        Filter::Whose(whose) => filter::is_whose(whose, player, you),
        _ => unreachable!("a player filter is Any or Whose"),
    }
}

/// The spells on the stack, as objects.
pub(crate) fn spells<L: Layout>(s: &State<L>) -> Bitboard<L::Slot> {
    s.stack()
        .iter()
        .filter_map(StackEntry::spell)
        .map(|r| r.slot)
        .collect()
}

pub(crate) fn in_zone<L: Layout>(s: &State<L>, zone: Zone) -> Bitboard<L::Slot> {
    s.zone(ZoneKey::Of(Seat::P0, zone)) | s.zone(ZoneKey::Of(Seat::P1, zone))
}

/// The objects or players a target spec can choose now, for a spell or ability controlled by
/// `you`. `itself` is the spell being cast, if one is, which is an illegal target for itself
/// ([CR 115.5](https://yawgatog.com/resources/magic-rules/#R1155)): "A spell or ability on the
/// stack is an illegal target for itself." Before it is cast it is not yet on the stack, but
/// it will have left the zone it is in when its targets are chosen. `x` is the value of X of
/// the spell or ability. `source` is an ability's source, which "another target" can't
/// choose.
fn candidates<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    spec: &TargetSpec,
    you: Seat,
    itself: Option<L::Slot>,
    x: u8,
    source: Option<L::Slot>,
) -> TargetSet<L> {
    match domain(s, spec, you, itself, source) {
        TargetSet::Objects(within) => {
            TargetSet::Objects(spec.filter.select_x(s, t, within, you, x))
        }
        players @ TargetSet::Players(_) => players,
    }
}

/// What [`candidates`] chooses from: the players a spec can choose, or the objects its filter
/// is still to pass on.
fn domain<L: Layout>(
    s: &State<L>,
    spec: &TargetSpec,
    you: Seat,
    itself: Option<L::Slot>,
    source: Option<L::Slot>,
) -> TargetSet<L> {
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
    TargetSet::Objects(within)
}

/// Whether [`candidates`] holds at least `spec.count` targets, found without testing the
/// filter on more objects than that takes.
fn enough_candidates<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    spec: &TargetSpec,
    you: Seat,
    itself: Option<L::Slot>,
    x: u8,
    source: Option<L::Slot>,
) -> bool {
    let need = usize::from(spec.count);
    match domain(s, spec, you, itself, source) {
        TargetSet::Objects(within) => {
            within
                .iter()
                .filter(|&slot| spec.filter.matches_x(s, t, slot, you, x))
                .take(need)
                .count()
                >= need
        }
        TargetSet::Players(players) => players.count_ones() as usize >= need,
    }
}

impl<L: Layout> TargetSet<L> {
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
fn targetable<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    ability: Option<&Ability>,
    you: Seat,
    itself: Option<L::Slot>,
    x: u8,
    source: Option<L::Slot>,
) -> bool {
    ability.is_none_or(|a| {
        a.targets
            .iter()
            .all(|spec| spec.optional || enough_candidates(s, t, spec, you, itself, x, source))
    })
}

/// The slot of an entry's ability source; a spell has none.
fn source_of<L: Layout>(entry: &StackEntry<L>) -> Option<L::Slot> {
    entry.object.filter(|_| !entry.is_spell()).map(|r| r.slot)
}

/// Whether a chosen target is still legal as its spell or ability resolves
/// ([CR 608.2b](https://yawgatog.com/resources/magic-rules/#R6082b)): "A target that's no
/// longer in the zone it was in when it was targeted is illegal. Other changes to the game
/// state may cause a target to no longer be legal". A zone change gives the object a new
/// incarnation, so a stale reference is illegal.
pub(crate) fn still_legal<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    entry: &StackEntry<L>,
    ability: &Ability,
    target: Target<L>,
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

/// What every check of one player's options reads, found once for all of them: the mana they
/// could pay with and whether an opponent limits them to sorcery speed, each found at the first
/// check that needs it.
struct Situation<L: Layout = Small> {
    player: Seat,
    /// None once found if more classes of sources than [`Funds`] holds: a stall for whatever
    /// needed them.
    funds: OnceCell<Option<Funds<L>>>,
    full: Cell<bool>,
    sorcery_speed_only: OnceCell<bool>,
}

impl<L: Layout> Situation<L> {
    fn of(player: Seat) -> Situation<L> {
        Situation {
            player,
            funds: OnceCell::new(),
            full: Cell::new(false),
            sorcery_speed_only: OnceCell::new(),
        }
    }

    /// Whether an opponent's permanent limits the player to sorcery speed: read only for a
    /// face that could otherwise be cast at instant speed.
    fn sorcery_speed_only(&self, s: &State<L>, t: &GameTable) -> bool {
        *self
            .sorcery_speed_only
            .get_or_init(|| sorcery_speed_limiter(s, t, self.player).is_some())
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
    fn complete(&self) -> Result<(), Full> {
        if self.full.get() { Err(Full) } else { Ok(()) }
    }
}

/// Whether a face may be cast now, as far as its card types and the timing rules say: what
/// [`refused`] checks first, which depends only on the face, so on the card.
fn in_time<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L>,
    face: &Face,
    from: From,
) -> bool {
    // CR 305.9: "If an object is both a land and another card type, it can be played only as
    // a land. It can't be cast as a spell." CR 712.11c: "Only the face that will be face up
    // on the stack is evaluated to determine if it can be cast."
    if face.types.intersects(Types::LAND) {
        return false;
    }
    match from {
        From::Priority => {
            (instant_speed(s, face, now.player) && !now.sorcery_speed_only(s, t))
                || sorcery_timing(s, now.player)
        }
        // During a resolution the stack is never empty, so a sorcery-speed limit forbids it.
        From::Resolution { .. } => !now.sorcery_speed_only(s, t),
    }
}

/// Why `player` may not cast the card in `slot` with this face and these casting options:
/// the first check it fails, or none if they may. A timing refusal names no permanent here
/// ([`refusal`] does).
///
/// [CR 601.3](https://yawgatog.com/resources/magic-rules/#R6013): "A player can begin to cast
/// a spell only if a rule or effect allows that player to cast it and no rule or effect
/// prohibits that player from casting it."
fn refused<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L>,
    slot: L::Slot,
    face_index: FaceIx,
    costs: Castings,
    from: From,
) -> Option<Refusal<L>> {
    let player = now.player;
    let face = t.def(s.object(slot).card).face(face_index);
    if !in_time(s, t, now, face, from) {
        return Some(Refusal::Timing { by: None });
    }
    let (extra, free) = match from {
        From::Priority => (permission(s, player, slot).map_or(0, |p| p.extra), false),
        From::Resolution { free } => (0, free),
    };
    // The checks are pure, so their order changes nothing but their cost: the mana, found
    // once for the decision, first, and the targets, which read the battlefield, last.
    // [CR 118.3](https://yawgatog.com/resources/magic-rules/#R1183): "A player can't pay a
    // cost without having the necessary resources to pay it fully."
    let x = x_range_start(face, costs, free).unwrap_or(0);
    let cost = spell_cost(face, costs, x, extra, free);
    if !now
        .funds(s, t)
        .is_some_and(|f| f.pays(s, t, cost, Bitboard::EMPTY))
    {
        return Some(Refusal::Mana);
    }
    // CR 601.2h: the player pays the total cost, so the cards to exile must be there.
    if ExileCost::of(face, costs).is_some_and(|cost| !cost.payable(s, t, player, slot)) {
        return Some(Refusal::Cost);
    }
    (!targetable(
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
    let def = t.def(s.object(slot).card);
    let flashback = permission(s, player, slot).is_some_and(|p| p.flashback);
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
        let instant = def.face_ixs().any(|f| {
            let face = def.face(f);
            !face.transformed && instant_speed(s, face, player)
        });
        if instant {
            most = Some(Refusal::Timing {
                by: sorcery_speed_limiter(s, t, player),
            });
        }
    }
    now.complete()?;
    Ok(most)
}

/// The cards `player` can cast at priority: their hand and the cards a permission names.
fn cast_sources<L: Layout>(s: &State<L>, player: Seat) -> Bitboard<L::Slot> {
    let mut cards = s.zone(ZoneKey::Of(player, Zone::Hand));
    for p in s.permissions() {
        if p.player == player && s.is_current(p.object) {
            cards.insert(p.object.slot);
        }
    }
    cards
}

/// Appends the spells `player` can cast now, by card, face and casting options.
fn cast_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L>,
    cards: Bitboard<L::Slot>,
    from: From,
    out: &mut L::Queue<Opt<L>, 64>,
) -> Result<(), Full> {
    let player = now.player;
    let free = matches!(from, From::Resolution { free: true });
    // Cards no face of which may be cast now are left out before collapsing, which saves
    // comparing them: whether one may depends only on its card, which interchangeable cards
    // share, so the cards kept are the same.
    let cards: Bitboard<L::Slot> = cards
        .iter()
        .filter(|&slot| {
            let def = t.def(s.object(slot).card);
            def.face_ixs().any(|f| {
                let face = def.face(f);
                !face.transformed && in_time(s, t, now, face, from)
            })
        })
        .collect();
    for slot in collapse(s, cards).iter() {
        let def = t.def(s.object(slot).card);
        let flashback =
            from == From::Priority && permission(s, player, slot).is_some_and(|p| p.flashback);
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
fn spent_by_cost<S: SlotId>(slot: S, a: &Ability) -> Bitboard<S> {
    let mut b = Bitboard::EMPTY;
    if a.cost.tap || a.cost.sacrifice_self {
        b.insert(slot);
    }
    b
}

/// The other untapped creatures `player` controls, which may crew the Vehicle in `vehicle`
/// (CR 702.122a). Summoning sickness doesn't stop them: tapping them is a cost of the
/// Vehicle's ability, not an ability of theirs with {T} (CR 302.6).
fn crew_candidates<L: Layout>(s: &State<L>, player: Seat, vehicle: L::Slot) -> Bitboard<L::Slot> {
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
fn power_of<L: Layout>(s: &State<L>, slot: L::Slot) -> u8 {
    u8::try_from(s.derived(slot).power.max(0)).unwrap_or(0)
}

/// The total power of the creatures that could crew the Vehicle in `vehicle`.
fn crew_power<L: Layout>(s: &State<L>, player: Seat, vehicle: L::Slot) -> u32 {
    crew_candidates(s, player, vehicle)
        .iter()
        .map(|slot| u32::from(power_of(s, slot)))
        .sum()
}

/// Whether `player` may activate ability `ability` of the object in `slot` now.
///
/// [CR 602.5](https://yawgatog.com/resources/magic-rules/#R6025): "A player can't begin to
/// activate an ability that's prohibited from being activated."
fn activatable<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L>,
    slot: L::Slot,
    ability: ObjectAbility,
) -> bool {
    let player = now.player;
    let o = s.object(slot);
    let Some((card, face, index)) = activated(s, slot, ability) else {
        return false;
    };
    let a = t.def(card).face(face).ability(index);
    if a.kind != AbilityKind::Activated {
        return false;
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
        return false;
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
            return false;
        }
    }
    if a.cost.crew > 0 && crew_power(s, player, slot) < u32::from(a.cost.crew) {
        return false;
    }
    // CR 118.3: "A player can't pay a cost without having the necessary resources to pay it
    // fully."
    if a.cost.remove.is_some_and(|kind| o.counters.get(kind) == 0) {
        return false;
    }
    let sorcery_speed = if let Some(loyalty) = a.cost.loyalty {
        // CR 606.3: "A player may activate a loyalty ability of a permanent they control any
        // time they have priority and the stack is empty during a main phase of their turn,
        // but only if no player has previously activated a loyalty ability of that permanent
        // that turn."
        if o.flags.has(ObjectFlags::LOYALTY_ACTIVATED) {
            return false;
        }
        // CR 606.6: "A loyalty ability with a negative loyalty cost ... can't be activated
        // unless the permanent has at least that many loyalty counters on it."
        if i16::from(o.counters.loyalty) + i16::from(loyalty) < 0 {
            return false;
        }
        !(o.flags.has(ObjectFlags::ENTERED_THIS_TURN)
            && has_rule(s, t, slot, Rule::InstantLoyaltyTheTurnItEntered))
    } else {
        // CR 602.5d: "Activated abilities that read 'Activate only as a sorcery' mean the
        // player must follow the timing rules for casting a sorcery spell".
        a.timing == Timing::Sorcery
    };
    if sorcery_speed && !sorcery_timing(s, player) {
        return false;
    }
    // The mana before the targets, which read the battlefield: both checks are pure.
    let cost = a.cost.mana.with_x(0);
    let spent = spent_by_cost(slot, a);
    let paid = now.funds(s, t).is_some_and(|funds| {
        if a.cost.waterbend > 0 {
            waterbend_payable(s, t, funds, player, cost, a.cost.waterbend, spent)
        } else {
            funds.pays(s, t, cost, spent)
        }
    });
    paid && targetable(s, t, Some(a), player, None, 0, Some(slot))
}

/// Whether `player` can pay `cost`, tapping artifacts and creatures for up to `bendable` of
/// its generic mana (CR 701.67a), with none of `exclude`. Tapping one that makes no mana now
/// pays as much as it ever can, so those pay first; a mana source pays at least as much
/// tapped for mana, so it never needs to.
fn waterbend_payable<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    funds: &Funds<L>,
    player: Seat,
    mut cost: ManaCost,
    bendable: u8,
    mut exclude: Bitboard<L::Slot>,
) -> bool {
    let free = benders(s, player) & !exclude & !funds.sources(exclude);
    for slot in free.iter().take(usize::from(bendable.min(cost.generic))) {
        cost.generic -= 1;
        exclude.insert(slot);
    }
    funds.pays(s, t, cost, exclude)
}

/// The untapped artifacts and creatures `player` controls, which may each pay for one
/// generic mana of a waterbend cost (CR 701.67a). Summoning sickness doesn't stop them:
/// tapping them is a cost of the ability, not an ability of theirs with {T} (CR 302.6).
fn benders<L: Layout>(s: &State<L>, player: Seat) -> Bitboard<L::Slot> {
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

/// Whether activating `ability` of the object in `slot` now would do nothing, as its card
/// declares ([`Needs`]): its cost would be paid and its effect find nothing to act on. The
/// activation is still legal (CR 602.2); [`Profile::hides_pointless`](crate::Profile) says
/// who isn't offered it.
pub fn pointless<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    ability: ObjectAbility,
) -> bool {
    let Some((card, face, index)) = activated(s, slot, ability) else {
        return false;
    };
    match t.def(card).face(face).ability(index).needs {
        Needs::Nothing => false,
        // CR 607.2a: a linked ability finds only the cards exiled with its own source.
        Needs::ExiledWithSource => s.exiled_with(slot).is_empty(),
    }
}

/// Appends the abilities `player` can activate now. CR 117.1b: "A player may activate an
/// activated ability any time they have priority."
fn activation_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L>,
    out: &mut L::Queue<Opt<L>, 64>,
) -> Result<(), Full> {
    let player = now.player;
    // Activations that would do nothing, left out for the player the profile hides them from,
    // where a card of the game has one.
    let prunes = s.profile().hides_pointless[player.index()] && t.needy();
    let mine = s.zone(ZoneKey::Of(Seat::P0, Zone::Battlefield))
        | s.zone(ZoneKey::Of(Seat::P1, Zone::Battlefield));
    let objects = mine.iter().filter(|&slot| s.controller(slot) == player);
    let objects: Bitboard<L::Slot> =
        objects.collect::<Bitboard<L::Slot>>() | s.zone(ZoneKey::Of(player, Zone::Hand));
    // Most objects have no activated ability; leaving them out before collapsing saves
    // comparing them. Whether one has is part of what collapsing compares (its card, face
    // and granted abilities), so the result is the same.
    let activated = |slot: L::Slot| {
        let o = s.object(slot);
        t.facts(o.card, o.face).activated
    };
    let objects: Bitboard<L::Slot> = objects
        .iter()
        .filter(|&slot| {
            activated(slot) != 0
                || (s.object(slot).zone == Zone::Battlefield
                    && s.derived(slot).granted.iter().any(Option::is_some))
        })
        .collect();
    for slot in collapse(s, objects).iter() {
        let granted = if s.object(slot).zone == Zone::Battlefield {
            s.derived(slot).granted.len() as u8
        } else {
            0
        };
        // Of its own abilities, only the activated ones can be activated (CR 602.1), so the
        // rest are not checked; granted ones are checked whatever they are.
        let mut own = activated(slot);
        let own = std::iter::from_fn(move || {
            (own != 0).then(|| {
                let i = own.trailing_zeros() as u8;
                own &= own - 1;
                i
            })
        });
        let abilities = own
            .map(ObjectAbility::Own)
            .chain((0..granted).map(ObjectAbility::Granted));
        for ability in abilities {
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

/// Begins casting a spell: moves it to the stack and starts the machine.
///
/// [CR 601.2a](https://yawgatog.com/resources/magic-rules/#R6012a): "To propose the casting of
/// a spell, a player first moves that card ... from where it is to the stack. It becomes the
/// topmost object on the stack. ... and that player becomes its controller."
///
/// [CR 712.11b](https://yawgatog.com/resources/magic-rules/#R71211b): "A player casting a modal
/// double-faced card ... chooses which face they are casting before putting it onto the
/// stack."
pub(crate) fn begin_cast<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    option: Opt<L>,
    resolving: Option<bool>,
) -> Result<(), Full> {
    let Opt::Cast { slot, face, costs } = option else {
        unreachable!("a cast option");
    };
    let permission = permission(s, player, slot);
    event::move_one(s, t, sink, slot, Zone::Stack, Reason::Cast)?;
    let card = s.object(slot).card;
    s.become_spell(slot, face, player);
    s.put_on_stack(StackEntry {
        object: Some(s.object_ref(slot)),
        controller: player,
        ability: None,
        card,
        face,
        costs,
        exile: permission.is_some_and(|p| p.exile || p.flashback),
        flashback: permission.is_some_and(|p| p.flashback),
        ..StackEntry::default()
    })?;
    let free = resolving.unwrap_or(false);
    let def_face = t.def(card).face(face);
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
pub(crate) fn begin_activate<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    slot: L::Slot,
    ability: ObjectAbility,
) -> Result<(), Full> {
    let (card, face, index) = activated(s, slot, ability).expect("an ability offered");
    let a = t.def(card).face(face).ability(index);
    if a.is_loyalty() {
        s.legacy_mut()
            .set_object_flags(slot, ObjectFlags::LOYALTY_ACTIVATED, true);
    }
    s.put_on_stack(StackEntry {
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
pub(crate) fn target_trigger<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    let entry = top(s);
    let Some(ability) = entry_ability(t, &entry) else {
        return Ok(());
    };
    if ability.targets.is_empty() {
        // The trigger is already on the stack, with no choices to announce first
        // ([CR 603.3](https://yawgatog.com/resources/magic-rules/#R6033)):
        // "its controller puts it on the stack as an object that's not a card".
        sink.note(ability_event(&entry));
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
        s.take_top_of_stack();
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

fn top<L: Layout>(s: &State<L>) -> StackEntry<L> {
    s.stack()
        .last()
        .expect("the spell or ability being cast")
        .clone()
}

/// The target specs of the spell or ability being cast.
fn specs<L: Layout>(t: &GameTable, entry: &StackEntry<L>) -> &'static [TargetSpec] {
    entry_ability(t, entry).map_or(&[], |a| a.targets)
}

/// Whether the target currently being announced (CR 601.2c, 603.3d) is for a removal
/// effect. This describes the choice without running the effect or changing legality.
pub fn target_removes_permanent<L: Layout>(s: &State<L>, t: &GameTable) -> bool {
    let Pending::Casting(c) = *s.pending() else {
        return false;
    };
    matches!(c.step, CastStep::TargetCount | CastStep::Target)
        && entry_ability(t, &top(s)).is_some_and(|ability| ability.removes_target(c.spec))
}

/// The total mana cost of the spell or ability being cast, once X is announced.
fn total_cost<L: Layout>(t: &GameTable, entry: &StackEntry<L>, c: &Casting) -> ManaCost {
    let face = t.def(entry.card).face(entry.face);
    match entry.ability {
        None => spell_cost(face, entry.costs, entry.x, c.extra, c.free),
        Some(ability) => {
            let mut cost = face.ability(ability).cost.mana.with_x(entry.x);
            cost.generic -= c.bent;
            cost
        }
    }
}

/// The total mana cost of the spell or ability being cast were its X announced as `x`; none
/// when nothing is being cast.
/// [CR 601.2f](https://yawgatog.com/resources/magic-rules/#R6012f): "The total cost is the
/// mana cost or alternative cost (as determined in rule 601.2b), plus all additional costs and
/// cost increases, and minus all cost reductions."
pub fn cost_being_cast<L: Layout>(s: &State<L>, t: &GameTable, x: u8) -> Option<ManaCost> {
    match *s.pending() {
        Pending::Casting(c) => Some(total_cost(t, &StackEntry { x, ..top(s) }, &c)),
        _ => None,
    }
}

/// What may be spent paying for the entry being cast, before its other costs are paid: all
/// but a source that pays with itself.
fn spent_by_entry<L: Layout>(t: &GameTable, entry: &StackEntry<L>) -> Bitboard<L::Slot> {
    match source_of(entry) {
        Some(source) => spent_by_cost(source, entry_ability_of(t, entry)),
        None => Bitboard::EMPTY,
    }
}

/// Moves the machine on to its next question, or finishes.
fn advance<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut c: Casting,
) -> Result<(), Full> {
    loop {
        match c.step {
            CastStep::X
            | CastStep::Plan
            | CastStep::Manual
            | CastStep::Crew
            | CastStep::Waterbend
            | CastStep::Exile => break,
            CastStep::TargetCount | CastStep::Target => {
                let entry = top(s);
                let specs = specs(t, &entry);
                let Some(spec) = specs.get(usize::from(c.spec)) else {
                    if c.triggered {
                        s.settle_question();
                        sink.push(ability_event(&entry));
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
    s.ask(Pending::Casting(c));
    Ok(())
}

/// Pays an ability's non-mana costs, then turns to mana. A spell's exile choices follow
/// mana payment, so its caster sees the mana spent before selecting the other costs.
///
/// [CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h): "The player pays the
/// total cost. First, they pay all costs that don't involve random elements or moving objects
/// from the library to a public zone, in any order." The solver never taps a source that pays
/// with itself (`spent_by_entry`), so paying an ability's costs first changes nothing.
fn begin_payment<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
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
                s.ask(Pending::Casting(c));
                return Ok(());
            }
        }
    }
    pay_mana(s, t, sink, c)
}

/// The answers of a waterbend step (CR 701.67a): tap one more artifact or creature, while the
/// rest stays payable, or pay what is left with mana, when it can be. Interchangeable ones are
/// one answer (§7.2). The human can also activate a mana ability
/// ([CR 601.2g](https://yawgatog.com/resources/magic-rules/#R6012g)): "they then have a
/// chance to activate mana abilities." Empty when there is no choice to make.
fn waterbend_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    c: &Casting,
) -> Result<L::Queue<Opt<L>, 64>, Full> {
    let mut out: L::Queue<Opt<L>, 64> = Default::default();
    let entry = top(s);
    let cost = total_cost(t, &entry, c);
    let mut objects: L::Queue<Opt<L>, 64> = Default::default();
    if c.left > 0 {
        let mut fewer = cost;
        fewer.generic -= 1;
        let funds = Funds::of(s, t, c.player)?;
        for slot in collapse(s, benders(s, c.player)).iter() {
            let rest = Bitboard::single(slot);
            if waterbend_payable(s, t, &funds, c.player, fewer, c.left - 1, rest) {
                objects.push(Opt::Object(slot))?;
            }
        }
    }
    // With only generic symbols, activating a mana source keeps at least as much payment
    // available as it did untapped. Activate before tapping any permanent as a bender.
    let manual_mana = s.profile().taps_mana == Some(c.player)
        && c.bent == 0
        && cost.symbols.iter().all(|&n| n == 0);
    if objects.is_empty() && !manual_mana {
        return Ok(out);
    }
    if mana::payable(s, t, c.player, cost, Bitboard::EMPTY)? {
        out.push(Opt::Done)?;
    }
    for &o in objects.iter() {
        out.push(o)?;
    }
    // Append the mana choices so existing candidate indices hold.
    if manual_mana {
        mana::waterbend_mana_options(s, t, c.player, &mut out);
    }
    if out.as_slice() == [Opt::Done] {
        return Ok(Default::default());
    }
    Ok(out)
}

/// Pays the mana of the total cost: by plan, or one source at a time under manual payment.
fn pay_mana<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut c: Casting,
) -> Result<(), Full> {
    let entry = top(s);
    let cost = total_cost(t, &entry, &c);
    match s.profile().payment {
        Payment::Manual => {
            c.remaining = cost;
            if cost.is_zero() {
                return after_mana(s, t, sink, c);
            }
            c.step = CastStep::Manual;
            s.ask(Pending::Casting(c));
            Ok(())
        }
        payment => {
            let plans = mana::plans(
                s,
                t,
                sink,
                c.player,
                cost,
                Bitboard::EMPTY,
                payment == Payment::Greedy,
            )?;
            debug_assert!(!plans.is_empty(), "a spell offered is payable");
            if plans.len() > 1 {
                c.step = CastStep::Plan;
                s.ask(Pending::Casting(c));
                return Ok(());
            }
            if let Some(plan) = plans.as_slice().first() {
                mana::pay(s, t, sink, c.player, cost, Bitboard::EMPTY, plan)?;
            }
            after_mana(s, t, sink, c)
        }
    }
}

/// Mana production and spending have completed before choosing cards for an exile cost.
/// [CR 601.2g](https://yawgatog.com/resources/magic-rules/#R6012g): "Mana abilities must be
/// activated before costs are paid." CR 601.2h permits spending that mana before exiling
/// cards; it does not permit activating mana abilities after paying the exile cost. Keeping
/// the unfinished cast pending also keeps session cancellation valid.
fn after_mana<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut c: Casting,
) -> Result<(), Full> {
    let entry = top(s);
    if entry.is_spell() {
        let face = t.def(entry.card).face(entry.face);
        if let Some(cost) = ExileCost::of(face, entry.costs) {
            c.step = CastStep::Exile;
            c.left = cost.count();
            c.spec = 0;
            s.ask(Pending::Casting(c));
            return Ok(());
        }
    }
    finish(s, t, sink, c)
}

/// The spell becomes cast, or the ability activated.
///
/// [CR 601.2i](https://yawgatog.com/resources/magic-rules/#R6012i): "Any abilities that
/// trigger when a spell is cast or put onto the stack trigger at this time. If the spell's
/// controller had priority before casting it, they get priority." CR 608.2g: after a spell
/// cast during a resolution, "no player receives priority"; the resolution it was cast in is
/// over by now, so the active player receives priority (CR 117.3b).
fn finish<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    c: Casting,
) -> Result<(), Full> {
    s.settle_question();
    let active = s.turn().active();
    s.give_priority(if c.resolving { active } else { c.player });
    let entry = top(s);
    if let Some(spell) = entry.spell() {
        let face = t.def(entry.card).face(entry.face);
        for (i, option) in face.casting.iter().enumerate() {
            if entry.costs.contains(i) {
                crate::coverage::hit(option);
            }
        }
        event::settle(
            s,
            t,
            sink,
            Event::Cast {
                spell,
                controller: entry.controller,
                targets: entry.targets.clone(),
                x: entry.x,
                costs: entry.costs,
                flashback: entry.flashback,
            },
        )?;
    } else {
        sink.push(ability_event(&entry));
    }
    Ok(())
}

/// The [`Event::Ability`] that says `entry`, an ability on the stack. It goes straight to the
/// sink: no trigger listens to it and it changes nothing.
fn ability_event<L: Layout>(entry: &StackEntry<L>) -> Event<L> {
    Event::Ability {
        source: entry.object,
        controller: entry.controller,
        card: entry.card,
        face: entry.face,
        ability: entry.ability.expect("an ability's entry"),
        targets: entry.targets.clone(),
        x: entry.x,
    }
}

/// The X values the payment can still cover, from the least allowed.
fn x_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    c: &Casting,
) -> Result<L::Queue<Opt<L>, 64>, Full> {
    let entry = top(s);
    let face = t.def(entry.card).face(entry.face);
    let least = if entry.is_spell() {
        x_range_start(face, entry.costs, c.free).unwrap_or(0)
    } else {
        0
    };
    let exclude = spent_by_entry(t, &entry);
    let mut out: L::Queue<Opt<L>, 64> = Default::default();
    for x in least..(least.saturating_add(64)) {
        let mut probe = entry.clone();
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
fn target_options<L: Layout>(s: &State<L>, t: &GameTable, c: &Casting) -> TargetSet<L> {
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
pub(crate) fn decision<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    c: &Casting,
) -> Result<Decision<L>, Full> {
    let options = |kind: DecisionKind, list: L::Queue<Opt<L>, 64>, prompt: Prompt| Decision {
        player: c.player,
        kind,
        candidates: Candidates::Options(list),
        prompt,
        about: Bitboard::EMPTY,
    };
    // The targets of one spec are one prompt, which says how many are still to choose.
    let target = Prompt::new(Purpose::Target).most(c.left);
    Ok(match c.step {
        CastStep::X => options(
            DecisionKind::X,
            x_options(s, t, c)?,
            Prompt::new(Purpose::X),
        ),
        CastStep::TargetCount => {
            let entry = top(s);
            let spec = &specs(t, &entry)[usize::from(c.spec)];
            let most = target_options(s, t, c).count().min(u32::from(spec.count));
            let mut list: L::Queue<Opt<L>, 64> = Default::default();
            for n in 0..=most {
                list.push(Opt::Number(n as u8))?;
            }
            let count = Prompt::new(Purpose::Target)
                .most(spec.count)
                .part(Part::Count);
            options(DecisionKind::TargetCount, list, count)
        }
        CastStep::Target => match target_options(s, t, c) {
            TargetSet::Objects(b) => Decision {
                player: c.player,
                kind: DecisionKind::Target,
                candidates: Candidates::Objects(collapse(s, b)),
                prompt: target,
                about: Bitboard::EMPTY,
            },
            TargetSet::Players(p) => {
                let mut list: L::Queue<Opt<L>, 64> = Default::default();
                for player in Seat::BOTH.into_iter().filter(|q| p & (1 << q.get()) != 0) {
                    list.push(Opt::Player(player))?;
                }
                options(DecisionKind::Target, list, target)
            }
        },
        CastStep::Plan => {
            let entry = top(s);
            let cost = total_cost(t, &entry, c);
            let plans = mana::plans(s, t, sink, c.player, cost, Bitboard::EMPTY, false)?;
            let mut list: L::Queue<Opt<L>, 64> = Default::default();
            for i in 0..plans.len() {
                list.push(Opt::Plan(index::<L>(i)))?;
            }
            let plan = Prompt::new(Purpose::Mana).cost(cost).part(Part::Plan);
            options(DecisionKind::PaymentPlan, list, plan)
        }
        CastStep::Crew => {
            let entry = top(s);
            Decision {
                player: c.player,
                kind: DecisionKind::Crew,
                candidates: Candidates::Objects(collapse(
                    s,
                    crew_candidates(s, c.player, source_of(&entry).expect("a Vehicle to crew")),
                )),
                // The crew cost's power, and what of it is still to tap.
                prompt: Prompt::new(Purpose::Crew)
                    .count(entry_ability(t, &entry).map_or(0, |a| a.cost.crew))
                    .most(c.left),
                about: Bitboard::EMPTY,
            }
        }
        CastStep::Manual => options(
            DecisionKind::ManaPayment,
            mana::manual_options(s, t, c.player, c.remaining, Bitboard::EMPTY)?,
            Prompt::new(Purpose::Mana).cost(c.remaining),
        ),
        CastStep::Waterbend => options(
            DecisionKind::Waterbend,
            waterbend_options(s, t, c)?,
            Prompt::new(Purpose::Waterbend).cost(total_cost(t, &top(s), c)),
        ),
        // The cards lie in the zone the cost exiles from, the hand or the graveyard.
        CastStep::Exile => Decision {
            player: c.player,
            kind: DecisionKind::ExileCost,
            candidates: Candidates::Objects(exile_choices(s, t, c)),
            prompt: Prompt::new(Purpose::ExileCost).most(c.left),
            about: Bitboard::EMPTY,
        },
    })
}

/// Applies an answer to the machine's question: an option, or an object for a target.
pub(crate) fn perform<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut c: Casting,
    option: Option<Opt<L>>,
    object: Option<L::Slot>,
) -> Result<(), Full> {
    match c.step {
        CastStep::X => {
            let Some(Opt::Number(x)) = option else {
                unreachable!("an X answer");
            };
            s.announce_x(x);
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
            s.announce_target(target)?;
            c.left -= 1;
        }
        CastStep::Plan => {
            let Some(Opt::Plan(i)) = option else {
                unreachable!("a payment plan");
            };
            let entry = top(s);
            let cost = total_cost(t, &entry, &c);
            let plans = mana::plans(s, t, sink, c.player, cost, Bitboard::EMPTY, false)?;
            mana::pay(
                s,
                t,
                sink,
                c.player,
                cost,
                Bitboard::EMPTY,
                &plans.as_slice()[Into::<usize>::into(i)],
            )?;
            return after_mana(s, t, sink, c);
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
            if let Some(Opt::Mana { slot, color }) = option {
                mana::activate(s, t, sink, slot, color)?;
                if waterbend_options(s, t, &c)?.is_empty() {
                    return pay_mana(s, t, sink, c);
                }
                s.ask(Pending::Casting(c));
                return Ok(());
            }
            let Some(Opt::Object(slot)) = option else {
                return pay_mana(s, t, sink, c);
            };
            event::set_tapped(s, t, sink, s.object_ref(slot), true)?;
            c.bent += 1;
            c.left -= 1;
            if waterbend_options(s, t, &c)?.is_empty() {
                return pay_mana(s, t, sink, c);
            }
            s.ask(Pending::Casting(c));
            return Ok(());
        }
        CastStep::Exile => {
            // CR 601.2h: "The player pays the total cost." The cards go from the hand or the
            // graveyard to exile as part of it.
            let slot = object.expect("a card to exile");
            if let Some(i) = s.graveyard(c.player).iter().position(|&g| g == slot) {
                // The cards above it move down one, so the next pick starts here.
                c.spec = i as u8;
            }
            event::move_one(s, t, sink, slot, Zone::Exile, Reason::Exile)?;
            c.left -= 1;
            if c.left > 0 {
                s.ask(Pending::Casting(c));
                return Ok(());
            }
            return finish(s, t, sink, c);
        }
        CastStep::Manual => {
            let option = option.expect("a manual payment");
            c.remaining = mana::manual_pay(s, t, sink, c.player, c.remaining, option)?;
            if c.remaining.is_zero() {
                return after_mana(s, t, sink, c);
            }
        }
    }
    advance(s, t, sink, c)
}

/// The options of a resolution's offer to cast a card (CR 608.2g): not casting it, then each
/// way of casting it.
pub(crate) fn offer_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    object: ObjectRef<L>,
    player: Seat,
    free: bool,
) -> Result<L::Queue<Opt<L>, 64>, Full> {
    let mut out: L::Queue<Opt<L>, 64> = Default::default();
    out.push(Opt::Pass)?;
    if s.is_current(object) {
        let mut cards = Bitboard::EMPTY;
        cards.insert(object.slot);
        let now = Situation::of(player);
        cast_options(s, t, &now, cards, From::Resolution { free }, &mut out)?;
        now.complete()?;
    }
    Ok(out)
}

/// Removes a spell from the stack to its owner's graveyard, or to exile when `exile` or its
/// entry says so. Returns whether it was there.
pub(crate) fn counter<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    spell: ObjectRef<L>,
    exile: bool,
) -> Result<bool, Full> {
    let Some(entry) = s.take_off_stack(spell) else {
        return Ok(false);
    };
    let to = if exile { Zone::Exile } else { Zone::Graveyard };
    leave_stack(s, t, sink, &entry, to, Reason::Counter)?;
    Ok(true)
}

/// Whether a spell that leaves the stack for `to` is exiled instead: one cast with flashback
/// ([CR 702.34a](https://yawgatog.com/resources/magic-rules/#R70234a): "If the flashback
/// cost was paid, exile this card instead of putting it anywhere else any time it would
/// leave the stack."), and one its permission exiles rather than put into a graveyard.
pub(crate) fn exiled_instead<L: Layout>(entry: &StackEntry<L>, to: Zone) -> bool {
    entry.flashback || (entry.exile && to == Zone::Graveyard)
}

/// Takes a spell off the stack for an effect that moves its card to `to` without its
/// resolving: where the card goes, or nothing if the spell is not on the stack.
pub(crate) fn unstack<L: Layout>(s: &mut State<L>, spell: ObjectRef<L>, to: Zone) -> Option<Zone> {
    let exiled = exiled_instead(&s.take_off_stack(spell)?, to);
    Some(if exiled { Zone::Exile } else { to })
}

/// Moves the card of a spell whose entry is off the stack, and which did not become a
/// permanent, to `to`, or to exile instead ([`exiled_instead`]).
pub(crate) fn leave_stack<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    entry: &StackEntry<L>,
    to: Zone,
    reason: Reason,
) -> Result<(), Full> {
    let exiled = exiled_instead(entry, to);
    let to = if exiled { Zone::Exile } else { to };
    let spell = entry.spell().expect("a spell leaving the stack");
    event::move_one(s, t, sink, spell.slot, to, reason)
}

/// The objects a pending reference names: stack targets, the sources of abilities on the stack
/// and cast permissions. They are never interchangeable with another object (§7.2).
///
/// An ability's source can be its own target: its targets are chosen before its costs are
/// paid ([CR 601.2c](https://yawgatog.com/resources/magic-rules/#R6012c), [CR
/// 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h), which [CR
/// 602.2b](https://yawgatog.com/resources/magic-rules/#R6022b) applies to activating: "The
/// remainder of the process for activating an ability is identical to the process for casting a
/// spell listed in rules 601.2b-i"). A Strip Mine targeting itself is sacrificed and its ability
/// does nothing; targeting its twin destroys the twin.
pub(crate) fn referenced<L: Layout>(s: &State<L>) -> Bitboard<L::Slot> {
    let mut b = Bitboard::EMPTY;
    for e in s.stack() {
        if let Some(source) = e.object.filter(|_| !e.is_spell()) {
            b.insert(source.slot);
        }
        for target in e.targets.iter() {
            if let Some(r) = target.as_object() {
                b.insert(r.slot);
            }
        }
    }
    for p in s.permissions() {
        b.insert(p.object.slot);
    }
    b
}
