//! The expert's combat math: each attack or block it weighs is played out to the end of
//! combat on an engine copy of a determinization of its observation, both players passing
//! priority, and the position it leaves is scored. In our attacks the defender blocks as we
//! would ourselves, by this same planner one level deep; everything else, such as damage
//! assignment, is the rollout policy's. The plans weighed are small: one attacker or two at
//! the player or a planeswalker, just enough power at each planeswalker, every safe attacker,
//! and every attacker at the player (the lethal check); one blocker or two, and the cheapest
//! chump blocks that keep us alive. The best is declared through, and the next declaration
//! weighs again. The engine offers interchangeable creatures as one option (§7.2), so a
//! plan's creature stands for any of its kind.

use beryl_cards::names;
use beryl_engine::card::{Keywords, Types};
use beryl_engine::state::ObjectFlags;
use beryl_engine::{
    Candidates, Choice, Decision, DecisionKind, GameTable, Observation, Opt, Outcome, Scratch,
    Seat, Slot, State, Step, TurnStep, apply, next,
};

use crate::rollout::{Inline, RolloutPolicy};

/// A win or a loss, far beyond any position's score.
const DECIDED: i32 = 100_000;

/// How much worse being dead on board to the opponent's next attack is than any trade.
const DEAD_ON_BOARD: i32 = 2_000;

/// One declaration to make in a simulation, at our next combat decisions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Declare {
    /// Attack with a creature, the player (`None`) or a planeswalker.
    Attack(Slot, Option<Slot>),
    /// Block an attacker with a creature.
    Block(Slot, Slot),
    /// Attack with every creature still able to, the player or a planeswalker.
    Every(Option<Slot>),
}

/// The declarations chosen for this combat, followed through its decisions so that the
/// attackers of one plan attack the same planeswalker and blockers of one plan block
/// together: the ones still to declare, and the follow-up answer for the one declared last.
#[derive(Clone, Debug, Default)]
pub(super) struct Plan {
    turn: u16,
    rest: Vec<Declare>,
    follow_up: Option<Opt>,
}

/// The combat simulator for one decision of `me`'s.
pub(super) struct Sim<'a> {
    pub t: &'a GameTable,
    pub policy: &'a RolloutPolicy,
    pub x: &'a mut Scratch,
    pub me: Seat,
    pub plan: &'a mut Plan,
    /// Whether this simulates the other side's blocks inside one of our attacks, so that
    /// its own simulations leave the attacker's choices to the rollout policy.
    pub nested: bool,
}

