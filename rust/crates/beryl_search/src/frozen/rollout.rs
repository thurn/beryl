//! `expert-frozen`'s copy of the rollout policy ([`crate::RolloutPolicy`]), as it stood when
//! frozen (see the `frozen` module). Never tune it.
//!
//! The rollout policy (architecture §19.1, decision D21): one cheap, greedy answer to every
//! decision, from per-card values and simple combat rules. It makes no copies and looks
//! nothing ahead, allocates nothing, and reads only an [`Observation`]. Wrapped as an agent it
//! is `greedy`; inside search it plays rollouts, answering mid-resolution questions inline
//! (§7.3).
//!
//! The rules, in brief:
//!
//! - Play a land first (the least valuable one), then cast or activate the most valuable
//!   option, or pass when nothing is worth anything.
//! - Stop drawing by choice once the library runs low ([`LIBRARY_LOW`]), unless an empty
//!   library wins instead.
//! - Aim harmful effects at the opponent's most valuable permanent or spell, and helpful ones
//!   at your own. A removal spell waits for a target worth [`REMOVAL_MIN`], a counterspell for
//!   a spell worth [`COUNTER_MIN`]; an overloaded one, for the opponent's permanents it hits
//!   to outweigh yours by as much.
//! - Attack with a creature when no untapped creature that could block it would kill it,
//!   at the opponent's most valuable planeswalker that the attackers declared so far leave
//!   alive, or at the player once they are at 6 life or less.
//! - Block when the blocker kills the attacker and survives, trades up, or survives; chump
//!   only when the unblocked damage would be lethal.
//! - Say yes to every "may" and every "unless pays"; keep the cards worth most, and scry to
//!   the bottom only the cards not worth drawing.

use beryl_engine::card::{
    Ability, AbilityKind, CardDef, CastingOption, Castings, Face, FaceAbility, FaceIx, Keywords,
    ObjectAbility, Players, Purpose, TargetKind, TargetSpec, Types,
};
use beryl_engine::filter::Filter;
use beryl_engine::state::{ObjectFlags, StackEntry};
use beryl_engine::two_player::TwoPlayerSeat as _;
use beryl_engine::{
    Answers, Candidates, Choice, Decision, DecisionKind, GameTable, Observation, Opt, Seat, Slot,
    Zone, ZoneKey,
};

use super::values::{CardInfo, CardValues};

/// The least value of a permanent worth a removal spell or ability.
pub const REMOVAL_MIN: i32 = 25;

/// The least value of a spell worth countering.
pub const COUNTER_MIN: i32 = 40;

/// The least a card on top of the library must be worth in hand to stay there.
const DRAW_MIN: i32 = 30;

/// The most cards a library may hold for its owner to stop drawing by choice.
const LIBRARY_LOW: usize = 10;

/// The most a land on the battlefield is worth.
const LAND_MAX: i32 = 15;

/// The rollout policy for one game: its per-card values. Stateless between decisions, so one
/// policy serves both seats, every determinization and every thread.
#[derive(Clone, Debug)]
pub struct RolloutPolicy {
    values: CardValues,
}

/// One candidate of a decision, objects and options alike.
#[derive(Clone, Copy)]
pub(crate) enum Candidate {
    Object(Slot),
    Option(Opt),
}

impl Candidate {
    pub(crate) fn option(self) -> Option<Opt> {
        match self {
            Candidate::Option(o) => Some(o),
            Candidate::Object(_) => None,
        }
    }

    pub(crate) fn object(self) -> Option<Slot> {
        match self {
            Candidate::Object(slot) | Candidate::Option(Opt::Object(slot)) => Some(slot),
            Candidate::Option(_) => None,
        }
    }
}

impl RolloutPolicy {
    pub fn new(t: &GameTable) -> RolloutPolicy {
        RolloutPolicy {
            values: CardValues::new(t),
        }
    }

