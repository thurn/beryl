//! Beza, the Bounding Spring. {2}{W}{W} Legendary Creature — Elemental Elk, 4/5. "When Beza
//! enters, create a Treasure token if an opponent controls more lands than you. You gain 4
//! life if an opponent has more life than you. Create two 1/1 blue Fish creature tokens if an
//! opponent controls more creatures than you. Draw a card if an opponent has more cards in
//! hand than you."

use beryl_engine::card::{
    Ability, CardDef, Colors, Face, Flow, ManaColor, ManaCost, Subtype, Subtypes, Supertypes,
    Trigger, Types,
};
use beryl_engine::effect::EffectCtx;
use beryl_engine::event::TokenEntry;
use beryl_engine::{DecisionKind, Seat, State, Zone, ZoneKey};
use beryl_engine::{Layout, bodies};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

use crate::names;
use crate::tokens::{TREASURE, TREASURE_TOKEN};

/// The Fish token's name: its subtype plus the word "Token"
/// ([CR 111.4](https://yawgatog.com/resources/magic-rules/#R1114)).
pub const FISH_TOKEN: &str = "Fish Token";

/// The Fish token: a 1/1 blue Fish creature token.
pub static FISH: CardDef = CardDef {
    name: FISH_TOKEN,
    faces: &[Face {
        types: Types::CREATURE,
        subtypes: Subtypes::of(&[Subtype::Fish]),
        colors: Colors::BLUE,
        power: Some(1),
        toughness: Some(1),
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// How many permanents of `types` `player` controls.
fn controls<L: Layout>(s: &State<L>, player: Seat, types: Types) -> usize {
    s.battlefield()
        .iter()
        .filter(|&&slot| s.controller(slot) == player && s.derived(slot).types.intersects(types))
        .count()
}

/// The four sentences in order, each comparison made as its sentence is applied, on
/// resolution ([CR 608.2h](https://yawgatog.com/resources/magic-rules/#R6082h)): "If an effect
/// requires information from the game (such as the number of creatures on the battlefield),
/// the answer is determined only once, when the effect is applied." The "if"s are not right
/// after the trigger condition, so none is an intervening "if" (CR 603.4): the ability always
/// triggers. A tie is not "more".
fn compare<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    let you = ctx.controller();
    let them = you.opponent();
    if controls(ctx.state(), them, Types::LAND) > controls(ctx.state(), you, Types::LAND) {
        ctx.create_tokens(&TREASURE, 1, TokenEntry::default());
    }
    if ctx.state().player(them).life() > ctx.state().player(you).life() {
        ctx.gain_life(you, 4);
    }
    let creatures = |s: &State<L>, p| controls(s, p, Types::CREATURE);
    if creatures(ctx.state(), them) > creatures(ctx.state(), you) {
        ctx.create_tokens(&FISH, 2, TokenEntry::default());
    }
    let hand = |s: &State<L>, p| s.zone(ZoneKey::Of(p, Zone::Hand)).count();
    if hand(ctx.state(), them) > hand(ctx.state(), you) {
        ctx.draw(you, 1);
    }
    Ok(())
}

pub static CARD: CardDef = CardDef {
    name: names::BEZA_THE_BOUNDING_SPRING,
    faces: &[Face {
        cost: ManaCost::new(2, [2, 0, 0, 0, 0, 0]),
        supertypes: Supertypes::LEGENDARY,
        types: Types::CREATURE,
        subtypes: Subtypes::of(&[Subtype::Elemental, Subtype::Elk]),
        colors: Colors::WHITE,
        power: Some(4),
        toughness: Some(5),
        abilities: &[Ability::triggered(Trigger::Enters, bodies!(compare))],
        ..Face::EMPTY
    }],
    tokens: &[&TREASURE, &FISH],
};

const BEZA: &str = names::BEZA_THE_BOUNDING_SPRING;

const PLAINS: &[Permanent] = &[Permanent::new(names::PLAINS); 4];
const PLAIN: Permanent = Permanent::new(names::PLAINS);
const ISLAND: Permanent = Permanent::new(names::ISLAND);
const SKYDIVER: Permanent = Permanent::new(names::THIEVING_SKYDIVER);
const SPELLBINDER: Permanent = Permanent::new(names::ELITE_SPELLBINDER);
/// Five lands and two creatures.
const BEHIND: &[Permanent] = &[PLAIN, PLAIN, PLAIN, PLAIN, PLAIN, SKYDIVER, SPELLBINDER];
/// Four lands and one creature.
const TIED: &[Permanent] = &[PLAIN, PLAIN, PLAIN, PLAIN, SKYDIVER];
/// Beza's four lands and Sink into Stupor's three.
const RESPONDING: &[Permanent] = &[PLAIN, PLAIN, PLAIN, PLAIN, ISLAND, ISLAND, ISLAND];
/// Two lands and two creatures.
const AHEAD_IN_CREATURES: &[Permanent] = &[PLAIN, PLAIN, SKYDIVER, SPELLBINDER];

const fn controls_count(player: Seat, card: &'static str, count: u8) -> Action {
    Action::assert(Assertion::Controls {
        player,
        card,
        count,
    })
}

const fn hand(player: Seat, count: u8) -> Action {
    Action::assert(Assertion::Count {
        player,
        zone: Zone::Hand,
        card: None,
        count,
    })
}

const fn life(player: Seat, life: i16) -> Action {
    Action::assert(Assertion::Life { player, life })
}

pub const SCENARIOS: &[Scenario] = &[
    // P1 has more lands (5 to 4), more life (20 to 15), more creatures (two to Beza alone)
    // and more cards in hand (two to none): all four.
    Scenario::new("Beza with every comparison true")
        .life(P0, 15)
        .hand(P0, &[BEZA])
        .battlefield(P0, PLAINS)
        .library(P0, &[names::ISLAND])
        .battlefield(P1, BEHIND)
        .hand(P1, &[names::PLAINS, names::PLAINS])
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::cast(P0, BEZA),
            Action::Resolve,
            Action::Resolve,
            controls_count(P0, TREASURE_TOKEN, 1),
            life(P0, 19),
            controls_count(P0, FISH_TOKEN, 2),
            Action::assert(Assertion::Pt {
                player: P0,
                card: FISH_TOKEN,
                token: true,
                power: 1,
                toughness: 1,
            }),
            hand(P0, 1),
        ]),
    // Four lands each, 20 life each, one creature each counting Beza, no cards in hand: a
    // tie is not more, so nothing.
    Scenario::new("Beza with every comparison tied")
        .hand(P0, &[BEZA])
        .battlefield(P0, PLAINS)
        .library(P0, &[names::ISLAND])
        .battlefield(P1, TIED)
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::cast(P0, BEZA),
            Action::Resolve,
            Action::Resolve,
            controls_count(P0, TREASURE_TOKEN, 0),
            life(P0, 20),
            controls_count(P0, FISH_TOKEN, 0),
            hand(P0, 0),
            controls_count(P0, BEZA, 1),
        ]),
    // As Beza's ability triggers, P1 has more creatures (two to one) and as many cards in hand
    // (none). P0 returns the Skydiver to P1's hand in response, so as the ability resolves the
    // creatures are tied and P1 has more cards in hand: no Fish, but a card.
    Scenario::new("Beza compares as its ability resolves")
        .hand(P0, &[BEZA, names::SINK_INTO_STUPOR])
        .battlefield(P0, RESPONDING)
        .library(P0, &[names::ISLAND])
        .battlefield(P1, AHEAD_IN_CREATURES)
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::cast(P0, BEZA),
            Action::Resolve,
            Action::cast(P0, names::SINK_INTO_STUPOR),
            Action::choose(P0, DecisionKind::Target, names::THIEVING_SKYDIVER),
            Action::Resolve,
            Action::assert(Assertion::Count {
                player: P1,
                zone: Zone::Hand,
                card: Some(names::THIEVING_SKYDIVER),
                count: 1,
            }),
            Action::Resolve,
            controls_count(P0, FISH_TOKEN, 0),
            hand(P0, 1),
            controls_count(P0, TREASURE_TOKEN, 0),
            life(P0, 20),
        ]),
    // A Treasure pays for a spell as it is cast, sacrificed for one mana of any color
    // (CR 111.10a): P0 counters Get Lost with Counterspell from an Island and Beza's Treasure.
    // The Treasure offers no activation of its own (§10): P0 may pass or counter.
    Scenario::new("Beza's Treasure pays for Counterspell")
        .hand(P0, &[BEZA, names::COUNTERSPELL, names::ISLAND])
        .battlefield(P0, PLAINS)
        .library(P0, &[names::ISLAND])
        .battlefield(P1, &[PLAIN, PLAIN, PLAIN, PLAIN, PLAIN])
        .hand(P1, &[names::GET_LOST])
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::cast(P0, BEZA),
            Action::Resolve,
            Action::Resolve,
            controls_count(P0, TREASURE_TOKEN, 1),
            // With nothing to cast, P0's priority is no decision: the Treasure adds none.
            Action::PlayLand {
                player: P0,
                card: names::ISLAND,
            },
            Action::cast(P1, names::GET_LOST),
            Action::assert(Assertion::Decision {
                player: P0,
                kind: DecisionKind::Priority,
                candidates: 2,
            }),
            Action::cast(P0, names::COUNTERSPELL),
            controls_count(P0, TREASURE_TOKEN, 0),
            Action::assert(Assertion::Mana {
                player: P0,
                color: ManaColor::Blue,
                amount: 0,
            }),
            Action::Resolve,
            Action::assert(Assertion::Count {
                player: P1,
                zone: Zone::Graveyard,
                card: Some(names::GET_LOST),
                count: 1,
            }),
            controls_count(P0, BEZA, 1),
        ]),
];
