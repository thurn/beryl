// Generated from the card store by `just card header`: notes go below the end marker.
//! Fractured Identity {3}{W}{U}
//! Sorcery
//!
//! Exile target nonland permanent. Each player other than its controller creates a token that's a
//! copy of it.
// End of the generated card header.

use crate::prelude::*;

/// "Its controller" and "it" are the permanent as it last existed on the battlefield ([CR 608.2h]):
/// "If an effect requires information from the game (such as the number of creatures on the
/// battlefield), the answer is determined only once, when the effect is applied. If the effect
/// requires information from a specific object, including the source of the ability itself, the
/// effect uses the current information of that object if it's in the public zone it was expected to
/// be in; if it's no longer in that zone, ... the effect uses the object's last known information."
/// So an exiled token, which ceases to exist, is copied all the same (CR 111.12). Each copy has the
/// permanent's copiable values (CR 707.2): not its counters, so a planeswalker's copy enters with
/// its printed loyalty (CR 306.5b).
///
/// [CR 111.2]: "The player who creates a token is its owner. The token enters the battlefield under
/// that player's control."
///
/// [CR 608.2h]: https://yawgatog.com/resources/magic-rules/#R6082h
/// [CR 111.2]: https://yawgatog.com/resources/magic-rules/#R1112
pub static CARD: CardDef = Printed::FracturedIdentity
    .abilities(&[Ability::spell()
        .target(&TargetSpec::permanent(Filter::Not(&Filter::Types(
            Types::LAND,
        ))))
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

    const PLANESWALKER: &str = Prop::Planeswalker;

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

    /// P0 casts Fractured Identity on P1's one nonland permanent, `target`; P0 gets a copy of it,
    /// which `then` checks.
    const fn fracture_of(
        name: &'static str,
        target: &'static [Permanent],
        then: &'static [Action],
    ) -> Scenario {
        fracture_with(name, &[], target, then)
    }

    /// [`fracture_of`] for a scenario about Fractured Identity and the cards `interaction`.
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
        // Counters are not copied: P1's planeswalker at 6 loyalty becomes P0's at its printed 3.
        fracture_of(
            "Fractured Identity on a planeswalker: the copy has its printed loyalty",
            &[Permanent::new(PLANESWALKER).counters(CounterKind::Loyalty, 6)],
            &[
                ME.casts(IDENTITY),
                Resolve,
                THEM.has(Zone::Exile, PLANESWALKER, 1),
                THEM.controls(PLANESWALKER, 0),
                ME.controls(PLANESWALKER, 1),
                ME.has_loyalty(PLANESWALKER, 3),
            ],
        ),
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
