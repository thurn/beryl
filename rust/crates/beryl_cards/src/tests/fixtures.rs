//! Gameplay positions for focused registry tests.

use crate::props::Prop;
use crate::{names, tokens};
use beryl_card::prelude::{Castings, Colors, FaceIx, ManaColor, Subtype, TextWord};
use beryl_scenario_types::prelude::{
    Action, CounterKind, DecisionKind, ME, P0, P1, Resolve, Scenario, THEM, TurnStep, Zone,
    advance_to, stack,
};

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

pub const CURRENCY_CONVERTER: &Scenario =
    &Scenario::new("Currency Converter chooses among its exiled cards")
        .interaction(&[names::JACE_VRYNS_PRODIGY])
        .battlefield(
            P0,
            &[
                names::CURRENCY_CONVERTER,
                names::ISLAND,
                names::ISLAND,
                names::JACE_VRYNS_PRODIGY,
            ],
        )
        .hand(P0, &[names::ISLAND, names::OUST])
        .library(P0, &[names::PLAINS, names::PLAINS, names::PLAINS])
        .library(P1, &[names::ISLAND, names::ISLAND])
        .actions(&[
            ME.activates(names::JACE_VRYNS_PRODIGY, 0),
            Resolve,
            ME.picks(names::ISLAND),
            Resolve,
            ME.yes(),
            ME.activates(names::CURRENCY_CONVERTER, 1),
            Resolve,
            ME.picks(names::OUST),
            Resolve,
            ME.yes(),
            ME.has(Zone::Exile, names::ISLAND, 1),
            ME.has(Zone::Exile, names::OUST, 1),
            advance_to(3, TurnStep::PrecombatMain),
            ME.activates(names::CURRENCY_CONVERTER, 2),
            Resolve,
            ME.deciding(DecisionKind::ChooseObject, 2),
            ME.picks(names::ISLAND),
            ME.has(Zone::Graveyard, names::ISLAND, 1),
            ME.has(Zone::Exile, names::OUST, 1),
            ME.controls(tokens::TREASURE_TOKEN, 1),
            ME.controls(tokens::ROGUE_TOKEN, 0),
        ]);

pub const MUTATE_FACE_DOWN: &Scenario =
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

pub const SHOW_AND_TELL_AURAS: &Scenario =
    &Scenario::new("Show and Tell chooses both Auras before either attachment")
        .interaction(&[names::SWIFT_RECONFIGURATION, names::HOLY_STRENGTH])
        .hand(P0, &[names::SHOW_AND_TELL, names::SWIFT_RECONFIGURATION])
        .battlefield(
            P0,
            &[
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
                Prop::Vanilla,
                Prop::FlashCreature,
            ],
        )
        .hand(P1, &[names::HOLY_STRENGTH])
        .battlefield(P1, &[Prop::Flier])
        .actions(&[
            ME.casts(names::SHOW_AND_TELL),
            ME.picks_up_to(names::SWIFT_RECONFIGURATION),
            THEM.picks_up_to(names::HOLY_STRENGTH),
            ME.deciding(DecisionKind::ChooseObject, 3),
            ME.picks(Prop::Flier),
            THEM.deciding(DecisionKind::ChooseObject, 3),
            THEM.picks(Prop::Vanilla),
            ME.controls(names::SWIFT_RECONFIGURATION, 1),
            THEM.controls(names::HOLY_STRENGTH, 1),
            THEM.creature(Prop::Flier, false),
            THEM.has_subtype(Prop::Flier, Subtype::Vehicle, true),
            ME.pt(Prop::Vanilla, 3, 3),
        ]);

pub const STAFF: &Scenario =
    &Scenario::new("its own Spirit and a Samurai each add a story counter, spent to draw")
        .interaction(&[names::THE_WANDERING_EMPEROR])
        .hand(P0, &[names::STAFF_OF_THE_STORYTELLER])
        .battlefield(
            P0,
            &[
                names::PLAINS,
                names::PLAINS,
                names::PLAINS,
                names::THE_WANDERING_EMPEROR,
            ],
        )
        .library(P0, &[names::ISLAND, names::ISLAND, names::ISLAND])
        .library(P1, &[names::PLAINS])
        .actions(&[
            ME.casts(names::STAFF_OF_THE_STORYTELLER),
            Resolve,
            Resolve,
            Resolve,
            ME.counters(names::STAFF_OF_THE_STORYTELLER, CounterKind::Story, 1),
            ME.controls("Spirit Token", 1),
            ME.activates(names::THE_WANDERING_EMPEROR, 2), // -1
            Resolve,
            Resolve,
            ME.counters(names::STAFF_OF_THE_STORYTELLER, CounterKind::Story, 2),
            ME.activates(names::STAFF_OF_THE_STORYTELLER, 2), // {W}, {T}, remove a story counter
            ME.counters(names::STAFF_OF_THE_STORYTELLER, CounterKind::Story, 1),
            ME.has(Zone::Hand, names::ISLAND, 1),
            ME.tapped(names::STAFF_OF_THE_STORYTELLER, 1),
            ME.deciding(DecisionKind::Priority, 2),
        ]);

