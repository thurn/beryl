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

/// The enters trigger's intervening "if" checks whether this Aura is still on the battlefield
/// when it resolves ([CR 603.4]): "If the condition isn’t true at that time, the ability is
/// removed from the stack and does nothing." If the Aura has left, the trigger neither
/// returns the creature nor creates the delayed sacrifice trigger.
///
/// The returned creature is a new object ([CR 400.7]): "An object that moves from one zone
/// to another becomes a new object". The replacement enchant ability and the delayed
/// trigger both refer to that new object. A creature Animate Dead returns with protection
/// from black stays on the battlefield until the delayed trigger resolves. Animate Dead
/// goes to the graveyard unattached ([CR 704.5m]): "If an Aura is attached to an illegal
/// object or player, or is not attached to an object or player, that Aura is put into its
/// owner’s graveyard."
///
/// [CR 603.4]: https://yawgatog.com/resources/magic-rules/#R6034
/// [CR 400.7]: https://yawgatog.com/resources/magic-rules/#R4007
/// [CR 704.5m]: https://yawgatog.com/resources/magic-rules/#R7045m
pub static CARD: CardDef = Printed::AnimateDead
    .abilities(&[
        Ability::spell().target(&TargetSpec::graveyard_card(Filter::Types(Types::CREATURE))),
        when_enters(&sequence(&[
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
        ]))
        .only_if(&Condition::OnBattlefield(Objects::This)),
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
        // The only target leaves in response, so the Aura spell has no legal target and
        // doesn't resolve (CR 608.2b); as a spell that didn't resolve it goes to its owner's
        // graveyard, never entering the battlefield, so its enters ability never exists.
        SETUP
            .titled("Animate Dead fizzles when the creature card is exiled in response")
            .interaction(&[Card::TormodsCrypt])
            .battlefield(P0, &[Card::Swamp, Card::Plains, Card::TormodsCrypt])
            .actions(&[
                ME.casts(ANIMATE),
                ME.activates(Card::TormodsCrypt, 0),
                ME.targets_player(THEM),
                Resolve,
                Resolve,
                ME.has(Zone::Graveyard, ANIMATE, 1),
                ME.controls(ANIMATE, 0),
                THEM.has(Zone::Exile, Prop::Vanilla, 1),
                ME.controls(Prop::Vanilla, 0),
            ]),
        // The creature dies: Animate Dead is attached to nothing and goes to the graveyard as a
        // state-based action (CR 704.5m). The creature goes to its owner's graveyard.
        GET_LOST_SETUP
            .titled("The Animate Dead creature destroyed: Animate Dead goes to the graveyard")
            .interaction(&[Card::GetLost])
            .actions(&[
                ME.casts(ANIMATE),
                Resolve,
                Resolve,
                ME.casts(Card::GetLost),
                ME.targets(Prop::Vanilla),
                Resolve,
                ME.has(Zone::Graveyard, ANIMATE, 1),
                ME.controls(ANIMATE, 0),
                THEM.has(Zone::Graveyard, Prop::Vanilla, 1),
            ]),
        // Rest in Peace exiles each card that would go to a graveyard (CR 614.1): Animate
        // Dead destroyed is exiled, and the creature it sacrifices is exiled with it.
        GET_LOST_SETUP
            .titled("Animate Dead destroyed under Rest in Peace exiles both")
            .interaction(&[Card::GetLost, Card::RestInPeace])
            .battlefield(P1, &[Card::RestInPeace])
            .actions(&[
                ME.casts(ANIMATE),
                Resolve,
                Resolve,
                ME.controls(Prop::Vanilla, 1),
                ME.casts(Card::GetLost),
                ME.targets(ANIMATE),
                Resolve,
                Resolve,
                ME.has(Zone::Exile, ANIMATE, 1),
                THEM.has(Zone::Exile, Prop::Vanilla, 1),
                ME.controls(Prop::Vanilla, 0),
            ]),
        // The sacrifice trigger is a triggered ability on the stack, and Stifle counters it:
        // the creature stays under P0's control, without the -1/-0 of the Aura now in the
        // graveyard.
        GET_LOST_SETUP
            .titled("Stifle on the sacrifice trigger leaves the creature")
            .interaction(&[Card::GetLost, Card::Stifle])
            .battlefield(P1, &[Card::Island])
            .hand(P1, &[Card::Stifle])
            .actions(&[
                ME.casts(ANIMATE),
                Resolve,
                Resolve,
                ME.casts(Card::GetLost),
                ME.targets(ANIMATE),
                Resolve,
                ME.passes(),
                THEM.casts(Card::Stifle),
                stack(0),
                ME.has(Zone::Graveyard, ANIMATE, 1),
                ME.controls(Prop::Vanilla, 1),
                ME.pt(Prop::Vanilla, 2, 1),
            ]),
        // Stifle on the enters trigger: the Aura entered attached to the creature card in the
        // graveyard (CR 303.4a), which is still a legal object for "enchant creature card in a
        // graveyard", so it stays on the battlefield (CR 704.5m doesn't apply) and the card
        // is not returned.
        SETUP
            .titled("Stifle on the enters trigger leaves the creature card")
            .interaction(&[Card::Stifle])
            .battlefield(P1, &[Card::Island])
            .hand(P1, &[Card::Stifle])
            .actions(&[
                ME.casts(ANIMATE),
                Resolve,
                ME.passes(),
                THEM.casts(Card::Stifle),
                stack(0),
                ME.controls(ANIMATE, 1),
                THEM.has(Zone::Graveyard, Prop::Vanilla, 1),
                ME.controls(Prop::Vanilla, 0),
            ]),
        // Protection is inactive in the graveyard. On the battlefield it prevents attachment,
        // so the Aura falls off and its delayed trigger makes the Knight's controller sacrifice it.
        SETUP
            .titled("Animate Dead can't attach to a creature with protection from black")
            .interaction(&[Card::WhiteKnight])
            .graveyard(P1, &[Card::WhiteKnight])
            .actions(&[
                ME.casts(ANIMATE),
                Resolve,
                ME.controls(ANIMATE, 1),
                Resolve,
                ME.controls(Card::WhiteKnight, 1),
                ME.has(Zone::Graveyard, ANIMATE, 1),
                Resolve,
                ME.controls(Card::WhiteKnight, 0),
                THEM.has(Zone::Graveyard, Card::WhiteKnight, 1),
            ]),
        // A spell can target the Aura and another creature, but the new enchant ability only
        // permits attachment to the creature this Aura returned. The card draw still happens.
        SETUP
            .titled("Aura Finesse can't move Animate Dead to another creature")
            .interaction(&[Card::AuraFinesse])
            .hand(P0, &[ANIMATE, Card::AuraFinesse])
            .battlefield(P0, &[Card::Swamp, Card::Plains, Card::Island])
            .battlefield(P1, &[Prop::Flier])
            .actions(&[
                ME.casts(ANIMATE),
                Resolve,
                Resolve,
                ME.casts(Card::AuraFinesse),
                ME.targets(Prop::Flier),
                Resolve,
                ME.controls(ANIMATE, 1),
                ME.pt(Prop::Vanilla, 1, 1),
                THEM.pt(Prop::Flier, 3, 1),
                ME.has(Zone::Hand, Card::Plains, 1),
            ]),
    ];
}
