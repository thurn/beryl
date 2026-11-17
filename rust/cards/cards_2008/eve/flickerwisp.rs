// Generated from the card store by `just card header`: notes go below the end marker.
//! Flickerwisp {1}{W}{W}
//! Creature — Elemental, 3/1
//!
//! Flying
//! When this creature enters, exile another target permanent. Return that card to the battlefield
//! under its owner's control at the beginning of the next end step.
// End of the generated card header.

use crate::prelude::*;

/// "Exile another target permanent" is the enters ability, and the card it exiles is a new object
/// ([CR 400.7]): "An object that moves from one zone to another becomes a new object with no
/// memory of, or relation to, its previous existence." The second sentence creates a delayed
/// triggered ability ([CR 603.7a]): "Delayed triggered abilities are created during the resolution
/// of spells or abilities". It carries that card ([CR 603.7c]): "A delayed triggered ability that
/// refers to a particular object still affects it even if the object changes characteristics.
/// However, if that object is no longer in the zone it's expected to be in at the time the delayed
/// triggered ability resolves, the ability won't affect it." A token that was exiled has ceased to
/// exist ([CR 111.7]), so nothing returns. The card comes back under its owner's control
/// ([CR 110.2a]), even if the Flickerwisp has left.
///
/// The delayed ability is the face's ability 1.
///
/// [CR 400.7]: https://yawgatog.com/resources/magic-rules/#R4007
/// [CR 603.7a]: https://yawgatog.com/resources/magic-rules/#R6037a
/// [CR 603.7c]: https://yawgatog.com/resources/magic-rules/#R6037c
/// [CR 111.7]: https://yawgatog.com/resources/magic-rules/#R1117
/// [CR 110.2a]: https://yawgatog.com/resources/magic-rules/#R1102a
pub static CARD: CardDef = Printed::Flickerwisp
    .keywords(Keywords::FLYING)
    .abilities(&[
        Ability::triggered(&Trigger::ENTERS)
            .target(&TargetSpec::permanent(Filter::Any).other())
            .effect(&sequence(&[exile(target()), delay_with(1, those())])),
        Ability::triggered(&Trigger::step(TurnStep::End, Players::EachPlayer).next())
            .effect(&return_to_battlefield(carried_object())),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const SPIRIT_TOKEN: &str = "Spirit Token";
    const WISP: &str = Card::Flickerwisp;

    pub const SCENARIOS: &[Scenario] = &[
        // P1's tapped Island is exiled and comes back at this turn's end step untapped.
        Scenario::new("Flickerwisp returns an exiled tapped land untapped at the end step")
            .hand(P0, &[WISP])
            .battlefield(P0, &[Card::Plains, Card::Plains, Card::Plains])
            .permanents(P1, &[Permanent::new(Card::Island).tapped()])
            .library(P0, &[Card::Plains])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.casts(WISP),
                Resolve,
                ME.targets(Card::Island),
                Resolve,
                THEM.has(Zone::Exile, Card::Island, 1),
                THEM.controls(Card::Island, 0),
                advance_to(1, TurnStep::End),
                Resolve,
                THEM.has(Zone::Exile, Card::Island, 0),
                THEM.controls(Card::Island, 1),
                THEM.tapped(Card::Island, 0),
            ]),
        // P0 steals P1's Equipment with Thieving Skydiver, then flickers it: it returns under
        // P1's control, its owner.
        Scenario::new("Flickerwisp returns a stolen permanent to its owner")
            .hand(P0, &[Card::ThievingSkydiver, WISP])
            .battlefield(
                P0,
                &[
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                ],
            )
            .battlefield(P1, &[Prop::Equipment])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Plains])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: Card::ThievingSkydiver,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                ME.x(1),
                Resolve,
                Resolve,
                ME.controls(Prop::Equipment, 1),
                ME.casts(WISP),
                Resolve,
                ME.targets(Prop::Equipment),
                Resolve,
                ME.controls(Prop::Equipment, 0),
                THEM.controls(Prop::Equipment, 0),
                advance_to(1, TurnStep::End),
                Resolve,
                THEM.controls(Prop::Equipment, 1),
                ME.controls(Prop::Equipment, 0),
            ]),
        // A token that is exiled ceases to exist and does not return. P0's Staff of the
        // Storyteller makes the Spirit token, and the Flickerwisp exiles it.
        Scenario::new("Flickerwisp exiles a token, which does not return")
            .interaction(&[Card::StaffOfTheStoryteller])
            .hand(P0, &[Card::StaffOfTheStoryteller, WISP])
            .battlefield(P0, &[Card::Plains; 5])
            .library(P0, &[Card::Plains])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.casts(Card::StaffOfTheStoryteller),
                Resolve,
                Resolve,
                Resolve,
                ME.controls(SPIRIT_TOKEN, 1),
                ME.casts(WISP),
                Resolve,
                ME.targets(SPIRIT_TOKEN),
                Resolve,
                ME.controls(SPIRIT_TOKEN, 0),
                advance_to(1, TurnStep::End),
                Resolve,
                ME.controls(SPIRIT_TOKEN, 0),
                ME.has(Zone::Exile, SPIRIT_TOKEN, 0),
            ]),
        // The exiled creature loses its counter and its attachments. The Aura goes to the
        // graveyard, and the Equipment stays on the battlefield without giving its bonus.
        Scenario::new("Flickerwisp returns a creature without its counter or attachments")
            .interaction(&[Card::HolyStrength, Card::LeoninScimitar])
            .hand(P0, &[WISP, Card::HolyStrength])
            .permanents(
                P0,
                &[
                    Permanent::new(Prop::Vanilla).counters(CounterKind::PlusOne, 1),
                    Permanent::new(Card::LeoninScimitar),
                    Permanent::new(Card::Plains),
                    Permanent::new(Card::Plains),
                    Permanent::new(Card::Plains),
                    Permanent::new(Card::Plains),
                    Permanent::new(Card::Plains),
                ],
            )
            .actions(&[
                ME.casts(Card::HolyStrength),
                Resolve,
                ME.activates(Card::LeoninScimitar, 1),
                Resolve,
                ME.pt(Prop::Vanilla, 5, 5),
                ME.casts(WISP),
                Resolve,
                ME.targets(Prop::Vanilla),
                Resolve,
                ME.has(Zone::Graveyard, Card::HolyStrength, 1),
                ME.controls(Card::LeoninScimitar, 1),
                advance_to(1, TurnStep::End),
                Resolve,
                ME.pt(Prop::Vanilla, 2, 1),
                ME.counters(Prop::Vanilla, CounterKind::PlusOne, 0),
            ]),
        // Underworld Breach enters after the end step starts, so its end step sacrifice never
        // triggers that turn. The delayed return also survives the loss of Flickerwisp.
        Scenario::new(
            "Underworld Breach returns after Flickerwisp dies and misses the end step trigger",
        )
        .interaction(&[Card::UnderworldBreach, Card::LightningBolt])
        .hand(P0, &[WISP, Card::LightningBolt])
        .battlefield(
            P0,
            &[Card::Plains, Card::Plains, Card::Plains, Card::Mountain],
        )
        .battlefield(P1, &[Card::UnderworldBreach])
        .library(P1, &[Card::Island; 2])
        .actions(&[
            ME.casts(WISP),
            Resolve,
            ME.targets(Card::UnderworldBreach),
            Resolve,
            ME.casts(Card::LightningBolt),
            ME.targets(WISP),
            Resolve,
            ME.has(Zone::Graveyard, WISP, 1),
            advance_to(1, TurnStep::End),
            Resolve,
            stack(0),
            THEM.controls(Card::UnderworldBreach, 1),
        ]),
    ];
}
