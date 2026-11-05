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
//! - Aim harmful effects at the opponent's most valuable permanent or spell, and helpful ones
//!   at your own. A removal spell waits for a target worth [`REMOVAL_MIN`], a counterspell for
//!   a spell worth [`COUNTER_MIN`].
//! - Attack with a creature when no untapped creature that could block it would kill it,
//!   at the opponent's most valuable planeswalker, or at the player once they are at 6 life
//!   or less.
//! - Block when the blocker kills the attacker and survives, trades up, or survives; chump
//!   only when the unblocked damage would be lethal.
//! - Say yes to every "may" and every "unless pays"; keep the cards worth most.

use beryl_engine::card::{
    Ability, AbilityIx, CardDef, CastingOption, Face, FaceAbility, Keywords, TargetKind,
    TargetSpec, Types,
};
use beryl_engine::filter::{Filter, Whose};
use beryl_engine::state::{ObjectFlags, StackEntry};
use beryl_engine::{
    Answers, Candidates, Choice, Decision, DecisionKind, GameTable, Observation, Opt, Outcome,
    Scratch, Seat, Slot, State, Step, Zone, apply, next,
};

use crate::values::{CardInfo, CardValues};

/// The least value of a permanent worth a removal spell or ability.
pub const REMOVAL_MIN: i32 = 25;

/// The least value of a spell worth countering.
pub const COUNTER_MIN: i32 = 40;

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
enum Candidate {
    Object(Slot),
    Option(Opt),
}

impl Candidate {
    fn option(self) -> Option<Opt> {
        match self {
            Candidate::Option(o) => Some(o),
            Candidate::Object(_) => None,
        }
    }

    fn object(self) -> Option<Slot> {
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

    /// The policy's answer to `d`, seen by `d.player` in `obs`.
    pub fn choose(&self, t: &GameTable, obs: Observation<'_>, d: &Decision) -> Choice {
        let p = Policy {
            values: &self.values,
            t,
            obs,
            me: d.player,
        };
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
            DecisionKind::YesNo | DecisionKind::PayLife => {
                let yes = d.kind == DecisionKind::YesNo || obs.player(p.me).life > 10;
                best(d, |c| {
                    Some(i32::from((c.option() == Some(Opt::Yes)) == yes))
                })
            }
            DecisionKind::PutOnBottom | DecisionKind::Discard => {
                best(d, |c| c.object().map(|slot| -p.in_hand(slot)))
            }
            DecisionKind::ChooseObject => best(d, |c| {
                c.object().map(|slot| {
                    let o = obs.object(slot);
                    if o.zone == Zone::Hand && o.owner == p.me {
                        -p.in_hand(slot)
                    } else {
                        p.value(slot)
                    }
                })
            }),
            DecisionKind::ChooseUpTo | DecisionKind::LegendRule => {
                best(d, |c| c.object().map(|slot| p.value(slot)))
            }
            DecisionKind::Crew => best(d, |c| c.object().map(|slot| -p.value(slot))),
            DecisionKind::Waterbend => best(d, |c| Some(i32::from(c.option() == Some(Opt::Done)))),
            DecisionKind::Attacker => best(d, |c| match c.option() {
                Some(Opt::Attack(slot)) => p.safe_attack(slot).then(|| 1 + p.value(slot)),
                _ => Some(0),
            }),
            DecisionKind::AttackTarget => best(d, |c| match c.option() {
                Some(Opt::Defender(None)) => Some(if obs.player(p.me.opponent()).life <= 6 {
                    1000
                } else {
                    0
                }),
                Some(Opt::Defender(Some(walker))) => Some(p.value(walker)),
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
            DecisionKind::TriggerOrder | DecisionKind::PaymentPlan | DecisionKind::ManaPayment => {
                Choice(0)
            }
        }
    }
}

/// The candidate with the highest score, the first of equals. A candidate scored `None` is
/// never chosen over one with a score; when none has one, the first is.
fn best(d: &Decision, mut score: impl FnMut(Candidate) -> Option<i32>) -> Choice {
    let mut top = (0, i32::MIN);
    let mut consider = |i: usize, c: Candidate| {
        if let Some(s) = score(c)
            && s > top.1
        {
            top = (i, s);
        }
    };
    match &d.candidates {
        Candidates::Objects(b) => b
            .iter()
            .enumerate()
            .for_each(|(i, slot)| consider(i, Candidate::Object(slot))),
        Candidates::Options(o) => o
            .iter()
            .enumerate()
            .for_each(|(i, &opt)| consider(i, Candidate::Option(opt))),
    }
    Choice(top.0 as u8)
}

/// One decision's view, for the rules above.
struct Policy<'a> {
    values: &'a CardValues,
    t: &'a GameTable,
    obs: Observation<'a>,
    me: Seat,
}

impl Policy<'_> {
    fn info(&self, slot: Slot) -> Option<CardInfo> {
        self.obs.card(slot).map(|card| self.values.get(card))
    }

