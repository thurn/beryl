// Generated from the card store by `just card header`: notes go below the end marker.
//! Panglacial Wurm {5}{G}{G}
//! Creature — Wurm, 9/5
//!
//! Trample
//! While you're searching your library, you may cast this card from your library.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::PanglacialWurm
    .keywords(Keywords::TRAMPLE)
    .abilities(&[Ability::cast_while_searching()])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const WURM: &str = Card::PanglacialWurm;
    const MAGE: &str = Card::TrinketMage;
    const WINDS: &str = Card::WindsOfAbandon;
    const ARTIFACT: &str = Prop::Artifact;
    const FOREST: &str = Card::Forest;
    const ISLAND: &str = Card::Island;
    const PLAINS: &str = Card::Plains;

    const fn cast_during_search(player: Player) -> Action {
        player.answers(
            DecisionKind::ChooseUpTo,
            Pick::Cast {
                card: WURM,
                face: FaceIx::FRONT,
                costs: Castings::NONE,
            },
        )
    }

    const fn mage_search(name: &'static str) -> Scenario {
        Scenario::new(name)
            .interaction(&[MAGE])
            .hand(P0, &[MAGE])
            .library(P0, &[ARTIFACT, WURM, PLAINS, FOREST])
    }

    const CAST_MAGE: Action = ME.casts(MAGE);
    const LANDS: &[&str] = &[
        ISLAND, ISLAND, ISLAND, FOREST, FOREST, FOREST, FOREST, FOREST, FOREST, FOREST,
    ];

    const fn wurm_on_stack(player: Player, depth: u16) -> Action {
        Action::assert(Assertion::StackObject {
            depth,
            name: WURM,
            spell: true,
            controller: player.seat(),
        })
    }

    pub const SCENARIOS: &[Scenario] = &[
        mage_search("Cast the Wurm, then find an artifact")
            .battlefield(P0, LANDS)
            .actions(&[
                CAST_MAGE,
                Resolve,
                Resolve,
                ME.yes(),
                ME.deciding(DecisionKind::ChooseUpTo, 3),
                cast_during_search(ME),
                ME.deciding(DecisionKind::ChooseUpTo, 2),
                ME.picks_up_to(ARTIFACT),
                ME.has(Zone::Hand, ARTIFACT, 1),
                ME.has(Zone::Library, WURM, 0),
                wurm_on_stack(ME, 0),
                ME.tapped(FOREST, 7),
                Resolve,
                ME.has(Zone::Battlefield, WURM, 1),
            ]),
        mage_search("Cast the Wurm, then find nothing")
            .battlefield(P0, LANDS)
            .actions(&[
                CAST_MAGE,
                Resolve,
                Resolve,
                ME.yes(),
                cast_during_search(ME),
                ME.done_picking(),
                ME.has_cards(Zone::Hand, 0),
                ME.has_cards(Zone::Library, 3),
                wurm_on_stack(ME, 0),
                Resolve,
                ME.has(Zone::Battlefield, WURM, 1),
            ]),
        mage_search("Six mana cannot cast the Wurm during the search")
            .battlefield(
                P0,
                &[
                    ISLAND, ISLAND, ISLAND, FOREST, FOREST, FOREST, FOREST, FOREST, FOREST,
                ],
            )
            .actions(&[
                CAST_MAGE,
                Resolve,
                Resolve,
                ME.yes(),
                ME.deciding(DecisionKind::ChooseUpTo, 2),
                ME.picks_up_to(ARTIFACT),
                ME.has(Zone::Hand, ARTIFACT, 1),
                ME.has(Zone::Library, WURM, 1),
            ]),
        Scenario::new("Cast the Wurm while searching for an opponent's spell")
            .interaction(&[WINDS])
            .hand(P0, &[WINDS])
            .battlefield(P0, &[PLAINS; 2])
            .battlefield(
                P1,
                &[
                    Prop::Flier,
                    FOREST,
                    FOREST,
                    FOREST,
                    FOREST,
                    FOREST,
                    FOREST,
                    FOREST,
                ],
            )
            .library(P1, &[WURM, ISLAND, PLAINS])
            .actions(&[
                ME.casts(WINDS),
                Resolve,
                THEM.deciding(DecisionKind::ChooseUpTo, 4),
                cast_during_search(THEM),
                THEM.picks_up_to(ISLAND),
                ME.has(Zone::Graveyard, WINDS, 1),
                THEM.has(Zone::Battlefield, ISLAND, 1),
                wurm_on_stack(THEM, 0),
                Resolve,
                THEM.has(Zone::Battlefield, WURM, 1),
            ]),
        mage_search("Cast two Wurms in a search with no artifact to find")
            .battlefield(
                P0,
                &[
                    ISLAND, ISLAND, ISLAND, FOREST, FOREST, FOREST, FOREST, FOREST, FOREST, FOREST,
                    FOREST, FOREST, FOREST, FOREST, FOREST, FOREST, FOREST, FOREST,
                ],
            )
            .library(P0, &[WURM, PLAINS, WURM, FOREST])
            .actions(&[
                CAST_MAGE,
                Resolve,
                Resolve,
                ME.yes(),
                ME.deciding(DecisionKind::ChooseUpTo, 3),
                cast_during_search(ME),
                ME.deciding(DecisionKind::ChooseUpTo, 2),
                cast_during_search(ME),
                wurm_on_stack(ME, 0),
                wurm_on_stack(ME, 1),
                ME.has_cards(Zone::Library, 2),
                Resolve,
                Resolve,
                ME.has(Zone::Battlefield, WURM, 2),
            ]),
        Scenario::new("The Wurm tramples over a blocker")
            .battlefield(P0, &[WURM])
            .battlefield(P1, &[Prop::Vanilla])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(WURM),
                THEM.chooses(DecisionKind::Blocker, Prop::Vanilla),
                advance_to(1, TurnStep::EndOfCombat),
                THEM.has(Zone::Graveyard, Prop::Vanilla, 1),
                THEM.life(12),
            ]),
    ];
}
