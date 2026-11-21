//! Positions involving face-down and merged permanents.

use beryl_card::prelude::{Castings, FaceIx};
use beryl_cards::names;
use beryl_cards::props::Prop;
use beryl_scenario::prelude::{
    Action, DecisionKind, ME, P0, P1, Resolve, Scenario, THEM, TurnStep, Zone, advance_to, stack,
};

pub const COPY_FACE_DOWN_PERMANENT: &Scenario =
    &Scenario::new("Fractured Identity copies a face-down permanent as a 2/2")
        .interaction(&[names::FRACTURED_IDENTITY])
        .at(1, P0, TurnStep::PostcombatMain)
        .battlefield(P0, &[names::ABHORRENT_OCULUS])
        .library(P0, &[Prop::Vanilla, names::ISLAND, names::ISLAND])
        .battlefield(
            P1,
            &[
                names::PLAINS,
                names::PLAINS,
                names::PLAINS,
                names::ISLAND,
                names::ISLAND,
            ],
        )
        .hand(P1, &[names::FRACTURED_IDENTITY])
        .library(P1, &[names::PLAINS])
        .actions(&[
            advance_to(2, TurnStep::Upkeep),
            Resolve,
            ME.picks(Prop::Vanilla),
            advance_to(2, TurnStep::PrecombatMain),
            THEM.casts(names::FRACTURED_IDENTITY),
            THEM.targets(Prop::Vanilla),
            Resolve,
            ME.has(Zone::Exile, Prop::Vanilla, 1),
            THEM.has_cards(Zone::Battlefield, 6),
            THEM.passes(),
        ]);

pub const COPY_MERGED_CREATURE: &Scenario =
    &Scenario::new("Glasspool Mimic copying Gemrazer's merged creature has both cards' abilities")
        .interaction(&[names::BLOOD_ARTIST, names::GET_LOST, names::GLASSPOOL_MIMIC])
        .battlefield(
            P0,
            &[
                names::FOREST,
                names::FOREST,
                names::FOREST,
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
                names::BLOOD_ARTIST,
            ],
        )
        .hand(P0, &[names::GEMRAZER, names::GLASSPOOL_MIMIC])
        .battlefield(
            P1,
            &[
                names::PLAINS,
                names::PLAINS,
                names::LEONIN_SCIMITAR,
                Prop::Flier,
            ],
        )
        .hand(P1, &[names::GET_LOST])
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::Cast {
                player: P0,
                card: names::GEMRAZER,
                face: FaceIx::FRONT,
                costs: Castings::only(0),
            },
            Resolve,
            ME.yes(),
            Resolve,
            ME.casts(names::GLASSPOOL_MIMIC),
            Resolve,
            ME.picks_up_to(names::GEMRAZER),
            ME.controls(names::GEMRAZER, 2),
            THEM.casts(names::GET_LOST),
            THEM.targets(Prop::Flier),
            ME.chooses(DecisionKind::TriggerOrder, names::GEMRAZER),
            ME.targets_player(THEM),
            ME.targets_player(THEM),
            Resolve,
            Resolve,
            ME.controls(names::GEMRAZER, 2),
            THEM.controls(Prop::Flier, 0),
            THEM.life(18),
            ME.life(22),
        ]);

pub const FACE_DOWN_CREATURE_LEAVES: &Scenario =
    &Scenario::new("A face-down permanent leaving the battlefield is revealed")
        .at(1, P0, TurnStep::PostcombatMain)
        .battlefield(P0, &[names::ABHORRENT_OCULUS])
        .library(P0, &[Prop::Vanilla, names::ISLAND, names::ISLAND])
        .battlefield(P1, &[names::PLAINS])
        .hand(P1, &[names::OUST])
        .library(P1, &[names::PLAINS])
        .actions(&[
            advance_to(2, TurnStep::Upkeep),
            Resolve,
            ME.picks(Prop::Vanilla),
            advance_to(2, TurnStep::PrecombatMain),
            THEM.casts(names::OUST),
            THEM.targets(Prop::Vanilla),
            Resolve,
            ME.library_top(&[names::ISLAND, Prop::Vanilla]),
            ME.life(23),
            THEM.passes(),
        ]);

pub const GEMRAZER_OVER_FACE_DOWN_CARD: &Scenario =
    &Scenario::new("Gemrazer over a face-down creature: a face-up Gemrazer, its card hidden")
        .interaction(&[names::ABHORRENT_OCULUS])
        .at(1, P0, TurnStep::PostcombatMain)
        .battlefield(
            P0,
            &[
                names::FOREST,
                names::FOREST,
                names::FOREST,
                names::ABHORRENT_OCULUS,
            ],
        )
        .hand(P0, &[names::GEMRAZER])
        .library(
            P0,
            &[
                Prop::Vanilla,
                names::COUNTERSPELL,
                names::ISLAND,
                names::ISLAND,
            ],
        )
        .battlefield(P1, &[names::LEONIN_SCIMITAR])
        .library(P1, &[names::ISLAND, names::ISLAND])
        .actions(&[
            advance_to(2, TurnStep::Upkeep),
            Resolve,
            ME.picks(Prop::Vanilla),
            ME.face_down(Prop::Vanilla, 1),
            advance_to(3, TurnStep::PrecombatMain),
            Action::Cast {
                player: P0,
                card: names::GEMRAZER,
                face: FaceIx::FRONT,
                costs: Castings::only(0),
            },
            ME.targets(Prop::Vanilla),
            Resolve,
            ME.yes(),
            Resolve,
            THEM.controls(names::LEONIN_SCIMITAR, 0),
            ME.controls(names::GEMRAZER, 1),
            ME.face_down(names::GEMRAZER, 0),
            ME.pt(names::GEMRAZER, 4, 4),
        ]);

