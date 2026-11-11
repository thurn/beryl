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
//! - Say yes to every "may" and every "unless pays"; keep the cards worth most, take the best
//!   a search finds, and scry to the bottom only the cards not worth drawing.

use beryl_engine::layout::Queue;
use beryl_engine::{Layout, Small, ZoneKey};
use std::cell::Cell;

use beryl_engine::card::{
    Ability, AbilityKind, CardDef, CastingOption, Castings, Face, FaceAbility, FaceIx, Keywords,
    ObjectAbility, Purpose, Replacement, TargetKind, TargetSpec, Trigger, Types,
};
use beryl_engine::filter::{Filter, Whose};
use beryl_engine::state::{ObjectFlags, StackEntry};
use beryl_engine::{
    Answers, Candidates, Choice, Decision, DecisionKind, GameTable, Observation, Opt, Outcome,
    Scratch, Seat, State, Step, Zone, apply_decided, next,
};

use beryl_engine::rng::SplitMix64;

use crate::values::{CardInfo, CardValues, Knowledge};

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
pub(crate) enum Candidate<L: Layout = Small> {
    Object(L::Slot),
    Option(Opt<L>),
}

impl<L: Layout> Candidate<L> {
    pub(crate) fn option(self) -> Option<Opt<L>> {
        match self {
            Candidate::Option(o) => Some(o),
            Candidate::Object(_) => None,
        }
    }

    pub(crate) fn object(self) -> Option<L::Slot> {
        match self {
            Candidate::Object(slot) | Candidate::Option(Opt::Object(slot)) => Some(slot),
            Candidate::Option(_) => None,
        }
    }
}

impl RolloutPolicy {
    pub fn new(t: &GameTable) -> RolloutPolicy {
        Self::with(t, Knowledge::Tuned)
    }

    /// The policy with the card values of tier `knowledge`.
    pub fn with(t: &GameTable, knowledge: Knowledge) -> RolloutPolicy {
        RolloutPolicy {
            values: CardValues::with(t, knowledge),
        }
    }

    /// Its card values.
    pub fn values(&self) -> &CardValues {
        &self.values
    }

