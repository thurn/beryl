//! Combat (CR 506 to 511) with test-only creatures: declarations one pick at a time,
//! evasion, damage and its division, first strike, planeswalkers and combat triggers.

use super::begin;
use crate::card::{
    Ability, AbilityIx, CardDef, Face, Flow, Keywords, Subtype, Subtypes, Trigger, Types,
};
use crate::effect::EffectCtx;
use crate::event::{Event, TokenEntry};
use crate::ids::{Seat, Slot};
use crate::profile::{Breakpoint, stops_for};
use crate::state::{CounterKind, ObjectFlags, Zone};
use crate::{
    Ask, Candidates, CardTable, Choice, Decision, DecisionKind, Decklist, GameTable, Opt, Profile,
    Replay, Scratch, State, Step, TurnStep, apply, next, zone,
};
use crate::{Layout, bodies};

/// A vanilla creature card with keywords.
macro_rules! creature {
    ($name:ident, $text:literal, $power:literal / $toughness:literal, $keywords:expr) => {
        static $name: CardDef = CardDef {
            name: $text,
            faces: &[Face {
                types: Types::CREATURE,
                power: Some($power),
                toughness: Some($toughness),
                keywords: $keywords,
                ..Face::EMPTY
            }],
            tokens: &[],
        };
    };
}

creature!(BEAR, "Bear", 2 / 2, Keywords::NONE);
creature!(GIANT, "Giant", 4 / 4, Keywords::NONE);
creature!(OGRE, "Ogre", 3 / 3, Keywords::NONE);
creature!(BIRD, "Bird", 2 / 2, Keywords::FLYING);
creature!(SPIDER, "Spider", 1 / 4, Keywords::REACH);
creature!(KNIGHT, "Knight", 2 / 2, Keywords::FIRST_STRIKE);
creature!(SENTRY, "Sentry", 1 / 3, Keywords::VIGILANCE);

static WALKER: CardDef = CardDef {
    name: "Walker",
    faces: &[Face {
        types: Types::PLANESWALKER,
        loyalty: Some(3),
        ..Face::EMPTY
    }],
    tokens: &[],
};

#[allow(clippy::unnecessary_wraps)] // a body's signature
fn gain_one<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    ctx.gain_life(ctx.controller(), 1);
    Ok(())
}

#[allow(clippy::unnecessary_wraps)] // a body's signature
fn gain_ten<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    ctx.gain_life(ctx.controller(), 10);
    Ok(())
}