impl Sim<'_> {
    /// The answer to a combat decision in the determinized position `start`.
    pub fn choose(&mut self, start: &State, d: &Decision) -> Choice {
        let turn = start.turn().number;
        if self.plan.turn != turn {
            *self.plan = Plan {
                turn,
                ..Plan::default()
            };
        }
        match d.kind {
            DecisionKind::AttackTarget | DecisionKind::BlockTarget => {
                if let Some(c) = self.plan.follow_up.take().and_then(|o| option(d, o)) {
                    return c;
                }
                self.follow_up(start, d)
            }
            DecisionKind::Attacker | DecisionKind::Blocker => {
                // The plan's next declaration, while it is still legal.
                if let Some(&next) = self.plan.rest.first()
                    && let Some(c) = declare(start, d, next)
                {
                    if !matches!(next, Declare::Every(_)) {
                        self.plan.rest.remove(0);
                    }
                    self.plan.follow_up = Some(follow_up(next));
                    return c;
                }
                let plans = if d.kind == DecisionKind::Attacker {
                    self.attack_plans(start, d)
                } else {
                    self.block_plans(start, d)
                };
                let mut top: (&[Declare], i32) = (&[], self.weigh(start, d, &[]));
                for p in &plans {
                    let score = self.weigh(start, d, p);
                    // Every attacker, the unsafe ones too, is the lethal check only.
                    if matches!(p.first(), Some(Declare::Every(_))) && score < DECIDED {
                        continue;
                    }
                    // Of the lines that win, the one with the most attackers: the most robust
                    // against what the simulation does not see.
                    if score >= DECIDED && top.1 >= DECIDED && p.len() > top.0.len() {
                        top = (p, score);
                    }
                    if score >= DECIDED && matches!(p.first(), Some(Declare::Every(_))) {
                        top = (p, score);
                        break;
                    }
                    if score > top.1 {
                        top = (p, score);
                    }
                }
                // When every line loses, block what we can all the same: the opponent may
                // still misplay.
                if top.1 <= -DECIDED
                    && d.kind == DecisionKind::Blocker
                    && let Some(chumps) = plans.last()
                {
                    top.0 = chumps;
                }
                let Some((&first, rest)) = top.0.split_first() else {
                    return option(d, Opt::Done).unwrap_or(Choice(0));
                };
                self.plan.rest = if matches!(first, Declare::Every(_)) {
                    top.0.to_vec()
                } else {
                    rest.to_vec()
                };
                self.plan.follow_up = Some(follow_up(first));
                declare(start, d, first).unwrap_or(Choice(0))
            }
            _ => Choice(0),
        }
    }

    /// The attacks worth weighing: each attacker alone and each pair together, at the player
    /// or a planeswalker, and every attacker at the player, the lethal check.
    fn attack_plans(&self, start: &State, d: &Decision) -> Vec<Vec<Declare>> {
        // A creature without power deals nothing, and Jace would rather loot.
        let attackers = offered(start, d, |o| match o {
            Opt::Attack(slot) if start.derived(slot).power > 0 => Some(slot),
            _ => None,
        });
        // A creature an untapped blocker would kill for nothing attacks only all in, as does
        // Jace, Vryn's Prodigy, whose loot is worth more than his damage.
        let all_in = !attackers.is_empty();
        let attackers: Vec<Slot> = attackers
            .into_iter()
            .filter(|&a| {
                !self.dies_for_nothing(start, a)
                    && self.t.def(start.card(a)).name != names::JACE_VRYNS_PRODIGY
            })
            .collect();
        let mut plans = Vec::new();
        for target in self.defenders(start) {
            for (i, &a) in attackers.iter().enumerate() {
                plans.push(vec![Declare::Attack(a, target)]);
                for &b in &attackers[i + 1..] {
                    plans.push(vec![Declare::Attack(a, target), Declare::Attack(b, target)]);
                }
            }
        }
        if all_in {
            plans.push(vec![Declare::Every(None)]);
        }
        if attackers.len() > 2 {
            plans.push(
                attackers
                    .iter()
                    .map(|&a| Declare::Attack(a, None))
                    .collect(),
            );
        }
        if !attackers.is_empty() {
            let kills = self.walker_kills(start, &attackers);
            if !kills.is_empty() {
                // The rest of the safe attackers at the player.
                let mut all = kills.clone();
                all.extend(
                    attackers
                        .iter()
                        .filter(|&&a| {
                            !kills
                                .iter()
                                .any(|k| matches!(k, Declare::Attack(b, _) if *b == a))
                        })
                        .map(|&a| Declare::Attack(a, None)),
                );
                plans.push(kills);
                plans.push(all);
            }
        }
        plans
    }

    /// Whether an untapped creature of the opponent's could block `attacker`, kill it and
    /// survive.
    fn dies_for_nothing(&self, s: &State, attacker: Slot) -> bool {
        let left =
            |slot: Slot| i16::from(s.derived(slot).toughness) - i16::from(s.object(slot).damage);
        s.battlefield().iter().any(|&b| {
            s.controller(b) != self.me
                && s.derived(b).types.intersects(Types::CREATURE)
                && !s.object(b).flags.has(ObjectFlags::TAPPED)
                && can_block(s, b, attacker)
                && i16::from(s.derived(b).power) >= left(attacker)
                && i16::from(s.derived(attacker).power) < left(b)
        })
    }

    /// Just enough power at each of the opponent's planeswalkers to kill it, the most
    /// valuable first: the weakest attacker that kills it alone, or the strongest together.
    fn walker_kills(&self, s: &State, attackers: &[Slot]) -> Vec<Declare> {
        let p = self
            .policy
            .view(self.t, Observation::new(s, self.me), self.me, true);
        let mut walkers: Vec<Slot> = self.defenders(s).into_iter().flatten().collect();
        walkers.sort_by_key(|&w| -p.value(w));
        let power = |a: Slot| i32::from(s.derived(a).power.max(0));
        let mut free = attackers.to_vec();
        free.sort_by_key(|&a| power(a));
        let mut plan = Vec::new();
        for w in walkers {
            let loyalty = i32::from(s.object(w).counters.loyalty);
            if let Some(i) = free.iter().position(|&a| power(a) >= loyalty) {
                plan.push(Declare::Attack(free.remove(i), Some(w)));
                continue;
            }
            let total: i32 = free.iter().map(|&a| power(a)).sum();
            if total < loyalty {
                continue;
            }
            let mut dealt = 0;
            while dealt < loyalty
                && let Some(a) = free.pop()
            {
                dealt += power(a);
                plan.push(Declare::Attack(a, Some(w)));
            }
        }
        plan
    }

    /// The blocks worth weighing: each blocker on each attacker it could block, two blockers
    /// on one attacker or on two, and the cheapest chump blocks that keep us alive.
    fn block_plans(&self, start: &State, d: &Decision) -> Vec<Vec<Declare>> {
        let blockers = offered(start, d, |o| match o {
            Opt::Block(slot) => Some(slot),
            _ => None,
        });
        let attackers = attacking(start, self.me);
        let pairs: Vec<(Slot, Slot)> = blockers
            .iter()
            .flat_map(|&b| attackers.iter().map(move |&a| (b, a)))
            .filter(|&(b, a)| can_block(start, b, a))
            .collect();
        let mut plans: Vec<Vec<Declare>> = pairs
            .iter()
            .map(|&(b, a)| vec![Declare::Block(b, a)])
            .collect();
        for (i, &(b, a)) in pairs.iter().enumerate() {
            for &(c, e) in &pairs[i + 1..] {
                // Two attackers each blocked, only when there are few enough to weigh.
                if c != b && (a == e || pairs.len() <= 12) {
                    plans.push(vec![Declare::Block(b, a), Declare::Block(c, e)]);
                }
            }
        }
        plans.push(self.chumps(start, &blockers, &attackers));
        plans
    }

    /// Chump blocks, the cheapest blocker in front of the strongest attacker it can block,
    /// until what gets through no longer kills us.
    fn chumps(&self, s: &State, blockers: &[Slot], attackers: &[Slot]) -> Vec<Declare> {
        let p = self
            .policy
            .view(self.t, Observation::new(s, self.me), self.me, true);
        let power = |a: Slot| i32::from(s.derived(a).power.max(0));
        let mut attackers = attackers.to_vec();
        attackers.sort_by_key(|&a| -power(a));
        let mut free = blockers.to_vec();
        free.sort_by_key(|&b| p.value(b));
        let mut through: i32 = attackers.iter().map(|&a| power(a)).sum();
        let life = i32::from(s.player(self.me).life);
        let mut plan = Vec::new();
        for a in attackers {
            if through < life {
                break;
            }
            if let Some(i) = free.iter().position(|&b| can_block(s, b, a)) {
                plan.push(Declare::Block(free.remove(i), a));
                through -= power(a);
            }
        }
        plan
    }

    /// What the creature just declared attacks, or blocks, with no plan to follow.
    fn follow_up(&mut self, start: &State, d: &Decision) -> Choice {
        let candidates: Vec<Opt> = match &d.candidates {
            Candidates::Options(o) => o.iter().copied().collect(),
            Candidates::Objects(b) => b.iter().map(Opt::Object).collect(),
        };
        let mut top = (0, i32::MIN);
        for (i, &o) in candidates.iter().enumerate() {
            let score = self.run(start, &[], Some(o), true);
            if score > top.1 {
                top = (i, score);
            }
        }
        Choice(top.0 as u8)
    }

    /// What an attacker could attack: the player, then each planeswalker the opponent
    /// controls.
    fn defenders(&self, s: &State) -> Vec<Option<Slot>> {
        let mut out = vec![None];
        out.extend(
            s.battlefield()
                .iter()
                .copied()
                .filter(|&w| {
                    s.controller(w) != self.me && s.derived(w).types.intersects(Types::PLANESWALKER)
                })
                .map(Some),
        );
        out
    }

    /// The score of declaring `plan` at decision `d`: for an attack, the worse of the
    /// defender blocking as planned and not blocking at all, so that an attack is not made
    /// on a block the defender need not make (one that leaves us dead on board if it does
    /// not come).
    fn weigh(&mut self, start: &State, d: &Decision, plan: &[Declare]) -> i32 {
        let planned = self.run(start, plan, None, true);
        if d.kind == DecisionKind::Attacker && !self.nested && planned < DECIDED {
            planned.min(self.run(start, plan, None, false))
        } else {
            planned
        }
    }

    /// Plays combat on from `start` with our next declarations `plan`, then stopping, and
    /// scores where it ends. `pending` answers the follow-up question of a declaration
    /// already made: what a declared attacker attacks, or what a declared blocker blocks.
    /// The opponent blocks as planned with `blocks`, else not at all.
    fn run(
        &mut self,
        start: &State,
        plan: &[Declare],
        mut pending: Option<Opt>,
        blocks: bool,
    ) -> i32 {
        let mut s = *start;
        let turn = s.turn().number;
        let mut inline = Inline {
            policy: self.policy,
            t: self.t,
        };
        let mut plan = plan.iter().copied().peekable();
        // The opponent's blocks, planned as we plan ours, one level deep.
        let mut theirs = Plan::default();
        loop {
            let step = s.turn().step;
            if s.turn().number != turn
                || matches!(
                    step,
                    TurnStep::EndOfCombat | TurnStep::PostcombatMain | TurnStep::End
                )
            {
                break;
            }
            let d = match next(&mut s, self.t, self.x, &mut inline) {
                Step::Over(_) => break,
                Step::Decide(d) => d,
            };
            let c = match (d.kind, d.player == self.me) {
                (DecisionKind::Priority, _) => Choice(0),
                (DecisionKind::Attacker | DecisionKind::Blocker, true) => {
                    let mut c = None;
                    while c.is_none()
                        && let Some(&p) = plan.peek()
                    {
                        c = declare(&s, &d, p);
                        pending = Some(follow_up(p));
                        // Every attacker is declared one decision at a time, until none is
                        // left.
                        if c.is_none() || !matches!(p, Declare::Every(_)) {
                            plan.next();
                        }
                    }
                    c.unwrap_or_else(|| option(&d, Opt::Done).unwrap_or(Choice(0)))
                }
                (DecisionKind::AttackTarget | DecisionKind::BlockTarget, true) => pending
                    .take()
                    .and_then(|want| option(&d, want))
                    .unwrap_or(Choice(0)),
                // An opponent declaring attackers here would be past this combat.
                (DecisionKind::Attacker, false) => option(&d, Opt::Done).unwrap_or(Choice(0)),
                (DecisionKind::Blocker, false) if !blocks => {
                    option(&d, Opt::Done).unwrap_or(Choice(0))
                }
                (DecisionKind::Blocker | DecisionKind::BlockTarget, false) if !self.nested => {
                    let mut sim = Sim {
                        t: self.t,
                        policy: self.policy,
                        x: &mut *self.x,
                        me: d.player,
                        plan: &mut theirs,
                        nested: true,
                    };
                    sim.choose(&s, &d)
                }
                _ => self
                    .policy
                    .choose(self.t, Observation::new(&s, d.player), &d),
            };
            apply(&mut s, self.t, self.x, &mut inline, c);
        }
        self.score(&s, s.turn().active == self.me)
    }

    /// A position's score for us: the permanents' values on each side, other than lands,
    /// and life, which counts for more as it runs low, Mana Crypt's flips counted; and the
    /// damage the opponent's creatures could swing back with next turn past our blockers.
    fn score(&self, s: &State, ours: bool) -> i32 {
        match s.outcome() {
            Some(Outcome::Win(p)) if p == self.me => return DECIDED,
            Some(Outcome::Win(_)) => return -DECIDED,
            Some(_) => return 0,
            None => {}
        }
        let me = self.me;
        let p = self.policy.view(self.t, Observation::new(s, me), me, true);
        let mut e = 0;
        for &slot in s.battlefield() {
            if s.derived(slot).types.intersects(Types::LAND) {
                continue;
            }
            let v = p.value(slot);
            e += if s.controller(slot) == me { v } else { -v };
        }
        // Mana Crypt's coin flips cost its controller 3 life half the time, every turn.
        let crypts = |p: Seat| {
            s.battlefield()
                .iter()
                .filter(|&&c| {
                    s.controller(c) == p && self.t.def(s.card(c)).name == names::MANA_CRYPT
                })
                .count() as i16
        };
        let mine = s.player(me).life - 2 * crypts(me);
        // Their creatures are a clock on us, the more so the lower our life.
        let clock: i32 = s
            .battlefield()
            .iter()
            .filter(|&&c| s.controller(c) != me && s.derived(c).types.intersects(Types::CREATURE))
            .map(|&c| i32::from(s.derived(c).power.max(0)))
            .sum();
        e -= clock * if mine <= 10 { 3 } else { 1 };
        e += life(mine) - life(s.player(me.opponent()).life - 2 * crypts(me.opponent()));
        // After our attack, our attackers stay tapped through their turn; after our blocks,
        // all of ours untap before they attack again.
        let back = swing_back(s, me, !ours, mine <= 5);
        let to_us = if back >= i32::from(mine) {
            DEAD_ON_BOARD
        } else {
            life(mine) - life(mine - back as i16)
        };
        // Or they swing at a planeswalker of ours instead.
        let to_walker = s
            .battlefield()
            .iter()
            .filter(|&&w| {
                s.controller(w) == me && s.derived(w).types.intersects(Types::PLANESWALKER)
            })
            .map(|&w| {
                if back >= i32::from(s.object(w).counters.loyalty) {
                    p.value(w)
                } else {
                    6 * back
                }
            })
            .max()
            .unwrap_or(0);
        e - to_us.max(to_walker) - self.exiled_next(s, &p, ours)
    }

    /// After our attack, what the opponent's The Wandering Emperor takes on their turn: "−2:
    /// Exile target tapped creature", our most valuable attacker still tapped, while she has
    /// the loyalty to survive it (at 2 she trades herself for it).
    fn exiled_next(&self, s: &State, p: &crate::rollout::Policy<'_>, ours: bool) -> i32 {
        let emperor = s.battlefield().iter().any(|&w| {
            s.controller(w) != self.me
                && self.t.def(s.card(w)).name == names::THE_WANDERING_EMPEROR
                && s.object(w).counters.loyalty >= 3
        });
        if !ours || !emperor {
            return 0;
        }
        s.battlefield()
            .iter()
            .filter(|&&c| {
                s.controller(c) == self.me
                    && s.derived(c).types.intersects(Types::CREATURE)
                    && s.object(c).flags.has(ObjectFlags::TAPPED)
            })
            .map(|&c| p.value(c))
            .max()
            .unwrap_or(0)
    }
}

