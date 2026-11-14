// Generated from the card store by `just card header`: notes go below the end marker.
//! Blood Artist {1}{B}
//! Creature — Vampire, 0/1
//!
//! Whenever this creature or another creature dies, target player loses 1 life and you gain 1 life.
// End of the generated card header.

use crate::prelude::*;

/// "This creature or another creature dies" is one ability on the event "a creature dies", which
/// sees Blood Artist itself leave in the same event as the others. Each creature that dies
/// triggers it once, so a sweeper that kills Blood Artist and two others triggers it three times
/// ([CR 603.10a]): "Some zone-change triggers look back in time." A creature that is exiled or
/// to a hand doesn't die: dying is going from the battlefield to a graveyard ([CR 700.4]): "The
/// term dies means 'is put into a graveyard from the battlefield.'"
///
/// [CR 603.10a]: https://yawgatog.com/resources/magic-rules/#R60310a
/// [CR 700.4]: https://yawgatog.com/resources/magic-rules/#R7004
pub static CARD: CardDef = Printed::BloodArtist
    .abilities(&[Ability::triggered(&Trigger::of(
        TriggerEvent::Dies,
        beryl_card::Subject::A(Filter::Types(Types::CREATURE)),
    ))
    .target(&TargetSpec::player())
    .effect(&sequence(&[
        lose_life(target_player(), 1),
        gain_life(you(), 1),
    ]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const ARTIST: &str = Card::BloodArtist;
    const SPIRIT_TOKEN: &str = "Spirit Token";

    pub const SCENARIOS: &[Scenario] = &[
        // An opponent's creature dies: one trigger, and the targeted player loses 1 while
        // Blood Artist's controller gains 1.
        Scenario::new("Blood Artist triggers when an opponent's creature dies")
            .interaction(&[Card::Damn])
            .battlefield(P0, &[Card::Swamp, Card::Swamp, ARTIST])
            .hand(P0, &[Card::Damn])
            .battlefield(P1, &[Card::EliteSpellbinder])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.casts(Card::Damn),
                ME.targets(Card::EliteSpellbinder),
                Resolve,
                ME.targets_player(THEM),
                Resolve,
                THEM.controls(Card::EliteSpellbinder, 0),
                ME.controls(ARTIST, 1),
                THEM.life(19),
                ME.life(21),
            ]),
        // The ability may target any player, the controller included: P0 drains itself, losing
        // 1 and gaining 1.
        Scenario::new("Blood Artist can target its own controller")
            .interaction(&[Card::Damn])
            .battlefield(P0, &[Card::Swamp, Card::Swamp, ARTIST])
            .hand(P0, &[Card::Damn])
            .battlefield(P1, &[Card::EliteSpellbinder])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.casts(Card::Damn),
                ME.targets(Card::EliteSpellbinder),
                Resolve,
                ME.targets_player(ME),
                Resolve,
                THEM.life(20),
                ME.life(20),
            ]),
        // A sweeper kills Blood Artist and two other creatures in one event: three triggers.
        Scenario::new("Blood Artist dying with two others triggers three times")
            .interaction(&[Card::Damn])
            .battlefield(
                P0,
                &[
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    ARTIST,
                    Prop::SmallCreature,
                ],
            )
            .hand(P0, &[Card::Damn])
            .battlefield(P1, &[Card::EliteSpellbinder])
            .library(P1, &[Card::Island])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: Card::Damn,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Resolve,
                // The three triggers go on the stack in an order P0 picks.
                ME.chooses(DecisionKind::TriggerOrder, ARTIST),
                ME.targets_player(THEM),
                ME.chooses(DecisionKind::TriggerOrder, ARTIST),
                ME.targets_player(THEM),
                ME.targets_player(THEM),
                stack(3),
                Resolve,
                Resolve,
                Resolve,
                ME.controls(ARTIST, 0),
                THEM.life(17),
                ME.life(23),
            ]),
        // A token creature dying triggers it, though a token ceases to exist in the graveyard
        // ([CR 704.5d](https://yawgatog.com/resources/magic-rules/#R7045d)): "If a token is in a
        // zone other than the battlefield, it ceases to exist". Staff of the Storyteller makes the
        // Spirit token.
        Scenario::new("Blood Artist triggers when a token creature dies")
            .interaction(&[Card::Damn, Card::StaffOfTheStoryteller])
            .battlefield(
                P0,
                &[
                    Card::Plains,
                    Card::Swamp,
                    Card::Swamp,
                    Card::Swamp,
                    Card::Swamp,
                    ARTIST,
                ],
            )
            .hand(P0, &[Card::StaffOfTheStoryteller, Card::Damn])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.casts(Card::StaffOfTheStoryteller),
                Resolve,
                Resolve,
                Resolve,
                ME.controls(SPIRIT_TOKEN, 1),
                ME.casts(Card::Damn),
                ME.targets(SPIRIT_TOKEN),
                Resolve,
                ME.targets_player(THEM),
                Resolve,
                ME.controls(SPIRIT_TOKEN, 0),
                THEM.life(19),
                ME.life(21),
            ]),
        // Exile isn't dying: Portable Hole exiles P1's creature and nothing triggers.
        Scenario::new("Blood Artist ignores a creature that is exiled")
            .battlefield(P0, &[Card::Plains, ARTIST])
            .hand(P0, &[Prop::Artifact])
            .battlefield(P1, &[Prop::SmallCreature])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.casts(Prop::Artifact),
                Resolve,
                Resolve,
                THEM.controls(Prop::SmallCreature, 0),
                stack(0),
                THEM.life(20),
                ME.life(20),
            ]),
        // Returning a creature to its owner's hand isn't dying either.
        Scenario::new("Blood Artist ignores a creature returned to a hand")
            .battlefield(P0, &[Card::Island, Card::Island, Card::Island, ARTIST])
            .hand(P0, &[Prop::Removal])
            .battlefield(P1, &[Prop::SmallCreature])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.casts(Prop::Removal),
                Resolve,
                THEM.controls(Prop::SmallCreature, 0),
                THEM.has(Zone::Hand, Prop::SmallCreature, 1),
                stack(0),
                THEM.life(20),
                ME.life(20),
            ]),
    ];
}