/// "Whenever this attacks, you gain 1 life. Whenever this deals combat damage to a player,
/// you gain 10 life."
static RAIDER: CardDef = CardDef {
    name: "Raider",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(2),
        abilities: &[
            Ability::triggered(Trigger::Attacks, bodies!(gain_one)),
            Ability::triggered(Trigger::CombatDamageToPlayer, bodies!(gain_ten)),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// The token Captain creates: a Raider of its own, whose attack trigger must not trigger.
static RECRUIT: CardDef = CardDef {
    name: "Recruit",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        abilities: &[Ability::triggered(Trigger::Attacks, bodies!(gain_one))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

#[allow(clippy::unnecessary_wraps)] // a body's signature
fn recruit<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    ctx.create_tokens(
        &RECRUIT,
        1,
        TokenEntry {
            tapped: true,
            attacking: true,
            ..TokenEntry::default()
        },
    );
    Ok(())
}

/// "Whenever you attack, create a 1/1 Recruit token that's tapped and attacking."
static CAPTAIN: CardDef = CardDef {
    name: "Captain",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        abilities: &[Ability::triggered(Trigger::YouAttack, bodies!(recruit))],
        ..Face::EMPTY
    }],
    tokens: &[&RECRUIT],
};

/// A 3/3 Vehicle artifact with crew 3.
static WAGON: CardDef = CardDef {
    name: "Wagon",
    faces: &[Face {
        types: Types::ARTIFACT,
        subtypes: Subtypes::of(&[Subtype::Vehicle]),
        power: Some(3),
        toughness: Some(3),
        abilities: &[Ability::crew(3)],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[
    &BEAR, &GIANT, &OGRE, &BIRD, &SPIDER, &KNIGHT, &SENTRY, &WALKER, &RAIDER, &CAPTAIN, &WAGON,
]);

const DECK: &Decklist = &[
    (4, "Bear"),
    (4, "Giant"),
    (4, "Ogre"),
    (4, "Bird"),
    (4, "Spider"),
    (4, "Knight"),
    (4, "Sentry"),
    (4, "Walker"),
    (4, "Raider"),
    (4, "Captain"),
    (4, "Wagon"),
];

/// A game stopped at the attacking player's first precombat main phase, with the event sink
/// recording.
struct Game {
    t: GameTable,
    s: State,
    x: Scratch,
    attacker: Seat,
}

impl Game {
    fn new() -> Self {
        Self::seeded(1)
    }

    fn seeded(seed: u64) -> Self {
        let t = GameTable::new(&CARDS, [DECK, DECK]).unwrap();
        let profile = Profile {
            breakpoint: Some(Breakpoint {
                turn: 1,
                step: TurnStep::PrecombatMain,
            }),
            ..Profile::EXACT
        };
        let mut s = State::new(&t, seed, profile);
        let d = begin(&t, &mut s);
        assert_eq!(d.kind, DecisionKind::Priority);
        let attacker = s.turn().active;
        Game {
            t,
            s,
            x: Scratch::recording(),
            attacker,
        }
    }

    fn defender(&self) -> Seat {
        self.attacker.opponent()
    }

    /// Puts a card of `player`'s from their library or hand onto the battlefield, as if it
    /// had been there since the turn began.
    fn put(&mut self, player: Seat, name: &str) -> Slot {
        let slot = self.cards(player, name)[0];
        self.put_slot(slot)
    }

    /// The slots of `player`'s cards named `name` not yet on the battlefield, lowest first.
    fn cards(&self, player: Seat, name: &str) -> Vec<Slot> {
        (self.s.zone(player, Zone::Library) | self.s.zone(player, Zone::Hand))
            .iter()
            .filter(|&slot| self.t.def(self.s.object(slot).card).name == name)
            .collect()
    }

    fn put_slot(&mut self, slot: Slot) -> Slot {
        zone::move_to(
            &mut self.s,
            &mut crate::event::Sink::off(),
            slot,
            Zone::Battlefield,
        )
        .unwrap();
        self.s.board().set_sick(slot, false);
        if let Some(loyalty) = self.t.def(self.s.card(slot)).front().loyalty {
            self.s
                .board()
                .set_counters(slot, CounterKind::Loyalty, loyalty);
        }
        slot
    }

    fn decide(&mut self) -> Decision {
        match next(&mut self.s, &self.t, &mut self.x, &mut Replay) {
            Step::Decide(d) => d,
            Step::Over(o) => panic!("the game ended: {o:?}"),
        }
    }

    /// Advances to the next decision, which must be of `kind`.
    fn expect(&mut self, kind: DecisionKind) -> Decision {
        let d = self.decide();
        assert_eq!(d.kind, kind, "{d:?}");
        d
    }

    /// Answers the next decision, of `kind`, with the option `opt`.
    fn pick(&mut self, kind: DecisionKind, opt: Opt) {
        let d = self.expect(kind);
        let i = (0..d.len() as u16)
            .find(|&i| d.candidates.option(Choice(i)) == Some(opt))
            .unwrap_or_else(|| panic!("{opt:?} not offered: {d:?}"));
        apply(&mut self.s, &self.t, &mut self.x, &mut Replay, Choice(i));
    }

    /// Answers the next decision, of `kind`, with the object `slot`.
    fn pick_object(&mut self, kind: DecisionKind, slot: Slot) {
        let d = self.expect(kind);
        let i = (0..d.len() as u16)
            .find(|&i| d.candidates.object(Choice(i)) == Some(slot))
            .unwrap_or_else(|| panic!("{slot} not offered: {d:?}"));
        apply(&mut self.s, &self.t, &mut self.x, &mut Replay, Choice(i));
    }

    /// Passes priority at the breakpoint, moving on towards combat.
    fn pass_to_combat(&mut self) {
        self.pick(DecisionKind::Priority, Opt::Pass);
    }

    /// Plays on until the postcombat main phase, where the breakpoint stops the game again.
    fn finish_combat(&mut self) {
        let turn = self.s.turn().number;
        self.s.set_profile(Profile {
            breakpoint: Some(Breakpoint {
                turn,
                step: TurnStep::PostcombatMain,
            }),
            ..Profile::EXACT
        });
        let d = self.decide();
        assert_eq!(
            (d.kind, self.s.turn().step),
            (DecisionKind::Priority, TurnStep::PostcombatMain),
            "{d:?}"
        );
    }

    fn steps(&self) -> Vec<TurnStep> {
        self.x
            .events
            .events()
            .iter()
            .filter_map(|e| match e {
                Event::StepBegins { step, .. } => Some(*step),
                _ => None,
            })
            .collect()
    }

    fn life(&self, player: Seat) -> i16 {
        self.s.player(player).life
    }

    fn on_battlefield(&self, slot: Slot) -> bool {
        self.s.object(slot).zone == Zone::Battlefield
    }
}

/// "Done", then attacking with each of these creatures, in slot order.
fn attacks(slots: &[Slot]) -> Vec<Opt> {
    let mut sorted = slots.to_vec();
    sorted.sort_unstable();
    std::iter::once(Opt::Done)
        .chain(sorted.into_iter().map(Opt::Attack))
        .collect()
}

fn options(d: &Decision) -> Vec<Opt> {
    (0..d.len() as u16)
        .map(|i| d.candidates.option(Choice(i)).unwrap())
        .collect()
}

#[test]
fn unblocked_attackers_tap_and_deal_damage_to_the_defending_player() {
    let mut g = Game::new();
    let (a, d) = (g.attacker, g.defender());
    let bear = g.put(a, "Bear");
    let other = g.put(a, "Bear");
    let ogre = g.put(a, "Ogre");
    g.pass_to_combat();
    // Two interchangeable Bears are one candidate (§7.2).
    let first = g.expect(DecisionKind::Attacker);
    assert_eq!(options(&first), attacks(&[bear.min(other), ogre]));
    g.pick(DecisionKind::Attacker, Opt::Attack(bear.min(other)));
    // The defending player controls no planeswalker: no defender choice.
    let second = g.expect(DecisionKind::Attacker);
    assert_eq!(options(&second), attacks(&[bear.max(other), ogre]));
    g.pick(DecisionKind::Attacker, Opt::Attack(bear.max(other)));
    // Once only "done" is left, it is applied silently (§7.2).
    g.pick(DecisionKind::Attacker, Opt::Done);
    assert!(g.s.object(bear).flags.has(ObjectFlags::TAPPED));
    assert!(!g.s.object(ogre).flags.has(ObjectFlags::TAPPED));
    g.finish_combat();
    assert_eq!(g.life(d), 16);
    assert_eq!(g.life(a), 20);
    // Combat is over: nothing is attacking any more.
    assert!(!g.s.object(bear).flags.has(ObjectFlags::ATTACKING));
    let damage = g.x.events.events().iter().filter(|e| {
        matches!(
            e,
            Event::Damage {
                combat: true,
                amount: 2,
                ..
            }
        )
    });
    assert_eq!(damage.count(), 2);
}

#[test]
fn only_untapped_creatures_controlled_since_the_turn_began_attack() {
    let mut g = Game::new();
    let a = g.attacker;
    let sick = g.put(a, "Bear");
    g.s.board().set_sick(sick, true);
    let tapped = g.put(a, "Ogre");
    g.s.board().set_tapped(tapped, true);
    // Only the defender's creature is ready: the attacking player declares nothing, and with
    // no attackers the declare blockers and combat damage steps are skipped (CR 508.8).
    g.put(g.defender(), "Bear");
    g.pass_to_combat();
    g.finish_combat();
    let steps = g.steps();
    assert!(steps.contains(&TurnStep::DeclareAttackers));
    assert!(!steps.contains(&TurnStep::DeclareBlockers));
    assert!(!steps.contains(&TurnStep::CombatDamage));
}

#[test]
fn vigilance_attacks_without_tapping() {
    let mut g = Game::new();
    let sentry = g.put(g.attacker, "Sentry");
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::Attack(sentry));
    assert!(g.s.object(sentry).flags.has(ObjectFlags::ATTACKING));
    assert!(!g.s.object(sentry).flags.has(ObjectFlags::TAPPED));
    g.finish_combat();
    assert_eq!(g.life(g.defender()), 19);
}

#[test]
fn an_attacked_planeswalker_loses_loyalty_and_dies_at_zero() {
    let mut g = Game::new();
    let (a, d) = (g.attacker, g.defender());
    let ogre = g.put(a, "Ogre");
    let bear = g.put(a, "Bear");
    let walker = g.put(d, "Walker");
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::Attack(ogre));
    // With a planeswalker to attack, the defender is a real choice (CR 508.1b).
    let target = g.expect(DecisionKind::AttackTarget);
    assert_eq!(
        target.ask,
        crate::Ask::AttackTarget {
            attackers: crate::Bitboard::single(ogre)
        }
    );
    assert_eq!(
        options(&target),
        vec![Opt::Defender(None), Opt::Defender(Some(walker))]
    );
    apply(&mut g.s, &g.t, &mut g.x, &mut Replay, Choice(1));
    g.pick(DecisionKind::Attacker, Opt::Attack(bear));
    g.pick(DecisionKind::AttackTarget, Opt::Defender(None));
    g.finish_combat();
    // 3 damage removes all 3 loyalty counters (CR 120.3c); at 0 loyalty the planeswalker
    // goes to its owner's graveyard (CR 704.5i). The Bear hit the player.
    assert_eq!(g.s.object(walker).zone, Zone::Graveyard);
    assert_eq!(g.life(d), 18);
}

#[test]
fn a_planeswalker_survives_damage_short_of_its_loyalty() {
    let mut g = Game::new();
    let (a, d) = (g.attacker, g.defender());
    let bear = g.put(a, "Bear");
    let walker = g.put(d, "Walker");
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::Attack(bear));
    g.pick(DecisionKind::AttackTarget, Opt::Defender(Some(walker)));
    g.finish_combat();
    assert!(g.on_battlefield(walker));
    assert_eq!(g.s.object(walker).counters.loyalty, 1);
    assert_eq!(g.life(d), 20);
}