/// What life is worth: each point 2, 10 more each of the last 10, and 15 more each of the
/// last 5, where one trick or loot away from dead.
fn life(l: i16) -> i32 {
    let l = i32::from(l.max(0));
    2 * l + 10 * l.min(10) + 15 * l.min(5)
}

/// The damage the opponent's creatures could deal `me` attacking next turn, each of our
/// creatures blocking one of them it could block, the strongest first: our untapped ones, or
/// with `untap`, all of them. With `fragile`, the opponent removes one of our blockers first,
/// as they will when one more blocker is all that keeps us alive.
fn swing_back(s: &State, me: Seat, untap: bool, fragile: bool) -> i32 {
    let creature = |slot: Slot| s.derived(slot).types.intersects(Types::CREATURE);
    let flies = |slot: Slot, k: Keywords| s.derived(slot).keywords.intersects(k);
    let mut attackers: Vec<(i32, bool)> = s
        .battlefield()
        .iter()
        .copied()
        .filter(|&a| s.controller(a) != me && creature(a) && s.derived(a).power > 0)
        .map(|a| (i32::from(s.derived(a).power), flies(a, Keywords::FLYING)))
        .collect();
    attackers.sort_unstable_by(|a, b| b.cmp(a));
    let mut blockers: Vec<bool> = s
        .battlefield()
        .iter()
        .copied()
        .filter(|&b| {
            s.controller(b) == me
                && creature(b)
                && (untap || !s.object(b).flags.has(ObjectFlags::TAPPED))
        })
        .map(|b| flies(b, Keywords::FLYING.union(Keywords::REACH)))
        .collect();
    if fragile && !blockers.is_empty() {
        // The best of them: one that blocks fliers too, if any.
        let i = blockers.iter().position(|&reach| reach).unwrap_or(0);
        blockers.swap_remove(i);
    }
    let mut damage = 0;
    for (power, flying) in attackers {
        match blockers.iter().position(|&reach| reach || !flying) {
            Some(i) => {
                blockers.swap_remove(i);
            }
            None => damage += power,
        }
    }
    damage
}