    /// What an object is worth: its card's value, plus a creature's power and a
    /// planeswalker's loyalty as they stand on the battlefield, where a land is worth at most
    /// [`LAND_MAX`]. An unknown card is worth 0.
    fn value(&self, slot: Slot) -> i32 {
        let Some(info) = self.info(slot) else {
            return 0;
        };
        let mut v = i32::from(info.value);
        let o = self.obs.object(slot);
        if o.zone == Zone::Battlefield {
            let d = self.obs.derived(slot);
            // A modal double-faced card played as its land face is worth a land.
            if d.types.intersects(Types::LAND) {
                v = v.min(LAND_MAX);
            }
            if d.types.intersects(Types::CREATURE) {
                v += 3 * i32::from(d.power.max(0));
            }
            if d.types.intersects(Types::PLANESWALKER) {
                v += 4 * i32::from(o.counters.loyalty);
            }
        }
        v
    }

    fn is_land(&self, slot: Slot) -> bool {
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
    fn in_hand(&self, slot: Slot) -> i32 {
        if !self.is_land(slot) {
            return self.value(slot);
        }
        let mine = |zone| self.obs.zone(self.me, zone);
        let lands = (mine(Zone::Hand) | mine(Zone::Battlefield))
            .iter()
            .filter(|&s| self.is_land(s))
            .count();
        if lands > 5 { 5 } else { 80 }
    }

    /// Keep a hand of two to five lands, and any hand of five cards or fewer.
    fn keep(&self) -> bool {
        let hand = self.obs.zone(self.me, Zone::Hand);
        let lands = hand.iter().filter(|&slot| self.is_land(slot)).count();
        hand.count() <= 5 || (2..=5).contains(&lands)
    }

    /// The score of a priority action or a cast offer; 0 is passing.
    fn action(&self, o: Opt) -> i32 {
        match o {
            Opt::PlayLand(slot) => 1000 - self.value(slot),
            Opt::Cast { slot, face, costs } => self.cast(slot, face, costs),
            Opt::Activate { slot, ability } => match ability {
                AbilityIx::Own(i) => self.activate(slot, i),
                AbilityIx::Granted(_) => 0,
            },
            _ => 0,
        }
    }

    fn cast(&self, slot: Slot, face: u8, costs: u8) -> i32 {
        let Some(card) = self.obs.card(slot) else {
            return 0;
        };
        let info = self.values.get(card);
        let f = &self.t.def(card).faces[usize::from(face)];
        if info.wheel && self.obs.zone(self.me, Zone::Hand).count() > 3 {
            return 0;
        }
        let mut v = i32::from(info.value);
        for (i, option) in f.casting.iter().enumerate() {
            if costs & (1 << i) != 0 {
                v += match option {
                    CastingOption::Kicker { .. } | CastingOption::Offspring(_) => 10,
                    CastingOption::Impending { .. } => -10,
                };
            }
        }
        match f.spell_ability() {
            Some(a) => self.worth_targeting(a, info.friendly(index_of(f, a)), v),
            None => v,
        }
    }

    fn activate(&self, slot: Slot, i: u8) -> i32 {
        let Some(card) = self.obs.card(slot) else {
            return 0;
        };
        let info = self.values.get(card);
        if info.never(i) {
            return 0;
        }
        let o = self.obs.object(slot);
        let a = &self.t.def(card).faces[usize::from(o.face)].abilities[usize::from(i)];
        let v = match a.cost.loyalty {
            Some(l) if l >= 0 => 20,
            Some(l) if l <= -8 => 200,
            Some(l) => 20 + 5 * i32::from(-l),
            None if a.cost.discard_self => 3,
            None => 15,
        };
        self.worth_targeting(a, info.friendly(i), v)
    }

    /// `v`, or 0 when the ability's first target is not worth it: a harmful one needs an
    /// opponent's object worth the removal or counter threshold, unless it may target
    /// nothing; a helpful one needs one of ours.
    fn worth_targeting(&self, a: &Ability, friendly: bool, v: i32) -> i32 {
        let Some(spec) = a.targets.first() else {
            return v;
        };
        let friendly = friendly || whose(&spec.filter) == Some(Whose::You);
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
    fn best_target(&self, spec: &TargetSpec, friendly: bool) -> Option<i32> {
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

    /// The spec being targeted for and whether its effect helps its controller: the first
    /// spec of the spell or ability on top of the stack, which a casting or a triggered
    /// ability being put on the stack puts there first (CR 601.2a).
    fn targeting(&self) -> Option<(&'static TargetSpec, bool)> {
        let e: &StackEntry = self.obs.stack().last()?;
        let def: &'static CardDef = self.t.def(e.card);
        let face: &'static Face = &def.faces[usize::from(e.face)];
        let a = match e.ability {
            Some(ability) => face.ability(ability),
            None => face.spell_ability()?,
        };
        let spec = a.targets.first()?;
        let index = match e.ability {
            Some(FaceAbility::Own(i)) => i,
            Some(FaceAbility::Grant(_)) => u8::MAX,
            None => index_of(face, a),
        };
        let friendly =
            self.values.get(e.card).friendly(index) || whose(&spec.filter) == Some(Whose::You);
        Some((spec, friendly))
    }

    /// A target's score: a candidate on the right side by its value, far above the wrong
    /// side; choosing none in between.
    fn target(&self, c: Candidate, friendly: bool) -> i32 {
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
                    let controller = if matches!(o.zone, Zone::Battlefield | Zone::Stack) {
                        self.obs.controller(slot)
                    } else {
                        o.owner
                    };
                    (controller, self.value(slot))
                }
                None => return 0,
            },
        };
        if controller == side { 1000 + v } else { -v }
    }