#[test]
fn flying_attackers_are_blocked_only_by_flying_or_reach() {
    let mut g = Game::new();
    let (a, d) = (g.attacker, g.defender());
    let bird = g.put(a, "Bird");
    let bear = g.put(a, "Bear");
    let ground = g.put(d, "Bear");
    let spider = g.put(d, "Spider");
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::Attack(bird));
    g.pick(DecisionKind::Attacker, Opt::Attack(bear));
    let blockers = g.expect(DecisionKind::Blocker);
    assert_eq!(blockers.player, d);
    assert_eq!(
        options(&blockers),
        vec![Opt::Done, Opt::Block(ground), Opt::Block(spider)]
    );
    // The ground Bear can block only the ground Bear: no choice of attacker.
    g.pick(DecisionKind::Blocker, Opt::Block(ground));
    let spider_targets = g.expect(DecisionKind::Blocker);
    assert_eq!(
        options(&spider_targets),
        vec![Opt::Done, Opt::Block(spider)]
    );
    g.pick(DecisionKind::Blocker, Opt::Block(spider));
    let d2 = g.expect(DecisionKind::BlockTarget);
    assert_eq!(d2.len(), 2);
    g.pick_object(DecisionKind::BlockTarget, bird);
    g.finish_combat();
    assert!(!g.on_battlefield(bear));
    assert!(!g.on_battlefield(ground));
    assert!(g.on_battlefield(bird));
    assert!(g.on_battlefield(spider));
    assert_eq!(g.life(d), 20);
}