/// The choice that makes declaration `want` at decision `d`: its creature, or one
/// interchangeable with it; for [`Declare::Every`], any attacker with power. `None` when it
/// is no longer legal.
fn declare(s: &State, d: &Decision, want: Declare) -> Option<Choice> {
    let Candidates::Options(options) = &d.candidates else {
        return None;
    };
    options
        .iter()
        .position(|&o| match (want, o) {
            (Declare::Attack(a, _), Opt::Attack(b)) | (Declare::Block(a, _), Opt::Block(b)) => {
                like(s, a, b)
            }
            (Declare::Every(_), Opt::Attack(b)) => s.derived(b).power > 0,
            _ => false,
        })
        .map(|i| Choice(i as u8))
}

/// The answer to the question that follows declaring `d`: what it attacks or blocks.
fn follow_up(d: Declare) -> Opt {
    match d {
        Declare::Attack(_, target) | Declare::Every(target) => Opt::Defender(target),
        Declare::Block(_, a) => Opt::Object(a),
    }
}

/// Every creature of ours a decision's options could declare: the ones it names, and those
/// interchangeable with one, which the engine offers as the same option (§7.2).
fn offered(s: &State, d: &Decision, pick: impl Fn(Opt) -> Option<Slot>) -> Vec<Slot> {
    let Candidates::Options(options) = &d.candidates else {
        return Vec::new();
    };
    let named: Vec<Slot> = options.iter().filter_map(|&o| pick(o)).collect();
    s.battlefield()
        .iter()
        .copied()
        .filter(|&c| named.iter().any(|&n| like(s, n, c)))
        .filter(|&c| {
            let f = s.object(c).flags;
            !f.has(ObjectFlags::ATTACKING) && !f.has(ObjectFlags::BLOCKING)
        })
        .collect()
}

