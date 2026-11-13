//! The rollout policy (architecture §19.1, decision D21): one cheap, greedy answer to every
//! decision, from per-card values and simple combat rules. It makes no copies and looks
//! nothing ahead, and reads only an [`Observation`]. Wrapped as an agent it
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

mod play;
mod policy;

use std::cell::Cell;

use beryl_engine::card::Purpose;
use beryl_engine::layout::Queue;
use beryl_engine::rng::SplitMix64;
use beryl_engine::two_player::TwoPlayerSeat as _;
use beryl_engine::{
    Candidates, Choice, Decision, DecisionKind, GameTable, Layout, Observation, Opt, Seat, Small,
    Zone,
};

use crate::values::{CardValues, Knowledge};

pub use play::{Inline, rollout, rollout_to, soft_rollout, soft_rollout_after, soft_rollout_to};
pub(crate) use policy::Policy;

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

/// Optional rules for the rollout policy, each off by default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Rules {
    pub hold: bool,
    pub waste: bool,
}

/// The rollout policy for one game: its per-card values and rules. Stateless between
/// decisions, so one policy serves both seats, every determinization and every thread.
#[derive(Clone, Debug)]
pub struct RolloutPolicy {
    values: CardValues,
    rules: Rules,
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
        Self::with_rules(t, knowledge, Rules::default())
    }

    /// The policy with the card values of tier `knowledge` and the given optional rules.
    pub fn with_rules(t: &GameTable, knowledge: Knowledge, rules: Rules) -> RolloutPolicy {
        RolloutPolicy {
            values: CardValues::with(t, knowledge),
            rules,
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
            rules: self.rules,
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

    /// Each candidate's greedy score in decision order. `None` ranks below any score.
    /// When the policy picks an answer directly, only the chosen candidate scores 1.
    pub fn scores<L: Layout>(
        &self,
        t: &GameTable,
        obs: Observation<'_, L>,
        d: &Decision<L>,
    ) -> Vec<Option<i32>> {
        let mut scores = Vec::with_capacity(d.len());
        self.decide_by(t, obs, d, false, &mut Pick::Scores(&mut scores));
        scores
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

    /// The rules' answer to `d`, its scores made a choice by `pick`. The policy picks
    /// one answer directly for yes/no decisions and numbers, even in a softmax rollout.
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
            DecisionKind::PlayOrDraw => {
                pick.choose_fixed(d, |c| Some(i32::from(c.option() == Some(Opt::Play))))
            }
            DecisionKind::KeepOrMulligan => {
                let keep = p.keep();
                pick.choose_fixed(d, |c| {
                    Some(i32::from((c.option() == Some(Opt::Keep)) == keep))
                })
            }
            // A "may" is taken; life for an untapped land is paid only from above 10.
            DecisionKind::YesNo => {
                let yes = d.prompt.purpose != Purpose::PayLife || obs.player(p.me).life() > 10;
                pick.choose_fixed(d, |c| {
                    Some(i32::from((c.option() == Some(Opt::Yes)) == yes))
                })
            }
            // A card leaving the player's own hand (a mulligan's, a discard's) is the one least
            // worth holding; anything else is taken by its value, as the legend to keep. Two
            // costs are paid with the least: a crew's creatures, an exile cost's cards.
            DecisionKind::ChooseObject => pick.choose(d, |c| {
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
            // A search's finds are worth taking, the best first, as Winds of Abandon's basic.
            // Other cards on top of a library are being chosen for the bottom, as a scry asks:
            // only those not worth drawing go. Anything else, such as lands to untap, is worth
            // taking by its value. With waste, decline to help an opponent's permanent.
            DecisionKind::ChooseUpTo => pick.choose(d, |c| match c.object() {
                Some(slot) if d.prompt.purpose == Purpose::Search => {
                    Some(1 + p.in_hand(slot).max(0))
                }
                Some(slot) if obs.object(slot).zone() == Zone::Library => {
                    Some(DRAW_MIN - p.in_hand(slot))
                }
                Some(slot)
                    if p.rules.waste
                        && obs.object(slot).zone() == Zone::Battlefield
                        && obs.controller(slot) != p.me =>
                {
                    None
                }
                Some(slot) => Some(p.value(slot)),
                None => Some(0),
            }),
            DecisionKind::Attacker => pick.choose(d, |c| match c.option() {
                Some(Opt::Attack(slot)) => p.safe_attack(slot).then(|| 1 + p.value(slot)),
                _ => Some(0),
            }),
            DecisionKind::AttackTarget => pick.choose(d, |c| match c.option() {
                Some(Opt::Defender(None)) => Some(if obs.player(p.me.opponent()).life() <= 6 {
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
                pick.choose_fixed(d, |c| match c.option() {
                    Some(Opt::Number(n)) => Some(if worth { i32::from(n) } else { -i32::from(n) }),
                    _ => None,
                })
            }
            DecisionKind::X => pick.choose_fixed(d, |c| match c.option() {
                Some(Opt::Number(n)) => Some(i32::from(n)),
                _ => None,
            }),
            // The first: a cost that offers permanents in place of mana lists mana first.
            DecisionKind::TriggerOrder | DecisionKind::PaymentPlan | DecisionKind::ManaPayment => {
                pick.fixed(d, Choice(0))
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
    for_each(d, &mut consider);
    Choice(top.0 as u16)
}

/// How a decision's scores become a choice.
enum Pick<'s, 'r> {
    /// The highest score, the first of equals ([`best`]).
    Best,
    /// A draw by the scores ([`Softmax`]).
    Soft(&'s mut Softmax<'r>),
    /// Collects the greedy scores in candidate order.
    Scores(&'s mut Vec<Option<i32>>),
}

impl Pick<'_, '_> {
    fn choose<L: Layout>(
        &mut self,
        d: &Decision<L>,
        mut score: impl FnMut(Candidate<L>) -> Option<i32>,
    ) -> Choice {
        match self {
            Pick::Best => best(d, score),
            Pick::Soft(soft) => soft.choose(d, score),
            Pick::Scores(scores) => best(d, |candidate| {
                let value = score(candidate);
                scores.push(value);
                value
            }),
        }
    }

    fn fixed<L: Layout>(&mut self, d: &Decision<L>, choice: Choice) -> Choice {
        if let Pick::Scores(scores) = self {
            scores.extend((0..d.len()).map(|i| Some(i32::from(i == usize::from(choice.0)))));
        }
        choice
    }

    fn choose_fixed<L: Layout>(
        &mut self,
        d: &Decision<L>,
        score: impl FnMut(Candidate<L>) -> Option<i32>,
    ) -> Choice {
        self.fixed(d, best(d, score))
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
        Candidates::Objects(_) => (d.candidates.objects())
            .enumerate()
            .for_each(|(i, slot)| f(i, Candidate::Object(slot))),
        Candidates::Options(o) => o
            .iter()
            .enumerate()
            .for_each(|(i, &opt)| f(i, Candidate::Option(opt))),
    }
}

#[cfg(test)]
mod tests {
    use beryl_cards::CARDS;
    use beryl_cards::decks::WU_CONTROL;
    use beryl_engine::{Profile, Replay, Scratch, State, Step, apply, next, observe};

    use super::*;

    /// A decision of a greedy mirror game with at least `n` candidates.
    fn decision(n: usize) -> Decision {
        let t = GameTable::new(&CARDS, [WU_CONTROL, WU_CONTROL]).unwrap();
        let policy = RolloutPolicy::new(&t);
        let mut s = State::new(&t, 0, Profile::SMART).unwrap();
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
            let start = State::new(&t, seed, Profile::SMART).unwrap();
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
        let hole = CARDS.get(beryl_cards::names::PORTABLE_HOLE).unwrap().faces[0].abilities[0]
            .targets()[0];
        let mut x = Scratch::new();
        let (mut cast, mut held) = (0, 0);
        for seed in 0..60 {
            let mut s = State::new(&t, seed, Profile::SMART).unwrap();
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