#[test]
fn a_defender_with_no_legal_block_is_not_asked() {
    let mut g = Game::new();
    let (a, d) = (g.attacker, g.defender());
    let bird = g.put(a, "Bird");
    g.put(d, "Bear");
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::Attack(bird));
    g.finish_combat();
    assert_eq!(g.life(d), 18);
}

#[test]
fn a_division_among_blockers_is_offered_by_which_blockers_die() {
    // A 3/3 blocked by a 2/2 and a 1/4: kill the 2/2, or kill neither. The 1/4 can't be
    // killed, so 3 outcomes collapse to 2.
    for kill in [false, true] {
        let mut g = Game::new();
        let (a, d) = (g.attacker, g.defender());
        let ogre = g.put(a, "Ogre");
        let bear = g.put(d, "Bear");
        let spider = g.put(d, "Spider");
        g.pass_to_combat();
        g.pick(DecisionKind::Attacker, Opt::Attack(ogre));
        g.pick(DecisionKind::Blocker, Opt::Block(bear));
        g.pick(DecisionKind::Blocker, Opt::Block(spider));
        let division = g.expect(DecisionKind::DamageDivision);
        assert_eq!(division.player, a);
        assert_eq!(options(&division), vec![Opt::Done, Opt::Lethal(bear)]);
        g.pick(
            DecisionKind::DamageDivision,
            if kill { Opt::Lethal(bear) } else { Opt::Done },
        );
        g.finish_combat();
        assert_eq!(g.on_battlefield(bear), !kill);
        assert!(g.on_battlefield(spider));
        // Both blockers dealt their damage: 3 kills the Ogre.
        assert!(!g.on_battlefield(ogre));
        if !kill {
            assert_eq!(g.s.object(bear).damage + g.s.object(spider).damage, 3);
            assert!(g.s.object(bear).damage < 2);
        }
    }
}

#[test]
fn a_division_that_must_kill_asks_only_how_many() {
    // A 4/4 blocked by two interchangeable 2/2s must kill one; whether it kills both is the
    // only choice.
    let mut g = Game::new();
    let (a, d) = (g.attacker, g.defender());
    let giant = g.put(a, "Giant");
    let x = g.put(d, "Bear");
    let y = g.put(d, "Bear");
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::Attack(giant));
    g.pick(DecisionKind::Blocker, Opt::Block(x.min(y)));
    g.pick(DecisionKind::Blocker, Opt::Block(x.max(y)));
    let division = g.expect(DecisionKind::DamageDivision);
    assert_eq!(options(&division), vec![Opt::Done, Opt::Lethal(x.max(y))]);
    g.pick(DecisionKind::DamageDivision, Opt::Lethal(x.max(y)));
    g.finish_combat();
    assert!(!g.on_battlefield(x));
    assert!(!g.on_battlefield(y));
    assert!(!g.on_battlefield(giant));
}

