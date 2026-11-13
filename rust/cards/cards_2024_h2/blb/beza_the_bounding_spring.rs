// Generated from the card store by `just card header`: notes go below the end marker.
//! Beza, the Bounding Spring {2}{W}{W}
//! Legendary Creature — Elemental Elk, 4/5
//!
//! When Beza enters, create a Treasure token if an opponent controls more lands than you. You gain
//! 4 life if an opponent has more life than you. Create two 1/1 blue Fish creature tokens if an
//! opponent controls more creatures than you. Draw a card if an opponent has more cards in hand
//! than you.
// End of the generated card header.

use crate::tokens::TREASURE;
use crate::{names, printed};
use beryl_card::effect::build::{
    EACH_OPPONENT, YOU, create_tokens, draw, gain_life, if_then, sequence,
};
use beryl_card::{
    Ability, Amount, CardDef, Comparison, Condition, Effect, Filter, Objects, Players, Trigger,
    Types, Zone, card,
};

/// The Fish token's name: its subtype plus the word "Token"
/// ([CR 111.4](https://yawgatog.com/resources/magic-rules/#R1114)).
pub const FISH_TOKEN: &str = "Fish Token";

/// The Fish token: a 1/1 blue Fish creature token.
pub static FISH: CardDef = card(FISH_TOKEN, &[printed::token::FISH_TOKEN]);

/// "An opponent has more [of an amount] than you": the most any of them has, against yours
/// ([CR 102.2](https://yawgatog.com/resources/magic-rules/#R1022): "In a two-player game, a
/// player's opponent is the other player"). A tie is not "more".
const fn an_opponent_has_more(amount: &'static Amount) -> Condition {
    Condition::Compare(
        Amount::Most(EACH_OPPONENT, amount),
        Comparison::MoreThan,
        *amount,
    )
}

/// "Lands you control".
const LANDS: Amount = Amount::Count(&Objects::All {
    zone: Zone::Battlefield,
    filter: Filter::All(&[Filter::Types(Types::LAND), Filter::Whose(&Players::You)]),
});
const LIFE: Amount = Amount::Life(YOU);
/// "Creatures you control".
const CREATURES: Amount = Amount::Count(&Objects::All {
    zone: Zone::Battlefield,
    filter: Filter::All(&[Filter::Types(Types::CREATURE), Filter::Whose(&Players::You)]),
});
const CARDS_IN_HAND: Amount = Amount::Count(&Objects::All {
    zone: Zone::Hand,
    filter: Filter::Whose(&Players::You),
});

/// The four sentences in order, each comparison made as its sentence is applied, on
/// resolution ([CR 608.2h](https://yawgatog.com/resources/magic-rules/#R6082h)): "If an effect
/// requires information from the game (such as the number of creatures on the battlefield),
/// the answer is determined only once, when the effect is applied." The "if"s are not right
/// after the trigger condition, so none is an intervening "if" (CR 603.4): the ability always
/// triggers.
static COMPARE: Effect = sequence(&[
    if_then(
        an_opponent_has_more(&LANDS),
        &create_tokens(YOU, &TREASURE, 1),
    ),
    if_then(an_opponent_has_more(&LIFE), &gain_life(YOU, 4)),
    if_then(
        an_opponent_has_more(&CREATURES),
        &create_tokens(YOU, &FISH, 2),
    ),
    if_then(an_opponent_has_more(&CARDS_IN_HAND), &draw(YOU, 1)),
]);

pub static CARD: CardDef = card(
    names::BEZA_THE_BOUNDING_SPRING,
    &[printed::BEZA_THE_BOUNDING_SPRING
        .abilities(&[Ability::triggered(&Trigger::ENTERS).effect(&COMPARE)])],
)
.creating(&[&TREASURE, &FISH]);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_card::ManaColor;

    use beryl_card::{DecisionKind, Seat, Zone};

    use beryl_scenario_types::{Action, Assertion, P0, P1, Permanent, Scenario};

    use super::FISH_TOKEN;
    use crate::tokens::TREASURE_TOKEN;
    use crate::{names, props};

    const BEZA: &str = names::BEZA_THE_BOUNDING_SPRING;

    const PLAINS: &[Permanent] = &[Permanent::new(names::PLAINS); 4];

    const PLAIN: Permanent = Permanent::new(names::PLAINS);

    const ISLAND: Permanent = Permanent::new(names::ISLAND);

    const TWO_DROP: Permanent = Permanent::new(props::SMALL_CREATURE);

    const SPELLBINDER: Permanent = Permanent::new(names::ELITE_SPELLBINDER);

    /// Five lands and two creatures.
    const BEHIND: &[Permanent] = &[PLAIN, PLAIN, PLAIN, PLAIN, PLAIN, TWO_DROP, SPELLBINDER];

    /// Four lands and one creature.
    const TIED: &[Permanent] = &[PLAIN, PLAIN, PLAIN, PLAIN, TWO_DROP];

    /// Beza's four lands and Sink into Stupor's three.
    const RESPONDING: &[Permanent] = &[PLAIN, PLAIN, PLAIN, PLAIN, ISLAND, ISLAND, ISLAND];

    /// Two lands and two creatures.
    const AHEAD_IN_CREATURES: &[Permanent] = &[PLAIN, PLAIN, TWO_DROP, SPELLBINDER];

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
            .permanents(P0, PLAINS)
            .library(P0, &[names::ISLAND])
            .permanents(P1, BEHIND)
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
            .permanents(P0, PLAINS)
            .library(P0, &[names::ISLAND])
            .permanents(P1, TIED)
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
        // (none). P0 returns the two-drop to P1's hand in response, so as the ability resolves the
        // creatures are tied and P1 has more cards in hand: no Fish, but a card.
        Scenario::new("Beza compares as its ability resolves")
            .hand(P0, &[BEZA, names::SINK_INTO_STUPOR])
            .permanents(P0, RESPONDING)
            .library(P0, &[names::ISLAND])
            .permanents(P1, AHEAD_IN_CREATURES)
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, BEZA),
                Action::Resolve,
                Action::cast(P0, names::SINK_INTO_STUPOR),
                Action::choose(P0, DecisionKind::Target, props::SMALL_CREATURE),
                Action::Resolve,
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Hand,
                    card: Some(props::SMALL_CREATURE),
                    count: 1,
                }),
                Action::Resolve,
                controls_count(P0, FISH_TOKEN, 0),
                hand(P0, 1),
                controls_count(P0, TREASURE_TOKEN, 0),
                life(P0, 20),
            ]),
        // A Treasure pays for a spell as it is cast, sacrificed for one mana of any color
        // (CR 111.10a): P0 counters Sink into Stupor, aimed at Beza, with Counterspell from an
        // Island and Beza's Treasure.
        // The Treasure offers no activation of its own (§10): P0 may pass or counter.
        Scenario::new("Beza's Treasure pays for Counterspell")
            .hand(P0, &[BEZA, names::COUNTERSPELL, names::ISLAND])
            .permanents(P0, PLAINS)
            .library(P0, &[names::ISLAND])
            .permanents(P1, &[PLAIN, PLAIN, ISLAND, ISLAND, ISLAND])
            .hand(P1, &[props::REMOVAL])
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
                Action::cast(P1, props::REMOVAL),
                Action::choose(P1, DecisionKind::Target, BEZA),
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
                    card: Some(props::REMOVAL),
                    count: 1,
                }),
                controls_count(P0, BEZA, 1),
            ]),
    ];
}
