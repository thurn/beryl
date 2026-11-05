//! The expert system (Milestone 1 decision 6): prioritized rules with card knowledge for the
//! WU Control mirror's 29 cards. It decides from its [`Observation`] only. It looks further
//! in two places, on engine copies of a determinization: combat ([`combat`]), where it plays
//! out the attacks and blocks it weighs, for combat math and lethal checks; and a payment
//! plan, where it reads which sources each plan would tap.
//!
//! The rules, in brief:
//!
//! - Counterspell discipline: counter a spell worth it, a softer counter only when its
//!   controller can't pay (Mana Tithe first when that leaves too little for No More Lies),
//!   the cheapest that works first; with one counter left, only a major threat. A minor
//!   spell waits rather than tap the counter's mana.
//! - Flash threats (Sphinx, Malcolm, The Wandering Emperor) wait for the opponent's end step
//!   or attack, unless the opponent cannot counter now or we need the blocker; instant-speed
//!   removal waits for an attacker or the end step unless its target is urgent. In response
//!   to a wheel or to Elite Spellbinder's trigger, what we could cast is cast.
//! - Teferi first, so nothing after him can be countered; Mana Crypt while the mana is
//!   worth the life; Overlord in full unless blockers are needed now; a wheel only for card
//!   advantage, never when winning or (but for a big gain) far behind; Strip Mine on an
//!   opponent short of lands; Thieving Skydiver kicked only for an artifact worth taking;
//!   Swift Reconfiguration never on Jace, Vryn's Prodigy; Aang's Iceberg never on an
//!   impending Overlord; Get Lost never on a Sphinx that would come back as a token; bounces
//!   on tokens and costly permanents, not on enters-the-battlefield creatures, Mana Crypt or
//!   a Swift Reconfiguration they could flash straight back.
//! - A counterspell we have seen, in their hand or in our Elite Spellbinder's exile, holds
//!   back our major threats; facing No More Lies, a major threat goes first, while we can
//!   still pay its {3}.
//! - Planeswalkers are worth their loyalty, more near an ultimate. Ours: an ultimate when it
//!   is there; protect the walker when the opponent's creatures threaten it, bounce a blocker
//!   to open lethal, else grow or draw.
//! - Dead on board, every removal and bounce aims at the creature that would kill us.
//! - Everything else is the rollout policy's ([`RolloutPolicy`]), valuing permanents as the
//!   expert does ([`appraise`]).

mod combat;

use beryl_cards::names;
use beryl_engine::card::{Ability, AbilityIx, FaceAbility, Keywords, Types};
use beryl_engine::rng::SplitMix64;
use beryl_engine::state::{ObjectFlags, StackEntry};
use beryl_engine::{
    Bitboard, Choice, Decision, DecisionKind, GameTable, Observation, Opt, Scratch, Seat, Slot,
    Step, TurnStep, Zone, apply, next,
};

use crate::Agent;
use crate::rollout::{Candidate, Inline, Policy, RolloutPolicy, best};

/// The least value of a spell that is a major threat: worth the last counterspell, and worth
/// tapping out for.
const MAJOR: i32 = 60;

/// The least value of a spell worth a counterspell while another stays in hand.
const COUNTER_SPARE: i32 = 45;

/// The least value of a spell worth Mana Tithe while its controller can't pay.
const TITHE_MIN: i32 = 35;

/// The least a card on top of a library must be worth in hand to be kept there.
const DRAW_MIN: i32 = 30;

/// The Map tokens Get Lost creates, and the back face of Jace, Vryn's Prodigy.
const MAP_TOKEN: &str = "Map Token";
const JACE_TELEPATH_UNBOUND: &str = "Jace, Telepath Unbound";

/// The expert, for one game: the table, the rollout policy it builds on and falls back to,
/// and the stream and working memory its combat simulations use.
pub struct ExpertAgent {
    table: GameTable,
    policy: RolloutPolicy,
    rng: SplitMix64,
    x: Scratch,
    plan: combat::Plan,
}

impl ExpertAgent {
    pub fn new(t: &GameTable, seed: u64) -> ExpertAgent {
        ExpertAgent {
            table: t.clone(),
            policy: RolloutPolicy::new(t),
            rng: SplitMix64::new(seed),
            x: Scratch::new(),
            plan: combat::Plan::default(),
        }
    }
}

impl Agent for ExpertAgent {
    fn choose(&mut self, obs: Observation<'_>, d: &Decision) -> Choice {
        let t = &self.table;
        let e = Expert {
            p: self.policy.view(t, obs, d.player, true),
        };
        match d.kind {
            DecisionKind::Priority => best(d, |c| c.option().map(|o| e.action(o))),
            DecisionKind::Attacker
            | DecisionKind::AttackTarget
            | DecisionKind::Blocker
            | DecisionKind::BlockTarget => {
                let mut sim = combat::Sim {
                    t,
                    policy: &self.policy,
                    x: &mut self.x,
                    me: d.player,
                    plan: &mut self.plan,
                    nested: false,
                };
                let start = obs.determinize(t, &mut self.rng);
                sim.choose(&start, d)
            }
            DecisionKind::YesNo => e
                .yes_no(d)
                .unwrap_or_else(|| self.policy.decide(t, obs, d, true)),
            DecisionKind::ChooseUpTo => e
                .choose_up_to(d)
                .unwrap_or_else(|| self.policy.decide(t, obs, d, true)),
            DecisionKind::Target => e
                .target(d)
                .unwrap_or_else(|| self.policy.decide(t, obs, d, true)),
            DecisionKind::TargetCount if e.bouncing() => {
                // Teferi's −3 draws a card whatever it returns: return nothing not worth it.
                let worth = e.deadly_crypt().is_some()
                    || e.bounce_spec().is_some_and(|spec| {
                        e.permanents(e.opp())
                            .filter(|&s| e.obs().matches(t, &spec.filter, s))
                            .any(|s| e.bounce_value(s) >= 15)
                    });
                best(d, |c| match c.option() {
                    Some(Opt::Number(n)) => Some(if worth { i32::from(n) } else { -i32::from(n) }),
                    _ => None,
                })
            }
            DecisionKind::PaymentPlan => {
                let mut start = obs.determinize(t, &mut self.rng);
                let mut inline = Inline {
                    policy: &self.policy,
                    t,
                };
                let x = &mut self.x;
                // A payment a resolving spell asks for is seen from a paused body; only a
                // casting's payment is the state's own open decision.
                if next(&mut start, t, x, &mut inline) != Step::Decide(*d) {
                    return self.policy.decide(t, obs, d, true);
                }
                best(d, |c| {
                    let Some(Opt::Plan(i)) = c.option() else {
                        return None;
                    };
                    // What each plan leaves untapped: a look at the payment, not ahead.
                    let mut s = start;
                    apply(&mut s, t, x, &mut inline, Choice(i));
                    let after = Expert {
                        p: self
                            .policy
                            .view(t, Observation::new(&s, d.player), d.player, true),
                    };
                    Some(after.castable())
                })
            }
            // Crew with the creatures that could not attack anyway, then the least valuable.
            DecisionKind::Crew => best(d, |c| {
                let slot = c.object()?;
                let sick = obs.object(slot).flags.has(ObjectFlags::SUMMONING_SICK);
                Some(i32::from(sick) * 1000 - e.value(slot))
            }),
            DecisionKind::PayLife => e.pay_life(d),
            DecisionKind::ChooseObject => e
                .take_from_hand(d)
                .unwrap_or_else(|| self.policy.decide(t, obs, d, true)),
            DecisionKind::X => e
                .x(d)
                .unwrap_or_else(|| self.policy.decide(t, obs, d, true)),
            _ => self.policy.decide(t, obs, d, true),
        }
    }
}

/// What a permanent is worth to the expert, where it differs from the rollout policy's
/// `base`: a planeswalker is worth half its card's value and 6 a loyalty counter, and 40
/// more within three of its ultimate, so that
/// one about to die counts for less than one about to ultimate; a permanent the other side's
/// Swift Reconfiguration has made a Vehicle no one will crew is worth little, as is a Mana
/// Crypt whose controller no longer needs the mana it costs them life for.
pub(crate) fn appraise(p: &Policy<'_>, slot: Slot, base: i32) -> Option<i32> {
    let obs = p.obs;
    let o = obs.object(slot);
    let controller = obs.controller(slot);
    let name = |s: Slot| obs.card(s).map(|c| p.t.def(c).name);
    if obs.derived(slot).types.intersects(Types::PLANESWALKER) {
        let loyalty = i32::from(o.counters.loyalty);
        // One within three of its ultimate is about to win the game.
        let ultimate = obs.card(slot).and_then(|c| {
            p.t.def(c).faces[usize::from(o.face)]
                .abilities
                .iter()
                .filter_map(|a| a.cost.loyalty)
                .min()
        });
        let near = ultimate.is_some_and(|u| u <= -6 && loyalty >= -i32::from(u) - 3);
        return Some(base / 2 + 6 * loyalty + if near { 40 } else { 0 });
    }
    let neutralized = obs.battlefield().iter().any(|&a| {
        obs.object(a).attached_to == Some(slot)
            && obs.controller(a) != controller
            && name(a) == Some(names::SWIFT_RECONFIGURATION)
    });
    if neutralized {
        return Some(5);
    }
    // Our Get Lost being aimed: destroying a Sphinx of False Conclusions only trades it for
    // its token copy (and gives its controller two Maps).
    if controller != p.me
        && returns_as_token(p, slot)
        && obs
            .stack()
            .last()
            .is_some_and(|e| e.controller == p.me && p.t.def(e.card).name == names::GET_LOST)
    {
        return Some(0);
    }
    // Jace, Vryn's Prodigy loots every turn and becomes a planeswalker.
    if name(slot) == Some(names::JACE_VRYNS_PRODIGY) && o.face == 0 {
        return Some(base + 20);
    }
    if name(slot) == Some(names::MANA_CRYPT) {
        let lands = obs
            .battlefield()
            .iter()
            .filter(|&&s| obs.controller(s) == controller)
            .filter(|&&s| obs.derived(s).types.intersects(Types::LAND))
            .count();
        if lands >= 5 {
            return Some(10);
        }
    }
    None
}