#[test]
fn damage_below_lethal_goes_onto_blockers_in_the_order_they_entered() {
    // A 3/3 blocked by two 1/4s kills neither, and its damage goes onto the first to enter
    // the battlefield, here the one in the higher slot: slots follow the deal, which is random.
    let mut g = Game::new();
    let (a, d) = (g.attacker, g.defender());
    let ogre = g.put(a, "Ogre");
    let spiders = g.cards(d, "Spider");
    let (low, high) = (spiders[0], spiders[1]);
    g.put_slot(high);
    g.put_slot(low);
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::Attack(ogre));
    g.pick(DecisionKind::Blocker, Opt::Block(low));
    g.pick(DecisionKind::Blocker, Opt::Block(high));
    g.finish_combat();
    assert_eq!((g.s.object(high).damage, g.s.object(low).damage), (3, 0));
}

#[test]
fn creatures_dying_together_reach_the_graveyard_in_the_order_they_entered() {
    // Two Bears blocking an Ogre and a Bear die at once, and lie in the graveyard as they
    // entered the battlefield, the one in the higher slot first: slots follow the deal, which
    // is random, so a slot order would let the seed decide what a graveyard's order shows.
    let mut g = Game::new();
    let (a, d) = (g.attacker, g.defender());
    let ogre = g.put(a, "Ogre");
    let bear = g.put(a, "Bear");
    let bears = g.cards(d, "Bear");
    let (low, high) = (bears[0], bears[1]);
    g.put_slot(high);
    g.put_slot(low);
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::Attack(ogre));
    g.pick(DecisionKind::Attacker, Opt::Attack(bear));
    // The defender's Bears are alike: the lower slot answers for both, then the other.
    g.pick(DecisionKind::Blocker, Opt::Block(low));
    g.pick_object(DecisionKind::BlockTarget, ogre);
    g.pick(DecisionKind::Blocker, Opt::Block(high));
    g.pick_object(DecisionKind::BlockTarget, bear);
    g.finish_combat();
    assert_eq!(g.s.graveyard(d), &[high, low]);
}

#[test]
fn first_strike_adds_a_damage_step_only_when_present() {
    let mut g = Game::new();
    let (a, d) = (g.attacker, g.defender());
    let knight = g.put(a, "Knight");
    let bear = g.put(d, "Bear");
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::Attack(knight));
    g.pick(DecisionKind::Blocker, Opt::Block(bear));
    g.finish_combat();
    // The Knight kills the Bear before it can strike back.
    assert!(!g.on_battlefield(bear));
    assert!(g.on_battlefield(knight));
    assert_eq!(g.s.object(knight).damage, 0);
    let steps = g.steps();
    let at = |step| steps.iter().position(|&s| s == step).unwrap();
    assert!(at(TurnStep::DeclareBlockers) < at(TurnStep::FirstStrikeDamage));
    assert!(at(TurnStep::FirstStrikeDamage) < at(TurnStep::CombatDamage));

    // With no first striker in combat there is one damage step, and both Bears die.
    let mut g = Game::new();
    let (a, d) = (g.attacker, g.defender());
    let bear = g.put(a, "Bear");
    g.put(a, "Knight");
    let blocker = g.put(d, "Bear");
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::Attack(bear));
    g.pick(DecisionKind::Attacker, Opt::Done);
    g.pick(DecisionKind::Blocker, Opt::Block(blocker));
    g.finish_combat();
    assert!(!g.on_battlefield(bear));
    assert!(!g.on_battlefield(blocker));
    let steps = g.steps();
    assert!(steps.contains(&TurnStep::CombatDamage));
    assert!(!steps.contains(&TurnStep::FirstStrikeDamage));
}

#[test]
fn a_first_striker_deals_no_regular_damage_and_others_deal_theirs_second() {
    // A Knight and a Bear attack; a 1/4 Spider blocks the Knight. The Knight deals 2 first,
    // then the Spider and the Bear deal theirs; the Knight does not strike again.
    let mut g = Game::new();
    let (a, d) = (g.attacker, g.defender());
    let knight = g.put(a, "Knight");
    let bear = g.put(a, "Bear");
    let spider = g.put(d, "Spider");
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::Attack(knight));
    g.pick(DecisionKind::Attacker, Opt::Attack(bear));
    g.pick(DecisionKind::Blocker, Opt::Block(spider));
    g.pick_object(DecisionKind::BlockTarget, knight);
    g.finish_combat();
    assert_eq!(g.s.object(spider).damage, 2);
    assert_eq!(g.s.object(knight).damage, 1);
    assert_eq!(g.life(d), 18);
}

