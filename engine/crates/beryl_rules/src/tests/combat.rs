//! Combat (CR 506 to 511) with test-only creatures: declarations one pick at a time,
//! evasion, damage and its division, first strike, planeswalkers and combat triggers. The
//! keywords that change combat damage, destruction and blocking are in [`keywords`].

use beryl_card::effect::build::{
    create_attacking_tokens, each_opponent, for_each_player, gain_life, that_player, you,
};

use super::begin;
use crate::card::{
    Ability, CardDef, Face, Keywords, ObjectAbility, Objects, Part, Players, Prompt, Purpose,
    RuleChange, Subtype, Subtypes, Trigger, Types,
};
use crate::ids::{Seat, Slot};
use crate::profile::{Breakpoint, stops_for};
use crate::replacement::event::Event;
use crate::state::{AttackTarget, CounterKind, Zone, ZoneKey};
use crate::zone::Place;
use crate::{
    Candidates, CardTable, Choice, Decision, DecisionKind, Decklist, GameTable, Opt, Profile,
    Replay, Scratch, State, Step, TurnStep, apply, next,
};
use beryl_card::{Governed, When};
use beryl_kernel::format::Format;
use beryl_kernel::two_player::TwoPlayerSeat as _;

/// A vanilla creature card with keywords.
macro_rules! creature {
    ($name:ident, $text:literal, $power:literal / $toughness:literal, $keywords:expr) => {
        static $name: CardDef = CardDef {
            name: $text,
            shape: beryl_card::definition::Shape::Single,
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
creature!(WURM, "Wurm", 5 / 5, Keywords::TRAMPLE);
creature!(CLERIC, "Cleric", 2 / 2, Keywords::LIFELINK);
creature!(GOLEM, "Golem", 2 / 2, Keywords::INDESTRUCTIBLE);
creature!(WRAITH, "Wraith", 3 / 3, Keywords::SWAMPWALK);

/// A land with the subtype Swamp, which a swampwalker's defender may control.
static SWAMP: CardDef = CardDef {
    name: "Swamp",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::LAND,
        subtypes: crate::card::SubtypeWords::new(Subtypes::of(&[Subtype::Swamp])),
        ..Face::EMPTY
    }],
    tokens: &[],
};

static WALKER: CardDef = CardDef {
    name: "Walker",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::PLANESWALKER,
        loyalty: Some(3),
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "Whenever this attacks, you gain 1 life. Whenever this deals combat damage to a player,
/// you gain 10 life."
static RAIDER: CardDef = CardDef {
    name: "Raider",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(2),
        abilities: &[
            Ability::triggered(&Trigger::ATTACKS).effect(&gain_life(you(), 1)),
            Ability::triggered(&Trigger::DEALS_COMBAT_DAMAGE_TO_A_PLAYER)
                .effect(&gain_life(you(), 10)),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// The token Captain creates: a Raider of its own, whose attack trigger must not trigger.
static RECRUIT: CardDef = CardDef {
    name: "Recruit",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        abilities: &[Ability::triggered(&Trigger::ATTACKS).effect(&gain_life(you(), 1))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "Whenever you attack, create a 1/1 Recruit token that's tapped and attacking."
static CAPTAIN: CardDef = CardDef {
    name: "Captain",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        abilities: &[
            Ability::triggered(&Trigger::attack(Players::You)).effect(&for_each_player(
                each_opponent(),
                &create_attacking_tokens(you(), &RECRUIT, 1, that_player()),
            )),
        ],
        ..Face::EMPTY
    }],
    tokens: &[&RECRUIT],
};

/// A 3/3 Vehicle artifact with crew 3.
static WAGON: CardDef = CardDef {
    name: "Wagon",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        subtypes: crate::card::SubtypeWords::new(Subtypes::of(&[Subtype::Vehicle])),
        power: Some(3),
        toughness: Some(3),
        abilities: &[Ability::crew(3)],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "This creature can't block."
static CANT_BLOCK: RuleChange = RuleChange {
    players: Players::You,
    what: Governed::Block(Objects::This),
    when: When::Never,
};

/// A 2/1 that can't block.
static GHOUL: CardDef = CardDef {
    name: "Ghoul",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(1),
        abilities: &[Ability::rule_change(&CANT_BLOCK)],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[
    &BEAR, &GIANT, &OGRE, &BIRD, &SPIDER, &KNIGHT, &SENTRY, &WALKER, &RAIDER, &CAPTAIN, &WAGON,
    &WURM, &CLERIC, &GOLEM, &WRAITH, &SWAMP, &GHOUL,
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
    (2, "Wurm"),
    (2, "Cleric"),
    (2, "Golem"),
    (2, "Wraith"),
    (2, "Swamp"),
    (2, "Ghoul"),
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
        let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [DECK, DECK]).unwrap();
        Self::with_table(t, seed)
    }

    fn with_table(t: GameTable, seed: u64) -> Self {
        let profile = Profile {
            breakpoint: Some(Breakpoint {
                turn: 1,
                step: TurnStep::PrecombatMain,
            }),
            ..Profile::EXACT
        };
        let mut s = State::new(&t, seed, profile).unwrap();
        let d = begin(&t, &mut s);
        assert_eq!(d.kind, DecisionKind::Priority);
        let attacker = s.turn().active();
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
        // Copies of a card are alike, so any order of them does.
        let cards = self.s.zone(ZoneKey::Of(player, Zone::Library))
            | self.s.zone(ZoneKey::Of(player, Zone::Hand));
        (crate::tests::fixture_slots(cards))
            .filter(|&slot| self.s.definition_name(&self.t, slot) == name)
            .collect()
    }

    fn put_slot(&mut self, slot: Slot) -> Slot {
        self.s
            .move_object(
                &mut crate::replacement::event::Sink::off(),
                slot,
                Zone::Battlefield,
                Place::Top,
            )
            .unwrap();
        self.s.board().set_sick(slot, false);
        if let Some(loyalty) = self.s.copiable(slot).face(&self.t).loyalty {
            self.s
                .board()
                .set_counters(slot, CounterKind::Loyalty, loyalty)
                .unwrap();
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
        let turn = self.s.turn().number();
        self.s.set_profile(Profile {
            breakpoint: Some(Breakpoint {
                turn,
                step: TurnStep::PostcombatMain,
            }),
            ..Profile::EXACT
        });
        let d = self.decide();
        assert_eq!(
            (d.kind, self.s.turn().step()),
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
        self.s.player(player).life()
    }

    fn on_battlefield(&self, slot: Slot) -> bool {
        self.s.object(slot).zone() == Zone::Battlefield
    }
}

fn options(d: &Decision) -> Vec<Opt> {
    (0..d.len() as u16)
        .map(|i| d.candidates.option(Choice(i)).unwrap())
        .collect()
}

#[test]
fn unblocked_attackers_each_record_a_combat_damage_event() {
    let mut g = Game::new();
    let bear = g.put(g.attacker, "Bear");
    let other = g.put(g.attacker, "Bear");
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::attack(bear.min(other)));
    g.pick(DecisionKind::Attacker, Opt::attack(bear.max(other)));
    stop_at(&mut g, TurnStep::DeclareAttackers);
    g.s.board().set_step(TurnStep::CombatDamage);
    crate::combat::begin_damage(&mut g.s, &g.t, &mut g.x.events).unwrap();
    let damage = g.x.events.events().iter().filter(|event| {
        matches!(
            event,
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
fn no_attack_declaration_skips_the_block_and_damage_steps() {
    let mut g = Game::new();
    let sick = g.put(g.attacker, "Bear");
    g.s.board().set_sick(sick, true);
    let tapped = g.put(g.attacker, "Ogre");
    g.s.board().set_tapped(tapped, true);
    g.put(g.defender(), "Bear");
    g.expect(DecisionKind::Priority);
    assert!(!crate::combat::can_declare_attackers(&g.s, &g.t));
    g.s.board().set_step(TurnStep::DeclareAttackers);
    crate::turn::end_step_before_actions(&mut g.s, &g.t, &mut g.x.events).unwrap();
    assert_eq!(g.s.turn().step(), TurnStep::EndOfCombat);
}

#[test]
fn an_attack_target_decision_identifies_the_attacker_being_aimed() {
    let mut g = Game::new();
    let ogre = g.put(g.attacker, "Ogre");
    let walker = g.put(g.defender(), "Walker");
    // Refresh characteristics before asking which planeswalker can be attacked.
    g.expect(DecisionKind::Priority);
    let target = crate::combat::attack_decision(&g.s, &g.t, Some(ogre)).unwrap();
    assert_eq!(target.kind, DecisionKind::AttackTarget);
    assert_eq!(
        (target.prompt, target.about),
        (
            Prompt::new(Purpose::Attack).part(Part::Aim),
            crate::Bitboard::single(ogre)
        )
    );
    assert_eq!(
        options(&target),
        vec![Opt::Defender(None), Opt::Defender(Some(walker))]
    );
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
    g.pick(DecisionKind::Attacker, Opt::attack(bird));
    g.pick(DecisionKind::Attacker, Opt::attack(bear));
    let blockers = g.expect(DecisionKind::Blocker);
    assert_eq!(blockers.player, d);
    // Blockers are offered in slot order, which the deal decides.
    assert_eq!(
        options(&blockers),
        vec![
            Opt::Done,
            Opt::Block(ground.min(spider)),
            Opt::Block(ground.max(spider))
        ]
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
        g.pick(DecisionKind::Attacker, Opt::attack(ogre));
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
            assert_eq!(g.s.object(bear).damage() + g.s.object(spider).damage(), 3);
            assert!(g.s.object(bear).damage() < 2);
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
    g.pick(DecisionKind::Attacker, Opt::attack(giant));
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
    g.pick(DecisionKind::Attacker, Opt::attack(ogre));
    g.pick(DecisionKind::Blocker, Opt::Block(low));
    g.pick(DecisionKind::Blocker, Opt::Block(high));
    g.finish_combat();
    assert_eq!(
        (g.s.object(high).damage(), g.s.object(low).damage()),
        (3, 0)
    );
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
    g.pick(DecisionKind::Attacker, Opt::attack(ogre));
    g.pick(DecisionKind::Attacker, Opt::attack(bear));
    // The defender's Bears are alike: the lower slot answers for both, then the other.
    g.pick(DecisionKind::Blocker, Opt::Block(low));
    g.pick_object(DecisionKind::BlockTarget, ogre);
    g.pick(DecisionKind::Blocker, Opt::Block(high));
    g.pick_object(DecisionKind::BlockTarget, bear);
    g.finish_combat();
    assert_eq!(g.s.graveyard(d), &[high, low]);
}

#[test]
fn a_first_striker_held_back_does_not_add_a_combat_damage_step() {
    let mut g = Game::new();
    let bear = g.put(g.attacker, "Bear");
    g.put(g.attacker, "Knight");
    let blocker = g.put(g.defender(), "Bear");
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::attack(bear));
    g.pick(DecisionKind::Attacker, Opt::Done);
    g.pick(DecisionKind::Blocker, Opt::Block(blocker));
    assert!(!crate::combat::first_strike_step(&g.s));
    g.finish_combat();
    assert!(!g.on_battlefield(bear));
    assert!(!g.on_battlefield(blocker));
    let steps = g.steps();
    assert!(steps.contains(&TurnStep::CombatDamage));
    assert!(!steps.contains(&TurnStep::FirstStrikeDamage));
}

#[test]
fn an_attacker_whose_planeswalker_left_deals_no_damage() {
    let mut g = Game::new();
    let (a, d) = (g.attacker, g.defender());
    let ogre = g.put(a, "Ogre");
    let walker = g.put(d, "Walker");
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::attack(ogre));
    g.pick(DecisionKind::AttackTarget, Opt::Defender(Some(walker)));
    stop_at(&mut g, TurnStep::DeclareAttackers);
    let target = |g: &Game| g.s.object(ogre).attack_target();
    assert_eq!(target(&g), Some(AttackTarget::Planeswalker(walker)));
    g.s.move_object(
        &mut crate::replacement::event::Sink::off(),
        walker,
        Zone::Graveyard,
        Place::Top,
    )
    .unwrap();
    // CR 506.4c: it is still an attacking creature, attacking nothing, and no player
    // becomes its defending player.
    assert_eq!(target(&g), Some(AttackTarget::Removed));
    assert_eq!(g.s.defending_player(ogre), None);
    g.finish_combat();
    assert_eq!(g.life(d), 20);
    assert_eq!(g.life(a), 20);
}

#[test]
fn combat_goes_on_when_every_declared_attacker_is_removed() {
    // CR 508.8 skips the declare blockers and combat damage steps only if no creature was
    // declared as an attacker, not when the attackers are gone by the end of the step.
    let mut g = Game::new();
    let bear = g.put(g.attacker, "Bear");
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::attack(bear));
    stop_at(&mut g, TurnStep::DeclareAttackers);
    g.s.move_object(
        &mut crate::replacement::event::Sink::off(),
        bear,
        Zone::Graveyard,
        Place::Top,
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
    g.pick(DecisionKind::Attacker, Opt::attack(first));
    g.pick(DecisionKind::Attacker, Opt::attack(second));
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
    let turn = g.s.turn().number();
    g.s.set_profile(Profile {
        breakpoint: Some(Breakpoint { turn, step }),
        ..Profile::EXACT
    });
    let d = g.decide();
    assert_eq!((d.kind, g.s.turn().step()), (DecisionKind::Priority, step));
}

#[test]
fn a_creature_put_onto_the_battlefield_attacking_was_never_declared() {
    let mut g = Game::new();
    let (a, d) = (g.attacker, g.defender());
    let captain = g.put(a, "Captain");
    g.pass_to_combat();
    let attackers = g.expect(DecisionKind::Attacker);
    assert_eq!(options(&attackers), vec![Opt::Done, Opt::attack(captain)]);
    g.pick(DecisionKind::Attacker, Opt::attack(captain));
    stop_at(&mut g, TurnStep::DeclareAttackers);
    while !g.s.stack().is_empty() {
        g.pick(DecisionKind::Priority, Opt::Pass);
    }
    // The Recruit entered tapped and attacking, but its attack trigger did not trigger.
    let recruit =
        g.s.battlefield()
            .iter()
            .copied()
            .find(|&slot| g.s.definition_name(&g.t, slot) == "Recruit")
            .unwrap();
    let o = g.s.object(recruit);
    assert!(o.is_attacking() && o.is_tapped());
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
    g.pick(DecisionKind::Attacker, Opt::attack(bear));
    let smart = Profile::SMART;
    for step in [TurnStep::DeclareAttackers, TurnStep::DeclareBlockers] {
        g.s.board().set_step(step);
        for player in [a, a.opponent()] {
            assert!(stops_for(smart, &g.s, player), "{step:?}");
        }
    }
    g.s.board().set_step(TurnStep::BeginningOfCombat);
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
            ability: ObjectAbility::own(0),
        },
    );
    // The Wagon can't crew itself; both creatures are needed for power 3.
    let first = g.expect(DecisionKind::ChooseObject);
    assert_eq!(
        first.candidates,
        Candidates::Objects([bear, sentry].into_iter().collect())
    );
    assert_eq!(first.prompt, Prompt::new(Purpose::Crew).count(3).most(3));
    g.pick_object(DecisionKind::ChooseObject, bear);
    // One creature is left, so the second pick is applied silently. Crew uses the stack.
    g.expect(DecisionKind::Priority);
    assert!(g.s.object(sentry).is_tapped());
    assert_eq!(g.s.stack().len(), 1);
    assert!(!is_creature(&g, wagon));
    g.pick(DecisionKind::Priority, Opt::Pass);
    g.expect(DecisionKind::Priority);
    assert!(g.s.stack().is_empty());
    assert!(is_creature(&g, wagon));
    assert!(g.s.derived(wagon).types.contains(Types::ARTIFACT));
    assert!(
        g.s.derived(wagon)
            .subtypes
            .contains(g.t.subtypes(), Subtype::Vehicle)
    );
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::attack(wagon));
    g.finish_combat();
    assert_eq!(g.life(d), 17);
    // Until end of turn: at the next turn's first stop it is an artifact again.
    let next_turn = g.s.turn().number() + 1;
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
        ability: ObjectAbility::own(0),
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
        if (g.s.turn().number(), g.s.turn().step()) == (turn, step) {
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
    let mut crewed = false;
    for seed in 0..12u64 {
        let mut g = Game::seeded(seed);
        g.s.set_profile(Profile::EXACT);
        for player in Seat::BOTH {
            for _ in 0..8 {
                let slot = *g.s.library(player).last().unwrap();
                let name = g.s.definition_name(&g.t, slot);
                g.put(player, name);
            }
        }
        let mut rng = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        let outcome = loop {
            match next(&mut g.s, &g.t, &mut g.x, &mut Replay) {
                Step::Over(outcome) => break outcome,
                Step::Decide(d) => {
                    kinds.insert(format!("{:?}", d.kind));
                    crewed |= d.prompt.purpose == Purpose::Crew;
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
    ] {
        assert!(kinds.contains(kind), "no {kind} decision in any game");
    }
    assert!(crewed, "no crew decision in any game");
}

/// Where passing takes a player, as `next_stop` says for the pass button: the smart stops
/// (§15), with the combat the active player can declare attackers in.
#[test]
fn passing_goes_to_the_next_smart_stop() {
    use crate::Observation;
    use crate::profile::{NextStop, StepStops, Stops, next_stop};
    let next = |g: &Game, p: Seat, stops| {
        next_stop(&g.t, Observation::new(&g.s, p), stops, StepStops::SMART)
    };
    let mut g = Game::new();
    let (a, d) = (g.attacker, g.defender());
    // Nothing can attack: main 1 passes to main 2, and the defender's next stop is the
    // opponent's end step. Under exact stops it is the next step.
    assert!(matches!(
        next(&g, a, Stops::Smart),
        NextStop::Step(TurnStep::PostcombatMain, _)
    ));
    assert!(matches!(
        next(&g, d, Stops::Smart),
        NextStop::Step(TurnStep::End, _)
    ));
    assert!(matches!(
        next(&g, a, Stops::Exact),
        NextStop::Step(TurnStep::BeginningOfCombat, _)
    ));
    // A creature ready to attack: to combat.
    let bear = g.put(a, "Bear");
    let ogre = g.put(d, "Ogre");
    // The same open decision, with the layers brought up to date.
    g.expect(DecisionKind::Priority);
    assert!(matches!(
        next(&g, a, Stops::Smart),
        NextStop::Step(TurnStep::BeginningOfCombat, _)
    ));
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::attack(bear));
    // Blockers being declared, passing deals combat damage.
    g.pick(DecisionKind::Blocker, Opt::Block(ogre));
    assert_eq!(g.s.turn().step(), TurnStep::DeclareBlockers);
    assert_eq!(next(&g, a, Stops::Smart), NextStop::CombatDamage);
    assert_eq!(next(&g, d, Stops::Exact), NextStop::CombatDamage);
    // After combat, the active player's pass ends the turn; the defender's goes to the end
    // step.
    g.finish_combat();
    assert_eq!(next(&g, a, Stops::Smart), NextStop::NextTurn);
    assert!(matches!(
        next(&g, d, Stops::Smart),
        NextStop::Step(TurnStep::End, _)
    ));
}

mod assignments;
mod group_picks;
mod keywords;
mod requirements;
