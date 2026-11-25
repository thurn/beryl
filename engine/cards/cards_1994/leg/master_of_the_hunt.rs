// Generated from the card store by `just card header`: notes go below the end marker.
//! Master of the Hunt {2}{G}{G}
//! Creature — Human, 2/2
//!
//! {2}{G}{G}: Create a 1/1 green Wolf creature token named Wolves of the Hunt. It has "bands with
//! other creatures named Wolves of the Hunt." (Any creatures named Wolves of the Hunt can attack in
//! a band as long as at least one has "bands with other creatures named Wolves of the Hunt." Bands
//! are blocked as a group. If at least two creatures named Wolves of the Hunt you control, one of
//! which has "bands with other creatures named Wolves of the Hunt," are blocking or being blocked
//! by the same creature, you divide that creature's combat damage, not its controller, among any of
//! the creatures it's being blocked by or is blocking.)
// End of the generated card header.

use crate::prelude::*;

/// The name of the Wolf tokens Master of the Hunt creates, which their banding ability reads.
pub const WOLVES_OF_THE_HUNT: &str = "Wolves of the Hunt";

/// A 1/1 green Wolf creature token named Wolves of the Hunt with "bands with other creatures
/// named Wolves of the Hunt".
pub static WOLVES: CardDef = Token::WolvesOfTheHunt
    .abilities(&[Ability::bands_with_other(&Filter::All(&[
        Filter::Types(Types::CREATURE),
        Filter::Named(WOLVES_OF_THE_HUNT),
    ]))])
    .card();

pub static CARD: CardDef = Printed::MasterOfTheHunt
    .abilities(&[
        Ability::activated(Cost::mana(mana("{2}{G}{G}"))).effect(&create_tokens(you(), &WOLVES, 1)),
    ])
    .card()
    .creating(&[&WOLVES]);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    use super::WOLVES_OF_THE_HUNT as WOLF;

    const MASTER: &str = Card::MasterOfTheHunt;
    const HERO: &str = Card::BenalishHero;
    const IMAGE: &str = Card::PhantasmalImage;
    const WOLVES: &[Permanent] = &[Permanent::token(WOLF), Permanent::token(WOLF)];

    /// Master of the Hunt and the mana to activate it twice.
    const EIGHT_FORESTS: &[&str] = &[
        MASTER,
        Card::Forest,
        Card::Forest,
        Card::Forest,
        Card::Forest,
        Card::Forest,
        Card::Forest,
        Card::Forest,
        Card::Forest,
    ];

    /// A library that holds Master of the Hunt, so that a game whose Wolves start on the
    /// battlefield has the card that creates them.
    const LIBRARY: &[&str] = &[MASTER, Card::Island, Card::Island];

    const START: Action = advance_to(1, TurnStep::BeginningOfCombat);
    const END: Action = advance_to(1, TurnStep::EndOfCombat);

    const fn band(card: &'static str, first: Option<&'static str>) -> Action {
        ME.answers(DecisionKind::Attacker, Pick::Band { card, first })
    }

    pub const SCENARIOS: &[Scenario] = &[
        // The ability has no tap symbol, so eight Forests pay for it twice in one turn.
        Scenario::new("Master of the Hunt creates a Wolf each time it is activated")
            .battlefield(P0, EIGHT_FORESTS)
            .actions(&[
                ME.activates(MASTER, 0),
                Resolve,
                ME.activates(MASTER, 0),
                Resolve,
                ME.token_pt(WOLF, 1, 1),
                ME.controls(WOLF, 2),
            ]),
        // The 2/1 blocks one Wolf, which blocks the whole band, so both Wolves deal it damage.
        // Their controller, not the blocker's, puts its 2 damage on one Wolf.
        Scenario::new(
            "two Wolves attack in a band and their controller divides the blocker's damage",
        )
        .permanents(P0, WOLVES)
        .library(P0, LIBRARY)
        .battlefield(P1, &[Prop::Vanilla])
        .actions(&[
            START,
            ME.passes(),
            band(WOLF, None),
            band(WOLF, Some(WOLF)),
            THEM.chooses(DecisionKind::Blocker, Prop::Vanilla),
            ME.answers(DecisionKind::DamageDivision, Pick::Number { n: 0 }),
            END,
            ME.controls(WOLF, 1),
            THEM.has(Zone::Graveyard, Prop::Vanilla, 1),
            THEM.life(20),
        ]),
        // A Wolf has no banding, so it may join Benalish Hero's band as its one creature
        // without banding, and the second Wolf may not join too: the Hero is not named
        // Wolves of the Hunt, so the three are not a "bands with other" band either.
        Scenario::new("a Wolf joins a banding band as its one creature without banding")
            .interaction(&[HERO])
            .battlefield(P0, &[HERO])
            .permanents(P0, WOLVES)
            .library(P0, LIBRARY)
            .battlefield(P1, &[Prop::Vanilla])
            .actions(&[
                START,
                ME.passes(),
                band(HERO, None),
                band(WOLF, Some(HERO)),
                ME.deciding(DecisionKind::Attacker, 3),
                ME.attacks_with(WOLF),
                THEM.chooses(DecisionKind::Blocker, Prop::Vanilla),
                THEM.chooses(DecisionKind::BlockTarget, HERO),
                ME.answers(DecisionKind::DamageDivision, Pick::Number { n: 0 }),
                END,
                ME.controls(HERO, 1),
                ME.controls(WOLF, 1),
                THEM.has(Zone::Graveyard, Prop::Vanilla, 1),
                THEM.life(19),
            ]),
        // Two Wolves block the 2/1 together, so the defending player divides its damage and
        // puts all of it on one of them.
        Scenario::new(
            "the defending player divides an attacker's damage between two blocking Wolves",
        )
        .battlefield(P0, &[Prop::Vanilla])
        .permanents(P1, WOLVES)
        .library(P1, LIBRARY)
        .actions(&[
            START,
            ME.passes(),
            ME.attacks_with(Prop::Vanilla),
            THEM.chooses(DecisionKind::Blocker, WOLF),
            THEM.chooses(DecisionKind::Blocker, WOLF),
            THEM.answers(DecisionKind::DamageDivision, Pick::Number { n: 0 }),
            END,
            THEM.controls(WOLF, 1),
            ME.has(Zone::Graveyard, Prop::Vanilla, 1),
        ]),
        // Phantasmal Image copies the Wolf's name and its "bands with other" ability, so the
        // two blocking together let their controller divide the attacker's damage.
        Scenario::new("a copy of a Wolf has its name and bands with it")
            .interaction(&[IMAGE])
            .battlefield(P0, &[Card::Island, Card::Island])
            .permanents(P0, &[Permanent::token(WOLF)])
            .library(P0, LIBRARY)
            .hand(P0, &[IMAGE])
            .battlefield(P1, &[Prop::Vanilla])
            .actions(&[
                ME.casts(IMAGE),
                Resolve,
                ME.picks_up_to(WOLF),
                ME.controls(WOLF, 2),
                START,
                ME.passes(),
                ME.answers(DecisionKind::Attacker, Pick::Done),
                advance_to(2, TurnStep::BeginningOfCombat),
                THEM.passes(),
                THEM.attacks_with(Prop::Vanilla),
                ME.chooses(DecisionKind::Blocker, WOLF),
                ME.chooses(DecisionKind::Blocker, WOLF),
                ME.answers(DecisionKind::DamageDivision, Pick::Number { n: 0 }),
                advance_to(2, TurnStep::EndOfCombat),
                ME.controls(WOLF, 1),
                THEM.has(Zone::Graveyard, Prop::Vanilla, 1),
            ]),
    ];
}
