// Generated from the card store by `just card header`: notes go below the end marker.
//! Fractured Identity {3}{W}{U}
//! Sorcery
//!
//! Exile target nonland permanent. Each player other than its controller creates a token that's a
//! copy of it.
// End of the generated card header.

use crate::prelude::*;

/// Fractured Identity copies even an exiled token and excludes its last controller from
/// receiving a copy. [CR 608.2h] says “the effect uses the object's last known information”
/// when the original permanent has left the battlefield.
///
/// [CR 608.2h]: https://yawgatog.com/resources/magic-rules/#R6082h
pub static CARD: CardDef = Printed::FracturedIdentity
    .abilities(&[Ability::spell()
        .target(&TargetSpec::permanent(nonland()))
        .effect(&sequence(&[
            exile(target()),
            create_copy(each_player_other_than(&its_controller()), target()),
        ]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;
    use crate::tokens::SAMURAI_TOKEN;

    const IDENTITY: &str = Card::FracturedIdentity;

    const EMPEROR: &str = Card::TheWanderingEmperor;

    const JACE: &str = Card::JaceVrynsProdigy;

    /// Jace, Vryn's Prodigy's back face (CR 712.6d).
    const TELEPATH: &str = "Jace, Telepath Unbound";

    const FIVE_LANDS: &[&str] = &[
        Card::Plains,
        Card::Plains,
        Card::Island,
        Card::Island,
        Card::Island,
    ];

    /// Cast Fractured Identity on an opposing permanent with the named interaction cards.
    const fn fracture_with(
        name: &'static str,
        interaction: &'static [&'static str],
        target: &'static [Permanent],
        then: &'static [Action],
    ) -> Scenario {
        Scenario::new(name)
            .interaction(interaction)
            .battlefield(P0, FIVE_LANDS)
            .hand(P0, &[IDENTITY])
            .permanents(P1, target)
            .actions(then)
    }

    pub const SCENARIOS: &[Scenario] = &[
        fracture_with(
            "Fractured Identity on a double-faced card front face up",
            &[JACE],
            &[Permanent::new(JACE)],
            &[
                ME.casts(IDENTITY),
                Resolve,
                THEM.controls(JACE, 0),
                ME.controls(JACE, 1),
                ME.creature(JACE, true),
            ],
        ),
        // CR 707.8a: the copy of a transformed permanent enters with its back face up.
        fracture_with(
            "Fractured Identity on a transformed double-faced card",
            &[JACE],
            &[Permanent::new(JACE).transformed()],
            &[
                ME.casts(IDENTITY),
                Resolve,
                THEM.controls(TELEPATH, 0),
                ME.controls(JACE, 0),
                ME.controls(TELEPATH, 1),
                ME.has_loyalty(TELEPATH, 5),
            ],
        ),
        // P0 targets their own Samurai token: it is exiled and ceases to exist, and P1, the one
        // other player, gets the copy, a 2/2.
        Scenario::new("Fractured Identity on its caster's own token")
            .interaction(&[EMPEROR])
            .battlefield(
                P0,
                &[
                    EMPEROR,
                    Card::Plains,
                    Card::Plains,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .hand(P0, &[IDENTITY])
            .actions(&[
                ME.activates(EMPEROR, 2),
                Resolve,
                ME.controls(SAMURAI_TOKEN, 1),
                ME.casts(IDENTITY),
                ME.targets(SAMURAI_TOKEN),
                Resolve,
                ME.controls(SAMURAI_TOKEN, 0),
                ME.controls(EMPEROR, 1),
                THEM.controls(SAMURAI_TOKEN, 1),
                THEM.token_pt(SAMURAI_TOKEN, 2, 2),
            ]),
    ];
}
