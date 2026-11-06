//! The expert system (Milestone 1 decision 6): prioritized rules with card knowledge for the
//! WU Control mirrors, cb8d5d5f's and the Vintage deck's before it. It decides from its
//! [`Observation`] only. It looks further in two places, on engine copies of a
//! determinization: combat ([`combat`]), where it plays out the attacks and blocks it weighs,
//! for combat math and lethal checks; and a payment plan, where it reads which sources each
//! plan would tap.
//!
//! The rules, in brief:
//!
//! - Counterspell discipline: counter a spell worth it, a softer counter only when its
//!   controller can't pay (Mana Tithe first when that leaves too little for No More Lies) or,
//!   for Reprieve, can't cast it again this turn, the cheapest that works first; with one
//!   counter left, only a major threat. Subtlety answers a creature or planeswalker spell,
//!   evoked only for one that would win. A minor spell waits rather than tap the counter's
//!   mana.
//! - Flash threats (Sphinx, Malcolm, The Wandering Emperor, Snapcaster Mage) and Thirst for
//!   Discovery wait for the opponent's end step or attack, unless the opponent cannot
//!   counter now or we need the blocker; instant-speed removal waits for an attacker or the
//!   end step unless its target is urgent. In response to a wheel or to Elite Spellbinder's
//!   trigger, what we could cast is cast.
//! - Teferi first, so nothing after him can be countered; Mana Crypt only while short of lands
//!   for the spells in hand; Overlord in full unless blockers are needed now; a wheel only for
//!   card advantage, never when winning or (but for a big gain) far behind; Strip Mine on an
//!   opponent short of lands; Thieving Skydiver kicked only for an artifact worth taking;
//!   Swift Reconfiguration never on Jace, Vryn's Prodigy; Aang's Iceberg never on an
//!   impending Overlord; Get Lost never on a Sphinx that would come back as a token; bounces
//!   on tokens and costly permanents, not on enters-the-battlefield creatures, Mana Crypt or
//!   a Swift Reconfiguration they could flash straight back.
//! - A counterspell we have seen, in their hand or in our Elite Spellbinder's exile, holds
//!   back our major threats; facing No More Lies, a major threat goes first, while we can
//!   still pay its {3}.
//! - Planeswalkers are worth their loyalty, more near an ultimate, and a Jace, Wielder of
//!   Mysteries near a win by decking most of all. Ours: an ultimate when it is there (Jace,
//!   Wielder's only when it wins); protect the walker when the opponent's creatures threaten
//!   it, bounce a blocker to open lethal, else grow or draw. No draw by choice once the
//!   library runs low, and no milling an opponent whose Jace, Wielder could still win.
//! - Dead on board, every removal and bounce aims at the creature that would kill us.
//! - Everything else is the rollout policy's ([`RolloutPolicy`]), valuing permanents as the
//!   expert does ([`appraise`]).

mod combat;

use beryl_engine::{Large, Layout, Small};
use std::cell::OnceCell;

use beryl_cards::names;
use beryl_engine::card::{
    Ability, AbilityIx, CastingOption, Castings, Face, FaceIx, Keywords, ManaColor, Types,
};
use beryl_engine::rng::SplitMix64;
use beryl_engine::state::{ObjectFlags, StackEntry};
use beryl_engine::{
    Ask, Bitboard, Choice, Decision, DecisionKind, GameTable, Observation, Opt, Scratch, Seat,
    Slot, Step, TurnStep, Zone, apply, next,
};

use crate::rollout::{Candidate, Inline, Policy, RolloutPolicy, best};
use crate::{Agent, Widen};

/// The most lands we cast Mana Crypt with.
const CRYPT_LANDS: i32 = 3;

/// The least value of a spell that is a major threat: worth the last counterspell, and worth
/// tapping out for.
const MAJOR: i32 = 60;

/// The least value of a spell worth a counterspell while another stays in hand.
const COUNTER_SPARE: i32 = 45;

/// The least value of a spell worth Mana Tithe while its controller can't pay.
const TITHE_MIN: i32 = 35;

/// The least a card on top of a library must be worth in hand to be kept there.
const DRAW_MIN: i32 = 30;

/// The library a Jace, Wielder of Mysteries wins from, near enough: at most this many cards.
const JACE_LIBRARY: usize = 12;

/// The library a Jace, Wielder of Mysteries mills its controller's opponent out of, near
/// enough.
const JACE_MILLS: usize = 15;

/// What a Jace, Wielder of Mysteries about to win is worth, before its loyalty.
const JACE_WINS: i32 = 150;

/// The removal spells: Oust, and those that take a permanent without giving life back (Winds
/// of Abandon gives a basic land, cast for one target; overloaded, it is a sweeper).
const REMOVAL: &[&str] = &[
    names::OUST,
    names::GET_LOST,
    names::AANGS_ICEBERG,
    names::SWIFT_RECONFIGURATION,
    names::SINK_INTO_STUPOR,
    names::PORTABLE_HOLE,
    names::WINDS_OF_ABANDON,
    names::FRACTURED_IDENTITY,
];

/// The Map tokens Get Lost creates, and the back face of Jace, Vryn's Prodigy.
const MAP_TOKEN: &str = "Map Token";
const JACE_TELEPATH_UNBOUND: &str = "Jace, Telepath Unbound";

/// The expert, for one game: the table, the rollout policy it builds on and falls back to,
/// and the stream and working memory its combat simulations use.
pub struct ExpertAgent<L: Layout = Small> {
    table: GameTable,
    policy: RolloutPolicy,
    rng: SplitMix64,
    x: Scratch<L>,
    plan: combat::Plan,
}

impl<L: Layout> ExpertAgent<L> {
    pub fn new(t: &GameTable, seed: u64) -> ExpertAgent<L> {
        ExpertAgent {
            table: t.clone(),
            policy: RolloutPolicy::new(t),
            rng: SplitMix64::new(seed),
            x: Scratch::new(),
            plan: combat::Plan::default(),
        }
    }
}

impl ExpertAgent<Large> {
    /// The expert in [`Large`], drawing on from `rng`: how an agent with no `Large` copy of its
    /// own plays on there ([`crate::FrozenExpertAgent`]).
    pub(crate) fn drawing_from(t: &GameTable, rng: SplitMix64) -> ExpertAgent<Large> {
        ExpertAgent {
            rng,
            ..ExpertAgent::new(t, 0)
        }
    }
}

/// The same expert in [`Large`]: its stream and combat plan go on where they were.
impl Widen for ExpertAgent<Small> {
    type Large = ExpertAgent<Large>;

    fn widen(&self) -> ExpertAgent<Large> {
        ExpertAgent {
            table: self.table.clone(),
            policy: self.policy.clone(),
            rng: self.rng,
            x: Scratch::new(),
            plan: self.plan.clone(),
        }
    }
}

