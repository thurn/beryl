// Generated from the card store by `just card header`: notes go below the end marker.
//! Gisela, the Broken Blade {2}{W}{W}
//! Legendary Creature — Angel Horror, 4/3
//!
//! Flying, first strike, lifelink
//! At the beginning of your end step, if you both own and control Gisela and a creature named
//! Bruna, the Fading Light, exile them, then meld them into Brisela, Voice of Nightmares.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = card_with_shape(
    Card::GiselaTheBrokenBlade,
    PrintedShape::GiselaTheBrokenBlade,
    &[Printed::GiselaTheBrokenBlade
        .keywords(
            Keywords::FLYING
                .union(Keywords::FIRST_STRIKE)
                .union(Keywords::LIFELINK),
        )
        .abilities(&[Ability::triggered(&Trigger::step(TurnStep::End, you()))
            .only_if(&own_and_control_meld_pair(Filter::Types(Types::CREATURE)))
            .effect(&meld(Filter::Types(Types::CREATURE)))])],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const GISELA: &str = Card::GiselaTheBrokenBlade;
    const BRUNA: &str = Card::BrunaTheFadingLight;
    const BRISELA: &str = Card::BriselaVoiceOfNightmares;
    const OKO: &str = Card::OkoThiefOfCrowns;

    /// The mana value of the `index`th permanent named `name` that `who` controls.
    const fn mana_value(who: Player, name: &'static str, index: u16, value: u8) -> Action {
        Action::assert(Assertion::ManaValue {
            object: SnapshotRef {
                zone: Zone::Battlefield,
                player: who.seat(),
                name: Some(name),
                index,
            },
            value,
        })
    }

    /// P0's Gisela and Bruna, after P0's first combat: they meld into Brisela at P0's end
    /// step. Each player has cards to draw until P0's second turn.
    const MELDED: Scenario = Scenario::setup()
        .interaction(&[BRUNA, BRISELA])
        .at(1, P0, TurnStep::PostcombatMain)
        .permanents(P0, &[Permanent::new(GISELA), Permanent::new(BRUNA)])
        .library(P0, &[Card::Plains, Card::Plains])
        .library(P1, &[Card::Island, Card::Island]);

    /// The actions that meld Gisela and Bruna at P0's first end step.
    const MELD: [Action; 2] = [advance_to(1, TurnStep::End), Resolve];

    pub const SCENARIOS: &[Scenario] = &[
        // One permanent with the combined back face, whose mana value is the sum of its
        // cards' front faces: 4 for Gisela and 7 for Bruna.
        MELDED
            .titled("Gisela and Bruna meld into Brisela at your end step")
            .actions(&[
                MELD[0],
                MELD[1],
                ME.controls(BRISELA, 1),
                ME.controls(GISELA, 0),
                ME.controls(BRUNA, 0),
                ME.pt(BRISELA, 9, 10),
                mana_value(ME, BRISELA, 0, 11),
                ME.has(Zone::Exile, GISELA, 0),
                ME.has(Zone::Exile, BRUNA, 0),
            ]),
        // The ability checks the pair at each of P0's end steps: with no Bruna at the first,
        // it does nothing, and it melds them at the next one once Bruna is cast.
        Scenario::new("Gisela melds at a later end step once Bruna arrives")
            .interaction(&[BRUNA, BRISELA])
            .at(1, P0, TurnStep::PostcombatMain)
            .battlefield(
                P0,
                &[
                    GISELA,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                ],
            )
            .permanents(
                P0,
                &[
                    Permanent::new(Card::Plains),
                    Permanent::new(Card::Plains),
                    Permanent::new(Card::Plains),
                ],
            )
            .hand(P0, &[BRUNA])
            .library(P0, &[Card::Plains, Card::Plains])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                advance_to(1, TurnStep::End),
                stack(0),
                ME.controls(GISELA, 1),
                advance_to(3, TurnStep::PrecombatMain),
                ME.casts(BRUNA),
                Resolve,
                ME.controls(BRUNA, 1),
                advance_to(3, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.done_attacking(),
                advance_to(3, TurnStep::End),
                Resolve,
                ME.controls(BRISELA, 1),
            ]),
        // [CR 701.42a] asks that "you both own and control" both. P1's Oko gives P0 P1's
        // Bruna for P0's 2/1, so P0 controls a Bruna it does not own, and Gisela's ability
        // does not trigger at P0's next end step.
        //
        // [CR 701.42a]: https://yawgatog.com/resources/magic-rules/#R70142a
        Scenario::new("Gisela does not meld with a Bruna you control but don't own")
            .interaction(&[BRUNA, BRISELA, OKO])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[GISELA, Prop::Vanilla])
            .battlefield(P1, &[BRUNA])
            .permanents(P1, &[Permanent::new(OKO).counters(CounterKind::Loyalty, 5)])
            .library(P0, &[Card::Plains, Card::Plains])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                THEM.activates(OKO, 2), // −5
                Resolve,
                ME.controls(BRUNA, 1),
                advance_to(3, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.done_attacking(),
                advance_to(3, TurnStep::End),
                stack(0),
                ME.controls(GISELA, 1),
                ME.controls(BRUNA, 1),
                ME.controls(BRISELA, 0),
            ]),
        // A melded permanent that leaves the battlefield becomes its two cards: Jace's −1
        // returns both to P0's hand.
        MELDED
            .titled("Brisela returned to its owner's hand is two cards")
            .interaction(&[BRUNA, BRISELA])
            .battlefield(P1, &[Card::JaceTheMindSculptor])
            .actions(&[
                MELD[0],
                MELD[1],
                advance_to(2, TurnStep::PrecombatMain),
                THEM.activates(Card::JaceTheMindSculptor, 2), // −1
                Resolve,
                ME.controls(BRISELA, 0),
                ME.has(Zone::Hand, GISELA, 1),
                ME.has(Zone::Hand, BRUNA, 1),
            ]),
        // Destroyed, the melded permanent dies once and both cards go to the graveyard. Its
        // controller destroys it: Brisela keeps P1 from casting a spell of mana value 2.
        MELDED
            .titled("Brisela destroyed puts both cards into the graveyard")
            .interaction(&[BRUNA, BRISELA, Card::GetLost])
            .battlefield(P0, &[Card::Plains, Card::Plains])
            .hand(P0, &[Card::GetLost])
            .actions(&[
                MELD[0],
                MELD[1],
                ME.casts(Card::GetLost),
                Resolve,
                ME.controls(BRISELA, 0),
                ME.has(Zone::Graveyard, GISELA, 1),
                ME.has(Zone::Graveyard, BRUNA, 1),
            ]),
        // [CR 713.8c]: the delayed ability that returns "that card" finds both cards, which
        // return as two permanents. Gisela was not on the battlefield as that end step began,
        // so her ability doesn't meld them again.
        //
        // [CR 713.8c]: https://yawgatog.com/resources/magic-rules/#R7138c
        MELDED
            .titled("Brisela exiled by Flickerwisp returns as Gisela and Bruna")
            .interaction(&[BRUNA, BRISELA, Card::Flickerwisp])
            .battlefield(P0, &[Card::Plains, Card::Plains, Card::Plains])
            .hand(P0, &[Card::Flickerwisp])
            .actions(&[
                MELD[0],
                MELD[1],
                advance_to(3, TurnStep::PrecombatMain),
                ME.casts(Card::Flickerwisp),
                Resolve,
                ME.targets(BRISELA),
                Resolve,
                ME.controls(BRISELA, 0),
                ME.has(Zone::Exile, GISELA, 1),
                ME.has(Zone::Exile, BRUNA, 1),
                advance_to(3, TurnStep::End),
                Resolve,
                stack(0),
                ME.controls(GISELA, 1),
                ME.controls(BRUNA, 1),
                ME.controls(BRISELA, 0),
            ]),
        // [CR 713.5c]: a copy of a melded permanent copies the combined back face as one
        // object, with mana value 0.
        //
        // [CR 713.5c]: https://yawgatog.com/resources/magic-rules/#R7135c
        MELDED
            .titled("Phantasmal Image copies Brisela as one object with mana value 0")
            .interaction(&[BRUNA, BRISELA, Card::PhantasmalImage])
            .battlefield(P0, &[Card::Island, Card::Island])
            .hand(P0, &[Card::PhantasmalImage])
            .actions(&[
                MELD[0],
                MELD[1],
                advance_to(3, TurnStep::PrecombatMain),
                ME.casts(Card::PhantasmalImage),
                Resolve,
                ME.picks_up_to(BRISELA),
                ME.controls(BRISELA, 2),
                ME.pt(BRISELA, 9, 10),
                ME.has_subtype(BRISELA, Subtype::Illusion, true),
                mana_value(ME, BRISELA, 0, 11),
                mana_value(ME, BRISELA, 1, 0),
            ]),
    ];
}