    fn creature(&self, slot: Slot) -> bool {
        self.obs.derived(slot).types.intersects(Types::CREATURE)
    }

    fn flags(&self, slot: Slot) -> ObjectFlags {
        self.obs.object(slot).flags
    }

    /// Whether `blocker` could block `attacker`, by evasion
    /// ([CR 702.9b](https://yawgatog.com/resources/magic-rules/#R7029b)): "A creature with
    /// flying can't be blocked except by creatures with flying and/or reach."
    fn evades(&self, attacker: Slot, blocker: Slot) -> bool {
        let has = |slot: Slot, k: Keywords| self.obs.derived(slot).keywords.intersects(k);
        has(attacker, Keywords::FLYING) && !has(blocker, Keywords::FLYING.union(Keywords::REACH))
    }

    /// Whether `by`'s power is at least the damage left to kill `slot`.
    fn kills(&self, by: Slot, slot: Slot) -> bool {
        let left =
            i16::from(self.obs.derived(slot).toughness) - i16::from(self.obs.object(slot).damage);
        i16::from(self.obs.derived(by).power) >= left
    }

    /// No untapped creature of the opponent that could block `attacker` would kill it, and
    /// it deals damage.
    fn safe_attack(&self, attacker: Slot) -> bool {
        self.obs.derived(attacker).power > 0
            && !self.obs.battlefield().iter().any(|&b| {
                self.obs.controller(b) != self.me
                    && self.creature(b)
                    && !self.flags(b).has(ObjectFlags::TAPPED)
                    && !self.evades(attacker, b)
                    && self.kills(b, attacker)
            })
    }

    fn attacking(&self, slot: Slot) -> bool {
        self.obs.controller(slot) != self.me && self.flags(slot).has(ObjectFlags::ATTACKING)
    }

    fn blocked(&self, attacker: Slot) -> bool {
        self.obs.battlefield().iter().any(|&b| {
            let o = self.obs.object(b);
            o.flags.has(ObjectFlags::BLOCKING) && o.combat == Some(attacker)
        })
    }

    /// The damage the attackers not yet blocked would deal us.
    fn unblocked_damage(&self) -> i32 {
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
    fn block(&self, blocker: Slot, attacker: Slot) -> Option<i32> {
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
            _ if self.unblocked_damage() >= i32::from(self.obs.player(self.me).life)
                && !self.flags(attacker).has(ObjectFlags::ATTACKS_PLANESWALKER) =>
            {
                // Above declining (0) at any blocker value, the cheapest blocker first.
                Some(20 + power - vb / 10)
            }
            _ => None,
        }
    }

    fn best_block(&self, blocker: Slot) -> Option<i32> {
        self.obs
            .battlefield()
            .iter()
            .filter_map(|&a| self.block(blocker, a))
            .max()
    }

    /// The creature just declared as a blocker, whose attacker is being chosen: ours,
    /// blocking, and blocking nothing yet (CR 509.1a).
    fn declaring_blocker(&self) -> Option<Slot> {
        self.obs.battlefield().iter().copied().find(|&b| {
            let o = self.obs.object(b);
            self.obs.controller(b) == self.me
                && o.flags.has(ObjectFlags::BLOCKING)
                && o.combat.is_none()
        })
    }
}

/// Whose objects a filter requires, if it says.
fn whose(f: &Filter) -> Option<Whose> {
    match f {
        Filter::Whose(w) => Some(*w),
        Filter::All(fs) => fs.iter().find_map(whose),
        _ => None,
    }
}

/// The index of one of a face's abilities.
fn index_of(face: &Face, a: &Ability) -> u8 {
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

/// Plays a game to its end under the rollout policy in both seats, every question answered
/// inline: the rollout loop search runs (§19.1). It copies nothing and allocates nothing.
pub fn rollout(s: &mut State, t: &GameTable, x: &mut Scratch, policy: &RolloutPolicy) -> Outcome {
    let mut inline = Inline { policy, t };
    loop {
        match next(s, t, x, &mut inline) {
            Step::Over(outcome) => return outcome,
            Step::Decide(d) => {
                let c = policy.choose(t, Observation::new(s, d.player), &d);
                apply(s, t, x, &mut inline, c);
            }
        }
    }
}
