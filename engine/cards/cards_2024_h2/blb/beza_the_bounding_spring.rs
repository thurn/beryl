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

pub const FISH_TOKEN: &str = "Fish Token";

pub static FISH: CardDef = Token::FishToken.card();

const fn an_opponent_has_more(amount: &'static Amount) -> Condition {
    Condition::Compare(
        Amount::Most(each_opponent(), amount),
        Comparison::MoreThan,
        *amount,
    )
}

/// The four comparisons happen during resolution. Beza's enters trigger has no condition
/// between the trigger event and its effect, the position that [CR 603.4] defines as
/// “When/Whenever/At [trigger event], if [condition], [effect].”
///
/// [CR 603.4]: https://yawgatog.com/resources/magic-rules/#R6034
pub static CARD: CardDef = Printed::BezaTheBoundingSpring
    .abilities(&[when_enters(&sequence(&[
        // Lands you control.
        if_then(
            an_opponent_has_more(&Amount::Count(&permanents(Filter::All(&[land(), yours()])))),
            &create_tokens(you(), &TREASURE, 1),
        ),
        if_then(
            an_opponent_has_more(&Amount::Life(you())),
            &gain_life(you(), 4),
        ),
        // Creatures you control.
        if_then(
            an_opponent_has_more(&Amount::Count(&permanents(Filter::All(&[
                creature(),
                yours(),
            ])))),
            &create_tokens(you(), &FISH, 2),
        ),
        if_then(
            an_opponent_has_more(&Amount::Count(&Objects::All {
                zone: Zone::Hand,
                filter: yours(),
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
                    Prop::Vanilla,
                    Prop::Flier,
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