    /// The rules' view of one decision of `me`'s, for agents that build on them: with
    /// `expert`, valuing permanents as the expert does.
    pub(crate) fn view<'a, L: Layout>(
        &'a self,
        t: &'a GameTable,
        obs: Observation<'a, L>,
        me: Seat,
        expert: bool,
    ) -> Policy<'a, L> {
        Policy {
            values: &self.values,
            t,
            obs,
            me,
            expert,
            may_draw: Cell::new(None),
        }
    }

    /// The policy's answer to `d`, seen by `d.player` in `obs`.
    pub fn choose<L: Layout>(
        &self,
        t: &GameTable,
        obs: Observation<'_, L>,
        d: &Decision<L>,
    ) -> Choice {
        self.decide(t, obs, d, false)
    }

    /// The policy's answer to `d`, with `expert`, valuing permanents as the expert does.
    pub(crate) fn decide<L: Layout>(
        &self,
        t: &GameTable,
        obs: Observation<'_, L>,
        d: &Decision<L>,
        expert: bool,
    ) -> Choice {
        self.decide_by(t, obs, d, expert, &mut Pick::Best)
    }

    /// The policy's answer to `d` as a search's rollout plays it: each candidate drawn with
    /// a weight of exp(score / temperature) ([`Softmax`]) wherever the rules score candidates
    /// by value, so that two rollouts from one position need not play the same game.
    pub fn choose_soft<L: Layout>(
        &self,
        t: &GameTable,
        obs: Observation<'_, L>,
        d: &Decision<L>,
        soft: &mut Softmax<'_>,
    ) -> Choice {
        self.decide_by(t, obs, d, false, &mut Pick::Soft(soft))
    }

    /// The rules' answer to `d`, its scores made a choice by `pick`. The decisions scored
    /// yes or no (mulligans, play or draw, a "may") and the numbers chosen are always the
    /// best: their scores are flags, not values a temperature could weigh.
    fn decide_by<L: Layout>(
        &self,
        t: &GameTable,
        obs: Observation<'_, L>,
        d: &Decision<L>,
        expert: bool,
        pick: &mut Pick<'_, '_>,
    ) -> Choice {
        let p = self.view(t, obs, d.player, expert);
        match d.kind {
            DecisionKind::Priority | DecisionKind::CastOffer => {
                pick.choose(d, |c| c.option().map(|o| p.action(o)))
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
            DecisionKind::PutOnBottom | DecisionKind::Discard | DecisionKind::ExileCost => {
                pick.choose(d, |c| c.object().map(|slot| -p.in_hand(slot)))
            }
            DecisionKind::ChooseObject => pick.choose(d, |c| {
                c.object().map(|slot| {
                    let o = obs.object(slot);
                    if o.zone == Zone::Hand && o.owner == p.me {
                        -p.in_hand(slot)
                    } else {
                        p.value(slot)
                    }
                })
            }),
            // A search's finds are worth taking, the best first, as Winds of Abandon's basic.
            // Other cards on top of a library are being chosen for the bottom, as a scry asks:
            // only those not worth drawing go. Anything else, such as lands to untap, is worth
            // taking by its value.
            DecisionKind::ChooseUpTo => pick.choose(d, |c| match c.object() {
                Some(slot) if d.prompt.purpose == Purpose::Search => {
                    Some(1 + p.in_hand(slot).max(0))
                }
                Some(slot) if obs.object(slot).zone == Zone::Library => {
                    Some(DRAW_MIN - p.in_hand(slot))
                }
                Some(slot) => Some(p.value(slot)),
                None => Some(0),
            }),
            DecisionKind::LegendRule => pick.choose(d, |c| c.object().map(|slot| p.value(slot))),
            DecisionKind::Crew => pick.choose(d, |c| c.object().map(|slot| -p.value(slot))),
            DecisionKind::Waterbend => best(d, |c| Some(i32::from(c.option() == Some(Opt::Done)))),
            DecisionKind::Attacker => pick.choose(d, |c| match c.option() {
                Some(Opt::Attack(slot)) => p.safe_attack(slot).then(|| 1 + p.value(slot)),
                _ => Some(0),
            }),
            DecisionKind::AttackTarget => pick.choose(d, |c| match c.option() {
                Some(Opt::Defender(None)) => Some(if obs.player(p.me.opponent()).life <= 6 {
                    1000
                } else {
                    0
                }),
                Some(Opt::Defender(Some(walker))) => Some(p.walker_target(walker)),
                _ => None,
            }),
            DecisionKind::Blocker => pick.choose(d, |c| match c.option() {
                Some(Opt::Block(blocker)) => p.best_block(blocker),
                _ => Some(0),
            }),
            DecisionKind::BlockTarget => {
                let blocker = p.declaring_blocker();
                pick.choose(d, |c| {
                    let attacker = c.object()?;
                    Some(blocker.and_then(|b| p.block(b, attacker)).unwrap_or(0))
                })
            }
            DecisionKind::DamageDivision => pick.choose(d, |c| match c.option() {
                Some(Opt::Lethal(blocker)) => Some(1 + p.value(blocker)),
                _ => Some(0),
            }),
            DecisionKind::Target => {
                let friendly = p.targeting().is_some_and(|(_, friendly)| friendly);
                pick.choose(d, |c| Some(p.target(c, friendly)))
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
pub(crate) fn best<L: Layout>(
    d: &Decision<L>,
    mut score: impl FnMut(Candidate<L>) -> Option<i32>,
) -> Choice {
    let mut top = (0, i32::MIN);
    let mut consider = |i: usize, c: Candidate<L>| {
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
    Choice(top.0 as u16)
}

/// How a decision's scores become a choice.
enum Pick<'s, 'r> {
    /// The highest score, the first of equals ([`best`]).
    Best,
    /// A draw by the scores ([`Softmax`]).
    Soft(&'s mut Softmax<'r>),
}

impl Pick<'_, '_> {
    fn choose<L: Layout>(
        &mut self,
        d: &Decision<L>,
        score: impl FnMut(Candidate<L>) -> Option<i32>,
    ) -> Choice {
        match self {
            Pick::Best => best(d, score),
            Pick::Soft(soft) => soft.choose(d, score),
        }
    }
}

/// Softmax rollouts for search: a candidate is drawn with a weight of exp((score − best
/// score) / temperature), so the greedy answer stays the likeliest and one a few points behind
/// it is played now and then. A greedy rollout is a fixed function of the position, so in a
/// world the search fully knows every rollout from a node replays one game and the tree
/// learns to exploit that one line; a drawn rollout is a distribution over games, as a world
/// it does not know is. A candidate scored `None` is never drawn while one has a score, nor
/// is one that ties an earlier candidate.
#[derive(Debug)]
pub struct Softmax<'r> {
    pub rng: &'r mut SplitMix64,
    /// In score points: a candidate this far behind the best is drawn 1/e as often.
    pub temperature: f64,
}

impl Softmax<'_> {
    fn choose<L: Layout>(
        &mut self,
        d: &Decision<L>,
        mut score: impl FnMut(Candidate<L>) -> Option<i32>,
    ) -> Choice {
        let mut scores = [None; 256];
        let mut n = 0;
        let mut top = i32::MIN;
        let mut consider = |i: usize, c: Candidate<L>| {
            let s = score(c);
            if let Some(s) = s {
                top = top.max(s);
            }
            scores[i] = s;
            n = i + 1;
        };
        for_each(d, &mut consider);
        if top == i32::MIN {
            return Choice(0);
        }
        // Equals are drawn as one, as the first of them: greedy plays the first of equals, and
        // the options it scores 0 (passing, a mana ability, a card it never casts) are not
        // alternatives to passing.
        let scores = &scores[..n];
        let weight = |i: usize| match scores[i] {
            Some(s) if !scores[..i].contains(&Some(s)) => {
                (f64::from(s - top) / self.temperature).exp()
            }
            _ => 0.0,
        };
        let total: f64 = (0..n).map(weight).sum();
        // A uniform draw in [0, total), from the top 53 bits.
        let mut u = (self.rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64 * total;
        let mut last = 0;
        for i in 0..n {
            let w = weight(i);
            if w > 0.0 {
                last = i;
                if u < w {
                    return Choice(i as u16);
                }
                u -= w;
            }
        }
        // Rounding left the draw past the last weight.
        Choice(last as u16)
    }
}

/// Calls `f` with each candidate of `d` and its index.
fn for_each<L: Layout>(d: &Decision<L>, mut f: impl FnMut(usize, Candidate<L>)) {
    match &d.candidates {
        Candidates::Objects(b) => b
            .iter()
            .enumerate()
            .for_each(|(i, slot)| f(i, Candidate::Object(slot))),
        Candidates::Options(o) => o
            .iter()
            .enumerate()
            .for_each(|(i, &opt)| f(i, Candidate::Option(opt))),
    }
}

/// One decision's view, for the rules above.
pub(crate) struct Policy<'a, L: Layout> {
    pub(crate) values: &'a CardValues,
    pub(crate) t: &'a GameTable,
    pub(crate) obs: Observation<'a, L>,
    pub(crate) me: Seat,
    /// Whether permanents are valued as the expert values them ([`crate::expert`]).
    pub(crate) expert: bool,
    /// [`Policy::may_draw`], once asked: every option of a decision may ask it.
    may_draw: Cell<Option<bool>>,
}

impl<L: Layout> Policy<'_, L> {
    pub(crate) fn info(&self, slot: L::Slot) -> Option<CardInfo> {
        self.obs.card(slot).map(|card| self.values.get(card))
    }

    /// What an object is worth: its card's value, plus a creature's power and a
    /// planeswalker's loyalty as they stand on the battlefield, where a land is worth at most
    /// [`LAND_MAX`]. An unknown card is worth 0.
    pub(crate) fn value(&self, slot: L::Slot) -> i32 {
        let Some(info) = self.info(slot) else {
            return 0;
        };
        let mut v = i32::from(info.value);
        let o = self.obs.object(slot);
        if o.zone == Zone::Battlefield {
            if self.expert
                && let Some(v) = crate::expert::appraise(self, slot, v)
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
                v += 4 * i32::from(o.counters.loyalty);
            }
        }
        v
    }

    pub(crate) fn is_land(&self, slot: L::Slot) -> bool {
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
    pub(crate) fn in_hand(&self, slot: L::Slot) -> i32 {
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
            .iter()
            .filter(|&s| self.is_land(s))
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
    /// [`DecisionKind::PutOnBottom`] picks them.
    pub(crate) fn keep(&self) -> bool {
        let mut kept = self.obs.zone(ZoneKey::Of(self.me, Zone::Hand));
        for _ in 0..self.obs.player(self.me).mulligans {
            match kept.iter().min_by_key(|&slot| self.in_hand(slot)) {
                Some(least) => kept.remove(least),
                None => break,
            }
        }
        let lands = kept.iter().filter(|&slot| self.is_land(slot)).count();
        kept.count() <= 5 || (2..=5).contains(&lands)
    }

    /// The score of a priority action or a cast offer; 0 is passing.
    pub(crate) fn action(&self, o: Opt<L>) -> i32 {
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

    pub(crate) fn cast(&self, slot: L::Slot, face: FaceIx, costs: Castings) -> i32 {
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
        let drains = info.draws != 0
            && f.abilities
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
            None => self.enters_worth(f, info, v),
        }
    }

    /// `v` for a permanent spell, or 0 for a noncreature one whose targeted enters trigger
    /// would find nothing worth it: Portable Hole cast onto a board with no nonland permanent of
    /// mana value 2 or less exiles nothing, for the trigger is removed from the stack
    /// ([CR 603.3d](https://yawgatog.com/resources/magic-rules/#R6033d)): "If no legal targets
    /// can be chosen, the ability is removed from the stack." A creature keeps its body.
    fn enters_worth(&self, f: &Face, info: CardInfo, v: i32) -> i32 {
        if f.types.intersects(Types::CREATURE) {
            return v;
        }
        f.abilities
            .iter()
            .filter(|a| {
                a.kind == AbilityKind::Triggered
                    && a.trigger == Trigger::Enters
                    && !a.targets.is_empty()
            })
            .fold(v, |v, a| {
                self.worth_targeting(a, info.friendly(index_of(f, a)), v)
            })
    }

    pub(crate) fn activate(&self, slot: L::Slot, i: u8) -> i32 {
        let Some(card) = self.obs.card(slot) else {
            return 0;
        };
        let info = self.values.get(card);
        if info.never(i) {
            return 0;
        }
        let o = self.obs.object(slot);
        if o.face == FaceIx::FRONT && info.draws(i) && !self.may_draw() {
            return 0;
        }
        if o.face == FaceIx::FRONT && info.empties(i) {
            return if self.obs.library(self.me).len() <= 7 {
                1000
            } else {
                0
            };
        }
        let a = &self.t.def(card).face(o.face).abilities[usize::from(i)];
        let v = match a.cost.loyalty {
            Some(l) if l >= 0 => 20,
            Some(l) if l <= -8 => 200,
            Some(l) => 20 + 5 * i32::from(-l),
            None if a.cost.discard_self => 3,
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
        if let Some(may) = self.may_draw.get() {
            return may;
        }
        let may = self.find_may_draw();
        self.may_draw.set(Some(may));
        may
    }

    fn find_may_draw(&self) -> bool {
        self.obs.library(self.me).len() > LIBRARY_LOW
            || self
                .obs
                .zone(ZoneKey::Of(self.me, Zone::Hand))
                .iter()
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
                    self.values.get(card).wins_instead
                        && self
                            .t
                            .def(card)
                            .face(self.obs.object(slot).face)
                            .abilities
                            .iter()
                            .any(|a| {
                                a.replacement == Replacement::WinInsteadOfDrawingFromEmptyLibrary
                            })
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
        let friendly = friendly || whose(&spec.filter) == Some(Whose::You);
        let min = match spec.kind {
            _ if friendly => 0,
            TargetKind::Spell => COUNTER_MIN,
            TargetKind::Player => 0,
            _ => REMOVAL_MIN,
        };
        // The best target is worth `min` exactly when some target is.
        if self.target_values(spec, friendly).any(|best| best >= min) {
            v
        } else if spec.optional && !matches!(spec.kind, TargetKind::Spell) {
            v / 2
        } else {
            0
        }
    }

    /// The most valuable object a spec could target on the side it should: the opponent's
    /// for a harmful effect, ours for a helpful one. Players count as 0.
    pub(crate) fn best_target(&self, spec: &TargetSpec, friendly: bool) -> Option<i32> {
        self.target_values(spec, friendly).max()
    }

    /// What each object a spec could target on the side it should is worth, as
    /// [`Policy::best_target`] weighs them, players as 0.
    fn target_values<'s>(
        &'s self,
        spec: &'s TargetSpec,
        friendly: bool,
    ) -> impl Iterator<Item = i32> + 's {
        let side = if friendly {
            self.me
        } else {
            self.me.opponent()
        };
        let ok = move |slot: L::Slot, controller: Seat| {
            controller == side && self.obs.matches(self.t, &spec.filter, slot)
        };
        let permanents = matches!(
            spec.kind,
            TargetKind::Permanent | TargetKind::SpellOrPermanent
        );
        let spells = matches!(spec.kind, TargetKind::Spell | TargetKind::SpellOrPermanent);
        let graveyard = spec.kind == TargetKind::GraveyardCard;
        let on_battlefield = if permanents {
            self.obs.battlefield()
        } else {
            &[]
        }
        .iter()
        .filter(move |&&slot| ok(slot, self.obs.controller(slot)))
        .map(|&slot| self.value(slot));
        let on_stack = if spells { self.obs.stack() } else { &[] }
            .iter()
            .filter_map(|e| e.spell().map(|r| (r.slot, e.controller)))
            .filter(move |&(slot, controller)| ok(slot, controller))
            .map(|(slot, _)| self.value(slot));
        let graveyard = if graveyard {
            self.obs.graveyard(side)
        } else {
            &[]
        }
        .iter()
        .filter(|&&slot| self.obs.matches(self.t, &spec.filter, slot))
        .map(|&slot| self.value(slot));
        let player = (spec.kind == TargetKind::Player).then_some(0);
        on_battlefield
            .chain(on_stack)
            .chain(graveyard)
            .chain(player)
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
        let e: &StackEntry<L> = self.obs.stack().last()?;
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
        let friendly =
            self.values.get(e.card).friendly(index) || whose(&spec.filter) == Some(Whose::You);
        Some((spec, friendly))
    }

    /// A target's score: a candidate on the right side by its value, far above the wrong
    /// side; choosing none in between.
    pub(crate) fn target(&self, c: Candidate<L>, friendly: bool) -> i32 {
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

    pub(crate) fn creature(&self, slot: L::Slot) -> bool {
        self.obs.derived(slot).types.intersects(Types::CREATURE)
    }

    pub(crate) fn flags(&self, slot: L::Slot) -> ObjectFlags {
        self.obs.object(slot).flags
    }

    /// Whether `blocker` could block `attacker`, by evasion
    /// ([CR 702.9b](https://yawgatog.com/resources/magic-rules/#R7029b)): "A creature with
    /// flying can't be blocked except by creatures with flying and/or reach."
    pub(crate) fn evades(&self, attacker: L::Slot, blocker: L::Slot) -> bool {
        let has = |slot: L::Slot, k: Keywords| self.obs.derived(slot).keywords.intersects(k);
        has(attacker, Keywords::FLYING) && !has(blocker, Keywords::FLYING.union(Keywords::REACH))
    }

    /// Whether `by`'s power is at least the damage left to kill `slot`.
    pub(crate) fn kills(&self, by: L::Slot, slot: L::Slot) -> bool {
        let left =
            i16::from(self.obs.derived(slot).toughness) - i16::from(self.obs.object(slot).damage);
        i16::from(self.obs.derived(by).power) >= left
    }

    /// No untapped creature of the opponent that could block `attacker` would kill it, and
    /// it deals damage.
    pub(crate) fn safe_attack(&self, attacker: L::Slot) -> bool {
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
    pub(crate) fn walker_target(&self, walker: L::Slot) -> i32 {
        let sent: i32 = self
            .obs
            .battlefield()
            .iter()
            .filter(|&&a| {
                let o = self.obs.object(a);
                self.obs.controller(a) == self.me
                    && o.flags.has(ObjectFlags::ATTACKING)
                    && o.combat == Some(walker)
            })
            .map(|&a| i32::from(self.obs.derived(a).power.max(0)))
            .sum();
        if sent < i32::from(self.obs.object(walker).counters.loyalty) {
            self.value(walker)
        } else {
            -1
        }
    }

    pub(crate) fn attacking(&self, slot: L::Slot) -> bool {
        self.obs.controller(slot) != self.me && self.flags(slot).has(ObjectFlags::ATTACKING)
    }

    pub(crate) fn blocked(&self, attacker: L::Slot) -> bool {
        self.obs.battlefield().iter().any(|&b| {
            let o = self.obs.object(b);
            o.flags.has(ObjectFlags::BLOCKING) && o.combat == Some(attacker)
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
    pub(crate) fn block(&self, blocker: L::Slot, attacker: L::Slot) -> Option<i32> {
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

    pub(crate) fn best_block(&self, blocker: L::Slot) -> Option<i32> {
        self.obs
            .battlefield()
            .iter()
            .filter_map(|&a| self.block(blocker, a))
            .max()
    }

    /// The creature just declared as a blocker, whose attacker is being chosen: ours,
    /// blocking, and blocking nothing yet (CR 509.1a).
    pub(crate) fn declaring_blocker(&self) -> Option<L::Slot> {
        self.obs.battlefield().iter().copied().find(|&b| {
            let o = self.obs.object(b);
            self.obs.controller(b) == self.me
                && o.flags.has(ObjectFlags::BLOCKING)
                && o.combat.is_none()
        })
    }
}

/// Whose objects a filter requires, if it says.
pub(crate) fn whose(f: &Filter) -> Option<Whose> {
    match f {
        Filter::Whose(w) => Some(*w),
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

impl<L: Layout> Answers<L> for Inline<'_> {
    fn inline(&self) -> bool {
        true
    }

    fn answer(&mut self, obs: Observation<'_, L>, d: &Decision<L>) -> Option<Choice> {
        Some(self.policy.choose(self.t, obs, d))
    }
}

/// Plays a game to its end under the rollout policy in both seats, every question answered
/// inline: the rollout loop search runs (§19.1). It copies nothing and allocates nothing.
///
/// Never inlined, so a caller's frame stays apart from the loop's: inlined into
/// `beryl_bench::rollout::counted_rollout`, it left Callgrind unable to follow that frame,
/// and the rollout benchmark counted half the work.
#[inline(never)]
pub fn rollout<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    policy: &RolloutPolicy,
) -> Outcome {
    rollout_to(s, t, x, policy, u16::MAX).expect("a game ends before turn 65535")
}

/// [`rollout`], cut off at the first decision of turn `turn` or later: the game's outcome if
/// it ends before then, else none, the state left at that decision.
pub fn rollout_to<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    policy: &RolloutPolicy,
    turn: u16,
) -> Option<Outcome> {
    let mut inline = Inline { policy, t };
    play_out(s, t, x, &mut inline, turn, |_, obs, d| {
        policy.choose(t, obs, d)
    })
}

/// [`rollout_to`] with every decision, inline ones too, drawn by `soft`
/// ([`RolloutPolicy::choose_soft`]).
pub fn soft_rollout_to<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    policy: &RolloutPolicy,
    soft: &mut Softmax<'_>,
    turn: u16,
) -> Option<Outcome> {
    let mut inline = SoftInline { policy, t, soft };
    play_out(s, t, x, &mut inline, turn, |inline, obs, d| {
        inline.policy.choose_soft(inline.t, obs, d, inline.soft)
    })
}

/// [`soft_rollout_to`] to the end of the game.
pub fn soft_rollout<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    policy: &RolloutPolicy,
    soft: &mut Softmax<'_>,
) -> Outcome {
    soft_rollout_to(s, t, x, policy, soft, u16::MAX).expect("a game ends before turn 65535")
}

/// The rollout loop: `answers` answers inline, `choose` every decision `next` returns.
fn play_out<A: Answers<L>, L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    answers: &mut A,
    turn: u16,
    mut choose: impl FnMut(&mut A, Observation<'_, L>, &Decision<L>) -> Choice,
) -> Option<Outcome> {
    loop {
        match next(s, t, x, answers) {
            Step::Over(outcome) => return Some(outcome),
            Step::Decide(_) if s.turn().number >= turn => return None,
            Step::Decide(d) => {
                let c = choose(answers, Observation::new(s, d.player), &d);
                apply_decided(s, t, x, answers, &d, c);
            }
        }
    }
}

/// [`Inline`] for a softmax rollout.
struct SoftInline<'a, 'r> {
    policy: &'a RolloutPolicy,
    t: &'a GameTable,
    soft: &'a mut Softmax<'r>,
}

impl<L: Layout> Answers<L> for SoftInline<'_, '_> {
    fn inline(&self) -> bool {
        true
    }

    fn answer(&mut self, obs: Observation<'_, L>, d: &Decision<L>) -> Option<Choice> {
        Some(self.policy.choose_soft(self.t, obs, d, self.soft))
    }
}

#[cfg(test)]
mod tests {
    use beryl_cards::CARDS;
    use beryl_cards::decks::WU_CONTROL;
    use beryl_engine::{Profile, Replay, apply, observe};

    use super::*;

    /// A decision of a greedy mirror game with at least `n` candidates.
    fn decision(n: usize) -> Decision {
        let t = GameTable::new(&CARDS, [WU_CONTROL, WU_CONTROL]).unwrap();
        let policy = RolloutPolicy::new(&t);
        let mut s = State::new(&t, 0, Profile::SMART);
        let mut x = Scratch::new();
        loop {
            let Step::Decide(d) = next(&mut s, &t, &mut x, &mut Replay) else {
                panic!("no decision with {n} candidates");
            };
            if d.len() >= n {
                return d;
            }
            let c = policy.choose(&t, observe(&s, &t, &mut x, d.player), &d);
            apply(&mut s, &t, &mut x, &mut Replay, c);
        }
    }

    /// Each candidate is drawn by exp(score / temperature); one scored `None`, or tied with
    /// an earlier one, never is.
    #[test]
    fn softmax_draws_by_score_and_counts_equals_once() {
        let d = decision(4);
        let scores = [Some(0), Some(0), None, Some(10)];
        let mut rng = SplitMix64::new(1);
        let mut drawn = [0u32; 4];
        for _ in 0..20_000 {
            let mut k = 0;
            let c = Softmax {
                rng: &mut rng,
                temperature: 10.0,
            }
            .choose(&d, |_| {
                k += 1;
                scores.get(k - 1).copied().flatten()
            });
            drawn[usize::from(c.0)] += 1;
        }
        assert_eq!((drawn[1], drawn[2]), (0, 0), "{drawn:?}");
        // The best is e times as likely as the one 10 points behind: 73% of draws.
        let best = f64::from(drawn[3]) / 20_000.0;
        assert!((best - 0.731).abs() < 0.015, "{drawn:?}");
    }

    /// With no candidate scored, the first is chosen, as greedy chooses it.
    #[test]
    fn softmax_falls_back_to_the_first() {
        let d = decision(2);
        let mut rng = SplitMix64::new(2);
        let c = Softmax {
            rng: &mut rng,
            temperature: 10.0,
        }
        .choose(&d, |_| None);
        assert_eq!(c, Choice(0));
    }

    /// At a temperature near zero the softmax policy plays greedy's game.
    #[test]
    fn a_cold_softmax_is_greedy() {
        let t = GameTable::new(&CARDS, [WU_CONTROL, WU_CONTROL]).unwrap();
        let policy = RolloutPolicy::new(&t);
        let mut rng = SplitMix64::new(3);
        for seed in 0..4 {
            let start = State::new(&t, seed, Profile::SMART);
            let mut x = Scratch::new();
            let greedy = rollout(&mut start.clone(), &t, &mut x, &policy);
            let mut soft = Softmax {
                rng: &mut rng,
                temperature: 1e-9,
            };
            let cold = soft_rollout(&mut start.clone(), &t, &mut x, &policy, &mut soft);
            assert_eq!(greedy, cold, "seed {seed}");
        }
    }

    /// Greedy casts Portable Hole only with something its enters trigger could exile: a
    /// nonland permanent of the opponent's with mana value 2 or less. Deep Searcher, anchored to
    /// greedy, once cast it onto a board of one land.
    #[test]
    fn greedy_holds_a_portable_hole_with_nothing_to_exile() {
        let t = GameTable::new(&CARDS, [WU_CONTROL, WU_CONTROL]).unwrap();
        let policy = RolloutPolicy::new(&t);
        let hole = beryl_cards::white::portable_hole::CARD.faces[0].abilities[0].targets[0];
        let mut x = Scratch::new();
        let (mut cast, mut held) = (0, 0);
        for seed in 0..60 {
            let mut s = State::new(&t, seed, Profile::SMART);
            while let Step::Decide(d) = next(&mut s, &t, &mut x, &mut Replay) {
                let obs = observe(&s, &t, &mut x, d.player);
                let target = obs.battlefield().iter().any(|&slot| {
                    obs.controller(slot) != d.player && obs.matches(&t, &hole.filter, slot)
                });
                let is_hole = |c: Choice| {
                    matches!(d.candidates.option(c), Some(Opt::Cast { slot, .. })
                        if obs.card(slot).is_some_and(|card| t.def(card).name == beryl_cards::names::PORTABLE_HOLE))
                };
                let g = policy.choose(&t, obs, &d);
                if is_hole(g) {
                    assert!(
                        target,
                        "seed {seed}: Portable Hole cast with nothing to exile"
                    );
                    cast += 1;
                } else if !target && (0..d.len()).any(|i| is_hole(Choice(i as u16))) {
                    held += 1;
                }
                apply(&mut s, &t, &mut x, &mut Replay, g);
            }
        }
        assert!(cast > 0 && held > 0, "cast {cast}, held {held}");
    }
}