pub const TRINISPHERE: &Scenario = &Scenario::new("Trinisphere: a one-mana spell costs three")
    .interaction(&[names::LIGHTNING_BOLT])
    .battlefield(
        P0,
        &[
            names::TRINISPHERE,
            names::MOUNTAIN,
            names::MOUNTAIN,
            names::MOUNTAIN,
        ],
    )
    .hand(P0, &[names::LIGHTNING_BOLT])
    .actions(&[
        ME.casts(names::LIGHTNING_BOLT),
        ME.targets_player(THEM),
        ME.tapped(names::MOUNTAIN, 3),
        Resolve,
        THEM.life(17),
    ]);

pub const OCULUS_SHORT_GRAVEYARD: &Scenario =
    &Scenario::new("Abhorrent Oculus can't be cast with five cards in the graveyard")
        .battlefield(P0, &[names::ISLAND; 3])
        .hand(P0, &[names::ABHORRENT_OCULUS, names::ISLAND])
        .graveyard(
            P0,
            &[
                names::COUNTERSPELL,
                Prop::Removal,
                Prop::UntargetedSpell,
                Prop::Artifact,
                names::OUST,
            ],
        )
        .actions(&[ME.deciding(DecisionKind::Priority, 2)]);

pub const FORK: &Scenario = &Scenario::new("Fork copies Braingeyser for 3 with a new target")
    .interaction(&[names::BRAINGEYSER, names::LIGHTNING_BOLT])
    .battlefield(P0, &[names::MOUNTAIN, names::MOUNTAIN, names::MOUNTAIN])
    .hand(P0, &[names::FORK, names::LIGHTNING_BOLT])
    .library(P0, &[names::PLAINS; 4])
    .battlefield(P1, &[names::ISLAND; 5])
    .hand(P1, &[names::BRAINGEYSER])
    .library(P1, &[names::PLAINS; 4])
    .at(2, P1, TurnStep::PrecombatMain)
    .actions(&[
        THEM.casts(names::BRAINGEYSER),
        THEM.x(3),
        THEM.targets_player(THEM),
        ME.casts(names::FORK),
        Resolve,
        ME.targets_player(ME),
        ME.colored(Zone::Stack, names::BRAINGEYSER, Colors::RED),
        THEM.colored(Zone::Stack, names::BRAINGEYSER, Colors::BLUE),
        Resolve,
        ME.has(Zone::Hand, names::PLAINS, 3),
        Resolve,
        THEM.has(Zone::Hand, names::PLAINS, 3),
        ME.has(Zone::Hand, names::LIGHTNING_BOLT, 1),
    ]);

pub const MALCOLM: &Scenario = &Scenario::new("Malcolm's fourth chorus counter casts the discard")
    .battlefield(P0, &[names::MALCOLM_ALLURING_SCOUNDREL])
    .library(
        P0,
        &[
            names::ISLAND,
            names::ISLAND,
            names::ISLAND,
            names::ISLAND,
            names::ISLAND,
            names::ISLAND,
            Prop::Flier,
        ],
    )
    .library(P1, &[names::PLAINS, names::PLAINS, names::PLAINS])
    .actions(&[
        advance_to(1, TurnStep::BeginningOfCombat),
        ME.passes(),
        ME.attacks_with(names::MALCOLM_ALLURING_SCOUNDREL),
        advance_to(1, TurnStep::CombatDamage),
        Resolve,
        ME.counters(names::MALCOLM_ALLURING_SCOUNDREL, CounterKind::Chorus, 1),
        advance_to(3, TurnStep::BeginningOfCombat),
        ME.passes(),
        ME.attacks_with(names::MALCOLM_ALLURING_SCOUNDREL),
        advance_to(3, TurnStep::CombatDamage),
        Resolve,
        advance_to(5, TurnStep::BeginningOfCombat),
        ME.passes(),
        ME.attacks_with(names::MALCOLM_ALLURING_SCOUNDREL),
        advance_to(5, TurnStep::CombatDamage),
        Resolve,
        ME.counters(names::MALCOLM_ALLURING_SCOUNDREL, CounterKind::Chorus, 3),
        advance_to(7, TurnStep::BeginningOfCombat),
        ME.passes(),
        ME.attacks_with(names::MALCOLM_ALLURING_SCOUNDREL),
        advance_to(7, TurnStep::CombatDamage),
        Resolve,
        ME.picks(Prop::Flier),
        ME.chooses(DecisionKind::CastOffer, Prop::Flier),
        stack(1),
        Resolve,
        ME.counters(names::MALCOLM_ALLURING_SCOUNDREL, CounterKind::Chorus, 4),
        ME.controls(Prop::Flier, 1),
        ME.has(Zone::Hand, names::ISLAND, 3),
        THEM.life(12),
    ]);

