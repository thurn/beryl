// Generated from the card store by `just card header`: notes go below the end marker.
//! Beza, the Bounding Spring {2}{W}{W}
//! Legendary Creature — Elemental Elk, 4/5
//!
//! When Beza enters, create a Treasure token if an opponent controls more lands than you. You gain
//! 4 life if an opponent has more life than you. Create two 1/1 blue Fish creature tokens if an
//! opponent controls more creatures than you. Draw a card if an opponent has more cards in hand
//! than you.
// End of the generated card header.

use crate::prelude::*;
use crate::tokens::TREASURE;

/// The Fish token's name: its subtype plus the word "Token"
/// ([CR 111.4](https://yawgatog.com/resources/magic-rules/#R1114)).
pub const FISH_TOKEN: &str = "Fish Token";

/// The Fish token: a 1/1 blue Fish creature token.
pub static FISH: CardDef = Token::FishToken.card();

/// "An opponent has more [of an amount] than you": the most any of them has, against yours
/// ([CR 102.2](https://yawgatog.com/resources/magic-rules/#R1022): "In a two-player game, a
/// player's opponent is the other player"). A tie is not "more".
const fn an_opponent_has_more(amount: &'static Amount) -> Condition {
    Condition::Compare(
        Amount::Most(each_opponent(), amount),
        Comparison::MoreThan,
        *amount,
    )
}

/// The four sentences in order, each comparison made as its sentence is applied, on
/// resolution ([CR 608.2h](https://yawgatog.com/resources/magic-rules/#R6082h)): "If an effect
/// requires information from the game (such as the number of creatures on the battlefield),
/// the answer is determined only once, when the effect is applied." The "if"s are not right
/// after the trigger condition, so none is an intervening "if" (CR 603.4): the ability always
/// triggers.
pub static CARD: CardDef = Printed::BezaTheBoundingSpring
    .abilities(&[Ability::triggered(&Trigger::ENTERS).effect(&sequence(&[
        // Lands you control.
        if_then(
            an_opponent_has_more(&Amount::Count(&Objects::All {
                zone: Zone::Battlefield,
                filter: Filter::All(&[Filter::Types(Types::LAND), Filter::Whose(&Players::You)]),
            })),
            &create_tokens(you(), &TREASURE, 1),
        ),
        if_then(
            an_opponent_has_more(&Amount::Life(you())),
            &gain_life(you(), 4),
        ),
        // Creatures you control.
        if_then(
            an_opponent_has_more(&Amount::Count(&Objects::All {
                zone: Zone::Battlefield,
                filter: Filter::All(&[
                    Filter::Types(Types::CREATURE),
                    Filter::Whose(&Players::You),
                ]),
            })),
            &create_tokens(you(), &FISH, 2),
        ),
        if_then(
            an_opponent_has_more(&Amount::Count(&Objects::All {
                zone: Zone::Hand,
                filter: Filter::Whose(&Players::You),
            })),
            &draw(you(), 1),
        ),
    ]))])
    .card()
    .creating(&[&TREASURE, &FISH]);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use super::FISH_TOKEN;
    use crate::prelude::*;
    use crate::tokens::TREASURE_TOKEN;

    const BEZA: &str = Card::BezaTheBoundingSpring;

    const PLAINS: &[&str] = &[Card::Plains; 4];

    pub const SCENARIOS: &[Scenario] = &[
        // P1 has more lands (5 to 4), more life (20 to 15), more creatures (two to Beza alone)
        // and more cards in hand (two to none): all four.
        Scenario::new("Beza with every comparison true")
            .life(P0, 15)
            .hand(P0, &[BEZA])
            .battlefield(P0, PLAINS)
            // Five lands and two creatures.
            .battlefield(
                P1,
                &[
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Prop::SmallCreature,
                    Card::EliteSpellbinder,
                ],
            )
            .hand(P1, &[Card::Plains, Card::Plains])
            .actions(&[
                ME.casts(BEZA),
                Resolve,
                Resolve,
                ME.controls(TREASURE_TOKEN, 1),
                ME.life(19),
                ME.controls(FISH_TOKEN, 2),
                ME.token_pt(FISH_TOKEN, 1, 1),
                ME.has_cards(Zone::Hand, 1),
            ]),
        // Four lands each, 20 life each, one creature each counting Beza, no cards in hand: a
        // tie is not more, so nothing.
        Scenario::new("Beza with every comparison tied")
            .hand(P0, &[BEZA])
            .battlefield(P0, PLAINS)
            // Four lands and one creature.
            .battlefield(
                P1,
                &[
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Prop::SmallCreature,
                ],
            )
            .actions(&[
                ME.casts(BEZA),
                Resolve,
                Resolve,
                ME.controls(TREASURE_TOKEN, 0),
                ME.life(20),
                ME.controls(FISH_TOKEN, 0),
                ME.has_cards(Zone::Hand, 0),
                ME.controls(BEZA, 1),
            ]),
        // As Beza's ability triggers, P1 has more creatures (two to one) and as many cards in hand
        // (none). P0 returns the two-drop to P1's hand in response, so as the ability resolves the
        // creatures are tied and P1 has more cards in hand: no Fish, but a card.
        Scenario::new("Beza compares as its ability resolves")
            .hand(P0, &[BEZA, Card::SinkIntoStupor])
            // Beza's four lands and Sink into Stupor's three.
            .battlefield(
                P0,
                &[
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            // Two lands and two creatures.
            .battlefield(
                P1,
                &[
                    Card::Plains,
                    Card::Plains,
                    Prop::SmallCreature,
                    Card::EliteSpellbinder,
                ],
            )
            .actions(&[
                ME.casts(BEZA),
                Resolve,
                ME.casts(Card::SinkIntoStupor),
                ME.targets(Prop::SmallCreature),
                Resolve,
                THEM.has(Zone::Hand, Prop::SmallCreature, 1),
                Resolve,
                ME.controls(FISH_TOKEN, 0),
                ME.has_cards(Zone::Hand, 1),
                ME.controls(TREASURE_TOKEN, 0),
                ME.life(20),
            ]),
        // A Treasure pays for a spell as it is cast, sacrificed for one mana of any color
        // (CR 111.10a): P0 counters Sink into Stupor, aimed at Beza, with Counterspell from an
        // Island and Beza's Treasure.
        // The Treasure offers no activation of its own (§10): P0 may pass or counter.
        Scenario::new("Beza's Treasure pays for Counterspell")
            .hand(P0, &[BEZA, Card::Counterspell, Card::Island])
            .battlefield(P0, PLAINS)
            .battlefield(
                P1,
                &[
                    Card::Plains,
                    Card::Plains,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .hand(P1, &[Prop::Removal])
            .actions(&[
                ME.casts(BEZA),
                Resolve,
                Resolve,
                ME.controls(TREASURE_TOKEN, 1),
                // With nothing to cast, P0's priority is no decision: the Treasure adds none.
                ME.plays(Card::Island),
                THEM.casts(Prop::Removal),
                THEM.targets(BEZA),
                ME.deciding(DecisionKind::Priority, 2),
                ME.casts(Card::Counterspell),
                ME.controls(TREASURE_TOKEN, 0),
                ME.mana(ManaColor::Blue, 0),
                Resolve,
                THEM.has(Zone::Graveyard, Prop::Removal, 1),
                ME.controls(BEZA, 1),
            ]),
    ];
}
