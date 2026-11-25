// Generated from the card store by `just card header`: notes go below the end marker.
//! Phantasmal Image {1}{U}
//! Creature — Illusion, 0/0
//!
//! You may have this creature enter as a copy of any creature on the battlefield, except it's an
//! Illusion in addition to its other types and it has "When this creature becomes the target of a
//! spell or ability, sacrifice it."
// End of the generated card header.

use crate::prelude::*;

/// Its Illusion type and sacrifice ability are copiable ([CR 707.9a]). The definition
/// grants the ability through the copy exception so an Image that enters as itself does
/// not have it.
///
/// [CR 707.9a]: https://yawgatog.com/resources/magic-rules/#R7079a
pub static CARD: CardDef = Printed::PhantasmalImage
    .abilities(&[Ability::replacing(&Replacement::enter_as_copy(
        &creature(),
        Some(ExceptIx::new(0)),
    ))])
    .grants(
        &[Ability::triggered(&Trigger::BECOMES_TARGET).effect(&sacrifice(Objects::TriggeredOn))],
    )
    .excepts(&[CopyException(&[
        CopyChange::AddSubtype(Subtype::Illusion),
        CopyChange::Has(GrantIx::new(0)),
    ])])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const IMAGE: &str = Card::PhantasmalImage;
    const BEZA: &str = Card::BezaTheBoundingSpring;
    const MOTHER: &str = Card::MotherOfRunes;
    const HOLY: &str = Card::HolyStrength;
    const HOLE: &str = Card::PortableHole;
    const GARGANTUAN: &str = Card::QuicksilverGargantuan;

    /// P1 controls Beza, the Bounding Spring; P0, at 10 life against P1's 20, holds
    /// Phantasmal Image and the Islands to cast it. Beza's enters ability gains its controller
    /// 4 life when an opponent has more; its other parts find nothing that P1 has more of.
    const fn beza(name: &'static str, images: &'static [&'static str]) -> Scenario {
        Scenario::new(name)
            .interaction(&[BEZA])
            .life(P0, 10)
            .battlefield(P0, &[Card::Island; 4])
            .hand(P0, images)
            .battlefield(P1, &[BEZA, Card::Plains, Card::Plains, Card::Plains])
    }

    /// Seven Islands for Quicksilver Gargantuan and a Plains for Portable Hole.
    const P1_LANDS: &[&str] = &[
        Card::Island,
        Card::Island,
        Card::Island,
        Card::Island,
        Card::Island,
        Card::Island,
        Card::Island,
        Card::Plains,
    ];

    pub const SCENARIOS: &[Scenario] = &[
        // Glorious Anthem keeps the original 0/0 Image alive. When the Dragon returns it and
        // the Sambar together, only the opponent's flier is already on the battlefield.
        Scenario::new("a returning Image can only copy the creature already there")
            .interaction(&[Card::WorldgorgerDragon, Card::GloriousAnthem, Card::GetLost])
            .hand(P0, &[Card::WorldgorgerDragon])
            .battlefield(
                P0,
                &[
                    IMAGE,
                    Prop::Vanilla,
                    Card::GloriousAnthem,
                    Card::Mountain,
                    Card::Mountain,
                    Card::Mountain,
                    Card::Mountain,
                    Card::Mountain,
                    Card::Mountain,
                ],
            )
            .hand(P1, &[Card::GetLost, Card::GetLost])
            .battlefield(
                P1,
                &[
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Prop::Flier,
                ],
            )
            .actions(&[
                ME.casts(Card::WorldgorgerDragon),
                Resolve,
                Resolve,
                ME.has(Zone::Exile, Prop::Vanilla, 1),
                ME.passes(),
                THEM.casts(Card::GetLost),
                THEM.targets(Card::WorldgorgerDragon),
                Resolve,
                Resolve,
                ME.deciding(DecisionKind::ChooseUpTo, 2),
                ME.picks_up_to(Prop::Flier),
                ME.controls(Prop::Vanilla, 1),
                ME.pt(Prop::Flier, 4, 2),
                ME.has_subtype(Prop::Flier, Subtype::Illusion, true),
            ]),
        // Ruling: "Any enters-the-battlefield abilities of the copied creature will trigger
        // when Phantasmal Image enters the battlefield." The Image enters as Beza, a 4/5, and
        // Beza's enters ability gains P0 4 life.
        beza("the copy's enters ability triggers", &[IMAGE]).actions(&[
            ME.casts(IMAGE),
            Resolve,
            ME.picks_up_to(BEZA),
            Resolve,
            ME.controls(BEZA, 1),
            ME.pt(BEZA, 4, 5),
            ME.life(14),
            THEM.controls(BEZA, 1),
        ]),
        // Declining to copy, the Image enters as itself, a 0/0, and dies
        // ([CR 704.5f](https://yawgatog.com/resources/magic-rules/#R7045f)): "If a creature
        // has toughness 0 or less, it's put into its owner's graveyard."
        beza("an Image that copies nothing dies", &[IMAGE]).actions(&[
            ME.casts(IMAGE),
            Resolve,
            ME.done_picking(),
            ME.has(Zone::Graveyard, IMAGE, 1),
            ME.life(10),
        ]),
        // Two Images of Beza are two legendary permanents with the same name under one
        // player ([CR 704.5j](https://yawgatog.com/resources/magic-rules/#R7045j)): "that
        // player chooses one of them, and the rest are put into their owners' graveyards." P1's
        // Beza stays. Each Image's copy of Beza gained P0 4 life as it entered.
        beza(
            "two Images of a legendary creature meet the legend rule",
            &[IMAGE, IMAGE],
        )
        .actions(&[
            ME.casts(IMAGE),
            Resolve,
            ME.picks_up_to(BEZA),
            Resolve,
            ME.casts(IMAGE),
            Resolve,
            ME.picks_up_to(BEZA),
            ME.picks(BEZA),
            Resolve,
            ME.controls(BEZA, 1),
            ME.has(Zone::Graveyard, IMAGE, 1),
            ME.life(18),
            THEM.controls(BEZA, 1),
        ]),
        // P1's Mother of Runes gives itself protection from white in response to the Image.
        // The Image chooses it all the same: what it copies is chosen, not targeted
        // ([CR 115.10a](https://yawgatog.com/resources/magic-rules/#R11510a)). Holy Strength
        // can target only P0's copy, which is sacrificed as it becomes the Aura spell's target,
        // and the Aura, its one target gone, does nothing
        // ([CR 608.3b](https://yawgatog.com/resources/magic-rules/#R6083b)).
        Scenario::new("an Aura spell targeting the copy has it sacrificed")
            .interaction(&[MOTHER, HOLY])
            .battlefield(
                P0,
                &[Card::Island, Card::Island, Card::Plains, Card::Plains],
            )
            .hand(P0, &[IMAGE, HOLY])
            .battlefield(P1, &[MOTHER])
            .actions(&[
                ME.casts(IMAGE),
                THEM.activates(MOTHER, 0),
                // Mother's ability resolves first, choosing white as it does.
                THEM.chooses_color(Colors::WHITE),
                Resolve,
                ME.picks_up_to(MOTHER),
                ME.controls(MOTHER, 1),
                // P1's Mother has protection from white, so the copy is the one target.
                ME.casts(HOLY),
                Resolve,
                ME.controls(MOTHER, 0),
                ME.has(Zone::Graveyard, IMAGE, 1),
                Resolve,
                ME.has(Zone::Graveyard, HOLY, 1),
            ]),
        // Ruling: "If the chosen creature is copying something else ..., then Phantasmal
        // Image enters the battlefield as whatever the chosen creature copied." P1's
        // Quicksilver Gargantuan copies P0's Mother of Runes, a 7/7 copy, and P1's Portable
        // Hole exiles the original. P0's Image copies the Gargantuan: Mother of Runes with the
        // Gargantuan's exception, 7/7, and the Image's own, which P0's Holy Strength then
        // shows. P0's own Portable Hole first exiles the Gargantuan, the only other Mother.
        Scenario::new("an Image of a Gargantuan that copied Mother of Runes is a 7/7 Mother")
            .interaction(&[GARGANTUAN, MOTHER, HOLY])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(
                P0,
                &[
                    MOTHER,
                    Card::Island,
                    Card::Island,
                    Card::Plains,
                    Card::Plains,
                ],
            )
            .hand(P0, &[IMAGE, HOLE, HOLY])
            .battlefield(P1, P1_LANDS)
            .hand(P1, &[GARGANTUAN, HOLE])
            .library(P0, &[Card::Island, Card::Island])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                THEM.casts(GARGANTUAN),
                Resolve,
                THEM.picks_up_to(MOTHER),
                THEM.pt(MOTHER, 7, 7),
                // P0's Mother is the one permanent P1's Portable Hole can target.
                THEM.casts(HOLE),
                Resolve,
                Resolve,
                ME.controls(MOTHER, 0),
                advance_to(3, TurnStep::PrecombatMain),
                ME.casts(IMAGE),
                Resolve,
                ME.picks_up_to(MOTHER),
                ME.pt(MOTHER, 7, 7),
                ME.casts(HOLE),
                Resolve,
                ME.targets(MOTHER),
                Resolve,
                THEM.controls(MOTHER, 0),
                ME.controls(MOTHER, 1),
                // The copy is the one creature left for Holy Strength to target.
                ME.casts(HOLY),
                Resolve,
                ME.has(Zone::Graveyard, IMAGE, 1),
            ]),
        // Counters are not copiable
        // ([CR 707.2](https://yawgatog.com/resources/magic-rules/#R7072)): an Image of a Mother of Runes with three
        // +1/+1 counters is a 1/1 with no counters, an Illusion in addition to its types.
        Scenario::new("an Image does not copy counters")
            .interaction(&[MOTHER])
            .battlefield(P0, &[Card::Island, Card::Island])
            .hand(P0, &[IMAGE])
            .permanents(
                P1,
                &[Permanent::new(MOTHER)
                    .tapped()
                    .counters(CounterKind::PlusOne, 3)],
            )
            .actions(&[
                ME.casts(IMAGE),
                Resolve,
                ME.picks_up_to(MOTHER),
                ME.pt(MOTHER, 1, 1),
                ME.counters(MOTHER, CounterKind::PlusOne, 0),
                ME.tapped(MOTHER, 0),
                ME.has_subtype(MOTHER, Subtype::Illusion, true),
                ME.has_subtype(MOTHER, Subtype::Human, true),
                THEM.pt(MOTHER, 4, 4),
            ]),
        // The Image of a Marit Lage token is a 20/20 card-less copy; copying copiable values of
        // a token ([CR 707.2](https://yawgatog.com/resources/magic-rules/#R7072)) doesn't make the Image a token.
        Scenario::new("an Image copies a token's characteristics but is not a token")
            .interaction(&[Card::DarkDepths])
            .battlefield(P0, &[Card::Island, Card::Island])
            .hand(P0, &[IMAGE])
            .permanents(
                P1,
                &[Permanent::new(Card::DarkDepths).counters(CounterKind::Ice, 0)],
            )
            .actions(&[
                Resolve,
                THEM.controls("Marit Lage", 1),
                ME.casts(IMAGE),
                Resolve,
                ME.picks_up_to("Marit Lage"),
                ME.pt("Marit Lage", 20, 20),
                ME.has_subtype("Marit Lage", Subtype::Illusion, true),
            ]),
        // When the Image copies Painter's Servant, its controller chooses a color as it enters.
        Scenario::new("an Image of Painter's Servant chooses a color as it enters")
            .interaction(&[Card::PaintersServant])
            .battlefield(P0, &[Card::Island, Card::Island])
            .battlefield(P1, &[Card::PaintersServant])
            .hand(P0, &[IMAGE])
            .actions(&[
                ME.casts(IMAGE),
                Resolve,
                ME.picks_up_to(Card::PaintersServant),
                ME.chooses_color(Colors::GREEN),
                ME.colored(Zone::Battlefield, Card::PaintersServant, Colors::GREEN),
            ]),
        // The copy has only the land card type in layer 1, so it cannot gain Illusion.
        // Life and Limb makes it a creature in layer 4.
        Scenario::new("an Image of an animated Forest gains no Illusion subtype")
            .interaction(&[Card::LifeAndLimb, Card::SparasHeadquarters])
            .battlefield(P0, &[Card::Island, Card::Island, Card::LifeAndLimb])
            .battlefield(P1, &[Card::SparasHeadquarters])
            .hand(P0, &[IMAGE, Card::Plains])
            .actions(&[
                ME.casts(IMAGE),
                Resolve,
                ME.picks_up_to(Card::SparasHeadquarters),
                ME.creature(Card::SparasHeadquarters, true),
                ME.has_subtype(Card::SparasHeadquarters, Subtype::Saproling, true),
                ME.has_subtype(Card::SparasHeadquarters, Subtype::Illusion, false),
            ]),
        // The Gargantuan gains the subtype and sacrifice trigger from the Image it copies.
        Scenario::new("a copy of an Image inherits its Illusion type and sacrifice trigger")
            .interaction(&[MOTHER, GARGANTUAN])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(
                P0,
                &[
                    MOTHER,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .hand(P0, &[GARGANTUAN, Prop::Equipment])
            .battlefield(P1, &[Card::Island, Card::Island, Card::Plains])
            .hand(P1, &[IMAGE, Prop::Artifact])
            .actions(&[
                THEM.casts(IMAGE),
                Resolve,
                THEM.picks_up_to(MOTHER),
                THEM.casts(Prop::Artifact),
                Resolve,
                Resolve,
                ME.controls(MOTHER, 0),
                advance_to(3, TurnStep::PrecombatMain),
                ME.casts(GARGANTUAN),
                Resolve,
                ME.picks_up_to(MOTHER),
                ME.has_subtype(MOTHER, Subtype::Illusion, true),
                ME.casts(Prop::Equipment),
                Resolve,
                ME.activates(Prop::Equipment, 1),
                Resolve,
                Resolve,
                ME.has(Zone::Graveyard, GARGANTUAN, 1),
                THEM.controls(MOTHER, 1),
            ]),
    ];
}