    /// The rules' view of one decision of `me`'s, for agents that build on them: with
    /// `expert`, valuing permanents as the expert does.
    pub(crate) fn view<'a>(
        &'a self,
        t: &'a GameTable,
        obs: Observation<'a>,
        me: Seat,
        expert: bool,
    ) -> Policy<'a> {
        Policy {
            values: &self.values,
            t,
            obs,
            me,
            expert,
        }
    }

    /// The policy's answer to `d`, seen by `d.player` in `obs`.
    pub fn choose(&self, t: &GameTable, obs: Observation<'_>, d: &Decision) -> Choice {
        self.decide(t, obs, d, false)
    }

    /// The policy's answer to `d`, with `expert`, valuing permanents as the expert does.
    pub(crate) fn decide(
        &self,
        t: &GameTable,
        obs: Observation<'_>,
        d: &Decision,
        expert: bool,
    ) -> Choice {
        let p = self.view(t, obs, d.player, expert);
        match d.kind {
            DecisionKind::Priority | DecisionKind::CastOffer => {
                best(d, |c| c.option().map(|o| p.action(o)))
            }
            DecisionKind::PlayOrDraw => best(d, |c| Some(i32::from(c.option() == Some(Opt::Play)))),
            DecisionKind::KeepOrMulligan => {
                let keep = p.keep();
                best(d, |c| {
                    Some(i32::from((c.option() == Some(Opt::Keep)) == keep))
                })
            }
            // A "may" is taken; life for an untapped land is paid only from above 10.
            DecisionKind::YesNo => {
                let yes = d.prompt.purpose != Purpose::PayLife || obs.player(p.me).life() > 10;
                best(d, |c| {
                    Some(i32::from((c.option() == Some(Opt::Yes)) == yes))
                })
            }
            // A card leaving the player's own hand (a mulligan's, a discard's) is the one least
            // worth holding; anything else is taken by its value, as the legend to keep. Two
            // costs are paid with the least: a crew's creatures, an exile cost's cards.
            DecisionKind::ChooseObject => best(d, |c| {
                c.object().map(|slot| {
                    let o = obs.object(slot);
                    match d.prompt.purpose {
                        Purpose::Crew => -p.value(slot),
                        Purpose::ExileCost => -p.in_hand(slot),
                        _ if o.zone() == Zone::Hand && o.owner() == p.me => -p.in_hand(slot),
                        _ => p.value(slot),
                    }
                })
            }),
            // Cards on top of a library are being chosen for the bottom, as a scry asks: only
            // those not worth drawing go. Anything else, such as lands to untap, is worth
            // taking by its value.
            DecisionKind::ChooseUpTo => best(d, |c| match c.object() {
                Some(slot) if obs.object(slot).zone() == Zone::Library => {
                    Some(DRAW_MIN - p.in_hand(slot))
                }
                Some(slot) => Some(p.value(slot)),
                None => Some(0),
            }),
            DecisionKind::Attacker => best(d, |c| match c.option() {
                Some(Opt::Attack(slot)) => p.safe_attack(slot).then(|| 1 + p.value(slot)),
                _ => Some(0),
            }),
            DecisionKind::AttackTarget => best(d, |c| match c.option() {
                Some(Opt::Defender(None)) => Some(if obs.player(p.me.opponent()).life() <= 6 {
                    1000
                } else {
                    0
                }),
                Some(Opt::Defender(Some(walker))) => Some(p.walker_target(walker)),
                _ => None,
            }),
            DecisionKind::Blocker => best(d, |c| match c.option() {
                Some(Opt::Block(blocker)) => p.best_block(blocker),
                _ => Some(0),
            }),
            DecisionKind::BlockTarget => {
                let blocker = p.declaring_blocker();
                best(d, |c| {
                    let attacker = c.object()?;
                    Some(blocker.and_then(|b| p.block(b, attacker)).unwrap_or(0))
                })
            }
            DecisionKind::DamageDivision => best(d, |c| match c.option() {
                Some(Opt::Lethal(blocker)) => Some(1 + p.value(blocker)),
                _ => Some(0),
            }),
            DecisionKind::Target => {
                let friendly = p.targeting().is_some_and(|(_, friendly)| friendly);
                best(d, |c| Some(p.target(c, friendly)))
            }
            DecisionKind::TargetCount => {
                let worth = p
                    .targeting()
                    .is_some_and(|(spec, friendly)| p.best_target(spec, friendly).is_some());
                best(d, |c| match c.option() {
                    Some(Opt::Number(n)) => Some(if worth { i32::from(n) } else { -i32::from(n) }),
                    _ => None,
                })
            }
            DecisionKind::X => best(d, |c| match c.option() {
                Some(Opt::Number(n)) => Some(i32::from(n)),
                _ => None,
            }),
            // The first: a cost that offers permanents in place of mana lists mana first.
            DecisionKind::TriggerOrder | DecisionKind::PaymentPlan | DecisionKind::ManaPayment => {
                Choice(0)
            }
        }
    }
}

