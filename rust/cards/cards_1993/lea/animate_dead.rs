// Generated from the card store by `just card header`: notes go below the end marker.
//! Animate Dead {1}{B}
//! Enchantment — Aura
//!
//! Enchant creature card in a graveyard
//! When this Aura enters, if it's on the battlefield, it loses "enchant creature card in a
//! graveyard" and gains "enchant creature put onto the battlefield with this Aura." Return
//! enchanted creature card to the battlefield under your control and attach this Aura to it. When
//! this Aura leaves the battlefield, that creature's controller sacrifices it.
//! Enchanted creature gets -1/-0.
// End of the generated card header.

use crate::prelude::*;

/// "Enchant creature card in a graveyard": the Aura spell targets a creature card in any
/// graveyard ([CR 303.4a]): "An Aura spell requires a target, which is defined by its enchant
/// ability." As it resolves it enters attached to that card ([CR 608.3c]).
///
/// The enters ability has an intervening "if" ([CR 603.4]): it does nothing unless Animate
/// Dead is still the object that entered. Then Animate Dead loses its first enchant ability
/// and gains the second, an effect of no duration in layer 6 ([CR 613.1f]); it returns the
/// card under its controller's control, as a new object ([CR 400.7]) that remembers it was put
/// onto the battlefield with this Aura; and it attaches itself to that creature. A creature
/// that left the graveyard first is not returned, and Animate Dead, attached to nothing, goes
/// to its owner's graveyard as a state-based action ([CR 704.5m]). So does an Animate Dead
/// that can't legally enchant the returned creature, as one with protection from black.
///
/// "When this Aura leaves the battlefield, that creature's controller sacrifices it" is part of
/// the enters ability's effect, so the effect creates it as a delayed triggered ability
/// ([CR 603.7a]): "Delayed triggered abilities are created during the resolution of spells or
/// abilities". It carries the creature put onto the battlefield with this Aura, and triggers
/// the next time this Aura leaves the battlefield. An Animate Dead that left before its enters
/// ability resolved never creates it. The creature's controller sacrifices it if it is still
/// that object ([CR 603.7c]).
///
/// "Enchanted creature gets -1/-0" is a static ability applied in layer 7c ([CR 613.4c]).
///
/// [CR 303.4a]: https://yawgatog.com/resources/magic-rules/#R3034a
/// [CR 608.3c]: https://yawgatog.com/resources/magic-rules/#R6083c
/// [CR 603.4]: https://yawgatog.com/resources/magic-rules/#R6034
/// [CR 613.1f]: https://yawgatog.com/resources/magic-rules/#R6131f
/// [CR 400.7]: https://yawgatog.com/resources/magic-rules/#R4007
/// [CR 704.5m]: https://yawgatog.com/resources/magic-rules/#R7045m
/// [CR 603.7a]: https://yawgatog.com/resources/magic-rules/#R6037a
/// [CR 603.7c]: https://yawgatog.com/resources/magic-rules/#R6037c
/// [CR 613.4c]: https://yawgatog.com/resources/magic-rules/#R6134c
pub static CARD: CardDef = Printed::AnimateDead
    .abilities(&[
        Ability::spell().target(&TargetSpec::graveyard_card(Filter::Types(Types::CREATURE))),
        Ability::triggered(&Trigger::ENTERS)
            .only_if(&Condition::OnBattlefield(Objects::This))
            .effect(&sequence(&[
                modify(
                    this(),
                    &[Modification::lose(0), Modification::grant(0)],
                    Duration::Indefinite,
                ),
                put_onto_battlefield(&PutOnto {
                    linked: true,
                    ..onto_battlefield_under(Objects::AttachedTo, Players::You)
                }),
                attach(this(), Objects::Those),
                delay_with(3, Objects::PutOntoBattlefieldWithThis),
            ])),
        Ability::static_ability(&LayerSpec {
            affected: Objects::AttachedTo,
            condition: None,
            modifications: &[Modification::ModifyPt(-1, 0)],
        }),
        Ability::triggered(&Trigger::LEAVES.next()).effect(&sacrifice(carried_object())),
    ])
    .grants(&[Ability::spell().target(&TargetSpec::creature().put_with_this())])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const ANIMATE: &str = Card::AnimateDead;

    /// A Swamp and a Plains for Animate Dead, and P1's creature card in their graveyard.
    const SETUP: Scenario = Scenario::setup()
        .hand(P0, &[ANIMATE])
        .battlefield(P0, &[Card::Swamp, Card::Plains])
        .graveyard(P1, &[Prop::Vanilla])
        .library(P0, &[Card::Plains])
        .library(P1, &[Card::Plains]);

    /// [`SETUP`] with two more Plains and Get Lost, to destroy Animate Dead with.
    const GET_LOST_SETUP: Scenario = SETUP
        .hand(P0, &[ANIMATE, Card::GetLost])
        .battlefield(P0, &[Card::Swamp, Card::Plains, Card::Plains, Card::Plains]);

    /// [`SETUP`] with Flickerwisp and three more Plains to cast it.
    const FLICKER_SETUP: Scenario = SETUP.hand(P0, &[ANIMATE, Card::Flickerwisp]).battlefield(
        P0,
        &[
            Card::Swamp,
            Card::Plains,
            Card::Plains,
            Card::Plains,
            Card::Plains,
        ],
    );

    pub const SCENARIOS: &[Scenario] = &[
        SETUP
            .titled("Animate Dead returns an opponent's creature card under your control")
            .actions(&[
                ME.casts(ANIMATE),
                Resolve,
                Resolve,
                ME.controls(Prop::Vanilla, 1),
                THEM.has(Zone::Graveyard, Prop::Vanilla, 0),
                ME.controls(ANIMATE, 1),
                // Enchanted creature gets -1/-0.
                ME.pt(Prop::Vanilla, 1, 1),
            ]),
        // The delayed ability makes the creature's controller sacrifice it, and it goes to its
        // owner's graveyard.
        GET_LOST_SETUP
            .titled("Animate Dead destroyed: the creature is sacrificed")
            .interaction(&[Card::GetLost])
            .actions(&[
                ME.casts(ANIMATE),
                Resolve,
                Resolve,
                ME.controls(Prop::Vanilla, 1),
                ME.casts(Card::GetLost),
                ME.targets(ANIMATE),
                Resolve,
                Resolve,
                ME.has(Zone::Graveyard, ANIMATE, 1),
                ME.controls(Prop::Vanilla, 0),
                THEM.has(Zone::Graveyard, Prop::Vanilla, 1),
            ]),
        // Animate Dead leaves before its enters ability resolves: the ability does nothing,
        // the card stays in the graveyard, and no delayed ability was created to trigger.
        GET_LOST_SETUP
            .titled("Animate Dead destroyed before its enters ability resolves")
            .interaction(&[Card::GetLost])
            .actions(&[
                ME.casts(ANIMATE),
                Resolve,
                ME.casts(Card::GetLost),
                Resolve,
                // The enters ability, which creates no delayed ability.
                Resolve,
                ME.has(Zone::Graveyard, ANIMATE, 1),
                ME.controls(Prop::Vanilla, 0),
                THEM.has(Zone::Graveyard, Prop::Vanilla, 1),
            ]),
        // Flickerwisp exiles the returned creature. Animate Dead is attached to nothing and
        // goes to the graveyard; its delayed ability finds no creature to sacrifice, and the
        // creature comes back at the end step under its owner's control, without the -1/-0.
        FLICKER_SETUP
            .titled("The creature flickered: Animate Dead falls off")
            .interaction(&[Card::Flickerwisp])
            .actions(&[
                ME.casts(ANIMATE),
                Resolve,
                Resolve,
                ME.casts(Card::Flickerwisp),
                Resolve,
                ME.targets(Prop::Vanilla),
                Resolve,
                Resolve,
                ME.has(Zone::Graveyard, ANIMATE, 1),
                THEM.has(Zone::Exile, Prop::Vanilla, 1),
                advance_to(1, TurnStep::End),
                Resolve,
                THEM.controls(Prop::Vanilla, 1),
                THEM.pt(Prop::Vanilla, 2, 1),
            ]),
        // The creature card would enter without being cast, so Containment Priest exiles it
        // instead, and Animate Dead, attached to nothing, goes to the graveyard. The delayed
        // ability the enters ability created triggers, and has nothing to sacrifice.
        SETUP
            .titled("Animate Dead under Containment Priest")
            .interaction(&[Card::ContainmentPriest])
            .battlefield(P1, &[Card::ContainmentPriest])
            .actions(&[
                ME.casts(ANIMATE),
                Resolve,
                Resolve,
                Resolve,
                THEM.has(Zone::Exile, Prop::Vanilla, 1),
                ME.controls(Prop::Vanilla, 0),
                ME.has(Zone::Graveyard, ANIMATE, 1),
            ]),
        // The card leaves the graveyard while the enters ability waits: Animate Dead is
        // attached to nothing and goes to the graveyard, and the ability does nothing.
        SETUP
            .titled("The creature card exiled in response to the enters ability")
            .interaction(&[Card::TormodsCrypt])
            .battlefield(P0, &[Card::Swamp, Card::Plains, Card::TormodsCrypt])
            .actions(&[
                ME.casts(ANIMATE),
                Resolve,
                ME.activates(Card::TormodsCrypt, 0),
                ME.targets_player(THEM),
                Resolve,
                ME.has(Zone::Graveyard, ANIMATE, 1),
                Resolve,
                THEM.has(Zone::Exile, Prop::Vanilla, 1),
                ME.controls(Prop::Vanilla, 0),
            ]),
        // Flickerwisp exiles Animate Dead: its delayed ability makes P0 sacrifice the
        // creature. At the end step Animate Dead returns without being cast, so P0 chooses
        // what it enchants (CR 303.4f), here the creature card back in P1's graveyard over
        // the one in P0's, and its enters ability returns that card again. It is not a
        // target, so no spell or ability targets it.
        FLICKER_SETUP
            .titled("Animate Dead flickered enchants a creature card as it returns")
            .interaction(&[Card::Flickerwisp])
            .graveyard(P0, &[Prop::Flier])
            .actions(&[
                ME.casts(ANIMATE),
                ME.targets(Prop::Vanilla),
                Resolve,
                Resolve,
                ME.casts(Card::Flickerwisp),
                Resolve,
                ME.targets(ANIMATE),
                Resolve,
                Resolve,
                ME.has(Zone::Exile, ANIMATE, 1),
                THEM.has(Zone::Graveyard, Prop::Vanilla, 1),
                advance_to(1, TurnStep::End),
                Resolve,
                ME.chooses(DecisionKind::ChooseObject, Prop::Vanilla),
                Resolve,
                ME.has(Zone::Graveyard, Prop::Flier, 1),
                ME.controls(ANIMATE, 1),
                ME.controls(Prop::Vanilla, 1),
                ME.pt(Prop::Vanilla, 1, 1),
            ]),
        // As above, but Tormod's Crypt empties P1's graveyard first: with no creature card
        // in a graveyard to enchant, Animate Dead stays in exile (CR 303.4g).
        FLICKER_SETUP
            .titled("Animate Dead with nothing to enchant stays in exile")
            .interaction(&[Card::Flickerwisp, Card::TormodsCrypt])
            .battlefield(
                P0,
                &[
                    Card::Swamp,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::TormodsCrypt,
                ],
            )
            .actions(&[
                ME.casts(ANIMATE),
                Resolve,
                Resolve,
                ME.casts(Card::Flickerwisp),
                Resolve,
                ME.targets(ANIMATE),
                Resolve,
                Resolve,
                ME.activates(Card::TormodsCrypt, 0),
                ME.targets_player(THEM),
                Resolve,
                THEM.has(Zone::Exile, Prop::Vanilla, 1),
                advance_to(1, TurnStep::End),
                Resolve,
                ME.has(Zone::Exile, ANIMATE, 1),
                ME.controls(ANIMATE, 0),
            ]),
    ];
}