pub const GEMRAZER_UNDER_CREATURE: &Scenario =
    &Scenario::new("Gemrazer under a creature: the creature's values, Gemrazer's abilities")
        .interaction(&[names::BLOOD_ARTIST])
        .battlefield(
            P0,
            &[
                names::FOREST,
                names::FOREST,
                names::FOREST,
                names::BLOOD_ARTIST,
            ],
        )
        .hand(P0, &[names::GEMRAZER])
        .battlefield(P1, &[names::LEONIN_SCIMITAR])
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::Cast {
                player: P0,
                card: names::GEMRAZER,
                face: FaceIx::FRONT,
                costs: Castings::only(0),
            },
            Resolve,
            ME.no(),
            Resolve,
            THEM.controls(names::LEONIN_SCIMITAR, 0),
            THEM.has(Zone::Graveyard, names::LEONIN_SCIMITAR, 1),
            ME.controls(names::BLOOD_ARTIST, 1),
            ME.pt(names::BLOOD_ARTIST, 0, 1),
            ME.controls(names::GEMRAZER, 0),
            ME.has_cards(Zone::Hand, 0),
            ME.has_cards(Zone::Graveyard, 0),
        ]);

pub const OCULUS_EXILES_SIX_CARDS: &Scenario =
    &Scenario::new("Abhorrent Oculus exiles six of seven graveyard cards as it's cast")
        .battlefield(P0, &[names::ISLAND; 3])
        .hand(P0, &[names::ABHORRENT_OCULUS])
        .graveyard(
            P0,
            &[
                names::COUNTERSPELL,
                Prop::Removal,
                Prop::UntargetedSpell,
                Prop::Artifact,
                names::OUST,
                Prop::Planeswalker,
                names::PLAINS,
            ],
        )
        .actions(&[
            ME.casts(names::ABHORRENT_OCULUS),
            ME.deciding(DecisionKind::ChooseObject, 2),
            ME.picks(Prop::Planeswalker),
            ME.has(Zone::Graveyard, names::PLAINS, 1),
            ME.has_cards(Zone::Exile, 6),
            ME.tapped(names::ISLAND, 3),
            Resolve,
            ME.pt(names::ABHORRENT_OCULUS, 5, 5),
        ]);

pub const OCULUS_MANIFESTS_DREAD: &Scenario =
    &Scenario::new("Abhorrent Oculus manifests dread on each opponent's upkeep only")
        .at(1, P0, TurnStep::PostcombatMain)
        .battlefield(P0, &[names::ABHORRENT_OCULUS])
        .library(
            P0,
            &[
                Prop::Vanilla,
                names::COUNTERSPELL,
                names::ISLAND,
                names::ISLAND,
            ],
        )
        .library(P1, &[names::PLAINS])
        .actions(&[
            advance_to(2, TurnStep::Upkeep),
            stack(1),
            Resolve,
            ME.deciding(DecisionKind::ChooseObject, 2),
            ME.picks(Prop::Vanilla),
            ME.face_down(Prop::Vanilla, 1),
            ME.pt(Prop::Vanilla, 2, 2),
            ME.has(Zone::Graveyard, names::COUNTERSPELL, 1),
            ME.library_top(&[names::ISLAND, names::ISLAND]),
            advance_to(3, TurnStep::Upkeep),
            stack(0),
        ]);

pub const TURN_MANIFESTED_CREATURE_FACE_UP: &Scenario =
    &Scenario::new("A manifested creature turns face up for its mana cost, entering no more")
        .interaction(&[names::SNAPCASTER_MAGE])
        .at(1, P0, TurnStep::PostcombatMain)
        .battlefield(P0, &[names::ABHORRENT_OCULUS, names::ISLAND, names::ISLAND])
        .graveyard(P0, &[names::COUNTERSPELL])
        .library(P0, &[names::SNAPCASTER_MAGE, names::ISLAND])
        .library(P1, &[names::PLAINS])
        .actions(&[
            advance_to(2, TurnStep::Upkeep),
            Resolve,
            ME.picks(names::SNAPCASTER_MAGE),
            Action::TurnFaceUp {
                player: P0,
                card: names::SNAPCASTER_MAGE,
            },
            ME.face_down(names::SNAPCASTER_MAGE, 0),
            ME.tapped(names::ISLAND, 2),
            ME.pt(names::SNAPCASTER_MAGE, 2, 1),
            stack(0),
        ]);