#[test]
fn an_attacker_stays_blocked_when_its_blocker_leaves() {
    let mut g = Game::new();
    let (a, d) = (g.attacker, g.defender());
    let ogre = g.put(a, "Ogre");
    let bear = g.put(d, "Bear");
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::Attack(ogre));
    g.pick(DecisionKind::Blocker, Opt::Block(bear));
    // Stop after blockers are declared and remove the blocker (CR 506.4, 509.1h).
    stop_at(&mut g, TurnStep::DeclareBlockers);
    zone::move_to(
        &mut g.s,
        &mut crate::event::Sink::off(),
        bear,
        Zone::Graveyard,
    )
    .unwrap();
    g.finish_combat();
    assert_eq!(g.life(d), 20);
    assert_eq!(g.s.object(ogre).damage, 0);
}

#[test]
fn an_attacker_whose_planeswalker_left_deals_no_damage() {
    let mut g = Game::new();
    let (a, d) = (g.attacker, g.defender());
    let ogre = g.put(a, "Ogre");
    let walker = g.put(d, "Walker");
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::Attack(ogre));
    g.pick(DecisionKind::AttackTarget, Opt::Defender(Some(walker)));
    stop_at(&mut g, TurnStep::DeclareAttackers);
    zone::move_to(
        &mut g.s,
        &mut crate::event::Sink::off(),
        walker,
        Zone::Graveyard,
    )
    .unwrap();
    g.finish_combat();
    assert_eq!(g.life(d), 20);
}

#[test]
fn combat_goes_on_when_every_declared_attacker_is_removed() {
    // CR 508.8 skips the declare blockers and combat damage steps only if no creature was
    // declared as an attacker, not when the attackers are gone by the end of the step.
    let mut g = Game::new();
    let bear = g.put(g.attacker, "Bear");
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::Attack(bear));
    stop_at(&mut g, TurnStep::DeclareAttackers);
    zone::move_to(
        &mut g.s,
        &mut crate::event::Sink::off(),
        bear,
        Zone::Graveyard,
    )
    .unwrap();
    g.finish_combat();
    let steps = g.steps();
    assert!(steps.contains(&TurnStep::DeclareBlockers), "{steps:?}");
    assert!(steps.contains(&TurnStep::CombatDamage), "{steps:?}");
}

#[test]
fn attackers_blocked_by_different_creatures_are_not_interchangeable() {
    // Two Ogres, one blocked by a Bear and one by a Sentry. A Spider joining the Bear kills
    // that Ogre (2 + 1 damage); joining the Sentry kills neither (1 + 1).
    let mut g = Game::new();
    let (a, d) = (g.attacker, g.defender());
    let x = g.put(a, "Ogre");
    let y = g.put(a, "Ogre");
    let (first, second) = (x.min(y), x.max(y));
    let bear = g.put(d, "Bear");
    let sentry = g.put(d, "Sentry");
    let spider = g.put(d, "Spider");
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::Attack(first));
    g.pick(DecisionKind::Attacker, Opt::Attack(second));
    // The Bear's two attackers are still interchangeable: it blocks the first silently.
    g.pick(DecisionKind::Blocker, Opt::Block(bear));
    g.pick(DecisionKind::Blocker, Opt::Block(sentry));
    g.pick_object(DecisionKind::BlockTarget, second);
    g.pick(DecisionKind::Blocker, Opt::Block(spider));
    let target = g.expect(DecisionKind::BlockTarget);
    assert_eq!(
        target.candidates,
        Candidates::Objects([first, second].into_iter().collect())
    );
    g.pick_object(DecisionKind::BlockTarget, first);
    g.pick(DecisionKind::DamageDivision, Opt::Done);
    g.finish_combat();
    assert!(!g.on_battlefield(first));
    assert!(g.on_battlefield(second));
}

/// Stops at the active player's priority in `step` of this turn.
fn stop_at(g: &mut Game, step: TurnStep) {
    let turn = g.s.turn().number;
    g.s.set_profile(Profile {
        breakpoint: Some(Breakpoint { turn, step }),
        ..Profile::EXACT
    });
    let d = g.decide();
    assert_eq!((d.kind, g.s.turn().step), (DecisionKind::Priority, step));
}