/// Whether a permanent dying comes back as a token copy of itself: a Sphinx of False
/// Conclusions that is not a token ("When this creature dies, if it isn't a token, create a
/// token that's a copy of it").
fn returns_as_token(p: &Policy<'_>, slot: Slot) -> bool {
    p.obs
        .card(slot)
        .is_some_and(|c| p.t.def(c).name == names::SPHINX_OF_FALSE_CONCLUSIONS)
        && !p.obs.object(slot).flags.has(ObjectFlags::TOKEN)
}

/// When a priority decision comes: what the expert may do then.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Timing {
    /// Our main phase with an empty stack.
    Main { precombat: bool },
    /// Something is on the stack.
    Respond(StackEntry),
    /// The opponent has declared attackers.
    TheirAttack,
    /// The opponent's end step.
    TheirEnd,
    /// Any other stop: pass.
    Other,
}

/// One decision's view, for the rules above.
struct Expert<'a> {
    p: Policy<'a>,
}

impl Expert<'_> {
    fn obs(&self) -> Observation<'_> {
        self.p.obs
    }

    fn me(&self) -> Seat {
        self.p.me
    }

    fn opp(&self) -> Seat {
        self.p.me.opponent()
    }

    /// What an object is worth, as the expert values permanents ([`appraise`]).
    fn value(&self, slot: Slot) -> i32 {
        self.p.value(slot)
    }

    /// Whether the creature card in `slot`, flashed in, could block one of `attackers` worth
    /// blocking and kill it or survive: flying lets it block any of them.
    fn good_block(&self, slot: Slot, attackers: &[Slot]) -> bool {
        let Some(card) = self.obs().card(slot) else {
            return false;
        };
        let f = self.p.t.def(card).front();
        let (power, toughness) = (f.power.unwrap_or(0), f.toughness.unwrap_or(0));
        attackers.iter().any(|&a| {
            let d = self.obs().derived(a);
            let left = d.toughness - self.obs().object(a).damage as i8;
            self.value(a) >= 20 && (power >= left || d.power < toughness)
        })
    }

    /// The name of the card in a slot, if known.
    fn name(&self, slot: Slot) -> Option<&'static str> {
        self.obs().card(slot).map(|c| self.p.t.def(c).name)
    }

    fn is(&self, slot: Slot, name: &str) -> bool {
        self.name(slot) == Some(name)
    }

    fn permanents(&self, controller: Seat) -> impl Iterator<Item = Slot> + '_ {
        let obs = self.obs();
        obs.battlefield()
            .iter()
            .copied()
            .filter(move |&s| obs.controller(s) == controller)
    }

    /// Whether a permanent has one of `types`; false for anything not on the battlefield.
    fn has_type(&self, slot: Slot, types: Types) -> bool {
        self.obs().object(slot).zone == Zone::Battlefield
            && self.obs().derived(slot).types.intersects(types)
    }

    fn creatures(&self, controller: Seat) -> impl Iterator<Item = Slot> + '_ {
        self.permanents(controller)
            .filter(|&s| self.has_type(s, Types::CREATURE))
    }

    fn tapped(&self, slot: Slot) -> bool {
        self.obs().object(slot).flags.has(ObjectFlags::TAPPED)
    }

    fn lands(&self, controller: Seat) -> usize {
        self.permanents(controller)
            .filter(|&s| self.has_type(s, Types::LAND))
            .count()
    }

    /// The mana a player could still make: their pool, an untapped land's one, and an
    /// untapped Mana Crypt's two.
    fn mana(&self, who: Seat) -> i32 {
        let pool: i32 = self
            .obs()
            .player(who)
            .mana
            .iter()
            .map(|&m| i32::from(m))
            .sum();
        pool + self
            .permanents(who)
            .filter(|&s| !self.tapped(s))
            .map(|s| {
                if self.is(s, names::MANA_CRYPT) {
                    2
                } else {
                    i32::from(self.has_type(s, Types::LAND))
                }
            })
            .sum::<i32>()
    }

    /// Whether the opponent could counter a spell now: the colors for Mana Drain or No More
    /// Lies, or a white mana for a Mana Tithe we have not seen.
    fn opp_can_counter(&self) -> bool {
        let mana = self.mana(self.opp());
        let (white, blue) = self.colors_up(self.opp());
        // Mana Drain is {U}{U}, No More Lies {W}{U}, Mana Tithe {W}.
        blue >= 2
            || (blue >= 1 && white >= 1 && mana >= 2)
            || (white >= 1 && !self.seen(names::MANA_TITHE))
    }

    /// How many of a player's untapped sources make white, and how many blue (a dual land
    /// counts for both); floating mana counts too.
    fn colors_up(&self, who: Seat) -> (i32, i32) {
        let pool = self.obs().player(who).mana;
        let (mut white, mut blue) = (i32::from(pool[0]), i32::from(pool[1]));
        for s in self.permanents(who).filter(|&s| !self.tapped(s)) {
            match self.name(s) {
                Some(names::PLAINS) => white += 1,
                Some(names::ISLAND | names::SINK_INTO_STUPOR) => blue += 1,
                Some(names::TUNDRA | names::SEACHROME_COAST | names::SPARAS_HEADQUARTERS) => {
                    white += 1;
                    blue += 1;
                }
                _ => {}
            }
        }
        (white, blue)
    }

    /// Whether the opponent's creatures could kill us next turn past our blockers.
    fn dying(&self) -> bool {
        // At 10 life or less, losing one blocker to removal is as good as losing the game.
        self.threat_past(None) >= self.life()
            || (self.life() <= 10 && self.threat_past_fewer(None, 1) >= self.life())
    }

    fn life(&self) -> i32 {
        i32::from(self.obs().player(self.me()).life)
    }

    fn hand(&self, who: Seat) -> Bitboard {
        self.obs().zone(who, Zone::Hand)
    }

    /// Our own Mana Crypt, when its next lost flip (3 damage, half the time) would kill us.
    fn deadly_crypt(&self) -> Option<Slot> {
        // The attack that kills us for certain comes first.
        if self.life() > 3 || self.dying() {
            return None;
        }
        self.permanents(self.me())
            .find(|&s| self.is(s, names::MANA_CRYPT))
    }

    /// Whether a permanent is an Overlord cast for impending, not yet a creature.
    fn impending(&self, slot: Slot) -> bool {
        self.obs().object(slot).flags.has(ObjectFlags::IMPENDING)
    }

    /// Whether a counterspell we have seen in the opponent's hand (Thundertrap Trainer and
    /// Elite Spellbinder show them) is one they have the mana for: Mana Drain or No More Lies.
    /// `left` is the mana we would have left to pay No More Lies's {3} with.
    fn counter_known(&self, left: i32) -> bool {
        let (white, blue) = self.colors_up(self.opp());
        let counter = |s: Slot| match self.name(s) {
            Some(names::MANA_DRAIN) => blue >= 2,
            Some(names::NO_MORE_LIES) => white >= 1 && blue >= 1 && left < 3,
            _ => false,
        };
        let mana = self.mana(self.opp());
        // Our Elite Spellbinder's exile shows the card too, castable for {2} more.
        let exiled = self.obs().permissions().iter().any(|p| {
            p.player == self.opp()
                && self.obs().object(p.object.slot).zone == Zone::Exile
                && counter(p.object.slot)
                && mana >= 2 + i32::from(p.extra)
        });
        exiled || (mana >= 2 && self.hand(self.opp()).iter().any(counter))
    }

    /// Whether we have seen a Mana Tithe in the opponent's hand, as Elite Spellbinder shows.
    fn tithe_known(&self) -> bool {
        self.hand(self.opp())
            .iter()
            .any(|s| self.is(s, names::MANA_TITHE))
    }

    /// What removing our permanent in `slot` would give back to the opponent: the permanent
    /// our Aang's Iceberg exiled "until this enchantment leaves the battlefield".
    fn released(&self, slot: Slot) -> i32 {
        if !self.is(slot, names::AANGS_ICEBERG) {
            return 0;
        }
        self.obs()
            .zone(self.opp(), Zone::Exile)
            .iter()
            .filter(|&s| self.obs().object(s).linked == Some(slot))
            .map(|s| self.value(s))
            .sum()
    }

    /// A card to take from the opponent's hand, as Elite Spellbinder does: by its value, but
    /// a Mana Crypt is little to a player with lands enough, and a wheel is their way out of
    /// an emptying library.
    fn take_from_hand(&self, d: &Decision) -> Option<Choice> {
        let beryl_engine::Candidates::Objects(b) = d.candidates else {
            return None;
        };
        if !b.iter().all(|s| {
            let o = self.obs().object(s);
            o.zone == Zone::Hand && o.owner == self.opp()
        }) {
            return None;
        }
        let library = self.obs().library(self.opp()).len();
        Some(best(d, |c| {
            let slot = c.object()?;
            Some(match self.name(slot) {
                Some(names::MANA_CRYPT) if self.lands(self.opp()) >= 5 => 10,
                Some(names::TIMETWISTER | names::TIME_SPIRAL) if library <= 10 => 100,
                _ => self.value(slot),
            })
        }))
    }

    /// What a player's permanents other than lands are worth.
    fn board(&self, who: Seat) -> i32 {
        self.permanents(who)
            .filter(|&s| !self.has_type(s, Types::LAND))
            .map(|s| self.value(s))
            .sum()
    }

    /// Whether a card named `name` is in our hand.
    fn holds(&self, name: &str) -> bool {
        self.hand(self.me()).iter().any(|s| self.is(s, name))
    }

    /// The counterspells in our hand.
    fn counters_in_hand(&self) -> impl Iterator<Item = Slot> + '_ {
        self.hand(self.me())
            .iter()
            .filter(|&s| self.counter_cost(s).is_some())
    }

    /// The mana a counterspell in hand costs; `None` for any other card.
    fn counter_cost(&self, slot: Slot) -> Option<i32> {
        match self.name(slot)? {
            names::MANA_TITHE => Some(1),
            names::MANA_DRAIN | names::NO_MORE_LIES => Some(2),
            _ => None,
        }
    }

    /// Whether the opponent's Teferi keeps us to sorcery speed (Teferi, Time Raveler: "Each
    /// opponent can cast spells only any time they could cast a sorcery").
    fn sorcery_only(&self) -> bool {
        self.permanents(self.opp())
            .any(|s| self.is(s, names::TEFERI_TIME_RAVELER))
    }

    fn timing(&self) -> Timing {
        let obs = self.obs();
        if let Some(&top) = obs.stack().last() {
            return Timing::Respond(top);
        }
        let turn = obs.turn();
        let ours = turn.active == self.me();
        match turn.step {
            TurnStep::PrecombatMain if ours => Timing::Main { precombat: true },
            TurnStep::PostcombatMain if ours => Timing::Main { precombat: false },
            TurnStep::DeclareAttackers if !ours => Timing::TheirAttack,
            TurnStep::End if !ours => Timing::TheirEnd,
            _ => Timing::Other,
        }
    }

    /// The score of a priority action; passing is 0, and nothing scored 0 or less is done.
    fn action(&self, o: Opt) -> i32 {
        let v = match self.timing() {
            Timing::Respond(top) => self.respond(top, o),
            Timing::Main { precombat } => self.main(o, precombat),
            Timing::TheirAttack => self.their_attack(o),
            Timing::TheirEnd => self.their_end(o),
            Timing::Other => 0,
        };
        // Into a Mana Tithe we know they hold, keep a mana to pay for it.
        if v > 0
            && let Opt::Cast { slot, face, costs } = o
            && let Some(card) = self.obs().card(slot)
            && self.tithe_known()
            && !self.dying()
            && self.mana(self.opp()) >= 1
            && self.colors_up(self.opp()).0 >= 1
            && self.mana_after(&self.p.t.def(card).faces[usize::from(face)], costs) < 1
        {
            return 0;
        }
        v
    }

    // --- Responding to the stack --------------------------------------------------------

    /// With something on the stack, counter the opponent's spell if it is worth it, by
    /// counterspell discipline; let everything else resolve.
    fn respond(&self, top: StackEntry, o: Opt) -> i32 {
        let Opt::Cast { slot, face, .. } = o else {
            return 0;
        };
        if top.controller == self.me() {
            return 0;
        }
        if !top.is_spell() {
            // Elite Spellbinder's trigger is about to exile a card from our hand: what we
            // could cast now is better cast than lost, as at their end step.
            let spellbinder = self.p.t.def(top.card).name == names::ELITE_SPELLBINDER;
            return if spellbinder { self.their_end(o) } else { 0 };
        }
        let threat = self.threat(&top);
        let spare = self.counters_in_hand().count() >= 2;
        let last_card = self.hand(self.opp()).count() <= 1;
        let min = if spare || last_card {
            COUNTER_SPARE
        } else {
            MAJOR
        };
        let theirs = self.mana(self.opp());
        let counter = match (self.name(slot), face) {
            // Mana Tithe: "Counter target spell unless its controller pays {1}."
            (Some(names::MANA_TITHE), _) if theirs < 1 && threat >= TITHE_MIN => 300,
            // Mana Tithe first when No More Lies alone could be paid for: paying {1} leaves too
            // little for No More Lies.
            (Some(names::MANA_TITHE), _)
                if theirs == 3
                    && threat >= min
                    && self.mana(self.me()) >= 3
                    && self.holds(names::NO_MORE_LIES) =>
            {
                300
            }
            // No More Lies: "Counter target spell unless its controller pays {3}."
            (Some(names::NO_MORE_LIES), _) if theirs < 3 && threat >= min => 290,
            (Some(names::MANA_DRAIN), _) if threat >= min => 280,
            // Sink into Stupor returns the spell to its owner's hand: a counter for tempo,
            // for a major threat when no counterspell can answer it.
            (Some(names::SINK_INTO_STUPOR), 0) if threat >= MAJOR => 150,
            _ => 0,
        };
        // A wheel we let resolve shuffles our hand away: what we could cast now is better
        // cast than lost.
        if counter == 0 && self.p.values.get(top.card).wheel {
            return self.instant_speed(o, 0);
        }
        counter
    }

    /// What letting an opponent's spell resolve would cost us: its value, or for a spell
    /// aimed at ours, the value of what it hits; for a wheel, what it gives them over us.
    fn threat(&self, e: &StackEntry) -> i32 {
        let def = self.p.t.def(e.card);
        let mut v = i32::from(self.p.values.get(e.card).value);
        if self.p.values.get(e.card).wheel {
            let theirs = self.hand(self.opp()).count() as i32;
            let ours = self.hand(self.me()).count() as i32;
            // Each player draws seven: they gain 7 - theirs, we 7 - ours.
            v = 20 * (ours - theirs);
            if def.name == names::TIME_SPIRAL {
                // It untaps six of their lands too.
                v += 30;
            }
            // Behind on board, fresh cards are their whole way back.
            if self.board(self.me()) - self.board(self.opp()) >= 100 {
                v = v.max(MAJOR + 10);
            }
        }
        // A counterspell of theirs aimed at our spell: as bad as losing the spell, unless we
        // can pay what it asks.
        let pays = match def.name {
            names::MANA_TITHE => Some(1),
            names::NO_MORE_LIES => Some(3),
            _ => None,
        };
        if pays.is_some_and(|n| self.mana(self.me()) >= n) {
            return 0;
        }
        for target in &e.targets {
            let Some(r) = target.as_object() else {
                continue;
            };
            let o = self.obs().object(r.slot);
            let ours = match o.zone {
                Zone::Battlefield | Zone::Stack => self.obs().controller(r.slot) == self.me(),
                _ => false,
            };
            if ours {
                v = v.max(self.value(r.slot) + self.released(r.slot));
            }
        }
        if e.costs != 0 && def.name == names::THIEVING_SKYDIVER {
            v += 20;
        }
        // Mana Crypt is little to a player with lands enough.
        if def.name == names::MANA_CRYPT && self.lands(self.opp()) >= 5 {
            v = v.min(10);
        }
        // Elite Spellbinder takes the best nonland card from our hand, the counterspell too.
        if def.name == names::ELITE_SPELLBINDER {
            v += self
                .hand(self.me())
                .iter()
                .filter(|&s| !self.p.is_land(s))
                .map(|s| self.value(s))
                .max()
                .unwrap_or(0)
                / 2;
        }
        v
    }

    // --- Our main phase -------------------------------------------------------------------

    fn main(&self, o: Opt, precombat: bool) -> i32 {
        match o {
            Opt::PlayLand(slot) => self.land(slot, precombat),
            Opt::Cast { slot, face, costs } => {
                let v = self.main_cast(slot, face, costs, precombat);
                if v > 0 && self.spoils_tax_room(slot, face, costs) {
                    v.min(50)
                } else {
                    v
                }
            }
            Opt::Activate {
                slot,
                ability: AbilityIx::Own(i),
            } => self.main_activate(slot, i, precombat),
            // Crewing a Vehicle of ours: only for an attack that kills.
            Opt::Activate {
                slot,
                ability: AbilityIx::Granted(_),
            } if precombat && self.crew_lethal(slot) => 950,
            _ => 0,
        }
    }

    /// Whether crewing our Vehicle in `vehicle` (crew 5, as Swift Reconfiguration makes one),
    /// with summoning-sick creatures first, makes an attack that gets enough damage past the
    /// opponent's untapped creatures to kill.
    fn crew_lethal(&self, vehicle: Slot) -> bool {
        // Crewed already, it is a creature until end of turn.
        if self.has_type(vehicle, Types::CREATURE) {
            return false;
        }
        let power = |s: Slot| i32::from(self.obs().derived(s).power.max(0));
        let sick = |s: Slot| self.obs().object(s).flags.has(ObjectFlags::SUMMONING_SICK);
        let mut crew: Vec<Slot> = self
            .creatures(self.me())
            .filter(|&s| !self.tapped(s))
            .collect();
        // Summoning-sick creatures may crew; they could not attack anyway.
        crew.sort_by_key(|&s| (!sick(s), power(s)));
        let mut crewed = 0;
        let mut attackers = Vec::new();
        for s in crew {
            if crewed < 5 {
                crewed += power(s);
            } else if !sick(s) {
                attackers.push(power(s));
            }
        }
        if crewed < 5 {
            return false;
        }
        attackers.push(power(vehicle));
        attackers.sort_unstable_by(|a, b| b.cmp(a));
        let blockers = self
            .creatures(self.opp())
            .filter(|&s| !self.tapped(s))
            .count();
        attackers.iter().skip(blockers).sum::<i32>()
            >= i32::from(self.obs().player(self.opp()).life)
    }

    /// Which land to play: one that enters untapped and makes the color we have less of,
    /// a tapped land when the mana wouldn't be used this turn, Strip Mine when it has a
    /// target, and a modal land face only when no other land is in hand.
    fn land(&self, slot: Slot, precombat: bool) -> i32 {
        let me = self.me();
        let lands = self.lands(me);
        // With lands enough, a looter out and nothing else in hand, a land is worth more in
        // hand, to discard.
        let looter = self.permanents(me).any(|s| {
            matches!(
                self.name(s),
                Some(
                    names::SPHINX_OF_FALSE_CONCLUSIONS
                        | names::JACE_VRYNS_PRODIGY
                        | names::MALCOLM_ALLURING_SCOUNDREL
                )
            )
        });
        let spells = self.hand(me).iter().any(|s| !self.p.is_land(s));
        if lands >= 7 && looter && !spells {
            return 0;
        }
        // Our last card waits for Jace, Vryn's Prodigy's loot after combat, which would
        // otherwise have to discard the card it draws; then below the loot, but still played if
        // he does not loot.
        if self.hand(me).count() == 1 && self.can_loot() {
            return if precombat { 0 } else { 40 };
        }
        let base = 1000;
        match self.name(slot) {
            Some(names::SINK_INTO_STUPOR) => {
                let others = self.hand(me).iter().any(|s| {
                    s != slot && !self.is(s, names::SINK_INTO_STUPOR) && self.p.is_land(s)
                });
                if others || lands >= 4 { 0 } else { base }
            }
            Some(names::SEACHROME_COAST) if lands <= 2 => base + 30,
            Some(names::SPARAS_HEADQUARTERS) => {
                if self.needs_land_mana() {
                    base + 1
                } else {
                    base + 40
                }
            }
            // Strip Mine is the land drop to strip with only once no other land would do.
            Some(names::STRIP_MINE) => {
                let others = self.hand(me).iter().any(|s| {
                    s != slot && self.p.is_land(s) && !self.is(s, names::SINK_INTO_STUPOR)
                });
                if self.strip_worth(lands + 1) && (!others || lands >= 4) {
                    base + 35
                } else {
                    base - 20
                }
            }
            Some(names::TUNDRA) => base + 25,
            Some(names::PLAINS) => base + 10 + 5 * self.short_of(true),
            Some(names::ISLAND) => base + 10 + 5 * self.short_of(false),
            _ => base + 5,
        }
    }

    /// 1 when we have fewer sources of white (`white`) or blue than of the other.
    fn short_of(&self, white: bool) -> i32 {
        let (mut w, mut u) = (0, 0);
        for s in self.permanents(self.me()) {
            match self.name(s) {
                Some(names::PLAINS) => w += 1,
                Some(names::ISLAND | names::SINK_INTO_STUPOR) => u += 1,
                Some(names::TUNDRA | names::SEACHROME_COAST | names::SPARAS_HEADQUARTERS) => {
                    w += 1;
                    u += 1;
                }
                _ => {}
            }
        }
        i32::from(if white { w < u } else { u <= w })
    }

    /// Whether a spell in hand costs exactly one more than we can make now, so that an
    /// untapped land drop would let us cast it.
    fn needs_land_mana(&self) -> bool {
        let mana = self.mana(self.me());
        self.hand(self.me()).iter().any(|s| {
            self.obs().card(s).is_some_and(|c| {
                let f = self.p.t.def(c).front();
                !f.types.intersects(Types::LAND) && i32::from(f.cost.mana_value()) == mana + 1
            })
        })
    }

    /// Whether to strip a land: the opponent is short of lands, and we would keep at least
    /// two and not fall behind them, with `ours` lands counting Strip Mine.
    fn strip_worth(&self, ours: usize) -> bool {
        let theirs = self.lands(self.opp());
        // After the strip we keep `ours - 1` lands and they `theirs - 1`.
        (1..=3).contains(&theirs) && ours > 2 && ours >= theirs
    }

    /// Whether casting the card in `slot` first would leave a major threat in hand too little
    /// to pay for No More Lies ("Counter target spell unless its controller pays {3}") that
    /// it could pay for now, while the opponent has the mana for No More Lies up.
    fn spoils_tax_room(&self, slot: Slot, face: u8, costs: u8) -> bool {
        let (white, blue) = self.colors_up(self.opp());
        if self.sorcery_only_for_them()
            || white < 1
            || blue < 1
            || self.mana(self.opp()) < 2
            || self.seen(names::NO_MORE_LIES)
        {
            return false;
        }
        let Some(card) = self.obs().card(slot) else {
            return false;
        };
        if i32::from(self.p.values.get(card).value) >= MAJOR {
            return false;
        }
        let mana = self.mana(self.me());
        let left = self.mana_after(&self.p.t.def(card).faces[usize::from(face)], costs);
        self.hand(self.me()).iter().any(|s| {
            s != slot
                && self.obs().card(s).is_some_and(|c| {
                    let f = self.p.t.def(c).front();
                    let cost = i32::from(f.cost.mana_value());
                    !f.types.intersects(Types::LAND)
                        && i32::from(self.p.values.get(c).value) >= MAJOR
                        && cost + 3 <= mana
                        && cost + 3 > left
                })
        })
    }

    /// Whether our Teferi, Time Raveler keeps the opponent to sorcery speed: "Each opponent
    /// can cast spells only any time they could cast a sorcery."
    fn sorcery_only_for_them(&self) -> bool {
        self.permanents(self.me())
            .any(|s| self.is(s, names::TEFERI_TIME_RAVELER))
    }

    fn main_cast(&self, slot: Slot, face: u8, costs: u8, precombat: bool) -> i32 {
        let Some(name) = self.name(slot) else {
            return 0;
        };
        let v = self.p.action(Opt::Cast { slot, face, costs });
        let card = self.obs().card(slot).expect("a known card");
        let f = &self.p.t.def(card).faces[usize::from(face)];
        let flash = f.keywords.intersects(Keywords::FLASH) || f.types.intersects(Types::INSTANT);
        let sorcery_only = self.sorcery_only();
        if name == names::AANGS_ICEBERG && self.deadly_crypt().is_some() {
            return 1800;
        }
        // Jace, Telepath Unbound's −3 let us cast it this turn only, for loyalty already paid.
        if self.obs().object(slot).zone == Zone::Graveyard && self.flashback(slot) {
            return 1500;
        }
        if let Some(v) = self.card_cast(slot, name, costs) {
            return v;
        }
        if let Some(removal) = self.removal_score(slot, name, face, -1) {
            // Instant-speed removal waits for an attacker or the end step unless its target
            // is urgent, Jace, Telepath Unbound's −3 lets us cast it this turn only, or the
            // opponent could not counter it now.
            let flashback = self.obs().object(slot).zone == Zone::Graveyard;
            let urgent = removal.0 >= MAJOR || removal.1 || flashback || !self.opp_can_counter();
            // Not with the counterspell's mana, short of a planeswalker or dying.
            let keeps = removal.1 || self.dying() || {
                let card = self.obs().card(slot).expect("a known card");
                self.keeps_counter_mana(i32::from(self.p.t.def(card).front().cost.mana_value()))
            };
            return if (!flash || sorcery_only || urgent) && keeps {
                700 + removal.0
            } else {
                0
            };
        }
        // Removal with nothing worth it waits.
        if matches!(
            name,
            names::OUST
                | names::GET_LOST
                | names::AANGS_ICEBERG
                | names::SWIFT_RECONFIGURATION
                | names::SINK_INTO_STUPOR
        ) || v <= 0
        {
            return 0;
        }
        // Sphinx and Malcolm wait for the opponent's turn, unless the opponent is tapped out
        // and can answer nothing now; instants always wait.
        // A blocker we will need, or one cast into an empty hand, is worth casting now too.
        let instant = f.types.intersects(Types::INSTANT);
        let now = !self.opp_can_counter() || self.dying() || self.hand(self.opp()).count() <= 1;
        if flash && !sorcery_only && (instant || !now) {
            return 0;
        }
        // Elite Spellbinder first, to see and tax the counterspell held for our threats.
        if name == names::ELITE_SPELLBINDER
            && self.opp_can_counter()
            && !self.hand(self.opp()).is_empty()
        {
            return 700;
        }
        let creature = f.types.intersects(Types::CREATURE);
        if precombat && creature && name != names::ADELINE_RESPLENDENT_CATHAR && self.can_attack() {
            // Creatures come down after combat, giving nothing away before it.
            return 0;
        }
        let walker = f.types.intersects(Types::PLANESWALKER);
        // A major threat waits while they hold a counterspell we have seen and can cast it.
        if i32::from(self.p.values.get(card).value) >= MAJOR
            && self.counter_known(self.mana_after(f, costs))
        {
            return 0;
        }
        // Dead on board, blockers come first: ones that can block what is coming.
        let fliers = self
            .creatures(self.opp())
            .any(|s| self.obs().derived(s).keywords.intersects(Keywords::FLYING));
        let blocks = !fliers
            || f.keywords
                .intersects(Keywords::FLYING.union(Keywords::REACH));
        let v = if creature && blocks && self.dying() {
            900 + v
        } else if walker {
            500 + v
        } else {
            100 + v
        };
        self.hold_for_counter(slot, v)
    }

    /// The score of casting one of the cards with rules of their own in our main phase, or
    /// `None` for any other.
    fn card_cast(&self, slot: Slot, name: &str, costs: u8) -> Option<i32> {
        let sorcery_only = self.sorcery_only();
        Some(match name {
            // Free mana, until there are lands enough and the coin flips cost too much life.
            names::MANA_CRYPT => {
                // Nor with nothing else in hand to spend its mana on.
                let lands = self.lands(self.me());
                let spells = self
                    .hand(self.me())
                    .iter()
                    .any(|s| s != slot && !self.p.is_land(s));
                let late = lands >= 7
                    || (lands >= 5 && (!spells || self.obs().player(self.me()).life <= 12));
                if late { 0 } else { 2000 }
            }
            names::TEFERI_TIME_RAVELER if self.counter_known(self.mana(self.me()) - 3) => 0,
            names::TEFERI_TIME_RAVELER => {
                // Teferi first when more is to follow: once he resolves, nothing we cast this
                // turn can be countered.
                let left = self.mana(self.me()) - 3;
                let more = self.hand(self.me()).iter().any(|s| {
                    s != slot
                        && self.obs().card(s).is_some_and(|c| {
                            let f = self.p.t.def(c).front();
                            !f.types.intersects(Types::LAND)
                                && i32::from(f.cost.mana_value()) <= left
                        })
                });
                if more && self.opp_can_counter() {
                    1600
                } else {
                    850
                }
            }
            names::TIMETWISTER | names::TIME_SPIRAL => {
                // After any threat worth casting, and after combat: a wheel shuffles away what
                // we hold.
                if self.wheel_worth(name == names::TIME_SPIRAL)
                    && !(self.obs().turn().step == TurnStep::PrecombatMain && self.can_attack())
                {
                    150
                } else {
                    0
                }
            }
            names::THIEVING_SKYDIVER => match self.stealable() {
                Some(worth) if costs != 0 => 400 + worth,
                _ if costs != 0 => 0,
                _ => self.hold_for_counter(slot, 20),
            },
            names::OVERLORD_OF_THE_MISTMOORS => {
                // In full when it can be: the impending option is the one with a cost bit. When
                // we need blockers now, impending leaves mana for another.
                let full = costs == 0;
                // Dying, impending's two flying Insects block now, ahead of anything else.
                match (full, self.dying()) {
                    (false, true) => 900,
                    (true, false) => 650,
                    _ => 600,
                }
            }
            names::THE_WANDERING_EMPEROR if !sorcery_only => {
                // Flash: she waits for the opponent's turn, unless her +1 on an attacker
                // makes our attack lethal, there is a tapped creature to exile now, or the
                // opponent is tapped out.
                let life = i32::from(self.obs().player(self.opp()).life);
                let power = self.attack_power(0);
                if power > 0 && power < life && power + 1 >= life {
                    1500
                } else if self.counter_known(self.mana(self.me()) - 4) {
                    // A counterspell we have seen waits for her.
                    0
                } else if !self.opp_can_counter() {
                    // A safe window for the best threat in hand, before any removal.
                    750
                } else if self.tapped_target(25).is_some() {
                    600
                } else {
                    0
                }
            }
            _ => return None,
        })
    }

    /// The mana we would have left after casting face `f` with the casting options `costs`.
    fn mana_after(&self, f: &beryl_engine::card::Face, costs: u8) -> i32 {
        use beryl_engine::card::CastingOption;
        let mut cost = i32::from(f.cost.mana_value());
        for (i, option) in f.casting.iter().enumerate() {
            if costs & (1 << i) != 0 {
                match option {
                    CastingOption::Kicker { cost: c, min_x } => {
                        cost += i32::from(c.mana_value() + min_x);
                    }
                    CastingOption::Offspring(c) => cost += i32::from(c.mana_value()),
                    // An alternative cost, instead of the mana cost.
                    CastingOption::Impending { cost: c, .. } => cost = i32::from(c.mana_value()),
                }
            }
        }
        self.mana(self.me()) - cost
    }

    /// Whether casting `card` would tap us out into a Mana Tithe we have not seen: the
    /// opponent holds a card and has white mana up, and no Mana Tithe is anywhere we can see.
    fn tithe_risk(&self, card: beryl_engine::CardIx) -> bool {
        let cost = i32::from(self.p.t.def(card).front().cost.mana_value());
        let opp = self.opp();
        let white = |s: Slot| {
            matches!(
                self.name(s),
                Some(
                    names::PLAINS
                        | names::TUNDRA
                        | names::SEACHROME_COAST
                        | names::SPARAS_HEADQUARTERS
                )
            )
        };
        self.mana(self.me()) - cost <= 0
            && !self.hand(opp).is_empty()
            && (self.obs().player(opp).mana[0] > 0
                || self.permanents(opp).any(|s| !self.tapped(s) && white(s)))
            && !self.seen(names::MANA_TITHE)
    }

    /// Whether we can see a card named `name` outside the opponent's hand and library.
    fn seen(&self, name: &str) -> bool {
        let obs = self.obs();
        let mut zones = obs.zone(self.me(), Zone::Hand);
        for p in Seat::BOTH {
            zones = zones | obs.zone(p, Zone::Graveyard) | obs.zone(p, Zone::Exile);
        }
        zones.iter().any(|s| self.is(s, name))
            || obs.battlefield().iter().any(|&s| self.is(s, name))
            || obs
                .stack()
                .iter()
                .any(|e| self.p.t.def(e.card).name == name)
    }

    /// `v`, or 0 when casting the card in `slot` would leave less mana than the cheapest
    /// counterspell in hand costs and the card is a minor one.
    fn hold_for_counter(&self, slot: Slot, v: i32) -> i32 {
        let Some(card) = self.obs().card(slot) else {
            return v;
        };
        let worth = i32::from(self.p.values.get(card).value);
        if worth >= COUNTER_SPARE || self.hand(self.opp()).is_empty() || self.dying() {
            return v;
        }
        let Some(keep) = self
            .counters_in_hand()
            .filter_map(|s| self.counter_cost(s))
            .min()
        else {
            return v;
        };
        let cost = i32::from(self.p.t.def(card).front().cost.mana_value());
        if self.mana(self.me()) - cost >= keep {
            v
        } else {
            0
        }
    }

    /// Whether spending `cost` leaves the mana for the cheapest counterspell or flash threat
    /// in hand, if any: what we hold mana up for on the opponent's turn.
    fn keeps_counter_mana(&self, cost: i32) -> bool {
        let flash = self
            .hand(self.me())
            .iter()
            .filter_map(|s| match self.name(s)? {
                names::THE_WANDERING_EMPEROR | names::SPHINX_OF_FALSE_CONCLUSIONS => Some(4),
                names::MALCOLM_ALLURING_SCOUNDREL => Some(2),
                _ => None,
            });
        // Only what our mana could cast at all is worth keeping it for.
        let mana = self.mana(self.me());
        self.counters_in_hand()
            .filter_map(|s| self.counter_cost(s))
            .chain(flash)
            .filter(|&keep| keep <= mana)
            .min()
            .is_none_or(|keep| mana - cost >= keep)
    }

    /// Soporific Springs: "As this land enters, you may pay 3 life. If you don't, it enters
    /// tapped." Pay only when its mana now is the one that lets a card in hand be cast, this
    /// turn or on theirs.
    fn pay_life(&self, d: &Decision) -> Choice {
        let springs = |s: Slot| self.is(s, names::SINK_INTO_STUPOR);
        let others = self.mana(self.me())
            - self
                .permanents(self.me())
                .filter(|&s| springs(s) && !self.tapped(s))
                .count() as i32;
        // Its {U} with the white and blue we have up, Soporific Springs left out.
        let (white, blue) = self.colors_up(self.me());
        let up = i32::from(
            self.permanents(self.me())
                .any(|s| springs(s) && !self.tapped(s)),
        );
        let enables = self.hand(self.me()).iter().any(|s| {
            !springs(s)
                && !self.p.is_land(s)
                && self.obs().card(s).is_some_and(|c| {
                    let cost = self.p.t.def(c).front().cost;
                    i32::from(cost.mana_value()) == others + 1
                        && i32::from(cost.symbols[0]) <= white
                        && i32::from(cost.symbols[1]) <= blue - up + 1
                })
        });
        let yes = enables && self.life() > 10;
        best(d, |c| {
            Some(i32::from((c.option() == Some(Opt::Yes)) == yes))
        })
    }

    /// Whether our Jace, Vryn's Prodigy could loot now: "{T}: Draw a card, then discard a card."
    fn can_loot(&self) -> bool {
        !self.obs().library(self.me()).is_empty()
            && self.creatures(self.me()).any(|s| {
                let o = self.obs().object(s);
                self.is(s, names::JACE_VRYNS_PRODIGY)
                    && o.face == 0
                    && !o.flags.has(ObjectFlags::TAPPED)
                    && !o.flags.has(ObjectFlags::SUMMONING_SICK)
            })
    }

    /// Whether any creature of ours could attack this turn.
    fn can_attack(&self) -> bool {
        self.creatures(self.me()).any(|s| {
            let o = self.obs().object(s);
            !o.flags.has(ObjectFlags::TAPPED)
                && !o.flags.has(ObjectFlags::SUMMONING_SICK)
                && self.obs().derived(s).power > 0
        })
    }

    /// Whether a wheel gives us more than the opponent: each player shuffles their hand away
    /// and draws seven, so we gain what the opponent's hand has over ours. Time Spiral also
    /// untaps six lands, so it is worth even cards.
    fn wheel_worth(&self, spiral: bool) -> bool {
        // Our hand after casting it, if it is cast from there.
        let wheel = if spiral {
            names::TIME_SPIRAL
        } else {
            names::TIMETWISTER
        };
        let from_hand = self.hand(self.me()).iter().any(|s| self.is(s, wheel));
        let ours = self.hand(self.me()).count() as i32 - i32::from(from_hand);
        let theirs = self.hand(self.opp()).count() as i32;
        let gain = theirs - ours;
        // Never refill the opponent's hand when the board is already winning.
        let power: i32 = self
            .creatures(self.me())
            .map(|s| i32::from(self.obs().derived(s).power.max(0)))
            .sum();
        let winning = power >= i32::from(self.obs().player(self.opp()).life);
        // Nor when far behind on board: seven fresh cards serve the side already winning.

        // Far behind, facing a swing for half our life or low on it, a wheel refills the hand
        // that wins the game for them.
        let behind = 2 * self.threat_past(None) >= self.life()
            || self.life() <= 7
            || (self.board(self.opp()) - self.board(self.me()) >= 100
                && self.life() < i32::from(self.obs().player(self.opp()).life));
        // Out of cards, it refuels us at least as much as them.
        let refill = gain >= 2 || (gain >= 0 && ours <= 1);
        // A wheel shuffles our graveyard back in: the way out of drawing from an empty library.
        let me = self.me();
        let pool = self.obs().library(me).len()
            + self.obs().graveyard(me).len()
            + self.hand(me).count() as usize;
        let decking = self.obs().library(me).len() <= 1 && pool > 8;
        // Nor while we hold a real spell the wheel would shuffle away.
        let holding = self
            .hand(self.me())
            .iter()
            .any(|s| !self.is(s, wheel) && !self.p.is_land(s) && self.value(s) >= COUNTER_SPARE);
        // Our hand empty and theirs full, it refuels us more than them, behind or not.
        let empty = ours == 0 && theirs >= 3;
        decking || (!winning && !holding && (empty || (!behind && refill)))
    }

    /// The value of the best artifact of the opponent's that Thieving Skydiver could take,
    /// if one is worth taking.
    fn stealable(&self) -> Option<i32> {
        self.artifacts(self.opp())
            .map(|s| self.value(s))
            .filter(|&v| v >= 30)
            .max()
    }

    /// A player's artifacts by their cards, Mana Crypt, Staff and Maps: not a creature that
    /// Swift Reconfiguration made a Vehicle, which no one could crew.
    fn artifacts(&self, controller: Seat) -> impl Iterator<Item = Slot> + '_ {
        self.permanents(controller).filter(|&s| {
            self.obs().card(s).is_some_and(|c| {
                let face = self.obs().object(s).face;
                self.p.t.def(c).faces[usize::from(face)]
                    .types
                    .intersects(Types::ARTIFACT)
            })
        })
    }

    /// The most valuable tapped creature of the opponent's worth at least `min`.
    fn tapped_target(&self, min: i32) -> Option<Slot> {
        self.creatures(self.opp())
            .filter(|&s| self.tapped(s))
            .filter(|&s| self.value(s) >= min)
            .max_by_key(|&s| self.value(s))
    }

    /// For a removal spell: the value of its best target, if worth it, and whether that
    /// target is a planeswalker. `None` for any other card, or a removal spell with nothing
    /// worth it: a target must be worth the card's own threshold, or `min` when that is not
    /// negative.
    fn removal_score(&self, slot: Slot, name: &str, face: u8, min: i32) -> Option<(i32, bool)> {
        // Oust's 3 life can be the 3 that keeps a low opponent alive.
        if name == names::OUST && self.obs().player(self.opp()).life <= 6 && !self.dying() {
            return None;
        }
        let threshold = match name {
            names::SINK_INTO_STUPOR if face == 0 => 50,
            names::OUST => 35,
            names::GET_LOST => 50,
            names::AANGS_ICEBERG => 45,
            names::SWIFT_RECONFIGURATION => 40,
            _ => return None,
        };
        // Dead on board, any creature is worth removing.
        let min = if min >= 0 {
            min
        } else if self.dying() {
            0
        } else {
            threshold
        };
        let card = self.obs().card(slot)?;
        let a: &Ability = self.p.t.def(card).faces[usize::from(face)]
            .spell_ability()
            .or_else(|| self.etb_targets(card))?;
        let spec = a.targets.first()?;
        let sink = name == names::SINK_INTO_STUPOR;
        let worth = |s: Slot| {
            if sink {
                self.bounce_value(s)
            } else if name == names::SWIFT_RECONFIGURATION && self.is(s, names::JACE_VRYNS_PRODIGY)
            {
                // A Vehicle Jace still loots, and still flips.
                0
            } else if name == names::GET_LOST && returns_as_token(&self.p, s) {
                // It would come back as a token copy of itself.
                0
            } else if name == names::AANGS_ICEBERG && self.impending(s) {
                // It would come back from Aang's Iceberg's exile in full, a 6/6 with Insects.
                0
            } else {
                self.value(s)
            }
        };
        // Before combat, the blocker whose removal opens lethal comes first.
        let opens = self.opens_lethal();
        let rank = |s: Slot| {
            let blocker = self.has_type(s, Types::CREATURE) && !self.tapped(s);
            worth(s) + if opens && blocker { 1000 } else { 0 }
        };
        let target = self
            .permanents(self.opp())
            .filter(|&s| self.obs().matches(self.p.t, &spec.filter, s))
            .max_by_key(|&s| rank(s))?;
        let walker = self.has_type(target, Types::PLANESWALKER);
        let ours = self.obs().turn().active == self.me();
        // A planeswalker our attack can kill this turn needs no removal.
        if walker
            && ours
            && self.attack_power(0) >= i32::from(self.obs().object(target).counters.loyalty)
        {
            return None;
        }
        let mut v = worth(target);
        // Removing their only untapped blocker before combat may open lethal.
        if ours && self.has_type(target, Types::CREATURE) && self.opens_lethal() {
            v += 1000;
        }
        (v > 0 && v >= min).then_some((v, walker))
    }

    /// Whether removing one untapped creature of the opponent's before combat would let our
    /// attack kill them when it would not now.
    fn opens_lethal(&self) -> bool {
        let life = i32::from(self.obs().player(self.opp()).life);
        self.attack_power(0) < life && self.attack_power(1) >= life
    }

    /// The damage our creatures able to attack would deal past the opponent's untapped
    /// creatures but `spared` of them, each blocking our strongest, as before combat.
    fn attack_power(&self, spared: usize) -> i32 {
        if self.obs().turn().step != TurnStep::PrecombatMain {
            return 0;
        }
        let mut powers: Vec<i32> = self
            .creatures(self.me())
            .filter(|&s| {
                let o = self.obs().object(s);
                !o.flags.has(ObjectFlags::TAPPED) && !o.flags.has(ObjectFlags::SUMMONING_SICK)
            })
            .map(|s| i32::from(self.obs().derived(s).power.max(0)))
            .collect();
        powers.sort_unstable_by(|a, b| b.cmp(a));
        let blockers = self
            .creatures(self.opp())
            .filter(|&s| !self.tapped(s))
            .count()
            .saturating_sub(spared);
        powers.iter().skip(blockers).sum()
    }

    /// The first targeted ability of a card's front face, such as Aang's Iceberg's "When
    /// this enchantment enters, exile up to one other target nonland permanent".
    fn etb_targets(&self, card: beryl_engine::CardIx) -> Option<&'static Ability> {
        self.p
            .t
            .def(card)
            .front()
            .abilities
            .iter()
            .find(|a| !a.targets.is_empty())
    }

    fn main_activate(&self, slot: Slot, i: u8, precombat: bool) -> i32 {
        let o = self.obs().object(slot);
        let Some(card) = self.obs().card(slot) else {
            return 0;
        };
        let def = self.p.t.def(card);
        if o.zone == Zone::Battlefield
            && self
                .obs()
                .derived(slot)
                .types
                .intersects(Types::PLANESWALKER)
        {
            return if self.walker_ability(slot) == Some(i) {
                800
            } else {
                0
            };
        }
        let a = &def.faces[usize::from(o.face)].abilities[usize::from(i)];
        match def.name {
            // "{T}, Sacrifice Strip Mine: Destroy target land."
            names::STRIP_MINE if !a.targets.is_empty() => {
                if self.strip_worth(self.lands(self.me())) {
                    900
                } else {
                    0
                }
            }
            // An explore is worth a mana only while the counterspell in hand stays castable.
            MAP_TOKEN if !precombat && self.keeps_counter_mana(1) => 40,
            // Drawing from an empty library loses the game.
            names::JACE_VRYNS_PRODIGY if self.obs().library(self.me()).is_empty() => 0,
            // A loot that flips him makes a planeswalker this turn, before combat too.
            names::JACE_VRYNS_PRODIGY if self.obs().graveyard(self.me()).len() >= 4 => 60,
            // Looting taps Jace through the opponent's turn: only when he isn't needed to block.
            names::JACE_VRYNS_PRODIGY if !precombat && self.threat_past(None) < self.life() => 45,
            names::STAFF_OF_THE_STORYTELLER if !precombat && self.keeps_counter_mana(1) => 30,
            _ => 0,
        }
    }

    // --- Planeswalkers ----------------------------------------------------------------------

    /// The loyalty ability a planeswalker of ours should activate, by index, if any.
    fn walker_ability(&self, slot: Slot) -> Option<u8> {
        let o = self.obs().object(slot);
        let card = self.obs().card(slot)?;
        let face = &self.p.t.def(card).faces[usize::from(o.face)];
        let loyalty = i32::from(o.counters.loyalty);
        // An ability that would leave no loyalty is no option, short of an ultimate; the
        // walker then grows instead.
        let keeps = |i: u8| {
            face.abilities[usize::from(i)]
                .cost
                .loyalty
                .is_some_and(|l| loyalty + i32::from(l) >= 1 || l <= -9)
        };
        self.preferred_ability(slot)
            .filter(|&i| keeps(i))
            .or_else(|| {
                face.abilities
                    .iter()
                    .position(|a| a.cost.loyalty.is_some_and(|l| l > 0))
                    .map(|i| i as u8)
            })
    }

    /// The loyalty ability the rules above prefer for a planeswalker of ours, by index.
    fn preferred_ability(&self, slot: Slot) -> Option<u8> {
        let o = self.obs().object(slot);
        let card = self.obs().card(slot)?;
        let def = self.p.t.def(card);
        let face = &def.faces[usize::from(o.face)];
        let loyalty = i32::from(o.counters.loyalty);
        let by_cost = |cost: i8| {
            face.abilities
                .iter()
                .position(|a| a.cost.loyalty == Some(cost))
                .map(|i| i as u8)
        };
        let threatened = self.threat_past(None);
        let face_name = def.face_name(o.face);
        match face_name {
            names::JACE_THE_MIND_SCULPTOR => {
                if loyalty >= 12 {
                    return by_cost(-12);
                }
                // +2 leaves him at loyalty + 2; the opponent's swing back decides the rest.
                if threatened >= loyalty + 2 {
                    // −1 returns the creature best returned: worth it if that saves him.
                    // Otherwise he is lost anyway: draw three.
                    let bounced = self
                        .creatures(self.opp())
                        .max_by_key(|&s| self.bounce_value(s));
                    if let Some(b) = bounced
                        && self.threat_past(Some(b)) < loyalty - 1
                    {
                        return by_cost(-1);
                    }
                    return by_cost(0);
                }
                // Returning their only untapped blocker opens lethal.
                if loyalty >= 2 && self.opens_lethal() {
                    return by_cost(-1);
                }
                // Dead on board ourselves, or low on life, he returns the biggest attacker.
                let low = self.life() <= 6 || self.dying();
                if low && loyalty >= 2 && self.creatures(self.opp()).next().is_some() {
                    return by_cost(-1);
                }
                // A big threat is worth returning even while he is safe.
                let big = self
                    .creatures(self.opp())
                    .any(|s| self.bounce_value(s) >= 80);
                if big && loyalty >= 2 {
                    return by_cost(-1);
                }
                if self.hand(self.me()).count() <= 2 && threatened < loyalty {
                    return by_cost(0);
                }
                by_cost(2)
            }
            names::TEFERI_TIME_RAVELER => {
                let target = self
                    .permanents(self.opp())
                    .filter(|&s| {
                        self.has_type(
                            s,
                            Types::ARTIFACT
                                .union(Types::CREATURE)
                                .union(Types::ENCHANTMENT),
                        ) && !self.has_type(s, Types::LAND)
                    })
                    .map(|s| self.bounce_value(s))
                    .max()
                    .unwrap_or(0);
                if target >= 45 || threatened > loyalty || self.deadly_crypt().is_some() {
                    by_cost(-3)
                } else {
                    by_cost(1)
                }
            }
            names::THE_WANDERING_EMPEROR => {
                // +1 on an attacker when one more power is lethal.
                let life = i32::from(self.obs().player(self.opp()).life);
                let power = self.attack_power(0);
                if power > 0 && power < life && power + 1 >= life {
                    return by_cost(1);
                }
                if loyalty >= 2 && self.tapped_target(25).is_some() {
                    return by_cost(-2);
                }
                if threatened > 0 && self.creatures(self.me()).next().is_none() {
                    return by_cost(-1);
                }
                by_cost(1)
            }
            JACE_TELEPATH_UNBOUND => {
                if loyalty >= 9 {
                    return by_cost(-9);
                }
                if loyalty >= 3 && self.flashback_worth() {
                    return by_cost(-3);
                }
                by_cost(1)
            }
            _ => face
                .abilities
                .iter()
                .position(|a| a.cost.loyalty.is_some_and(|l| l >= 0))
                .map(|i| i as u8),
        }
    }

    /// The power the opponent's creatures, but `except`, could swing at us or a planeswalker
    /// of ours next turn past our creatures, each blocking one attacker it can.
    fn threat_past(&self, except: Option<Slot>) -> i32 {
        self.threat_past_fewer(except, 0)
    }

    /// [`Expert::threat_past`] with `fewer` of our blockers gone first, the best of them.
    fn threat_past_fewer(&self, except: Option<Slot>, fewer: usize) -> i32 {
        let has = |s: Slot, k: Keywords| self.obs().derived(s).keywords.intersects(k);
        let mut attackers: Vec<(i32, bool)> = self
            .creatures(self.opp())
            .filter(|&s| Some(s) != except)
            .map(|s| {
                let power = i32::from(self.obs().derived(s).power.max(0));
                (power, has(s, Keywords::FLYING))
            })
            .filter(|&(p, _)| p > 0)
            .collect();
        attackers.sort_unstable_by(|a, b| b.cmp(a));
        // Each of our creatures blocks the strongest attacker it can: fliers only by fliers
        // (CR 702.9b).
        let mut reach: Vec<bool> = self
            .creatures(self.me())
            .map(|s| has(s, Keywords::FLYING.union(Keywords::REACH)))
            .collect();
        for _ in 0..fewer.min(reach.len()) {
            let i = reach.iter().position(|&r| r).unwrap_or(0);
            reach.swap_remove(i);
        }
        let mut through = 0;
        for (power, flying) in attackers {
            match reach.iter().position(|&r| r || !flying) {
                Some(i) => {
                    reach.swap_remove(i);
                }
                None => through += power,
            }
        }
        through
    }

    /// Whether Jace, Telepath Unbound's −3 has a spell worth casting again in our graveyard.
    fn flashback_worth(&self) -> bool {
        self.obs()
            .graveyard(self.me())
            .iter()
            .any(|&s| self.flashback(s))
    }

    /// Whether the card in our graveyard at `s` is worth casting again now.
    fn flashback(&self, s: Slot) -> bool {
        let Some(name) = self.name(s) else {
            return false;
        };
        // Jace's −3 costs no mana; casting the card still does.
        let affordable = self.obs().card(s).is_some_and(|c| {
            i32::from(self.p.t.def(c).front().cost.mana_value()) <= self.mana(self.me())
        });
        affordable
            && match name {
                names::GET_LOST | names::OUST => self.removal_score(s, name, 0, -1).is_some(),
                names::TIMETWISTER => self.wheel_worth(false),
                _ => false,
            }
    }

    // --- The opponent's turn ------------------------------------------------------------------

    /// Their attackers are declared: remove one worth it, flash in the Emperor to exile one,
    /// or a blocker when the attack is lethal.
    fn their_attack(&self, o: Opt) -> i32 {
        let attackers: Vec<Slot> = self
            .creatures(self.opp())
            .filter(|&s| self.obs().object(s).flags.has(ObjectFlags::ATTACKING))
            .collect();
        if attackers.is_empty() {
            return 0;
        }
        let damage: i32 = attackers
            .iter()
            .map(|&s| i32::from(self.obs().derived(s).power.max(0)))
            .sum();
        let lethal = damage >= i32::from(self.obs().player(self.me()).life);
        // An attack for half our life or more, or one that kills a planeswalker of ours, is
        // worth any removal on it.
        let kills_walker = self.permanents(self.me()).any(|w| {
            self.has_type(w, Types::PLANESWALKER) && {
                let at: i32 = attackers
                    .iter()
                    .filter(|&&a| self.obs().object(a).combat == Some(w))
                    .map(|&a| i32::from(self.obs().derived(a).power.max(0)))
                    .sum();
                at > 0 && at >= i32::from(self.obs().object(w).counters.loyalty)
            }
        });
        let pressing = 2 * damage >= self.life() || kills_walker;
        let best = attackers.iter().map(|&s| self.value(s)).max().unwrap_or(0);
        match o {
            Opt::Cast { slot, face, .. } => match self.name(slot) {
                Some(names::THE_WANDERING_EMPEROR) if best >= 20 => 600,
                Some(
                    names::SWIFT_RECONFIGURATION
                    | names::AANGS_ICEBERG
                    | names::GET_LOST
                    | names::SINK_INTO_STUPOR,
                ) if best >= 35 || pressing => {
                    let min = if pressing { 0 } else { -1 };
                    self.removal_score(slot, self.name(slot).unwrap_or(""), face, min)
                        .map_or(0, |(v, _)| 500 + v)
                }
                Some(names::SPHINX_OF_FALSE_CONCLUSIONS | names::MALCOLM_ALLURING_SCOUNDREL)
                    if lethal
                        || (self.good_block(slot, &attackers)
                            && !self.obs().card(slot).is_some_and(|c| self.tithe_risk(c))) =>
                {
                    400
                }
                _ => 0,
            },
            Opt::Activate {
                slot,
                ability: AbilityIx::Own(i),
            } if self.has_type(slot, Types::PLANESWALKER)
                && self.walker_ability(slot) == Some(i) =>
            {
                800
            }
            _ => 0,
        }
    }

    /// Their end step: use the mana that would otherwise go unused, on removal, a flash
    /// threat, then cards.
    fn their_end(&self, o: Opt) -> i32 {
        self.instant_speed(o, -1)
    }

    /// What to cast or activate with mana that would otherwise go unused: removal whose
    /// target is worth its threshold, or `min` when that is not negative, a flash threat,
    /// cards.
    fn instant_speed(&self, o: Opt, min: i32) -> i32 {
        match o {
            Opt::Cast { slot, face, .. } => {
                let Some(name) = self.name(slot) else {
                    return 0;
                };
                if name == names::AANGS_ICEBERG && self.deadly_crypt().is_some() {
                    return 1800;
                }
                if let Some((v, _)) = self.removal_score(slot, name, face, min) {
                    return 600 + v;
                }
                match name {
                    names::THE_WANDERING_EMPEROR => 500,
                    names::SPHINX_OF_FALSE_CONCLUSIONS => 480,
                    names::MALCOLM_ALLURING_SCOUNDREL => 450,
                    _ => 0,
                }
            }
            Opt::Activate {
                slot,
                ability: AbilityIx::Own(i),
            } => {
                if self.obs().object(slot).zone == Zone::Battlefield
                    && self.has_type(slot, Types::PLANESWALKER)
                {
                    return if self.walker_ability(slot) == Some(i) {
                        800
                    } else {
                        0
                    };
                }
                match self.name(slot) {
                    Some(names::STAFF_OF_THE_STORYTELLER | names::JACE_VRYNS_PRODIGY)
                        if !self.obs().library(self.me()).is_empty() =>
                    {
                        100
                    }
                    Some(names::SPARAS_HEADQUARTERS) if self.lands(self.me()) >= 5 => 90,
                    _ => 0,
                }
            }
            _ => 0,
        }
    }

    // --- Paying, targeting and choosing ------------------------------------------------------

    /// What the mana we have left could still cast: the values of the cards in hand our
    /// untapped sources could pay for, color by color, and a little for each mana.
    fn castable(&self) -> i32 {
        let me = self.me();
        let pool = self.obs().player(me).mana;
        // White only, blue only, either, and colorless sources.
        let (mut w, mut u, mut both, mut c) = (i32::from(pool[0]), i32::from(pool[1]), 0, 0);
        c += i32::from(pool[5]);
        for s in self.permanents(me).filter(|&s| !self.tapped(s)) {
            match self.name(s) {
                Some(names::PLAINS) => w += 1,
                Some(names::ISLAND | names::SINK_INTO_STUPOR) => u += 1,
                Some(names::TUNDRA | names::SEACHROME_COAST | names::SPARAS_HEADQUARTERS) => {
                    both += 1;
                }
                Some(names::STRIP_MINE) => c += 1,
                Some(names::MANA_CRYPT) => c += 2,
                _ => {}
            }
        }
        let total = w + u + both + c;
        let mut v = 3 * total;
        for slot in self.hand(me).iter() {
            let Some(card) = self.obs().card(slot) else {
                continue;
            };
            let f = self.p.t.def(card).front();
            if f.types.intersects(Types::LAND) {
                continue;
            }
            let (need_w, need_u) = (i32::from(f.cost.symbols[0]), i32::from(f.cost.symbols[1]));
            let short = (need_w - w).max(0) + (need_u - u).max(0);
            if short <= both && i32::from(f.cost.mana_value()) <= total {
                v += self.value(slot);
            }
        }
        v
    }

    /// What returning a permanent of the opponent's to its owner's hand is worth: a token
    /// ceases to exist, but Mana Crypt comes back for free and a creature with an
    /// enters-the-battlefield ability comes back with it again.
    fn bounce_value(&self, slot: Slot) -> i32 {
        let mut v = self.value(slot);
        // Before combat, returning a blocker may open lethal.
        if self.opens_lethal() && self.has_type(slot, Types::CREATURE) && !self.tapped(slot) {
            v += 500;
        }
        // Dead on board, or low on life, the creature that would kill us is the one to return.
        if (self.dying() || self.life() <= 6) && self.has_type(slot, Types::CREATURE) {
            v += 20 * i32::from(self.obs().derived(slot).power.max(0));
        }
        if self.obs().object(slot).flags.has(ObjectFlags::TOKEN) {
            return v + 15;
        }
        // Their Swift Reconfiguration on our creature: returning it frees the creature, but it
        // has flash and costs {W}, so with a white mana up they put it straight back.
        if self.is(slot, names::SWIFT_RECONFIGURATION) {
            if self.colors_up(self.opp()).0 >= 1 {
                return 0;
            }
            if let Some(host) = self.obs().object(slot).attached_to
                && self.obs().controller(host) == self.me()
                && let Some(card) = self.obs().card(host)
            {
                v += i32::from(self.p.values.get(card).value);
            }
        }
        match self.name(slot) {
            Some(names::MANA_CRYPT) => 0,
            Some(
                names::ELITE_SPELLBINDER
                | names::THUNDERTRAP_TRAINER
                | names::THIEVING_SKYDIVER
                | names::STAFF_OF_THE_STORYTELLER,
            ) => v / 4,
            // Recast for impending it is little set back; a full one costs seven again.
            Some(names::OVERLORD_OF_THE_MISTMOORS) if self.impending(slot) => v / 4,
            _ => v,
        }
    }

    /// The target spec of the bounce being cast or activated.
    fn bounce_spec(&self) -> Option<&'static beryl_engine::card::TargetSpec> {
        let e = self.obs().stack().last()?;
        let face = &self.p.t.def(e.card).faces[usize::from(e.face)];
        let a = match e.ability {
            Some(ability) => face.ability(ability),
            None => face.spell_ability()?,
        };
        a.targets.first()
    }

    /// Whether the spell or ability being cast or activated returns a permanent to its
    /// owner's hand: Sink into Stupor, Teferi's −3, Jace's −1.
    fn bouncing(&self) -> bool {
        self.obs().stack().last().is_some_and(|e| {
            e.controller == self.me()
                && matches!(
                    self.p.t.def(e.card).name,
                    names::SINK_INTO_STUPOR
                        | names::TEFERI_TIME_RAVELER
                        | names::JACE_THE_MIND_SCULPTOR
                )
        })
    }

    /// Our spell or ability's target, where the rollout policy's would be wrong: a bounce
    /// by [`Expert::bounce_value`]; while the opponent's attackers are declared, an attacker,
    /// the one dealing most when the attack is lethal, else the most valuable.
    fn target(&self, d: &Decision) -> Option<Choice> {
        // A Mana Crypt about to kill us goes, to our own Aang's Iceberg or Teferi.
        if let Some(crypt) = self.deadly_crypt()
            && let Some(e) = self.obs().stack().last()
            && e.controller == self.me()
            && matches!(
                self.p.t.def(e.card).name,
                names::AANGS_ICEBERG | names::TEFERI_TIME_RAVELER
            )
            && let beryl_engine::Candidates::Objects(b) = d.candidates
            && b.contains(crypt)
        {
            return Some(best(d, |c| Some(i32::from(c.object()? == crypt))));
        }
        // Jace, Telepath Unbound's −3: the card worth casting again.
        if let Some(e) = self.obs().stack().last()
            && e.controller == self.me()
            && self.p.t.def(e.card).face_name(e.face) == JACE_TELEPATH_UNBOUND
            && let Some(ability) = e.ability
            && self.p.t.def(e.card).faces[usize::from(e.face)]
                .ability(ability)
                .cost
                .loyalty
                == Some(-3)
        {
            return Some(best(d, |c| {
                let slot = c.object()?;
                Some(self.value(slot) + if self.flashback(slot) { 1000 } else { 0 })
            }));
        }
        // Swift Reconfiguration: a Vehicle Jace, Vryn's Prodigy still loots and flips.
        if let Some(e) = self.obs().stack().last()
            && e.controller == self.me()
            && e.is_spell()
            && self.p.t.def(e.card).name == names::SWIFT_RECONFIGURATION
            && matches!(d.candidates, beryl_engine::Candidates::Objects(_))
        {
            // In their combat, ranked as removal there: an attacker is removed from combat as
            // it stops being a creature.
            return Some(best(d, |c| {
                let slot = c.object()?;
                Some(if self.obs().controller(slot) != self.opp() {
                    -self.value(slot)
                } else if self.is(slot, names::JACE_VRYNS_PRODIGY) {
                    0
                } else {
                    self.their_attack_rank(slot)
                })
            }));
        }
        if self.bouncing() && matches!(d.candidates, beryl_engine::Candidates::Objects(_)) {
            return Some(best(d, |c| {
                let slot = c.object()?;
                let o = self.obs().object(slot);
                Some(if self.obs().controller(slot) != self.opp() {
                    -self.value(slot)
                } else if o.zone == Zone::Battlefield {
                    self.bounce_value(slot)
                } else {
                    // A spell of theirs on the stack: what Sink into Stupor was cast against.
                    100 + self.value(slot)
                })
            }));
        }
        let turn = self.obs().turn();
        if turn.active == self.me() && turn.step == TurnStep::PrecombatMain && self.opens_lethal() {
            return self.blocker_target(d);
        }
        if turn.active == self.me() || turn.step != TurnStep::DeclareAttackers {
            return self.dying_target(d);
        }
        // An effect that helps its target, as The Wandering Emperor's +1, is the policy's.
        if self.p.targeting().is_none_or(|(_, friendly)| friendly) {
            return None;
        }
        if !self.creatures(self.opp()).any(|s| self.attacking(s))
            || !matches!(d.candidates, beryl_engine::Candidates::Objects(_))
        {
            return None;
        }
        Some(best(d, |c| {
            let slot = c.object()?;
            Some(if self.obs().controller(slot) == self.opp() {
                self.their_attack_rank(slot)
            } else {
                -self.value(slot)
            })
        }))
    }

    fn attacking(&self, slot: Slot) -> bool {
        let o = self.obs().object(slot);
        o.zone == Zone::Battlefield
            && self.obs().controller(slot) == self.opp()
            && o.flags.has(ObjectFlags::ATTACKING)
    }

    /// In their combat, what removing their permanent in `slot` is worth: a lethal attack's
    /// attackers first, the ones dealing most; in an attack for half our life, any attacker;
    /// otherwise its value, and an attacker also what it would kill of our planeswalkers, so
    /// that a harmless attacker does not draw the answer a bigger threat needs.
    fn their_attack_rank(&self, slot: Slot) -> i32 {
        let v = self.value(slot);
        if !self.attacking(slot) {
            return v;
        }
        let power = |s: Slot| i32::from(self.obs().derived(s).power.max(0));
        let damage: i32 = self
            .creatures(self.opp())
            .filter(|&s| self.attacking(s) && self.obs().object(s).combat.is_none())
            .map(power)
            .sum();
        if damage >= self.life() {
            return 100_000 + 1000 * power(slot) + v;
        }
        if 2 * damage >= self.life() && self.obs().object(slot).combat.is_none() {
            return 100_000 + v;
        }
        let walker = self.obs().object(slot).combat.filter(|&w| {
            self.has_type(w, Types::PLANESWALKER)
                && power(slot) >= i32::from(self.obs().object(w).counters.loyalty)
        });
        v + walker.map_or(0, |w| self.value(w))
    }

    /// Before combat, when removing one blocker opens lethal, a harmful effect's target is an
    /// untapped creature of theirs.
    fn blocker_target(&self, d: &Decision) -> Option<Choice> {
        let (_, friendly) = self.p.targeting()?;
        if friendly || !matches!(d.candidates, beryl_engine::Candidates::Objects(_)) {
            return None;
        }
        Some(best(d, |c| {
            let slot = c.object()?;
            Some(if self.obs().controller(slot) != self.opp() {
                -self.value(slot)
            } else if self.has_type(slot, Types::CREATURE) && !self.tapped(slot) {
                1000 + self.value(slot)
            } else {
                self.value(slot)
            })
        }))
    }

    /// Dead on board, a harmful effect's target is the creature that would kill us.
    fn dying_target(&self, d: &Decision) -> Option<Choice> {
        let (_, friendly) = self.p.targeting()?;
        if friendly
            || !self.dying()
            || !matches!(d.candidates, beryl_engine::Candidates::Objects(_))
        {
            return None;
        }
        Some(best(d, |c| {
            let slot = c.object()?;
            let o = self.obs().object(slot);
            Some(if self.obs().controller(slot) != self.opp() {
                -self.value(slot)
            } else if o.zone == Zone::Battlefield && self.has_type(slot, Types::CREATURE) {
                self.value(slot) + 40 * i32::from(self.obs().derived(slot).power.max(0))
            } else {
                self.value(slot)
            })
        }))
    }

    /// Up to some permanents, as Time Spiral's "You untap up to six lands": ours only.
    fn choose_up_to(&self, d: &Decision) -> Option<Choice> {
        let beryl_engine::Candidates::Options(o) = &d.candidates else {
            return None;
        };
        let on_battlefield = o.iter().any(
            |&o| matches!(o, Opt::Object(s) if self.obs().object(s).zone == Zone::Battlefield),
        );
        if !on_battlefield {
            return None;
        }
        Some(best(d, |c| match c.option() {
            Some(Opt::Object(s)) if self.obs().controller(s) == self.me() => {
                Some(1 + self.value(s))
            }
            Some(Opt::Object(_)) => None,
            _ => Some(0),
        }))
    }

    // --- Questions while resolving ----------------------------------------------------------

    /// Jace, the Mind Sculptor's +2: put the top card on the bottom when it is one the
    /// opponent would want, or one we would not. A Map's explore: put the revealed card into
    /// the graveyard only if it is not worth drawing.
    fn yes_no(&self, d: &Decision) -> Option<Choice> {
        let e = self.obs().stack().last()?;
        let name = self.p.t.def(e.card).name;
        let player =
            if name == names::JACE_THE_MIND_SCULPTOR && e.ability == Some(FaceAbility::Own(0)) {
                e.targets.iter().next()?.as_player()?
            } else if name == MAP_TOKEN {
                self.me()
            } else {
                return None;
            };
        let &top = self.obs().library(player).last()?;
        let worth = self.p.in_hand(top);
        let away = if player == self.me() {
            worth < DRAW_MIN
        } else {
            worth >= DRAW_MIN
        };
        let want = if away { Opt::Yes } else { Opt::No };
        Some(best(d, |c| Some(i32::from(c.option() == Some(want)))))
    }

    /// Thieving Skydiver's kicker X: the least that reaches the best artifact worth taking.
    fn x(&self, d: &Decision) -> Option<Choice> {
        let e = self.obs().stack().last()?;
        if self.p.t.def(e.card).name != names::THIEVING_SKYDIVER {
            return None;
        }
        let want = self
            .artifacts(self.opp())
            .max_by_key(|&s| self.value(s))
            .and_then(|s| self.obs().card(s))
            .map_or(1, |c| self.p.t.def(c).front().cost.mana_value().max(1));
        Some(best(d, |c: Candidate| match c.option() {
            Some(Opt::Number(n)) if n >= want => Some(-i32::from(n)),
            Some(Opt::Number(n)) => Some(i32::from(n) - 1000),
            _ => None,
        }))
    }
}