pub const RED_PREVENTION: &Scenario =
    &Scenario::new("Circle of Protection: Red prevents Lightning Bolt's damage")
        .interaction(&[names::LIGHTNING_BOLT])
        .battlefield(
            P0,
            &[
                names::CIRCLE_OF_PROTECTION_RED,
                names::PLAINS,
                names::MOUNTAIN,
            ],
        )
        .battlefield(P1, &[names::HILL_GIANT])
        .hand(P0, &[names::LIGHTNING_BOLT])
        .actions(&[
            ME.casts(names::LIGHTNING_BOLT),
            ME.targets_player(ME),
            ME.activates(names::CIRCLE_OF_PROTECTION_RED, 0),
            ME.deciding(DecisionKind::ChooseObject, 2),
            ME.picks(names::LIGHTNING_BOLT),
            Resolve,
            ME.life(20),
            ME.has(Zone::Graveyard, names::LIGHTNING_BOLT, 1),
        ]);

pub const MIND_BEND: &Scenario =
    &Scenario::new("Mind Bend makes Circle of Protection: Red a circle against white")
        .interaction(&[names::CIRCLE_OF_PROTECTION_RED])
        .battlefield(
            P0,
            &[
                names::CIRCLE_OF_PROTECTION_RED,
                names::ISLAND,
                names::PLAINS,
            ],
        )
        .battlefield(P1, &[Prop::Flier])
        .hand(P0, &[names::MIND_BEND])
        .actions(&[
            ME.casts(names::MIND_BEND),
            ME.targets(names::CIRCLE_OF_PROTECTION_RED),
            Resolve,
            ME.chooses_word(TextWord::Color(Colors::RED)),
            ME.chooses_word(TextWord::Color(Colors::WHITE)),
            advance_to(2, TurnStep::BeginningOfCombat),
            THEM.passes(),
            ME.passes(),
            THEM.attacks_with(Prop::Flier),
            ME.activates(names::CIRCLE_OF_PROTECTION_RED, 0),
            ME.picks(Prop::Flier),
            advance_to(2, TurnStep::EndOfCombat),
            ME.life(20),
        ]);

pub const GARGANTUAN_COPIES_TARMOGOYF: &Scenario =
    &Scenario::new("the Gargantuan copies Tarmogoyf as a 7/7")
        .life(P0, 10)
        .battlefield(P1, &[names::TARMOGOYF])
        .interaction(&[
            names::TARMOGOYF,
            names::GLASSPOOL_MIMIC,
            names::LIGHTNING_BOLT,
        ])
        .battlefield(P0, &[names::ISLAND; 10])
        .hand(P0, &[names::QUICKSILVER_GARGANTUAN, names::GLASSPOOL_MIMIC])
        .graveyard(P1, &[names::PLAINS, names::LIGHTNING_BOLT])
        .actions(&[
            THEM.pt(names::TARMOGOYF, 2, 3),
            ME.casts(names::QUICKSILVER_GARGANTUAN),
            Resolve,
            ME.picks_up_to(names::TARMOGOYF),
            ME.pt(names::TARMOGOYF, 7, 7),
            ME.casts(names::GLASSPOOL_MIMIC),
            Resolve,
            ME.picks_up_to(names::TARMOGOYF),
            ME.controls(names::TARMOGOYF, 2),
            ME.has_subtype(names::TARMOGOYF, Subtype::Rogue, true),
            THEM.pt(names::TARMOGOYF, 2, 3),
        ]);

