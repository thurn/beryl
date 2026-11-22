// Generated from the card store by `just card header`: notes go below the end marker.
//! Delver of Secrets {U}
//! Creature — Human Wizard, 1/1
//!
//! At the beginning of your upkeep, look at the top card of your library. You may reveal that card.
//! If an instant or sorcery card is revealed this way, transform this creature.
//!
//! Insectile Aberration
//! Creature — Human Insect, 3/2
//!
//! Flying
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = card_with_shape(
    Card::DelverOfSecrets,
    PrintedShape::DelverOfSecrets,
    &[
        Printed::DelverOfSecrets.abilities(&[Ability::triggered(&Trigger::step(
            TurnStep::Upkeep,
            you(),
        ))
        .effect(&sequence(&[
            look_at(you(), top_of_library(&Players::You, 1)),
            may(you(), &reveal(those())),
            if_then(
                Condition::Is(those(), instant_or_sorcery()),
                &transform(this()),
            ),
        ]))]),
        Printed::InsectileAberration.keywords(Keywords::FLYING),
    ],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const DELVER: &str = Card::DelverOfSecrets;
    const ABERRATION: &str = "Insectile Aberration";
    const BOLT: &str = Card::LightningBolt;
    const IMAGE: &str = Card::PhantasmalImage;
    const HOLY_STRENGTH: &str = Card::HolyStrength;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("transforming keeps the counter and Aura")
            .interaction(&[BOLT, HOLY_STRENGTH])
            .permanents(
                P0,
                &[
                    Permanent::new(DELVER).counters(CounterKind::PlusOne, 1),
                    Permanent::new(Card::Plains),
                    Permanent::new(Card::Mountain),
                    Permanent::new(Prop::Vanilla),
                ],
            )
            .hand(P0, &[HOLY_STRENGTH, BOLT])
            .library(P0, &[BOLT, Card::Island])
            .actions(&[
                ME.casts(HOLY_STRENGTH),
                ME.targets(DELVER),
                Resolve,
                ME.pt(DELVER, 3, 4),
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.done_attacking(),
                advance_to(3, TurnStep::Upkeep),
                Resolve,
                ME.yes(),
                ME.counters(ABERRATION, CounterKind::PlusOne, 1),
                ME.controls(HOLY_STRENGTH, 1),
                ME.pt(ABERRATION, 5, 5),
                ME.library_top(&[BOLT]),
            ]),
        Scenario::new("keeping an instant hidden leaves Delver untransformed")
            .at(2, P1, TurnStep::End)
            .interaction(&[BOLT])
            .battlefield(P0, &[DELVER, Card::Mountain])
            .hand(P0, &[BOLT])
            .library(P0, &[BOLT])
            .actions(&[
                advance_to(3, TurnStep::Upkeep),
                Resolve,
                ME.no(),
                ME.controls(DELVER, 1),
                ME.library_top(&[BOLT]),
            ]),
        Scenario::new("a land can be revealed and stays on top")
            .at(2, P1, TurnStep::End)
            .interaction(&[BOLT])
            .battlefield(P0, &[DELVER, Card::Mountain])
            .hand(P0, &[BOLT])
            .library(P0, &[Card::Island])
            .actions(&[
                advance_to(3, TurnStep::Upkeep),
                Resolve,
                ME.yes(),
                ME.controls(DELVER, 1),
                ME.library_top(&[Card::Island]),
            ]),
        Scenario::new("a single-faced copy of Delver cannot transform")
            .interaction(&[IMAGE, BOLT])
            .battlefield(P0, &[DELVER, Card::Island, Card::Island])
            .hand(P0, &[IMAGE])
            .library(P0, &[BOLT, Card::Island])
            .actions(&[
                ME.casts(IMAGE),
                Resolve,
                ME.picks_up_to(DELVER),
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.done_attacking(),
                advance_to(2, TurnStep::End),
                THEM.passes(),
                ME.chooses(DecisionKind::TriggerOrder, DELVER),
                Resolve,
                ME.yes(),
                Resolve,
                ME.yes(),
                ME.controls(DELVER, 1),
                ME.has_subtype(DELVER, Subtype::Illusion, true),
                ME.controls(ABERRATION, 1),
            ]),
        Scenario::new(
            "a single-faced copy of Insectile Aberration has its back-face characteristics",
        )
        .interaction(&[IMAGE])
        .permanents(
            P0,
            &[
                Permanent::new(DELVER).transformed(),
                Permanent::new(Card::Island),
                Permanent::new(Card::Island),
            ],
        )
        .hand(P0, &[IMAGE])
        .actions(&[
            ME.casts(IMAGE),
            Resolve,
            ME.picks_up_to(ABERRATION),
            ME.controls(ABERRATION, 2),
            ME.has_subtype(ABERRATION, Subtype::Illusion, true),
        ]),
    ];
}