impl<L: Layout> Agent<L> for ExpertAgent<L> {
    fn choose(&mut self, obs: Observation<'_, L>, d: &Decision) -> Choice {
        let t = &self.table;
        let e = Expert::new(self.policy.view(t, obs, d.player, true));
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
                    let mut s = start.clone();
                    apply(&mut s, t, x, &mut inline, Choice(i));
                    let after = Expert::new(self.policy.view(
                        t,
                        Observation::new(&s, d.player),
                        d.player,
                        true,
                    ));
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
pub(crate) fn appraise<L: Layout>(p: &Policy<'_, L>, slot: Slot, base: i32) -> Option<i32> {
    let obs = p.obs;
    let o = obs.object(slot);
    let controller = obs.controller(slot);
    let name = |s: Slot| obs.card(s).map(|c| p.t.def(c).name);
    // Jace, Wielder of Mysteries on a short library is about to win: "If you would draw a
    // card while your library has no cards in it, you win the game instead."
    if name(slot) == Some(names::JACE_WIELDER_OF_MYSTERIES) && wins_by_decking(p, controller) {
        return Some(JACE_WINS + 6 * i32::from(o.counters.loyalty));
    }
    if obs.derived(slot).types.intersects(Types::PLANESWALKER) {
        let loyalty = i32::from(o.counters.loyalty);
        // One within three of its ultimate is about to win the game.
        let ultimate = obs.card(slot).and_then(|c| {
            p.t.def(c)
                .face(o.face)
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
    // A creature whose enters-the-battlefield ability is spent is worth its body.
    if matches!(
        name(slot),
        Some(names::ELITE_SPELLBINDER | names::THUNDERTRAP_TRAINER | names::THIEVING_SKYDIVER)
    ) {
        return Some(base / 2 + 3 * i32::from(obs.derived(slot).power.max(0)));
    }
    // Jace, Vryn's Prodigy loots every turn and becomes a planeswalker.
    if name(slot) == Some(names::JACE_VRYNS_PRODIGY) && o.face == FaceIx::FRONT {
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

/// Whether a library is short enough that `who`'s Jace, Wielder of Mysteries would win within
/// a few turns: their own, which its +1 draws from one a turn, its −8 seven and their draw
/// step one; or their opponent's, which its +1 mills two a turn.
fn wins_by_decking<L: Layout>(p: &Policy<'_, L>, who: Seat) -> bool {
    p.obs.library(who).len() <= JACE_LIBRARY || p.obs.library(who.opponent()).len() <= JACE_MILLS
}

/// Whether a permanent dying comes back as a token copy of itself: a Sphinx of False
/// Conclusions that is not a token ("When this creature dies, if it isn't a token, create a
/// token that's a copy of it").
fn returns_as_token<L: Layout>(p: &Policy<'_, L>, slot: Slot) -> bool {
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
struct Expert<'a, L: Layout> {
    p: Policy<'a, L>,
    /// [`Expert::lethal_blockers`], worked out once.
    lethal: OnceCell<Option<Vec<Slot>>>,
}

impl<'a, L: Layout> Expert<'a, L> {
    fn new(p: Policy<'a, L>) -> Expert<'a, L> {
        Expert {
            p,
            lethal: OnceCell::new(),
        }
    }
}

impl<L: Layout> Expert<'_, L> {
    fn obs(&self) -> Observation<'_, L> {
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
            .amounts()
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
        // Counterspell is {U}{U}, Reprieve {1}{W}.
        blue >= 2
            || (blue >= 1 && white >= 1 && mana >= 2)
            || (white >= 1 && mana >= 2 && self.in_their_deck(names::REPRIEVE))
            || (white >= 1
                && self.in_their_deck(names::MANA_TITHE)
                && !self.seen(names::MANA_TITHE))
    }

    /// Whether the opponent's deck holds a card named `name`.
    fn in_their_deck(&self, name: &str) -> bool {
        self.p
            .t
            .deck(self.opp())
            .iter()
            .any(|&c| self.p.t.def(c).name == name)
    }

    /// Whether the opponent could win by drawing from an empty library: they control a Jace,
    /// Wielder of Mysteries, or one is in their deck and not yet in their graveyard or exile,
    /// so that milling them may hand them the game.
    fn their_jace_live(&self) -> bool {
        let opp = self.opp();
        let gone = (self.obs().zone(opp, Zone::Graveyard) | self.obs().zone(opp, Zone::Exile))
            .iter()
            .any(|s| self.is(s, names::JACE_WIELDER_OF_MYSTERIES));
        self.p.wins_from_empty(opp)
            || (self.in_their_deck(names::JACE_WIELDER_OF_MYSTERIES) && !gone)
    }

    /// How many of a player's untapped sources make white, and how many blue (a dual land
    /// counts for both); floating mana counts too.
    fn colors_up(&self, who: Seat) -> (i32, i32) {
        let pool = self.obs().player(who).mana;
        let (mut white, mut blue) = (
            i32::from(pool[ManaColor::White]),
            i32::from(pool[ManaColor::Blue]),
        );
        for s in self.permanents(who).filter(|&s| !self.tapped(s)) {
            let (w, u) = self.makes(s);
            white += i32::from(w);
            blue += i32::from(u);
        }
        (white, blue)
    }

    /// Whether the land in `slot` makes white, and whether blue: Soporific Springs is Sink
    /// into Stupor's back face.
    fn makes(&self, slot: Slot) -> (bool, bool) {
        match self.name(slot) {
            Some(names::PLAINS) => (true, false),
            Some(names::ISLAND | names::SINK_INTO_STUPOR) => (false, true),
            // Floodfarm Verge makes {U} only with an Island or Plains out, as it nearly
            // always has.
            Some(
                names::TUNDRA
                | names::SEACHROME_COAST
                | names::SPARAS_HEADQUARTERS
                | names::FLOODFARM_VERGE,
            ) => (true, true),
            _ => (false, false),
        }
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
            Some(names::MANA_DRAIN | names::COUNTERSPELL) => blue >= 2,
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

    /// What removing the permanent in `slot` would give back to the other side: the
    /// permanent an Aang's Iceberg exiled "until this enchantment leaves the battlefield".
    fn released(&self, slot: Slot) -> i32 {
        if !self.is(slot, names::AANGS_ICEBERG) {
            return 0;
        }
        self.obs()
            .zone(self.obs().controller(slot).opponent(), Zone::Exile)
            .iter()
            .filter(|&s| self.obs().object(s).linked == Some(slot))
            .map(|s| self.value(s))
            .sum()
    }

    /// A card to take from a look at the opponent's hand, as Elite Spellbinder's: by its
    /// value, but a Mana Crypt is little to a player with lands enough, and a wheel is their
    /// way out of an emptying library. From a look at our own library, as Thundertrap
    /// Trainer's, or for the top of a scry, dying: the removal we can cast now.
    fn take_from_hand(&self, d: &Decision) -> Option<Choice> {
        let (player, zone) = match d.ask {
            Ask::Look { player, zone, .. } => (player, zone),
            Ask::Scry { .. } => (self.me(), Zone::Library),
            _ => return None,
        };
        // Our library: dying, the answer we can cast now.
        if zone == Zone::Library && player == self.me() && self.dying() {
            let mana = self.mana(self.me());
            return Some(best(d, |c| {
                let slot = c.object()?;
                let card = self.obs().card(slot)?;
                let now = i32::from(self.p.t.def(card).front().cost.mana_value()) <= mana;
                let answer = self.name(slot).is_some_and(|n| REMOVAL.contains(&n));
                Some(self.value(slot) + 1000 * i32::from(now && answer))
            }));
        }
        if zone != Zone::Hand || player != self.opp() {
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

    /// How many of our lands, tapped or not, make white, and how many blue (a dual land
    /// counts for both).
    fn sources(&self) -> (i32, i32) {
        let (mut white, mut blue) = (0, 0);
        for s in self.permanents(self.me()) {
            let (w, u) = self.makes(s);
            white += i32::from(w);
            blue += i32::from(u);
        }
        (white, blue)
    }

    /// Whether our lands have the colors the card in `slot` costs.
    fn colors_for(&self, slot: Slot) -> bool {
        let Some(card) = self.obs().card(slot) else {
            return false;
        };
        let (white, blue) = self.sources();
        let cost = self.p.t.def(card).front().cost;
        i32::from(cost.symbols[0]) <= white && i32::from(cost.symbols[1]) <= blue
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

    /// The mana each counterspell in hand worth holding up costs: one our untapped sources
    /// have the colors for (they stay as they are through the opponent's turn), and Mana
    /// Tithe, which a player with a mana to spare pays, only when it is the only one.
    fn counter_mana(&self) -> impl Iterator<Item = i32> + '_ {
        let (white, blue) = self.colors_up(self.me());
        let castable = move |s: Slot| {
            self.obs().card(s).is_some_and(|c| {
                let cost = self.p.t.def(c).front().cost;
                i32::from(cost.symbols[0]) <= white && i32::from(cost.symbols[1]) <= blue
            })
        };
        let others = self
            .counters_in_hand()
            .any(|s| castable(s) && !self.is(s, names::MANA_TITHE));
        self.counters_in_hand()
            .filter(move |&s| castable(s) && (!others || !self.is(s, names::MANA_TITHE)))
            .filter_map(|s| self.counter_cost(s))
    }

    /// The mana a counterspell in hand costs; `None` for any other card.
    fn counter_cost(&self, slot: Slot) -> Option<i32> {
        match self.name(slot)? {
            names::MANA_TITHE => Some(1),
            names::MANA_DRAIN | names::NO_MORE_LIES | names::COUNTERSPELL | names::REPRIEVE => {
                Some(2)
            }
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
            && self.mana_after(self.p.t.def(card).face(face), costs) < 1
        {
            return 0;
        }
        v
    }

    // --- Responding to the stack --------------------------------------------------------

    /// With something on the stack, counter the opponent's spell if it is worth it, by
    /// counterspell discipline; let everything else resolve.
    fn respond(&self, top: StackEntry, o: Opt) -> i32 {
        let Opt::Cast { slot, face, costs } = o else {
            return 0;
        };
        if top.controller == self.me() {
            return 0;
        }
        if self.is(slot, names::SUBTLETY) {
            return self.subtlety(&top, slot, costs);
        }
        if !top.is_spell() {
            // Elite Spellbinder's trigger is about to exile a card from our hand: what we
            // could cast now is better cast than lost, as at their end step.
            let spellbinder = self.p.t.def(top.card).name == names::ELITE_SPELLBINDER;
            return if spellbinder { self.their_end(o) } else { 0 };
        }
        let counter = self.counter_score(&top, slot, face);
        // A wheel we let resolve shuffles our hand away, and their Teferi keeps us to sorcery
        // speed ("Each opponent can cast spells only any time they could cast a sorcery"):
        // what we could cast now is better cast than lost, unless we counter it.
        let teferi = self.p.t.def(top.card).name == names::TEFERI_TIME_RAVELER;
        let countered = self.hand(self.me()).iter().any(|s| {
            self.obs().card(s).is_some_and(|c| {
                i32::from(self.p.t.def(c).front().cost.mana_value()) <= self.mana(self.me())
            }) && self.counter_score(&top, s, FaceIx::FRONT) > 0
        });
        if counter == 0 && !countered && (self.p.values.get(top.card).wheel || teferi) {
            return self.instant_speed(o, if teferi { -1 } else { 0 });
        }
        counter
    }

    /// The score of countering the opponent's spell `top` with the card in `slot`, cast as
    /// face `face`, by counterspell discipline; 0 when it is no counter or not worth it.
    fn counter_score(&self, top: &StackEntry, slot: Slot, face: FaceIx) -> i32 {
        let threat = self.threat(top);
        let spare = self.counters_in_hand().count() >= 2;
        let last_card = self.hand(self.opp()).count() <= 1;
        let min = if spare || last_card {
            COUNTER_SPARE
        } else {
            MAJOR
        };
        let theirs = self.mana(self.opp());
        match (self.name(slot), face) {
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
            (Some(names::MANA_DRAIN | names::COUNTERSPELL), _) if threat >= min => 280,
            // Reprieve: "Return target spell to its owner's hand. Draw a card." A counter for
            // tempo when its controller has not the mana to cast it again this turn, and a
            // card either way.
            (Some(names::REPRIEVE), _)
                if threat >= COUNTER_SPARE.min(min)
                    && theirs < i32::from(self.p.t.def(top.card).front().cost.mana_value())
                    && self.draw_safe(1) =>
            {
                200
            }
            // Sink into Stupor returns the spell to its owner's hand: a counter for tempo,
            // for a major threat when no counterspell can answer it, and only when its
            // controller has not the mana to cast it again this turn.
            (Some(names::SINK_INTO_STUPOR), FaceIx::FRONT)
                if threat >= MAJOR
                    && theirs < i32::from(self.p.t.def(top.card).front().cost.mana_value()) =>
            {
                150
            }
            _ => 0,
        }
    }

    /// The score of casting Subtlety, in full or for evoke, against the opponent's spell
    /// `top`: "choose up to one target creature spell or planeswalker spell. Its owner puts
    /// it on their choice of the top or bottom of their library." They draw it again, so it
    /// is tempo, behind the counterspells: in full for a spell worth a counter, for evoke
    /// (two cards for it) only for one that would win the game.
    fn subtlety(&self, top: &StackEntry, slot: Slot, costs: Castings) -> i32 {
        let types = self.p.t.def(top.card).face(top.face).types;
        if !top.is_spell() || !types.intersects(Types::CREATURE.union(Types::PLANESWALKER)) {
            return 0;
        }
        let threat = self.threat(top);
        let Some(card) = self.obs().card(slot) else {
            return 0;
        };
        let evoke = costs
            .paid(self.p.t.def(card).front())
            .any(|o| matches!(o, CastingOption::Evoke { .. }));
        match evoke {
            false if threat >= MAJOR => 160,
            true if threat >= 2 * MAJOR || (threat >= MAJOR && self.dying()) => 120,
            _ => 0,
        }
    }

    /// What letting an opponent's spell resolve would cost us: its value, or for a spell
    /// aimed at ours, the value of what it hits; for a wheel, what it gives them over us.
    fn threat(&self, e: &StackEntry) -> i32 {
        let def = self.p.t.def(e.card);
        let mut v = i32::from(self.p.values.get(e.card).value);
        if def.name == names::JACE_WIELDER_OF_MYSTERIES && wins_by_decking(&self.p, e.controller) {
            v = v.max(JACE_WINS);
        }
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
        if !e.costs.is_empty() && def.name == names::THIEVING_SKYDIVER {
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
                } else if v > 0 && !self.opp_can_counter() {
                    // A window the opponent cannot counter in is for our best threat first,
                    // ahead of removal that could wait.
                    let worth = self
                        .obs()
                        .card(slot)
                        .map_or(0, |c| i32::from(self.p.values.get(c).value));
                    let removal = self
                        .name(slot)
                        .is_some_and(|n| self.removal_score(slot, n, face, -1).is_some());
                    if worth >= MAJOR && !removal {
                        v.max(850 + worth)
                    } else {
                        v
                    }
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
            } if precombat && self.crew_lethal(slot, 0) => 950,
            _ => 0,
        }
    }

    /// Whether crewing our Vehicle in `vehicle` (crew 5, as Swift Reconfiguration makes one),
    /// with summoning-sick creatures first and `extra` more power from a creature cast first,
    /// makes an attack that gets enough damage past the opponent's untapped creatures to
    /// kill.
    fn crew_lethal(&self, vehicle: Slot, extra: i32) -> bool {
        // Crewed already, it is a creature until end of turn.
        if self.has_type(vehicle, Types::CREATURE) {
            return false;
        }
        let power = |s: Slot| i32::from(self.obs().derived(s).power.max(0));
        let flies = |s: Slot| self.obs().derived(s).keywords.intersects(Keywords::FLYING);
        let sick = |s: Slot| self.obs().object(s).flags.has(ObjectFlags::SUMMONING_SICK);
        let mut crew: Vec<Slot> = self
            .creatures(self.me())
            .filter(|&s| !self.tapped(s))
            .collect();
        // Summoning-sick creatures may crew; they could not attack anyway.
        crew.sort_by_key(|&s| (!sick(s), power(s)));
        let mut crewed = extra;
        let mut attackers = Vec::new();
        for s in crew {
            if crewed < 5 {
                crewed += power(s);
            } else if !sick(s) {
                attackers.push((power(s), flies(s)));
            }
        }
        if crewed < 5 {
            return false;
        }
        attackers.push((power(vehicle), flies(vehicle)));
        let blockers: Vec<bool> = self
            .creatures(self.opp())
            .filter(|&s| !self.tapped(s))
            .map(|s| self.reaches(s))
            .collect();
        through(attackers, blockers) >= i32::from(self.obs().player(self.opp()).life)
    }

    /// Our uncrewed Vehicle, if any: a creature of ours the opponent's Swift Reconfiguration
    /// made one.
    fn vehicle(&self) -> Option<Slot> {
        self.permanents(self.me()).find(|&s| {
            !self.has_type(s, Types::CREATURE)
                && self.obs().battlefield().iter().any(|&a| {
                    self.obs().object(a).attached_to == Some(s)
                        && self.is(a, names::SWIFT_RECONFIGURATION)
                })
        })
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
        let (w, u) = self.sources();
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
    fn spoils_tax_room(&self, slot: Slot, face: FaceIx, costs: Castings) -> bool {
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
        let left = self.mana_after(self.p.t.def(card).face(face), costs);
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

    fn main_cast(&self, slot: Slot, face: FaceIx, costs: Castings, precombat: bool) -> i32 {
        let Some(name) = self.name(slot) else {
            return 0;
        };
        let v = self.p.action(Opt::Cast { slot, face, costs });
        let card = self.obs().card(slot).expect("a known card");
        let f = self.p.t.def(card).face(face);
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
        // Overloaded, a removal spell is a sweeper, weighed as the rollout policy does.
        let overloaded = costs
            .paid(f)
            .any(|o| matches!(o, CastingOption::Overload { .. }));
        if !overloaded && let Some(removal) = self.removal_score(slot, name, face, -1) {
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
            // Sink into Stupor's bounce goes after the removal that answers for good, which
            // then takes the target both want.
            let bounce = if name == names::SINK_INTO_STUPOR {
                40
            } else {
                0
            };
            return if (!flash || sorcery_only || urgent) && keeps {
                700 + removal.0 - bounce
            } else {
                0
            };
        }
        // Removal with nothing worth it waits.
        if (REMOVAL.contains(&name) && !overloaded) || v <= 0 {
            return 0;
        }
        // Subtlety's evoke is two cards for its enters ability, which answers only a spell.
        if costs
            .paid(f)
            .any(|o| matches!(o, CastingOption::Evoke { .. }))
        {
            return 0;
        }
        let creature = f.types.intersects(Types::CREATURE);
        // A creature that crews our Vehicle for a lethal attack comes down before combat.
        if precombat
            && creature
            && self
                .vehicle()
                .is_some_and(|v| self.crew_lethal(v, i32::from(f.power.unwrap_or(0))))
        {
            return 950;
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
            // Before the cheap spells that would spend the mana to pay for Mana Tithe.
            return 900;
        }
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
    fn card_cast(&self, slot: Slot, name: &str, costs: Castings) -> Option<i32> {
        let sorcery_only = self.sorcery_only();
        Some(match name {
            // Its coin flips, 3 damage on half our upkeeps, cost more life over these long
            // games than its mana is worth, but while we are short of lands for the spells in
            // hand.
            names::MANA_CRYPT => {
                let lands = self.lands(self.me()) as i32;
                let stuck = self.hand(self.me()).iter().any(|s| {
                    s != slot
                        && !self.p.is_land(s)
                        && self.obs().card(s).is_some_and(|c| {
                            i32::from(self.p.t.def(c).front().cost.mana_value()) > lands + 1
                        })
                });
                if lands <= CRYPT_LANDS && stuck && self.life() > 10 {
                    990
                } else {
                    0
                }
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
                Some(worth) if !costs.is_empty() => 400 + worth,
                _ if !costs.is_empty() => 0,
                _ => self.hold_for_counter(slot, 20),
            },
            names::OVERLORD_OF_THE_MISTMOORS => {
                // In full when it can be: the impending option is the one with a cost bit. When
                // we need blockers now, impending leaves mana for another.
                let full = costs.is_empty();
                // Dying, impending's two flying Insects block now, ahead of anything else.
                // In full only when that does not tap the counter we hold up into a counter
                // they could have.
                let safe = !self.opp_can_counter() || self.keeps_counter_mana(7);
                match (full, self.dying()) {
                    (false, true) => 900,
                    (true, false) if safe => 650,
                    (true, false) => 550,
                    _ => 600,
                }
            }
            names::THE_WANDERING_EMPEROR if !sorcery_only => {
                // Flash: she waits for the opponent's turn, unless her +1 on an attacker
                // makes our attack lethal, there is a tapped creature to exile now, or the
                // opponent is tapped out.
                if self.emperor_lethal().is_some() {
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
    fn mana_after(&self, f: &Face, costs: Castings) -> i32 {
        let mut cost = i32::from(f.cost.mana_value());
        for option in costs.paid(f) {
            match option {
                CastingOption::Kicker { cost: c, min_x } => {
                    cost += i32::from(c.mana_value() + min_x);
                }
                CastingOption::Offspring(c) => cost += i32::from(c.mana_value()),
                // An alternative cost, instead of the mana cost.
                CastingOption::Impending { cost: c, .. }
                | CastingOption::Overload { cost: c, .. }
                | CastingOption::Evoke { cost: c, .. } => cost = i32::from(c.mana_value()),
            }
        }
        self.mana(self.me()) - cost
    }

    /// Whether casting `card` would tap us out into a Mana Tithe we have not seen: the
    /// opponent holds a card and has white mana up, and no Mana Tithe is anywhere we can see.
    fn tithe_risk(&self, card: beryl_engine::CardIx) -> bool {
        let cost = i32::from(self.p.t.def(card).front().cost.mana_value());
        let opp = self.opp();
        let white = |s: Slot| self.makes(s).0;
        self.mana(self.me()) - cost <= 0
            && !self.hand(opp).is_empty()
            && (self.obs().player(opp).mana[ManaColor::White] > 0
                || self.permanents(opp).any(|s| !self.tapped(s) && white(s)))
            && self.in_their_deck(names::MANA_TITHE)
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
        // Under their Teferi our counters wait for our own turn.
        if self.sorcery_only() {
            return v;
        }
        // Against a hand of three or more, any spell short of a major threat waits.
        let minor = if self.hand(self.opp()).count() >= 3 {
            MAJOR
        } else {
            COUNTER_SPARE
        };
        if worth >= minor
            || self.hand(self.opp()).is_empty()
            || self.dying()
            || !self.opp_major_next()
        {
            return v;
        }
        let Some(keep) = self.counter_mana().min() else {
            return v;
        };
        let cost = i32::from(self.p.t.def(card).front().cost.mana_value());
        if self.mana(self.me()) - cost >= keep {
            v
        } else {
            0
        }
    }

    /// Whether the opponent could cast a major threat on their next turn, worth the counter
    /// held up for it: the cheapest cost three (Teferi, Adeline), with their lands, a land
    /// drop and Mana Crypt's two.
    fn opp_major_next(&self) -> bool {
        let opp = self.opp();
        let crypts = self
            .permanents(opp)
            .filter(|&s| self.is(s, names::MANA_CRYPT))
            .count();
        self.lands(opp) + 1 + 2 * crypts >= 3
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
        // Only what our mana could cast at all is worth keeping it for, and nothing under
        // their Teferi, which keeps us from casting it on their turn.
        if self.sorcery_only() {
            return true;
        }
        let mana = self.mana(self.me());
        self.counter_mana()
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

    /// Whether `draws` draws by choice now leave a card for our next draw step, the draw from
    /// an empty library losing the game
    /// ([CR 704.5b](https://yawgatog.com/resources/magic-rules/#R7045b)); once the library
    /// runs low, only as the rollout policy may draw (a wheel in hand, or our Jace, Wielder
    /// of Mysteries out).
    fn draw_safe(&self, draws: usize) -> bool {
        self.obs().library(self.me()).len() > draws && self.p.may_draw()
    }

    /// Whether our Jace, Vryn's Prodigy could loot now: "{T}: Draw a card, then discard a card."
    fn can_loot(&self) -> bool {
        self.draw_safe(1)
            && self.creatures(self.me()).any(|s| {
                let o = self.obs().object(s);
                self.is(s, names::JACE_VRYNS_PRODIGY)
                    && o.face == FaceIx::FRONT
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
        // Cast from the graveyard (Jace, Telepath Unbound's −3), it is not shuffled in either.
        let own = from_hand
            || self
                .obs()
                .graveyard(self.me())
                .iter()
                .any(|&s| self.is(s, wheel));
        let ours = self.hand(self.me()).count() as i32 - i32::from(from_hand);
        let theirs = self.hand(self.opp()).count() as i32;
        let gain = theirs - ours;
        // Never refill the opponent's hand when the board is already winning.
        let power: i32 = self
            .creatures(self.me())
            .map(|s| i32::from(self.obs().derived(s).power.max(0)))
            .sum();
        let winning = power >= i32::from(self.obs().player(self.opp()).life);
        // Nor when far behind, facing a swing for half our life or low on it: a wheel refills
        // the hand that wins the game for them.
        let behind = 2 * self.threat_past(None) >= self.life()
            || self.life() <= 7
            || (self.board(self.opp()) - self.board(self.me()) >= 100
                && self.life() < i32::from(self.obs().player(self.opp()).life));
        // Out of cards, it refuels us at least as much as them.
        let refill = gain >= 2 || (gain >= 0 && ours <= 1);
        // A wheel shuffles our graveyard back in: the way out of drawing from an empty library.
        let me = self.me();
        // The cards it shuffles into our library, to draw seven from.
        let pool = self.obs().library(me).len()
            + self.obs().graveyard(me).len()
            + self.hand(me).count() as usize
            - usize::from(own);
        let decking = self.obs().library(me).len() <= 1 && pool >= 8;
        // Nor while we hold a real spell the wheel would shuffle away, one our lands have the
        // colors for.
        let holding = self.hand(self.me()).iter().any(|s| {
            !self.is(s, wheel)
                && !self.p.is_land(s)
                && self.value(s) >= COUNTER_SPARE
                && self.colors_for(s)
        });
        // Our hand empty and theirs full, it refuels us more than them, behind or not; so
        // does one that gives them two cards or fewer and us three more than them.
        let empty = (ours == 0 && theirs >= 3) || (theirs >= 5 && gain >= 3);
        // Never with fewer than seven cards to draw, nor by choice with too few to leave one for
        // our next draw step
        // ([CR 704.5b](https://yawgatog.com/resources/magic-rules/#R7045b): "If a player
        // attempted to draw a card from a library with no cards in it ... that player loses
        // the game").
        let (survives, drawable) = (pool >= 7, pool >= 8);
        // When their next attack kills us through our blockers, it is the way out, while it
        // leaves mana to cast what it draws: Time Spiral untaps six lands, Timetwister costs
        // three.
        let left = if spiral {
            self.mana(me) - 6 + self.lands(me).min(6) as i32
        } else {
            self.mana(me) - 3
        };
        let out = self.threat_past(None) >= self.life() && left >= 3;
        survives
            && (decking
                || out
                || (drawable && !winning && !holding && (empty || (!behind && refill))))
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
                self.p.t.def(c).face(face).types.intersects(Types::ARTIFACT)
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
    fn removal_score(&self, slot: Slot, name: &str, face: FaceIx, min: i32) -> Option<(i32, bool)> {
        // Oust's 3 life can be the 3 that keeps a low opponent alive, or that our attack this
        // turn falls short by, with the removal we have; and the card it puts into their
        // library is one to draw from a library that would otherwise run out.
        let life = i32::from(self.obs().player(self.opp()).life);
        if name == names::OUST
            && ((!self.dying() && (life <= 6 || self.attack_power(&[], None) + 3 >= life))
                || self.lethal_blockers().is_some()
                || self.obs().library(self.opp()).len() <= 1)
        {
            return None;
        }
        let threshold = match name {
            names::SINK_INTO_STUPOR if face == FaceIx::FRONT => 50,
            names::OUST | names::WINDS_OF_ABANDON => 35,
            names::GET_LOST => 50,
            names::AANGS_ICEBERG => 45,
            names::SWIFT_RECONFIGURATION => 40,
            names::PORTABLE_HOLE => 25,
            // Fractured Identity takes the permanent and gives us a copy: for a threat.
            names::FRACTURED_IDENTITY => 55,
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
        let a: &Ability = self
            .p
            .t
            .def(card)
            .face(face)
            .spell_ability()
            .or_else(|| self.etb_targets(card))?;
        let spec = a.targets.first()?;
        let target = self
            .permanents(self.opp())
            .filter(|&s| self.obs().matches(self.p.t, &spec.filter, s))
            .max_by_key(|&s| self.removal_rank(name, s))?;
        let walker = self.has_type(target, Types::PLANESWALKER);
        let ours = self.obs().turn().active == self.me();
        // A planeswalker our attack can kill this turn needs no removal.
        if walker
            && ours
            && self.attack_power(&[], None) >= i32::from(self.obs().object(target).counters.loyalty)
        {
            return None;
        }
        let v = self.removal_rank(name, target);
        (v > 0 && v >= min).then_some((v, walker))
    }

    /// What the removal spell `name` taking the opponent's permanent in `slot` is worth: its
    /// value, more for the creature that would kill us or an Aang's Iceberg holding ours, and
    /// 1000 more for a blocker whose removal opens lethal; 0 for one it would not answer.
    fn removal_rank(&self, name: &str, slot: Slot) -> i32 {
        let worth = if name == names::SINK_INTO_STUPOR {
            self.bounce_value(slot)
        } else if name == names::SWIFT_RECONFIGURATION && self.is(slot, names::JACE_VRYNS_PRODIGY) {
            // A Vehicle Jace still loots, and still flips.
            0
        } else if name == names::GET_LOST && returns_as_token(&self.p, slot) {
            // It would come back as a token copy of itself.
            0
        } else if name == names::AANGS_ICEBERG && self.impending(slot) {
            // It would come back from Aang's Iceberg's exile in full, a 6/6 with Insects.
            0
        } else {
            self.value(slot) + self.killer_bonus(slot) + self.released(slot)
        };
        // Not for Oust, whose 3 life can be the 3 they survive at, nor for a blocker it would
        // not remove.
        let opens = name != names::OUST && worth > 0 && self.lethal_blocker(slot);
        worth + if opens { 1000 } else { 0 }
    }

    /// Whether removing untapped creatures of the opponent's before combat, as many as our
    /// removal and bounces could take now, would let our attack kill them when it would not
    /// now.
    fn opens_lethal(&self) -> bool {
        self.lethal_blockers().is_some_and(|b| !b.is_empty())
    }

    /// Whether `slot` is one of the blockers whose removal before combat opens lethal.
    fn lethal_blocker(&self, slot: Slot) -> bool {
        self.lethal_blockers().is_some_and(|b| b.contains(&slot))
    }

    /// The fewest untapped creatures of the opponent's whose removal before combat lets our
    /// attack kill them, no more than we have removal and bounces for this turn: empty when
    /// the attack kills already, `None` when no such set exists.
    fn lethal_blockers(&self) -> Option<&[Slot]> {
        self.lethal
            .get_or_init(|| {
                let life = i32::from(self.obs().player(self.opp()).life);
                if self.obs().turn().step != TurnStep::PrecombatMain
                    || self.obs().turn().active != self.me()
                {
                    return None;
                }
                let blockers: Vec<Slot> = self
                    .creatures(self.opp())
                    .filter(|&s| !self.tapped(s))
                    .collect();
                // A wall of more blockers than this is not one removal or two from lethal.
                let most = if blockers.len() > 12 {
                    0
                } else {
                    self.removals_now().min(blockers.len())
                };
                // Every subset of `size` blockers, as a bit set over `blockers`.
                for size in 0..=most {
                    let found = (0u32..1 << blockers.len())
                        .filter(|m| m.count_ones() as usize == size)
                        .map(|m| {
                            (0..blockers.len())
                                .filter(|&i| m & (1 << i) != 0)
                                .map(|i| blockers[i])
                                .collect::<Vec<Slot>>()
                        })
                        .find(|removed| self.attack_power(removed, None) >= life);
                    if found.is_some() {
                        return found;
                    }
                }
                None
            })
            .as_deref()
    }

    /// How many of the opponent's creatures our removal and bounces could take this turn:
    /// each planeswalker of ours with a bounce it has the loyalty for and has not used (Jace,
    /// the Mind Sculptor's −1, Teferi's −3), and the removal spells in hand our mana could
    /// cast, the cheapest first. Oust is left out: its 3 life can be the 3 they survive at.
    fn removals_now(&self) -> usize {
        let walkers = self
            .permanents(self.me())
            .filter(|&s| {
                !self
                    .obs()
                    .object(s)
                    .flags
                    .has(ObjectFlags::LOYALTY_ACTIVATED)
            })
            .filter(|&s| {
                let loyalty = self.obs().object(s).counters.loyalty;
                match self.name(s) {
                    Some(names::JACE_THE_MIND_SCULPTOR) => loyalty >= 1,
                    Some(names::TEFERI_TIME_RAVELER) => loyalty >= 3,
                    _ => false,
                }
            })
            .count();
        let mut costs: Vec<i32> = self
            .hand(self.me())
            .iter()
            .filter(|&s| {
                self.name(s)
                    .is_some_and(|n| n != names::OUST && REMOVAL.contains(&n))
            })
            .filter_map(|s| self.obs().card(s))
            .map(|c| i32::from(self.p.t.def(c).front().cost.mana_value()))
            .collect();
        costs.sort_unstable();
        let mut mana = self.mana(self.me());
        let spells = costs
            .iter()
            .take_while(|&&c| {
                mana -= c;
                mana >= 0
            })
            .count();
        // One of them being aimed now, already paid for: a removal spell, or a walker's
        // bounce (a minus ability).
        let aiming = self.obs().stack().last().is_some_and(|e| {
            let def = self.p.t.def(e.card);
            let bounce = matches!(
                def.name,
                names::JACE_THE_MIND_SCULPTOR | names::TEFERI_TIME_RAVELER
            ) && e.ability.is_some_and(|a| {
                def.face(e.face)
                    .ability(a)
                    .cost
                    .loyalty
                    .is_some_and(|l| l < 0)
            });
            e.controller == self.me()
                && (bounce || (def.name != names::OUST && REMOVAL.contains(&def.name)))
        });
        walkers + spells + usize::from(aiming)
    }

    /// The damage our creatures able to attack would deal past the opponent's untapped
    /// creatures but those `removed`, `pumped` with one more power, as before combat; blocked
    /// as [`through`] has it.
    fn attack_power(&self, removed: &[Slot], pumped: Option<Slot>) -> i32 {
        if self.obs().turn().step != TurnStep::PrecombatMain {
            return 0;
        }
        let flies = |s: Slot, k: Keywords| self.obs().derived(s).keywords.intersects(k);
        let attackers: Vec<(i32, bool)> = self
            .creatures(self.me())
            .filter(|&s| {
                let o = self.obs().object(s);
                !o.flags.has(ObjectFlags::TAPPED) && !o.flags.has(ObjectFlags::SUMMONING_SICK)
            })
            .map(|s| {
                let power =
                    i32::from(self.obs().derived(s).power.max(0)) + i32::from(pumped == Some(s));
                (power, flies(s, Keywords::FLYING))
            })
            .collect();
        let blockers: Vec<bool> = self
            .creatures(self.opp())
            .filter(|&s| !self.tapped(s) && !removed.contains(&s))
            .map(|s| self.reaches(s))
            .collect();
        through(attackers, blockers)
    }

    /// Whether a creature could block a flier: it has flying or reach.
    fn reaches(&self, slot: Slot) -> bool {
        self.obs()
            .derived(slot)
            .keywords
            .intersects(Keywords::FLYING.union(Keywords::REACH))
    }

    /// The creature of ours whose +1/+1 counter from The Wandering Emperor's +1 ("Put a +1/+1
    /// counter on up to one target creature. It gains first strike until end of turn.") makes
    /// our attack lethal when it is not without it: one that gets past the blockers.
    fn emperor_lethal(&self) -> Option<Slot> {
        let life = i32::from(self.obs().player(self.opp()).life);
        if self.attack_power(&[], None) >= life {
            return None;
        }
        self.creatures(self.me())
            .find(|&c| self.attack_power(&[], Some(c)) >= life)
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
        let a = &def.face(o.face).abilities[usize::from(i)];
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
            names::JACE_VRYNS_PRODIGY | names::STAFF_OF_THE_STORYTELLER if !self.draw_safe(1) => 0,
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
        let face = self.p.t.def(card).face(o.face);
        let loyalty = i32::from(o.counters.loyalty);
        // An ability that would leave no loyalty is no option, short of an ultimate or a bounce
        // that opens lethal; the walker then grows instead.
        let lethal = self.opens_lethal()
            && matches!(
                self.name(slot),
                Some(names::JACE_THE_MIND_SCULPTOR | names::TEFERI_TIME_RAVELER)
            );
        let keeps = |i: u8| {
            face.abilities[usize::from(i)]
                .cost
                .loyalty
                .is_some_and(|l| loyalty + i32::from(l) >= 1 || l <= -8 || lethal)
        };
        // Nor one that draws by choice when that could deck us, as Teferi, Hero of
        // Dominaria's +1.
        let draws = |i: u8| {
            o.face == FaceIx::FRONT && self.p.values.get(card).draws(i) && !self.draw_safe(1)
        };
        self.preferred_ability(slot)
            .filter(|&i| keeps(i) && !draws(i))
            .or_else(|| {
                face.abilities
                    .iter()
                    .enumerate()
                    .position(|(i, a)| a.cost.loyalty.is_some_and(|l| l > 0) && !draws(i as u8))
                    .map(|i| i as u8)
            })
    }

    /// The loyalty ability the rules above prefer for a planeswalker of ours, by index.
    fn preferred_ability(&self, slot: Slot) -> Option<u8> {
        let o = self.obs().object(slot);
        let card = self.obs().card(slot)?;
        let def = self.p.t.def(card);
        let face = def.face(o.face);
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
            names::JACE_THE_MIND_SCULPTOR => by_cost(self.mind_sculptor(loyalty, threatened)),
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
                if self.emperor_lethal().is_some() {
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
            // −8: "Draw seven cards. Then if your library has no cards in it, you win the
            // game." Only when it wins.
            names::JACE_WIELDER_OF_MYSTERIES => {
                if loyalty >= 8 && self.obs().library(self.me()).len() <= 7 {
                    by_cost(-8)
                } else {
                    by_cost(1)
                }
            }
            // −8 for the emblem; −3 puts a threat third from the top of its owner's library;
            // +1 draws, unless that would deck us.
            names::TEFERI_HERO_OF_DOMINARIA => {
                if loyalty >= 8 {
                    return by_cost(-8);
                }
                let target = self
                    .permanents(self.opp())
                    .filter(|&s| !self.has_type(s, Types::LAND))
                    .map(|s| self.value(s) + self.killer_bonus(s))
                    .max()
                    .unwrap_or(0);
                let decking = !self.draw_safe(1) && target > 0;
                if loyalty >= 3 && (target >= MAJOR || threatened > loyalty || decking) {
                    by_cost(-3)
                } else {
                    by_cost(1)
                }
            }
            JACE_TELEPATH_UNBOUND => {
                // The −9 emblem mills the opponent five a spell: not toward their own Jace,
                // Wielder of Mysteries.
                if loyalty >= 9 && !self.their_jace_live() {
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

    /// The loyalty cost of the ability our Jace, the Mind Sculptor should activate, with
    /// `loyalty` and facing `threatened` damage from the opponent's swing back.
    fn mind_sculptor(&self, loyalty: i32, threatened: i32) -> i8 {
        if loyalty >= 12 {
            return -12;
        }
        // Returning a blocker opens lethal.
        if self.opens_lethal() {
            return -1;
        }
        // A token worth returning is gone for good.
        let token = self.creatures(self.opp()).any(|s| {
            self.obs().object(s).flags.has(ObjectFlags::TOKEN) && self.bounce_value(s) >= 40
        });
        // +2 leaves him at loyalty + 2; the opponent's swing back decides the rest.
        if threatened >= loyalty + 2 {
            // −1 returns the creature best returned: worth it if that saves him, or on a token
            // worth it. Otherwise he is lost anyway: draw three.
            let bounced = self
                .creatures(self.opp())
                .max_by_key(|&s| self.bounce_value(s));
            if token || bounced.is_some_and(|b| self.threat_past(Some(b)) < loyalty - 1) {
                return -1;
            }
            return if self.draw_safe(3) { 0 } else { 2 };
        }
        // Dead on board ourselves, or low on life, he returns the biggest attacker.
        let low = self.life() <= 6 || self.dying();
        if low && loyalty >= 2 && self.creatures(self.opp()).next().is_some() {
            return -1;
        }
        // A big threat is worth returning even while he is safe.
        let big = self
            .creatures(self.opp())
            .any(|s| self.bounce_value(s) >= 80);
        if (big || token) && loyalty >= 2 {
            return -1;
        }
        if self.hand(self.me()).count() <= 2 && threatened < loyalty && self.draw_safe(3) {
            return 0;
        }
        2
    }

    /// The power the opponent's creatures, but `except`, could swing at us or a planeswalker
    /// of ours next turn past our creatures, each blocking one attacker it can.
    fn threat_past(&self, except: Option<Slot>) -> i32 {
        self.threat_past_fewer(except, 0)
    }

    /// [`Expert::threat_past`] with `fewer` of our blockers gone first, the best of them.
    fn threat_past_fewer(&self, except: Option<Slot>, fewer: usize) -> i32 {
        let attackers: Vec<(i32, bool)> = self
            .creatures(self.opp())
            .filter(|&s| Some(s) != except)
            .map(|s| {
                let power = i32::from(self.obs().derived(s).power.max(0));
                (
                    power,
                    self.obs().derived(s).keywords.intersects(Keywords::FLYING),
                )
            })
            .filter(|&(p, _)| p > 0)
            .collect();
        let mut blockers: Vec<bool> = self.creatures(self.me()).map(|s| self.reaches(s)).collect();
        // The best of them go first: those that block fliers too.
        for _ in 0..fewer.min(blockers.len()) {
            let i = blockers.iter().position(|&r| r).unwrap_or(0);
            blockers.swap_remove(i);
        }
        through(attackers, blockers)
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
                names::GET_LOST | names::OUST => {
                    self.removal_score(s, name, FaceIx::FRONT, -1).is_some()
                }
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
        // An attack that kills us, or leaves us at 3 life or less, is worth a chump blocker.
        let chump = damage >= i32::from(self.obs().player(self.me()).life) - 3;
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
                    if chump
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
                // A counterspell we have seen waits for any of them.
                let cost = self
                    .obs()
                    .card(slot)
                    .map_or(0, |c| i32::from(self.p.t.def(c).front().cost.mana_value()));
                if self.counter_known(self.mana(self.me()) - cost) {
                    return 0;
                }
                match name {
                    names::THE_WANDERING_EMPEROR => 500,
                    names::SPHINX_OF_FALSE_CONCLUSIONS => 480,
                    names::MALCOLM_ALLURING_SCOUNDREL => 450,
                    names::SNAPCASTER_MAGE => 300,
                    // "Draw three cards. Then discard two cards unless you discard an artifact
                    // card."
                    names::THIRST_FOR_DISCOVERY if self.draw_safe(3) => 200,
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
                        if self.draw_safe(1) =>
                    {
                        100
                    }
                    Some(names::SPARAS_HEADQUARTERS) if self.lands(self.me()) >= 5 => 90,
                    // Anything else the rollout policy would do, as Currency Converter's loot,
                    // with mana that would go unused.
                    _ if min < 0 && self.p.action(o) > 0 => 50,
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
        let (mut w, mut u, mut both, mut c) = (
            i32::from(pool[ManaColor::White]),
            i32::from(pool[ManaColor::Blue]),
            0,
            0,
        );
        c += i32::from(pool[ManaColor::Colorless]);
        for s in self.permanents(me).filter(|&s| !self.tapped(s)) {
            match (self.makes(s), self.name(s)) {
                ((true, true), _) => both += 1,
                ((true, false), _) => w += 1,
                ((false, true), _) => u += 1,
                (_, Some(names::STRIP_MINE)) => c += 1,
                (_, Some(names::MANA_CRYPT)) => c += 2,
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

    /// What returning a permanent of the opponent's to its owner's hand is worth: what it
    /// takes from them, and more for a blocker whose return opens lethal, the creature that
    /// would kill us, or an Aang's Iceberg holding a permanent of ours.
    fn bounce_value(&self, slot: Slot) -> i32 {
        // Before combat, returning a blocker may open lethal; dead on board, the creature
        // that would kill us is the one to return.
        let lethal = if self.lethal_blocker(slot) { 500 } else { 0 };
        self.returned(slot) + lethal + self.killer_bonus(slot) + self.released(slot)
    }

    /// What returning the permanent in `slot` to its owner's hand takes from them, by itself:
    /// a token ceases to exist, but Mana Crypt comes back for free and a creature with an
    /// enters-the-battlefield ability comes back with it again.
    fn returned(&self, slot: Slot) -> i32 {
        let v = self.value(slot);
        if self.obs().object(slot).flags.has(ObjectFlags::TOKEN) {
            return v + 15;
        }
        // A planeswalker comes back with its printed loyalty: returning it takes the loyalty
        // it gained and the mana to cast it again.
        if self.has_type(slot, Types::PLANESWALKER)
            && let Some(card) = self.obs().card(slot)
        {
            let f = self.p.t.def(card).face(self.obs().object(slot).face);
            let gained = i32::from(self.obs().object(slot).counters.loyalty)
                - i32::from(f.loyalty.unwrap_or(0));
            return 6 * gained.max(0) + 4 * i32::from(f.cost.mana_value());
        }
        match self.name(slot) {
            // Their Swift Reconfiguration on our creature: returning it frees the creature,
            // but it has flash and costs {W}, so with a white mana up they put it straight
            // back.
            Some(names::SWIFT_RECONFIGURATION) if self.colors_up(self.opp()).0 >= 1 => 0,
            Some(names::SWIFT_RECONFIGURATION) => {
                let host = self
                    .obs()
                    .object(slot)
                    .attached_to
                    .filter(|&h| self.obs().controller(h) == self.me())
                    .and_then(|h| self.obs().card(h));
                v + host.map_or(0, |c| i32::from(self.p.values.get(c).value))
            }
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

    /// Dead on board, low on life, or at 10 or less facing a swing of half of it, what
    /// removing or returning a creature of theirs is worth beyond its value: the creature
    /// that would kill us is the one to answer.
    fn killer_bonus(&self, slot: Slot) -> i32 {
        let pressed = self.dying()
            || self.life() <= 6
            || (self.life() <= 10 && 2 * self.threat_past(None) >= self.life());
        if pressed && self.has_type(slot, Types::CREATURE) {
            20 * i32::from(self.obs().derived(slot).power.max(0))
        } else {
            0
        }
    }

    /// The target spec of the bounce being cast or activated.
    fn bounce_spec(&self) -> Option<&'static beryl_engine::card::TargetSpec> {
        let e = self.obs().stack().last()?;
        let face = self.p.t.def(e.card).face(e.face);
        let a = match e.ability {
            Some(ability) => face.ability(ability),
            None => face.spell_ability_with(e.costs)?,
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
    /// by [`Expert::bounce_value`]; removal by the ranking that chose to cast it; while the
    /// opponent's attackers are declared, an attacker, the one dealing most when the attack
    /// is lethal, else the most valuable.
    fn target(&self, d: &Decision) -> Option<Choice> {
        if let Some(c) = self.crypt_target(d).or_else(|| self.mill_target(d)) {
            return Some(c);
        }
        // The Wandering Emperor's +1 for lethal: on the attacker that gets past the blockers.
        if let Some(e) = self.obs().stack().last()
            && e.controller == self.me()
            && self.p.t.def(e.card).name == names::THE_WANDERING_EMPEROR
            && let Some(pump) = self.emperor_lethal()
            && let beryl_engine::Candidates::Objects(b) = d.candidates
            && b.contains(pump)
        {
            return Some(best(d, |c| Some(i32::from(c.object()? == pump))));
        }
        // Jace, Telepath Unbound's −3: the card worth casting again.
        if let Some(e) = self.obs().stack().last()
            && e.controller == self.me()
            && self.p.t.def(e.card).face_name(e.face) == JACE_TELEPATH_UNBOUND
            && let Some(ability) = e.ability
            && self
                .p
                .t
                .def(e.card)
                .face(e.face)
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
            // In their combat, an attacker ranked as removal there: it is removed from combat
            // as it stops being a creature. Otherwise as the ranking that chose to cast it.
            return Some(best(d, |c| {
                let slot = c.object()?;
                Some(if self.obs().controller(slot) != self.opp() {
                    -self.value(slot)
                } else if self.is(slot, names::JACE_VRYNS_PRODIGY) {
                    0
                } else if self.attacking(slot) {
                    self.their_attack_rank(slot)
                } else {
                    self.removal_rank(names::SWIFT_RECONFIGURATION, slot)
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
        if let Some(c) = self.removal_target(d) {
            return Some(c);
        }
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

    /// A Mana Crypt about to kill us goes, to our own Aang's Iceberg or Teferi.
    fn crypt_target(&self, d: &Decision) -> Option<Choice> {
        let crypt = self.deadly_crypt()?;
        let e = self.obs().stack().last()?;
        if e.controller == self.me()
            && matches!(
                self.p.t.def(e.card).name,
                names::AANGS_ICEBERG | names::TEFERI_TIME_RAVELER
            )
            && let beryl_engine::Candidates::Objects(b) = d.candidates
            && b.contains(crypt)
        {
            return Some(best(d, |c| Some(i32::from(c.object()? == crypt))));
        }
        None
    }

    /// Jace, Wielder of Mysteries' +1: "Target player mills two cards." Not the opponent while
    /// an empty library could win them the game; our own would win it for us.
    fn mill_target(&self, d: &Decision) -> Option<Choice> {
        let e = self.obs().stack().last()?;
        if e.controller != self.me()
            || self.p.t.def(e.card).name != names::JACE_WIELDER_OF_MYSTERIES
            || !matches!(d.candidates, beryl_engine::Candidates::Options(_))
        {
            return None;
        }
        let mill_self = self.their_jace_live();
        Some(best(d, |c| match c.option() {
            Some(Opt::Player(p)) => Some(i32::from((p == self.me()) == mill_self)),
            _ => None,
        }))
    }

    /// Our removal spell's target, or our Aang's Iceberg's enters trigger's, outside their
    /// declare attackers step: by the ranking that chose to cast it
    /// ([`Expert::removal_rank`]).
    fn removal_target(&self, d: &Decision) -> Option<Choice> {
        let turn = self.obs().turn();
        let e = self.obs().stack().last()?;
        let name = self.p.t.def(e.card).name;
        if e.controller != self.me()
            || !(e.is_spell() || name == names::AANGS_ICEBERG)
            || !REMOVAL.contains(&name)
            || (turn.active != self.me() && turn.step == TurnStep::DeclareAttackers)
            || !matches!(d.candidates, beryl_engine::Candidates::Objects(_))
        {
            return None;
        }
        Some(best(d, |c| {
            let slot = c.object()?;
            Some(if self.obs().controller(slot) == self.opp() {
                self.removal_rank(name, slot)
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

    /// Before combat, when removing blockers opens lethal, a harmful effect's target is one of
    /// those blockers.
    fn blocker_target(&self, d: &Decision) -> Option<Choice> {
        let (_, friendly) = self.p.targeting()?;
        // Not Oust: its 3 life can be the 3 they survive at.
        let oust = self
            .obs()
            .stack()
            .last()
            .is_some_and(|e| self.p.t.def(e.card).name == names::OUST);
        if friendly || oust || !matches!(d.candidates, beryl_engine::Candidates::Objects(_)) {
            return None;
        }
        Some(best(d, |c| {
            let slot = c.object()?;
            Some(if self.obs().controller(slot) != self.opp() {
                -self.value(slot)
            } else if self.lethal_blocker(slot) {
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

    /// Permanents to untap, as Time Spiral's "You untap up to six lands": ours only.
    fn choose_up_to(&self, d: &Decision) -> Option<Choice> {
        if !matches!(d.ask, Ask::Untap { .. }) {
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

    /// A fateseal, as Jace, the Mind Sculptor's +2: put the top card on the bottom when it is
    /// one the opponent would want, or one we would not. An explore, as a Map's: put the
    /// revealed card into the graveyard only if it is not worth drawing.
    fn yes_no(&self, d: &Decision) -> Option<Choice> {
        let player = match d.ask {
            Ask::Fateseal { player } => player,
            Ask::Explore => self.me(),
            _ => return None,
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

/// The damage `attackers`, by power and whether they fly, deal past `blockers`, by whether
/// each could block a flier: each blocker blocks the strongest attacker it can, fliers only
/// by fliers ([CR 702.9b](https://yawgatog.com/resources/magic-rules/#R7029b): "A creature
/// with flying can't be blocked except by creatures with flying and/or reach"), a ground
/// attacker by a blocker that could not block a flier when there is one.
fn through(mut attackers: Vec<(i32, bool)>, mut blockers: Vec<bool>) -> i32 {
    attackers.sort_unstable_by(|a, b| b.cmp(a));
    let mut through = 0;
    for (power, flying) in attackers {
        let blocker = if flying {
            blockers.iter().position(|&r| r)
        } else {
            blockers
                .iter()
                .position(|&r| !r)
                .or_else(|| blockers.iter().position(|&r| r))
        };
        match blocker {
            Some(i) => {
                blockers.swap_remove(i);
            }
            None => through += power,
        }
    }
    through
}