#[test]
fn attack_and_combat_damage_triggers_fire() {
    let mut g = Game::new();
    let (a, d) = (g.attacker, g.defender());
    let raider = g.put(a, "Raider");
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::Attack(raider));
    g.finish_combat();
    assert_eq!(g.life(a), 31);
    assert_eq!(g.life(d), 18);
}

#[test]
fn a_creature_put_onto_the_battlefield_attacking_was_never_declared() {
    let mut g = Game::new();
    let (a, d) = (g.attacker, g.defender());
    let captain = g.put(a, "Captain");
    g.pass_to_combat();
    let attackers = g.expect(DecisionKind::Attacker);
    assert_eq!(options(&attackers), vec![Opt::Done, Opt::Attack(captain)]);
    g.pick(DecisionKind::Attacker, Opt::Attack(captain));
    stop_at(&mut g, TurnStep::DeclareAttackers);
    while !g.s.stack().is_empty() {
        g.pick(DecisionKind::Priority, Opt::Pass);
    }
    // The Recruit entered tapped and attacking, but its attack trigger did not trigger.
    let recruit =
        g.s.battlefield()
            .iter()
            .copied()
            .find(|&slot| g.t.def(g.s.object(slot).card).name == "Recruit")
            .unwrap();
    let flags = g.s.object(recruit).flags;
    assert!(flags.has(ObjectFlags::ATTACKING) && flags.has(ObjectFlags::TAPPED));
    g.finish_combat();
    assert_eq!(g.life(a), 20);
    assert_eq!(g.life(d), 18);
}

#[test]
fn smart_stops_come_after_attackers_and_after_blockers_are_declared() {
    let mut g = Game::new();
    let a = g.attacker;
    let bear = g.put(a, "Bear");
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::Attack(bear));
    let smart = Profile::SMART;
    for step in [TurnStep::DeclareAttackers, TurnStep::DeclareBlockers] {
        g.s.turn.step = step;
        for player in [a, a.opponent()] {
            assert!(stops_for(smart, &g.s, player), "{step:?}");
        }
    }
    g.s.turn.step = TurnStep::BeginningOfCombat;
    assert!(!stops_for(smart, &g.s, a));
}

fn is_creature(g: &Game, slot: Slot) -> bool {
    g.s.derived(slot).types.contains(Types::CREATURE)
}

#[test]
fn crew_taps_creatures_one_at_a_time_and_makes_an_artifact_creature_until_end_of_turn() {
    let mut g = Game::new();
    let (a, d) = (g.attacker, g.defender());
    let wagon = g.put(a, "Wagon");
    let bear = g.put(a, "Bear");
    let sentry = g.put(a, "Sentry");
    // A summoning-sick creature can crew: tapping it is a cost of the Vehicle's ability.
    g.s.board().set_sick(sentry, true);
    g.expect(DecisionKind::Priority);
    assert!(!is_creature(&g, wagon));
    g.pick(
        DecisionKind::Priority,
        Opt::Activate {
            slot: wagon,
            ability: AbilityIx::Own(0),
        },
    );
    // The Wagon can't crew itself; both creatures are needed for power 3.
    let first = g.expect(DecisionKind::Crew);
    assert_eq!(
        first.candidates,
        Candidates::Objects([bear, sentry].into_iter().collect())
    );
    assert_eq!(first.ask, Ask::Crew { crew: 3, left: 3 });
    g.pick_object(DecisionKind::Crew, bear);
    // One creature is left, so the second pick is applied silently. Crew uses the stack.
    g.expect(DecisionKind::Priority);
    assert!(g.s.object(sentry).flags.has(ObjectFlags::TAPPED));
    assert_eq!(g.s.stack().len(), 1);
    assert!(!is_creature(&g, wagon));
    g.pick(DecisionKind::Priority, Opt::Pass);
    g.expect(DecisionKind::Priority);
    assert!(g.s.stack().is_empty());
    assert!(is_creature(&g, wagon));
    assert!(g.s.derived(wagon).types.contains(Types::ARTIFACT));
    assert!(g.s.derived(wagon).subtypes.contains(Subtype::Vehicle));
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::Attack(wagon));
    g.finish_combat();
    assert_eq!(g.life(d), 17);
    // Until end of turn: at the next turn's first stop it is an artifact again.
    let next_turn = g.s.turn().number + 1;
    stop_at_turn(&mut g, next_turn, TurnStep::PrecombatMain);
    assert!(!is_creature(&g, wagon));
}

