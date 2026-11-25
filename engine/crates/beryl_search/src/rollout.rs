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
pub(crate) use play::{RolloutEnd, soft_rollout_after_within, soft_rollout_within};
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

/// The value and targeting constants the fitter can vary.
#[derive(Clone, Copy, Debug)]
pub struct PolicyWeights {
    pub removal_min: i32,
    pub counter_min: i32,
    pub land_max: i32,
    pub power: i32,
    pub loyalty: i32,
}

impl PolicyWeights {
    pub const TUNED: Self = Self {
        removal_min: REMOVAL_MIN,
        counter_min: COUNTER_MIN,
        land_max: LAND_MAX,
        power: 3,
        loyalty: 4,
    };
}

/// Optional rules for the rollout policy, each off by default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Rules {
    pub hold: bool,
    pub waste: bool,
    /// Appraise alternate-win threats in the combat planner.
    pub walker: bool,
}

/// The rollout policy for one game: its per-card values and rules. Stateless between
/// decisions, so one policy serves both seats, every determinization and every thread.
#[derive(Clone, Debug)]
pub struct RolloutPolicy {
    values: CardValues,
    rules: Rules,
    weights: PolicyWeights,
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
            weights: if knowledge == Knowledge::Fitted {
                crate::overlay::fitted::WEIGHTS
            } else {
                PolicyWeights::TUNED
            },
        }
    }

    /// Scores decisions using the tuned overlay's flags and candidate fit values.
    pub fn with_fit(
        t: &GameTable,
        rules: Rules,
        weights: PolicyWeights,
        overrides: &[(&str, i16)],
    ) -> Self {
        Self {
            values: CardValues::with_overrides(t, Knowledge::Tuned, overrides),
            rules,
            weights,
        }
    }

    pub(super) fn appraises_walkers(&self) -> bool {
        self.rules.walker
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
            weights: self.weights,
            may_draw: Cell::new(None),
        }
    }

    /// The policy's answer to `d`, seen by `d.decider` in `obs`. A decider who is not the
    /// player the decision belongs to, controlling them this turn, gets the answer the policy
    /// ranks lowest for that player ([`RolloutPolicy::worst`]).
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
        let mut scores = ScoreList::new();
        self.decide_by(t, obs, d, false, &mut Pick::Scores(&mut scores));
        scores.as_slice().to_vec()
    }

    /// The candidate of `d` the policy ranks lowest for `d.player`, the first of equals,
    /// which a player controlling them answers with ([CR 724.5]: the controller "makes all
    /// choices and decisions the controlled player is allowed to make"): legal, and with no
    /// claim to strength. A candidate the policy scores `None` is chosen only if all are.
    /// Out of line, so a game no player controls pays for the test alone.
    ///
    /// [CR 724.5]: https://yawgatog.com/resources/magic-rules/#R7245
    #[cold]
    #[inline(never)]
    fn worst<L: Layout>(
        &self,
        t: &GameTable,
        obs: Observation<'_, L>,
        d: &Decision<L>,
        expert: bool,
    ) -> Choice {
        let mut scores = ScoreList::new();
        self.decide_by(t, obs, d, expert, &mut Pick::Scores(&mut scores));
        let mut bottom = (0, i32::MAX);
        for (i, &s) in scores.as_slice().iter().enumerate() {
            if let Some(s) = s
                && s < bottom.1
            {
                bottom = (i, s);
            }
        }
        Choice(bottom.0 as u16)
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
        if d.decider != d.player && !matches!(pick, Pick::Scores(_)) {
            return self.worst(t, obs, d, expert);
        }
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
            DecisionKind::ChooseUpTo => pick.choose(d, |c| match c.object() {
                Some(slot) => up_to(&p, obs, d, slot),
                None => Some(0),
            }),
            DecisionKind::Attacker => pick.choose(d, |c| match c.option() {
                Some(Opt::Attack { slot, band: None }) => p.attack_score(slot),
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
                let blocker = d.about.sole();
                pick.choose(d, |c| {
                    let attacker = c.object()?;
                    Some(blocker.and_then(|b| p.block(b, attacker)).unwrap_or(0))
                })
            }
            // Both sides divide for their own creatures. The options name the opposing
            // creatures that can receive lethal damage from the source.
            DecisionKind::DamageDivision => pick.choose(d, |c| match c.option() {
                Some(Opt::Lethal(recipient)) => Some(1 + p.value(recipient)),
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
    Scores(&'s mut ScoreList),
}

/// A [`DecisionKind::ChooseUpTo`] candidate's score, or none when it is not worth taking.
/// A search's finds are worth taking, the best first, as Winds of Abandon's basic. Other cards
/// on top of a library are being chosen for the bottom, as a scry asks: only those not worth
/// drawing go. Anything else, such as lands to untap, is worth taking by its value. With waste,
/// decline to help an opponent's permanent.
fn up_to<L: Layout>(
    p: &Policy<'_, L>,
    obs: Observation<'_, L>,
    d: &Decision<L>,
    slot: L::Slot,
) -> Option<i32> {
    let zone = obs.object(slot).zone();
    if d.prompt.purpose == Purpose::Search {
        Some(1 + p.in_hand(slot).max(0))
    } else if zone == Zone::Library {
        Some(DRAW_MIN - p.in_hand(slot))
    } else if p.rules.waste && zone == Zone::Battlefield && obs.controller(slot) != p.me {
        None
    } else if d.prompt.purpose == Purpose::Replace && zone == Zone::Battlefield {
        // A permanent offered to replace the start of a turn, as Time Vault's untap, costs the
        // whole turn it skips, and its untap waits for the next turn that begins: take the
        // turn. Two players who each skip a turn this way would otherwise skip every turn
        // after it, the untaps waiting for a turn that never begins.
        None
    } else {
        Some(p.value(slot))
    }
}

/// A decision's scores in candidate order, on the stack: a decision has at most 256
/// candidates, and collecting them allocates nothing.
pub(crate) struct ScoreList {
    list: [Option<i32>; 256],
    len: usize,
}

impl ScoreList {
    fn new() -> ScoreList {
        ScoreList {
            list: [None; 256],
            len: 0,
        }
    }

    fn push(&mut self, score: Option<i32>) {
        if let Some(place) = self.list.get_mut(self.len) {
            *place = score;
            self.len += 1;
        }
    }

    fn as_slice(&self) -> &[Option<i32>] {
        &self.list[..self.len]
    }
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
            for i in 0..d.len() {
                scores.push(Some(i32::from(i == usize::from(choice.0))));
            }
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
        Candidates::Objects(_) => (d.object_candidates())
            .enumerate()
            .for_each(|(i, slot)| f(i, Candidate::Object(slot))),
        Candidates::Options(o) => o
            .iter()
            .enumerate()
            .for_each(|(i, &opt)| f(i, Candidate::Option(opt))),
    }
}