pub const BREACH_LOOP: &Scenario = &Scenario::new("the Breach loop empties a 53-card library")
    .interaction(&[names::LIONS_EYE_DIAMOND, names::BRAIN_FREEZE])
    .battlefield(
        P0,
        &[names::UNDERWORLD_BREACH, names::ISLAND, names::ISLAND],
    )
    .hand(P0, &[names::LIONS_EYE_DIAMOND, names::BRAIN_FREEZE])
    .library(P0, &[names::ISLAND; 30])
    .library(P1, &[names::PLAINS; 53])
    .actions(LOOP);

const LED: &str = names::LIONS_EYE_DIAMOND;
const FREEZE: &str = names::BRAIN_FREEZE;

const BLUE: [Action; 2] = [ME.activates(LED, 0), ME.chooses_color(Colors::BLUE)];

/// Escapes Lion's Eye Diamond and lets it resolve. Of the other cards in the graveyard,
/// Brain Freeze lies on top, so P0 picks an Island first and the rest follow.
const ESCAPE_LED: [Action; 3] = [ME.escapes(LED), ME.picks(names::ISLAND), Resolve];

const LOOP: &[Action] = &[
    // Round 1: storm 1.
    ME.casts(LED),
    Resolve,
    ME.casts(FREEZE),
    ME.targets_player(ME),
    BLUE[0],
    BLUE[1],
    Resolve,
    ME.targets_player(ME),
    Resolve,
    Resolve,
    ME.has_cards(Zone::Graveyard, 8),
    ME.mana(ManaColor::Blue, 3),
    // Round 2: storm 3.
    ESCAPE_LED[0],
    ESCAPE_LED[1],
    ESCAPE_LED[2],
    ME.escapes(FREEZE),
    ME.targets_player(ME),
    BLUE[0],
    BLUE[1],
    Resolve,
    ME.targets_player(ME),
    ME.targets_player(ME),
    ME.targets_player(ME),
    Resolve,
    Resolve,
    Resolve,
    Resolve,
    ME.has_cards(Zone::Graveyard, 14),
    ME.mana(ManaColor::Blue, 4),
    // Round 3: storm 5.
    ESCAPE_LED[0],
    ESCAPE_LED[1],
    ESCAPE_LED[2],
    ME.escapes(FREEZE),
    ME.targets_player(THEM),
    BLUE[0],
    BLUE[1],
    Resolve,
    ME.targets_player(ME),
    ME.targets_player(ME),
    ME.targets_player(THEM),
    ME.targets_player(THEM),
    ME.targets_player(THEM),
    Resolve,
    Resolve,
    Resolve,
    Resolve,
    Resolve,
    Resolve,
    THEM.has_cards(Zone::Library, 41),
    ME.has_cards(Zone::Graveyard, 14),
    ME.mana(ManaColor::Blue, 5),
    // Round 4: storm 7.
    ESCAPE_LED[0],
    ESCAPE_LED[1],
    ESCAPE_LED[2],
    ME.escapes(FREEZE),
    ME.targets_player(THEM),
    BLUE[0],
    BLUE[1],
    Resolve,
    ME.targets_player(THEM),
    ME.targets_player(THEM),
    ME.targets_player(THEM),
    ME.targets_player(THEM),
    ME.targets_player(THEM),
    ME.targets_player(THEM),
    ME.targets_player(THEM),
    Resolve,
    Resolve,
    Resolve,
    Resolve,
    Resolve,
    Resolve,
    Resolve,
    Resolve,
    THEM.has_cards(Zone::Library, 17),
    ME.has_cards(Zone::Graveyard, 8),
    ME.mana(ManaColor::Blue, 6),
    // Round 5: storm 9, and P1's library is empty.
    ESCAPE_LED[0],
    ESCAPE_LED[1],
    ESCAPE_LED[2],
    ME.escapes(FREEZE),
    ME.targets_player(THEM),
    Resolve,
    ME.targets_player(THEM),
    ME.targets_player(THEM),
    ME.targets_player(THEM),
    ME.targets_player(THEM),
    ME.targets_player(THEM),
    ME.targets_player(THEM),
    ME.targets_player(THEM),
    ME.targets_player(THEM),
    ME.targets_player(THEM),
    Resolve,
    Resolve,
    Resolve,
    Resolve,
    Resolve,
    Resolve,
    Resolve,
    Resolve,
    Resolve,
    Resolve,
    THEM.has_cards(Zone::Library, 0),
    ME.mana(ManaColor::Blue, 4),
];