#[test]
fn crew_is_offered_only_with_enough_power() {
    let mut g = Game::new();
    let a = g.attacker;
    let wagon = g.put(a, "Wagon");
    let bear = g.put(a, "Bear");
    let priority = g.expect(DecisionKind::Priority);
    let crew = Opt::Activate {
        slot: wagon,
        ability: AbilityIx::Own(0),
    };
    assert!(!options(&priority).contains(&crew));
    g.s.board().set_tapped(bear, true);
    g.put(a, "Ogre");
    let priority = g.expect(DecisionKind::Priority);
    assert!(options(&priority).contains(&crew));
}

/// Passes priority until the active player's first priority in `step` of `turn`.
fn stop_at_turn(g: &mut Game, turn: u16, step: TurnStep) {
    g.s.set_profile(Profile {
        breakpoint: Some(Breakpoint { turn, step }),
        ..Profile::EXACT
    });
    loop {
        let d = g.decide();
        if (g.s.turn().number, g.s.turn().step) == (turn, step) {
            assert_eq!(d.kind, DecisionKind::Priority);
            return;
        }
        g.pick(DecisionKind::Priority, Opt::Pass);
    }
}

/// Plays whole games from a battlefield of random creatures and planeswalkers with uniformly
/// random choices, checking the invariants after every decision: every declaration and
/// division the engine offers must lead on.
#[test]
fn random_combat_games_keep_the_invariants() {
    let mut kinds = std::collections::BTreeSet::new();
    for seed in 0..12u64 {
        let mut g = Game::seeded(seed);
        g.s.set_profile(Profile::EXACT);
        for player in Seat::BOTH {
            for _ in 0..8 {
                let slot = *g.s.library(player).last().unwrap();
                let name = g.t.def(g.s.object(slot).card).name;
                g.put(player, name);
            }
        }
        let mut rng = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        let outcome = loop {
            match next(&mut g.s, &g.t, &mut g.x, &mut Replay) {
                Step::Over(outcome) => break outcome,
                Step::Decide(d) => {
                    kinds.insert(format!("{:?}", d.kind));
                    rng ^= rng << 13;
                    rng ^= rng >> 7;
                    rng ^= rng << 17;
                    let c = Choice((rng % d.len() as u64) as u16);
                    apply(&mut g.s, &g.t, &mut g.x, &mut Replay, c);
                }
            }
        };
        assert!(
            !matches!(outcome, crate::Outcome::Stalled(_)),
            "seed {seed}: {outcome:?}"
        );
    }
    for kind in [
        "Attacker",
        "AttackTarget",
        "Blocker",
        "BlockTarget",
        "DamageDivision",
        "Crew",
    ] {
        assert!(kinds.contains(kind), "no {kind} decision in any game");
    }
}

/// Where passing takes a player, as `next_stop` says for the pass button: the smart stops
/// (§15), with the combat the active player can declare attackers in.
#[test]
fn passing_goes_to_the_next_smart_stop() {
    use crate::Observation;
    use crate::profile::{NextStop, StepStops, Stops, next_stop};
    let next =
        |g: &Game, p: Seat, stops| next_stop(Observation::new(&g.s, p), stops, StepStops::SMART);
    let mut g = Game::new();
    let (a, d) = (g.attacker, g.defender());
    // Nothing can attack: main 1 passes to main 2, and the defender's next stop is the
    // opponent's end step. Under exact stops it is the next step.
    assert_eq!(
        next(&g, a, Stops::Smart),
        NextStop::Step(TurnStep::PostcombatMain)
    );
    assert_eq!(next(&g, d, Stops::Smart), NextStop::Step(TurnStep::End));
    assert_eq!(
        next(&g, a, Stops::Exact),
        NextStop::Step(TurnStep::BeginningOfCombat)
    );
    // A creature ready to attack: to combat.
    let bear = g.put(a, "Bear");
    let ogre = g.put(d, "Ogre");
    // The same open decision, with the layers brought up to date.
    g.expect(DecisionKind::Priority);
    assert_eq!(
        next(&g, a, Stops::Smart),
        NextStop::Step(TurnStep::BeginningOfCombat)
    );
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::Attack(bear));
    // Blockers being declared, passing deals combat damage.
    g.pick(DecisionKind::Blocker, Opt::Block(ogre));
    assert_eq!(g.s.turn().step, TurnStep::DeclareBlockers);
    assert_eq!(next(&g, a, Stops::Smart), NextStop::CombatDamage);
    assert_eq!(next(&g, d, Stops::Exact), NextStop::CombatDamage);
    // After combat, the active player's pass ends the turn; the defender's goes to the end
    // step.
    g.finish_combat();
    assert_eq!(next(&g, a, Stops::Smart), NextStop::NextTurn);
    assert_eq!(next(&g, d, Stops::Smart), NextStop::Step(TurnStep::End));
}