/// The candidate with the highest score, the first of equals. A candidate scored `None` is
/// never chosen over one with a score; when none has one, the first is.
pub(crate) fn best(d: &Decision, mut score: impl FnMut(Candidate) -> Option<i32>) -> Choice {
    let mut top = (0, i32::MIN);
    let mut consider = |i: usize, c: Candidate| {
        if let Some(s) = score(c)
            && s > top.1
        {
            top = (i, s);
        }
    };
    match &d.candidates {
        Candidates::Objects(_) => (d.candidates.objects())
            .enumerate()
            .for_each(|(i, slot)| consider(i, Candidate::Object(slot))),
        Candidates::Options(o) => o
            .iter()
            .enumerate()
            .for_each(|(i, &opt)| consider(i, Candidate::Option(opt))),
    }
    Choice(top.0 as u16)
}

/// One decision's view, for the rules above.
pub(crate) struct Policy<'a> {
    pub(crate) values: &'a CardValues,
    pub(crate) t: &'a GameTable,
    pub(crate) obs: Observation<'a>,
    pub(crate) me: Seat,
    /// Whether permanents are valued as the expert values them ([`super::expert`]).
    pub(crate) expert: bool,
}

impl Policy<'_> {
    pub(crate) fn info(&self, slot: Slot) -> Option<CardInfo> {
        self.obs.card(slot).map(|card| self.values.get(card))
    }

    /// What an object is worth: its card's value, plus a creature's power and a
    /// planeswalker's loyalty as they stand on the battlefield, where a land is worth at most
    /// [`LAND_MAX`]. An unknown card is worth 0.
    pub(crate) fn value(&self, slot: Slot) -> i32 {
        let Some(info) = self.info(slot) else {
            return 0;
        };
        let mut v = i32::from(info.value);
        let o = self.obs.object(slot);
        if o.zone() == Zone::Battlefield {
            if self.expert
                && let Some(v) = super::expert::appraise(self, slot, v)
            {
                return v;
            }
            let d = self.obs.derived(slot);
            // A modal double-faced card played as its land face is worth a land.
            if d.types.intersects(Types::LAND) {
                v = v.min(LAND_MAX);
            }
            if d.types.intersects(Types::CREATURE) {
                v += 3 * i32::from(d.power.max(0));
            }
            if d.types.intersects(Types::PLANESWALKER) {
                v += 4 * i32::from(o.counters().loyalty);
            }
        }
        v
    }

    pub(crate) fn is_land(&self, slot: Slot) -> bool {
        self.obs.card(slot).is_some_and(|card| {
            self.t
                .def(card)
                .faces
                .iter()
                .any(|f| f.types.intersects(Types::LAND))
        })
    }

    /// What a card in hand is worth keeping: a land is worth much while lands are short and
    /// little once there are six.
    pub(crate) fn in_hand(&self, slot: Slot) -> i32 {
        if !self.is_land(slot) {
            return self.value(slot);
        }
        // To the expert, a modal double-faced card with a spell in front is worth the spell
        // at least.
        let spell = self.expert
            && self
                .obs
                .card(slot)
                .is_some_and(|c| !self.t.def(c).front().types.intersects(Types::LAND));
        let mine = |zone| self.obs.zone(ZoneKey::Of(self.me, zone));
        let lands = (mine(Zone::Hand) | mine(Zone::Battlefield))
            .filter(|s| self.is_land(s))
            .count();
        let land = if lands > 5 { 5 } else { 80 };
        if spell {
            land.max(self.value(slot))
        } else {
            land
        }
    }

    /// Keep a hand of two to five lands, and any hand of five cards or fewer, judging the hand
    /// left once a card per mulligan taken goes to the bottom: the least worth keeping, as
    /// the pick for the bottom ([`Purpose::PutOnBottom`]) takes them.
    pub(crate) fn keep(&self) -> bool {
        let mut kept = self.obs.zone(ZoneKey::Of(self.me, Zone::Hand));
        for _ in 0..self.obs.player(self.me).mulligans() {
            // Of the least wanted alike, the first in candidate order goes: the policy's pick.
            let least = self.obs.in_candidate_order(kept);
            match least.min_by_key(|&slot| self.in_hand(slot)) {
                Some(least) => kept.remove(least),
                None => break,
            }
        }
        let lands = kept.filter(|slot| self.is_land(slot)).count();
        kept.count() <= 5 || (2..=5).contains(&lands)
    }

    /// The score of a priority action or a cast offer; 0 is passing.
    pub(crate) fn action(&self, o: Opt) -> i32 {
        match o {
            Opt::PlayLand(slot) => 1000 - self.value(slot),
            Opt::Cast { slot, face, costs } => self.cast(slot, face, costs),
            Opt::Activate { slot, ability } => match ability {
                ObjectAbility::Own(i) => self.activate(slot, i.get()),
                ObjectAbility::Granted(_) => 0,
            },
            _ => 0,
        }
    }

    pub(crate) fn cast(&self, slot: Slot, face: FaceIx, costs: Castings) -> i32 {
        let Some(card) = self.obs.card(slot) else {
            return 0;
        };
        let info = self.values.get(card);
        let f = self.t.def(card).face(face);
        if info.wheel && self.obs.zone(ZoneKey::Of(self.me, Zone::Hand)).count() > 3 {
            return 0;
        }
        if info.never_cast {
            return 0;
        }
        let drains = f
            .abilities
            .iter()
            .enumerate()
            .any(|(i, a)| a.kind == AbilityKind::Triggered && info.draws(i as u8));
        if face == FaceIx::FRONT && drains && !self.may_draw() {
            return 0;
        }
        let mut v = i32::from(info.value);
        for option in costs.paid(f) {
            v += match option {
                CastingOption::Kicker { .. } | CastingOption::Offspring(_) => 10,
                CastingOption::Impending { .. } | CastingOption::Overload { .. } => -10,
                // A card from hand and the permanent itself, for its enters ability alone.
                CastingOption::Evoke { .. } => -30,
            };
        }
        // Overloaded, a spell hits each permanent its first target's filter accepts
        // ([CR 702.96a](https://yawgatog.com/resources/magic-rules/#R70296a)): "change its
        // text by replacing all instances of the word 'target' with the word 'each.'"
        let overloaded = costs
            .paid(f)
            .any(|o| matches!(o, CastingOption::Overload { .. }));
        if overloaded
            && let Some(spec) = f.spell_ability().and_then(|a| a.targets.first())
            && self.sweep(spec) < REMOVAL_MIN
        {
            return 0;
        }
        match f.spell_ability_with(costs) {
            Some(a) if face == FaceIx::FRONT && info.draws(index_of(f, a)) && !self.may_draw() => 0,
            Some(a) => self.worth_targeting(a, info.friendly(index_of(f, a)), v),
            None => v,
        }
    }

    pub(crate) fn activate(&self, slot: Slot, i: u8) -> i32 {
        let Some(card) = self.obs.card(slot) else {
            return 0;
        };
        let info = self.values.get(card);
        if info.never(i) {
            return 0;
        }
        let o = self.obs.object(slot);
        if o.face() == FaceIx::FRONT && info.draws(i) && !self.may_draw() {
            return 0;
        }
        if o.face() == FaceIx::FRONT && info.empties(i) {
            return if self.obs.library(self.me).len() <= 7 {
                1000
            } else {
                0
            };
        }
        let a = &self.t.def(card).face(o.face()).abilities[usize::from(i)];
        let v = match a.cost.loyalty_change() {
            Some(l) if l >= 0 => 20,
            Some(l) if l <= -8 => 200,
            Some(l) => 20 + 5 * i32::from(-l),
            None if a.cost.does(beryl_engine::card::CostAction::Discard) => 3,
            None => 15,
        };
        self.worth_targeting(a, info.friendly(i), v)
    }

    /// Whether drawing by choice is safe: our library holds more than [`LIBRARY_LOW`] cards,
    /// we hold a wheel to refill it, or a permanent of ours wins us the game instead of the
    /// draw from an empty library that would lose it
    /// ([CR 704.5b](https://yawgatog.com/resources/magic-rules/#R7045b)): "If a player
    /// attempted to draw a card from a library with no cards in it since the last time
    /// state-based actions were checked, that player loses the game."
    pub(crate) fn may_draw(&self) -> bool {
        self.obs.library(self.me).len() > LIBRARY_LOW
            || self
                .obs
                .zone(ZoneKey::Of(self.me, Zone::Hand))
                .any(|s| self.info(s).is_some_and(|info| info.wheel))
            || self.wins_from_empty(self.me)
    }

    /// Whether `who` controls a permanent that wins them the game instead of the draw from an
    /// empty library, as Jace, Wielder of Mysteries: "If you would draw a card while your
    /// library has no cards in it, you win the game instead."
    pub(crate) fn wins_from_empty(&self, who: Seat) -> bool {
        self.obs.battlefield().iter().any(|&slot| {
            self.obs.controller(slot) == who
                && self.obs.card(slot).is_some_and(|card| {
                    self.t
                        .def(card)
                        .face(self.obs.object(slot).face())
                        .abilities
                        .iter()
                        .any(beryl_engine::wins_instead_of_drawing)
                })
        })
    }

    /// `v`, or 0 when the ability's first target is not worth it: a harmful one needs an
    /// opponent's object worth the removal or counter threshold, unless it may target
    /// nothing; a helpful one needs one of ours.
    pub(crate) fn worth_targeting(&self, a: &Ability, friendly: bool, v: i32) -> i32 {
        let Some(spec) = a.targets.first() else {
            return v;
        };
        let friendly = friendly || matches!(whose(&spec.filter), Some(Players::You));
        let min = match spec.kind {
            _ if friendly => 0,
            TargetKind::Spell => COUNTER_MIN,
            TargetKind::Player => 0,
            _ => REMOVAL_MIN,
        };
        match self.best_target(spec, friendly) {
            Some(best) if best >= min => v,
            _ if spec.optional && !matches!(spec.kind, TargetKind::Spell) => v / 2,
            _ => 0,
        }
    }

    /// The most valuable object a spec could target on the side it should: the opponent's
    /// for a harmful effect, ours for a helpful one. Players count as 0.
    pub(crate) fn best_target(&self, spec: &TargetSpec, friendly: bool) -> Option<i32> {
        let side = if friendly {
            self.me
        } else {
            self.me.opponent()
        };
        let ok = |slot: Slot, controller: Seat| {
            controller == side && self.obs.matches(self.t, &spec.filter, slot)
        };
        let mut top: Option<i32> = None;
        let mut offer = |v: i32| top = Some(top.map_or(v, |t| t.max(v)));
        let permanents = matches!(
            spec.kind,
            TargetKind::Permanent | TargetKind::SpellOrPermanent
        );
        let spells = matches!(spec.kind, TargetKind::Spell | TargetKind::SpellOrPermanent);
        if permanents {
            for &slot in self.obs.battlefield() {
                if ok(slot, self.obs.controller(slot)) {
                    offer(self.value(slot));
                }
            }
        }
        if spells {
            for e in self.obs.stack() {
                if let Some(r) = e.spell()
                    && ok(r.slot, e.controller)
                {
                    offer(self.value(r.slot));
                }
            }
        }
        match spec.kind {
            TargetKind::GraveyardCard => {
                for &slot in self.obs.graveyard(side) {
                    if self.obs.matches(self.t, &spec.filter, slot) {
                        offer(self.value(slot));
                    }
                }
            }
            TargetKind::Player => offer(0),
            _ => {}
        }
        top
    }

    /// What hitting each permanent `spec`'s filter accepts gains a harmful effect: the value of
    /// the opponent's, less that of ours.
    pub(crate) fn sweep(&self, spec: &TargetSpec) -> i32 {
        self.obs
            .battlefield()
            .iter()
            .filter(|&&slot| self.obs.matches(self.t, &spec.filter, slot))
            .map(|&slot| {
                if self.obs.controller(slot) == self.me {
                    -self.value(slot)
                } else {
                    self.value(slot)
                }
            })
            .sum()
    }

    /// The spec being targeted for and whether its effect helps its controller: the first
    /// spec of the spell or ability on top of the stack, which a casting or a triggered
    /// ability being put on the stack puts there first (CR 601.2a).
    pub(crate) fn targeting(&self) -> Option<(&'static TargetSpec, bool)> {
        let e: &StackEntry = self.obs.stack().last()?;
        let def: &'static CardDef = self.t.def(e.card);
        let face: &'static Face = def.face(e.face);
        let a = match e.ability {
            Some(ability) => face.ability(ability),
            None => face.spell_ability_with(e.costs)?,
        };
        let spec = a.targets.first()?;
        let index = match e.ability {
            Some(FaceAbility::Own(i)) => i.get(),
            Some(FaceAbility::Grant(_)) => u8::MAX,
            None => index_of(face, a),
        };
        let friendly = self.values.get(e.card).friendly(index)
            || matches!(whose(&spec.filter), Some(Players::You));
        Some((spec, friendly))
    }

    /// A target's score: a candidate on the right side by its value, far above the wrong
    /// side; choosing none in between.
    pub(crate) fn target(&self, c: Candidate, friendly: bool) -> i32 {
        let side = if friendly {
            self.me
        } else {
            self.me.opponent()
        };
        let (controller, v) = match c {
            Candidate::Option(Opt::Player(p)) => (p, 0),
            Candidate::Option(Opt::Done) => return 500,
            _ => match c.object() {
                Some(slot) => {
                    let o = self.obs.object(slot);
                    let controller = if matches!(o.zone(), Zone::Battlefield | Zone::Stack) {
                        self.obs.controller(slot)
                    } else {
                        o.owner()
                    };
                    (controller, self.value(slot))
                }
                None => return 0,
            },
        };
        if controller == side { 1000 + v } else { -v }
    }

    pub(crate) fn creature(&self, slot: Slot) -> bool {
        self.obs.derived(slot).types.intersects(Types::CREATURE)
    }

    pub(crate) fn flags(&self, slot: Slot) -> ObjectFlags {
        self.obs.object(slot).flags()
    }

    /// Whether `blocker` could block `attacker`, by evasion
    /// ([CR 702.9b](https://yawgatog.com/resources/magic-rules/#R7029b)): "A creature with
    /// flying can't be blocked except by creatures with flying and/or reach."
    pub(crate) fn evades(&self, attacker: Slot, blocker: Slot) -> bool {
        let has = |slot: Slot, k: Keywords| self.obs.derived(slot).keywords.intersects(k);
        has(attacker, Keywords::FLYING) && !has(blocker, Keywords::FLYING.union(Keywords::REACH))
    }

    /// Whether `by`'s power is at least the damage left to kill `slot`.
    pub(crate) fn kills(&self, by: Slot, slot: Slot) -> bool {
        let left =
            i16::from(self.obs.derived(slot).toughness) - i16::from(self.obs.object(slot).damage());
        i16::from(self.obs.derived(by).power) >= left
    }

    /// No untapped creature of the opponent that could block `attacker` would kill it, and
    /// it deals damage.
    pub(crate) fn safe_attack(&self, attacker: Slot) -> bool {
        self.obs.derived(attacker).power > 0
            && !self.obs.battlefield().iter().any(|&b| {
                self.obs.controller(b) != self.me
                    && self.creature(b)
                    && !self.flags(b).has(ObjectFlags::TAPPED)
                    && !self.evades(attacker, b)
                    && self.kills(b, attacker)
            })
    }

    /// The score of attacking `walker`, against 0 for the player: its value, unless the
    /// attackers already declared at it deal all its loyalty. Damage dealt to a planeswalker
    /// removes that much loyalty
    /// ([CR 120.3c](https://yawgatog.com/resources/magic-rules/#R1203c)): "Damage dealt to
    /// a planeswalker causes that many loyalty counters to be removed from that
    /// planeswalker."
    pub(crate) fn walker_target(&self, walker: Slot) -> i32 {
        let sent: i32 = self
            .obs
            .battlefield()
            .iter()
            .filter(|&&a| {
                let o = self.obs.object(a);
                self.obs.controller(a) == self.me
                    && o.flags().has(ObjectFlags::ATTACKING)
                    && o.combat() == Some(walker)
            })
            .map(|&a| i32::from(self.obs.derived(a).power.max(0)))
            .sum();
        if sent < i32::from(self.obs.object(walker).counters().loyalty) {
            self.value(walker)
        } else {
            -1
        }
    }

    pub(crate) fn attacking(&self, slot: Slot) -> bool {
        self.obs.controller(slot) != self.me && self.flags(slot).has(ObjectFlags::ATTACKING)
    }

    pub(crate) fn blocked(&self, attacker: Slot) -> bool {
        self.obs.battlefield().iter().any(|&b| {
            let o = self.obs.object(b);
            o.flags().has(ObjectFlags::BLOCKING) && o.combat() == Some(attacker)
        })
    }

    /// The damage the attackers not yet blocked would deal us.
    pub(crate) fn unblocked_damage(&self) -> i32 {
        self.obs
            .battlefield()
            .iter()
            .filter(|&&a| {
                self.attacking(a)
                    && !self.flags(a).has(ObjectFlags::ATTACKS_PLANESWALKER)
                    && !self.blocked(a)
            })
            .map(|&a| i32::from(self.obs.derived(a).power.max(0)))
            .sum()
    }

    /// The score of `blocker` blocking `attacker`, if it is a block worth making: killing it
    /// and surviving, trading up, surviving, or chumping when the damage would be lethal.
    pub(crate) fn block(&self, blocker: Slot, attacker: Slot) -> Option<i32> {
        if !self.attacking(attacker) || self.blocked(attacker) || self.evades(attacker, blocker) {
            return None;
        }
        let (kill, die) = (self.kills(blocker, attacker), self.kills(attacker, blocker));
        let (va, vb) = (self.value(attacker), self.value(blocker));
        let power = i32::from(self.obs.derived(attacker).power.max(0));
        match (kill, die) {
            (true, false) => Some(300 + va),
            (true, true) if va >= vb => Some(200 + va - vb),
            (false, false) => Some(100 + power),
            _ if self.unblocked_damage() >= i32::from(self.obs.player(self.me).life())
                && !self.flags(attacker).has(ObjectFlags::ATTACKS_PLANESWALKER) =>
            {
                // Above declining (0) at any blocker value, the cheapest blocker first.
                Some(20 + power - vb / 10)
            }
            _ => None,
        }
    }

    pub(crate) fn best_block(&self, blocker: Slot) -> Option<i32> {
        self.obs
            .battlefield()
            .iter()
            .filter_map(|&a| self.block(blocker, a))
            .max()
    }

    /// The creature just declared as a blocker, whose attacker is being chosen: ours,
    /// blocking, and blocking nothing yet (CR 509.1a).
    pub(crate) fn declaring_blocker(&self) -> Option<Slot> {
        self.obs.battlefield().iter().copied().find(|&b| {
            let o = self.obs.object(b);
            self.obs.controller(b) == self.me
                && o.flags().has(ObjectFlags::BLOCKING)
                && o.combat().is_none()
        })
    }
}

/// Whose objects a filter requires, if it says.
pub(crate) fn whose(f: &Filter) -> Option<&'static Players> {
    match f {
        Filter::Whose(players) => Some(players),
        Filter::All(fs) => fs.iter().find_map(whose),
        _ => None,
    }
}

/// The index of one of a face's abilities.
pub(crate) fn index_of(face: &Face, a: &Ability) -> u8 {
    face.abilities
        .iter()
        .position(|b| std::ptr::eq(a, b))
        .map_or(u8::MAX, |i| i as u8)
}

/// The rollout policy as an inline answer source (§7.3): a resolving body asks it and runs on,
/// so a rollout never pauses.
pub struct Inline<'a> {
    pub policy: &'a RolloutPolicy,
    pub t: &'a GameTable,
}

impl Answers for Inline<'_> {
    fn inline(&self) -> bool {
        true
    }

    fn answer(&mut self, obs: Observation<'_>, d: &Decision) -> Option<Choice> {
        Some(self.policy.choose(self.t, obs, d))
    }
}
