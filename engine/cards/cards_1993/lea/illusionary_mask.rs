// Generated from the card store by `just card header`: notes go below the end marker.
//! Illusionary Mask {2}
//! Artifact
//!
//! {X}: You may choose a creature card in your hand whose mana cost could be paid by some amount
//! of, or all of, the mana you spent on {X}. If you do, you may cast that card face down as a 2/2
//! creature spell without paying its mana cost. If the creature that spell becomes as it resolves
//! has not been turned face up and would assign or deal damage, be dealt damage, or become tapped,
//! instead it's turned face up and assigns or deals damage, is dealt damage, or becomes tapped.
//! Activate only as a sorcery.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::IllusionaryMask
    .abilities(&[Ability::activated(Cost::mana(ManaCost::X))
        .effect(&may_cast_face_down_within_x(
            you(),
            cards_in(Zone::Hand, &you(), Filter::Types(Types::CREATURE)),
        ))
        .as_a_sorcery()])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const MASK: &str = Card::IllusionaryMask;
    /// A 2/10 for {2}{W}{W}: Lightning Bolt kills it face down and not face up.
    const ANCIENTS: &str = Card::IndomitableAncients;
    const BOLT: &str = Card::LightningBolt;
    const PLAINS: &str = Card::Plains;

    /// Illusionary Mask and four Plains, with the Ancients in hand.
    const fn table(name: &'static str) -> Scenario {
        Scenario::new(name)
            .battlefield(P0, &[MASK, PLAINS, PLAINS, PLAINS, PLAINS])
            .hand(P0, &[ANCIENTS])
    }

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Illusionary Mask casts a creature card face down for the mana spent on X")
            .battlefield(P0, &[MASK, PLAINS, PLAINS, PLAINS, PLAINS])
            .hand(P0, &[ANCIENTS])
            .actions(&[
                ME.activates(MASK, 0),
                ME.x(4),
                ME.tapped(PLAINS, 4),
                Resolve,
                ME.chooses(DecisionKind::CastOffer, ANCIENTS),
                stack(1),
                ME.colored(Zone::Stack, ANCIENTS, Colors::NONE),
                Resolve,
                ME.face_down(ANCIENTS, 1),
                ME.pt(ANCIENTS, 2, 2),
            ]),
        table("Illusionary Mask offers no card that less mana spent on X could not pay for")
            .actions(&[
                ME.activates(MASK, 0),
                ME.x(2),
                Resolve,
                ME.has(Zone::Hand, ANCIENTS, 1),
                stack(0),
            ]),
        Scenario::new("The kinds of mana spent on X count, not only how much")
            .battlefield(
                P0,
                &[MASK, Card::Island, Card::Island, Card::Island, Card::Island],
            )
            .hand(P0, &[ANCIENTS])
            .actions(&[
                ME.activates(MASK, 0),
                ME.x(4),
                Resolve,
                ME.has(Zone::Hand, ANCIENTS, 1),
                stack(0),
            ]),
        table("A creature cast with Illusionary Mask is turned face up as it is dealt damage")
            .interaction(&[BOLT])
            .hand(P1, &[BOLT])
            .battlefield(P1, &[Card::Mountain])
            .actions(&[
                ME.activates(MASK, 0),
                ME.x(4),
                Resolve,
                ME.chooses(DecisionKind::CastOffer, ANCIENTS),
                Resolve,
                ME.face_down(ANCIENTS, 1),
                ME.passes(),
                THEM.casts(BOLT),
                THEM.targets(ANCIENTS),
                ME.face_down(ANCIENTS, 0),
                ME.controls(ANCIENTS, 1),
                ME.pt(ANCIENTS, 2, 10),
            ]),
        table("A creature cast with Illusionary Mask is turned face up as it becomes tapped")
            .interaction(&[Card::FireIce])
            .hand(P1, &[Card::FireIce])
            .battlefield(P1, &[Card::Island, Card::Island])
            .actions(&[
                ME.activates(MASK, 0),
                ME.x(4),
                Resolve,
                ME.chooses(DecisionKind::CastOffer, ANCIENTS),
                Resolve,
                ME.face_down(ANCIENTS, 1),
                ME.passes(),
                THEM.casts_face(Card::FireIce, FaceIx::BACK),
                THEM.targets(ANCIENTS),
                ME.face_down(ANCIENTS, 0),
                ME.tapped(ANCIENTS, 1),
                ME.pt(ANCIENTS, 2, 10),
            ]),
        table("A creature cast with Illusionary Mask stays turned up by it after the Mask is gone")
            .interaction(&[BOLT, Card::WearTear])
            .hand(P1, &[Card::WearTear, BOLT])
            .battlefield(P1, &[Card::Mountain, Card::Mountain, Card::Mountain])
            .actions(&[
                ME.activates(MASK, 0),
                ME.x(4),
                Resolve,
                ME.chooses(DecisionKind::CastOffer, ANCIENTS),
                Resolve,
                ME.face_down(ANCIENTS, 1),
                ME.passes(),
                THEM.casts(Card::WearTear),
                Resolve,
                ME.has(Zone::Graveyard, MASK, 1),
                ME.face_down(ANCIENTS, 1),
                ME.passes(),
                THEM.casts(BOLT),
                THEM.targets(ANCIENTS),
                ME.face_down(ANCIENTS, 0),
                ME.pt(ANCIENTS, 2, 10),
            ]),
        table("A face-down creature from Illusionary Mask returns to its owner's hand face up")
            .hand(P1, &[Prop::Removal])
            .battlefield(P1, &[Card::Island, Card::Island, Card::Island])
            .actions(&[
                ME.activates(MASK, 0),
                ME.x(4),
                Resolve,
                ME.chooses(DecisionKind::CastOffer, ANCIENTS),
                Resolve,
                ME.face_down(ANCIENTS, 1),
                ME.passes(),
                THEM.casts(Prop::Removal),
                THEM.targets(ANCIENTS),
                ME.has(Zone::Hand, ANCIENTS, 1),
                ME.controls(ANCIENTS, 0),
            ]),
        table("A creature cast with Illusionary Mask is turned face up as it would assign damage")
            .battlefield(P1, &[Prop::Vanilla])
            .actions(&[
                ME.activates(MASK, 0),
                ME.x(4),
                Resolve,
                ME.chooses(DecisionKind::CastOffer, ANCIENTS),
                Resolve,
                ME.face_down(ANCIENTS, 1),
                advance_to(2, TurnStep::BeginningOfCombat),
                THEM.passes(),
                THEM.attacks_with(Prop::Vanilla),
                ME.chooses(DecisionKind::Blocker, ANCIENTS),
                advance_to(2, TurnStep::EndOfCombat),
                ME.face_down(ANCIENTS, 0),
                ME.pt(ANCIENTS, 2, 10),
                THEM.has(Zone::Graveyard, Prop::Vanilla, 1),
            ]),
        table("A creature cast with Illusionary Mask is turned face up as it attacks").actions(&[
            ME.activates(MASK, 0),
            ME.x(4),
            Resolve,
            ME.chooses(DecisionKind::CastOffer, ANCIENTS),
            Resolve,
            ME.face_down(ANCIENTS, 1),
            advance_to(3, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(ANCIENTS),
            ME.face_down(ANCIENTS, 0),
            ME.pt(ANCIENTS, 2, 10),
        ]),
        Scenario::new("A creature cast with Illusionary Mask enters and turns face up untriggered")
            .interaction(&[BOLT, Card::BloodTollHarpy])
            .battlefield(P0, &[MASK, Card::Swamp, Card::Swamp, Card::Swamp])
            .hand(P0, &[Card::BloodTollHarpy])
            .hand(P1, &[BOLT])
            .battlefield(P1, &[Card::Mountain])
            .actions(&[
                ME.activates(MASK, 0),
                ME.x(3),
                Resolve,
                ME.chooses(DecisionKind::CastOffer, Card::BloodTollHarpy),
                Resolve,
                ME.face_down(Card::BloodTollHarpy, 1),
                stack(0),
                ME.passes(),
                THEM.casts(BOLT),
                THEM.targets(Card::BloodTollHarpy),
                ME.has(Zone::Graveyard, Card::BloodTollHarpy, 1),
                stack(0),
                ME.life(20),
                THEM.life(20),
            ]),
        Scenario::new("A face-down spell cast with Illusionary Mask can be countered")
            .battlefield(P0, &[MASK, PLAINS, PLAINS, PLAINS, PLAINS])
            .hand(P0, &[ANCIENTS])
            .hand(P1, &[Prop::Instant])
            .battlefield(P1, &[Card::Island, Card::Island])
            .actions(&[
                ME.activates(MASK, 0),
                ME.x(4),
                Resolve,
                ME.chooses(DecisionKind::CastOffer, ANCIENTS),
                stack(1),
                THEM.casts(Prop::Instant),
                ME.has(Zone::Graveyard, ANCIENTS, 1),
                ME.controls(ANCIENTS, 0),
            ]),
        Scenario::new(
            "A creature with morph cast with Illusionary Mask turns up for its morph cost",
        )
        .interaction(&[Card::ExaltedAngel])
        .battlefield(
            P0,
            &[
                MASK, PLAINS, PLAINS, PLAINS, PLAINS, PLAINS, PLAINS, PLAINS, PLAINS, PLAINS,
                PLAINS,
            ],
        )
        .hand(P0, &[Card::ExaltedAngel])
        .actions(&[
            ME.activates(MASK, 0),
            ME.x(6),
            Resolve,
            ME.chooses(DecisionKind::CastOffer, Card::ExaltedAngel),
            Resolve,
            ME.face_down(Card::ExaltedAngel, 1),
            Action::TurnFaceUp {
                player: P0,
                card: Card::ExaltedAngel,
            },
            ME.face_down(Card::ExaltedAngel, 0),
            ME.pt(Card::ExaltedAngel, 4, 5),
            ME.tapped(PLAINS, 10),
        ]),
    ];
}