/// Whether two creatures are interchangeable: the same card, the same power and toughness.
fn like(s: &State, a: Slot, b: Slot) -> bool {
    a == b
        || (s.controller(a) == s.controller(b)
            && s.card(a) == s.card(b)
            && s.derived(a).power == s.derived(b).power
            && s.derived(a).toughness == s.derived(b).toughness
            && s.object(a).flags.has(ObjectFlags::TAPPED)
                == s.object(b).flags.has(ObjectFlags::TAPPED))
}

/// Whether `blocker` could block `attacker`, by evasion
/// ([CR 702.9b](https://yawgatog.com/resources/magic-rules/#R7029b)): "A creature with
/// flying can't be blocked except by creatures with flying and/or reach."
fn can_block(s: &State, blocker: Slot, attacker: Slot) -> bool {
    let has = |slot: Slot, k: Keywords| s.derived(slot).keywords.intersects(k);
    !has(attacker, Keywords::FLYING) || has(blocker, Keywords::FLYING.union(Keywords::REACH))
}

/// The opponent's creatures attacking `me`.
fn attacking(s: &State, me: Seat) -> Vec<Slot> {
    s.battlefield()
        .iter()
        .copied()
        .filter(|&a| s.controller(a) != me && s.object(a).flags.has(ObjectFlags::ATTACKING))
        .collect()
}

/// The choice naming `want` among a decision's candidates.
fn option(d: &Decision, want: Opt) -> Option<Choice> {
    match &d.candidates {
        Candidates::Options(o) => o.iter().position(|&o| o == want),
        Candidates::Objects(b) => match want {
            Opt::Object(slot) => b.iter().position(|s| s == slot),
            _ => None,
        },
    }
    .map(|i| Choice(i as u8))
}
