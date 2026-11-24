//! Positions used to exercise particular session views and animation scripts.
//! A script's view index includes assertions in the scenario's action list.

use beryl_card::prelude::{Castings, FaceIx, ManaColor};
use beryl_cards::props::Prop;
use beryl_cards::{names, tokens};
use beryl_scenario::prelude::{
    Action, CounterKind, DecisionKind, ME, P0, P1, Permanent, Resolve, Scenario, THEM, TurnStep,
    Zone, advance_to, stack, wins,
};

mod combat;
pub use combat::{
    ADELINE_ATTACKS_PLANESWALKER, ADELINE_TOKEN, EMPEROR_EXILES_ATTACKER, EMPEROR_GROWS_CREATURE,
    SPHINX, TEFERI_DIES_IN_COMBAT, TWO_PLANESWALKERS_ATTACKED,
};

mod permanents;
pub use permanents::{
    COPY_FACE_DOWN_PERMANENT, COPY_MERGED_CREATURE, FACE_DOWN_CREATURE_LEAVES,
    GEMRAZER_OVER_FACE_DOWN_CARD, GEMRAZER_UNDER_CREATURE, OCULUS_EXILES_SIX_CARDS,
    OCULUS_MANIFESTS_DREAD, TURN_MANIFESTED_CREATURE_FACE_UP,
};

pub const BEZA: &Scenario = &Scenario::new("Beza's Treasure pays for Counterspell")
    .hand(
        P0,
        &[
            names::BEZA_THE_BOUNDING_SPRING,
            names::COUNTERSPELL,
            names::ISLAND,
        ],
    )
    .battlefield(P0, &[names::PLAINS; 4])
    .battlefield(
        P1,
        &[
            names::PLAINS,
            names::PLAINS,
            names::ISLAND,
            names::ISLAND,
            names::ISLAND,
        ],
    )
    .hand(P1, &[Prop::Removal])
    .actions(&[
        ME.casts(names::BEZA_THE_BOUNDING_SPRING),
        Resolve,
        Resolve,
        ME.controls(tokens::TREASURE_TOKEN, 1),
        ME.plays(names::ISLAND),
        THEM.casts(Prop::Removal),
        THEM.targets(names::BEZA_THE_BOUNDING_SPRING),
        ME.deciding(DecisionKind::Priority, 2),
        ME.casts(names::COUNTERSPELL),
        ME.controls(tokens::TREASURE_TOKEN, 0),
        ME.mana(ManaColor::Blue, 0),
        Resolve,
        THEM.has(Zone::Graveyard, Prop::Removal, 1),
        ME.controls(names::BEZA_THE_BOUNDING_SPRING, 1),
    ]);

pub const COUNTERSPELL: &Scenario = &beryl_cards::library::actions::counter::GRAVEYARD[0];

pub const CREW_RECONFIGURED_VEHICLE: &Scenario =
    &Scenario::new("crewing the Vehicle makes it a creature until end of turn")
        .battlefield(
            P0,
            &[names::PLAINS, Prop::Vanilla, Prop::Flier, Prop::Flier],
        )
        .hand(P0, &[names::SWIFT_RECONFIGURATION])
        .library(P0, &[names::PLAINS, names::PLAINS])
        .library(P1, &[names::PLAINS, names::PLAINS])
        .actions(&[
            ME.casts(names::SWIFT_RECONFIGURATION),
            ME.targets(Prop::Vanilla),
            Resolve,
            ME.creature(Prop::Vanilla, false),
            Action::activate_granted(P0, Prop::Vanilla, 0),
            ME.tapped(Prop::Flier, 2),
            Resolve,
            ME.creature(Prop::Vanilla, true),
            ME.pt(Prop::Vanilla, 2, 1),
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.done_attacking(),
            advance_to(2, TurnStep::Upkeep),
            ME.creature(Prop::Vanilla, false),
        ]);

pub const DAMN_NORMAL_CAST: &Scenario = &Scenario::new("Damn for {B}{B} destroys its target alone")
    .battlefield(P0, &[names::SWAMP, names::SWAMP, Prop::Vanilla])
    .hand(P0, &[names::DAMN])
    .battlefield(P1, &[Prop::Flier])
    .library(P1, &[names::ISLAND])
    .actions(&[
        ME.casts(names::DAMN),
        ME.targets(Prop::Flier),
        Resolve,
        THEM.controls(Prop::Flier, 0),
        ME.controls(Prop::Vanilla, 1),
    ]);

pub const DAMN_OVERLOADED: &Scenario =
    &Scenario::new("Damn overloaded destroys each creature, targeting none")
        .battlefield(
            P0,
            &[
                names::PLAINS,
                names::PLAINS,
                names::PLAINS,
                names::PLAINS,
                Prop::Vanilla,
            ],
        )
        .hand(P0, &[names::DAMN])
        .battlefield(P1, &[Prop::Flier, Prop::Vanilla])
        .library(P1, &[names::ISLAND])
        .actions(&[
            ME.deciding(DecisionKind::Priority, 2),
            Action::Cast {
                player: P0,
                card: names::DAMN,
                face: FaceIx::FRONT,
                costs: Castings::only(0),
            },
            Resolve,
            ME.controls(Prop::Vanilla, 0),
            THEM.controls(Prop::Flier, 0),
            THEM.controls(Prop::Vanilla, 0),
            THEM.has_cards(Zone::Graveyard, 2),
            ME.has_cards(Zone::Graveyard, 2),
        ]);

pub const DAMN_SWEEPS_TOKENS: &Scenario =
    &Scenario::new("Damn overloaded takes tokens and face-down creatures at once")
        .interaction(&[names::ADELINE_RESPLENDENT_CATHAR])
        .battlefield(
            P0,
            &[
                names::PLAINS,
                names::PLAINS,
                names::PLAINS,
                names::PLAINS,
                names::ADELINE_RESPLENDENT_CATHAR,
            ],
        )
        .permanents(P0, &[Permanent::token(tokens::HUMAN_TOKEN)])
        .hand(P0, &[names::DAMN])
        .battlefield(P1, &[Prop::Flier])
        .permanents(P1, &[Permanent::new(Prop::Vanilla).face_down()])
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::Cast {
                player: P0,
                card: names::DAMN,
                face: FaceIx::FRONT,
                costs: Castings::only(0),
            },
            Resolve,
            ME.controls(names::ADELINE_RESPLENDENT_CATHAR, 0),
            ME.controls(tokens::HUMAN_TOKEN, 0),
            THEM.controls(Prop::Flier, 0),
            ME.has_cards(Zone::Graveyard, 2),
            THEM.has(Zone::Graveyard, Prop::Vanilla, 1),
        ]);

pub const JACES_OPPONENT_DRAWS_FROM_EMPTY_LIBRARY: &Scenario =
    &Scenario::new("the opponent's draw from an empty library still loses")
        .at(2, P1, TurnStep::Upkeep)
        .battlefield(P0, &[names::JACE_WIELDER_OF_MYSTERIES])
        .battlefield(P1, &[names::SWAMP, names::ISLAND, names::ISLAND])
        .hand(P1, &[Prop::FlashCreature])
        .library(P1, &[])
        .actions(&[THEM.passes(), wins(ME)]);

pub const JACE_DRAWS_LIBRARY_EMPTY: &Scenario =
    &Scenario::new("−8 draws the library empty and wins")
        .permanents(
            P0,
            &[Permanent::new(names::JACE_WIELDER_OF_MYSTERIES).counters(CounterKind::Loyalty, 8)],
        )
        .library(
            P0,
            &[
                names::ISLAND,
                names::PLAINS,
                names::ISLAND,
                names::PLAINS,
                names::ISLAND,
            ],
        )
        .actions(&[
            ME.activates(names::JACE_WIELDER_OF_MYSTERIES, 2), // −8
            wins(ME),
            ME.has_cards(Zone::Hand, 5),
        ]);

pub const JACE_MILLS_HIMSELF_EMPTY: &Scenario =
    &Scenario::new("+1 milling his controller empty wins on the draw")
        .battlefield(P0, &[names::JACE_WIELDER_OF_MYSTERIES])
        .library(P0, &[names::ISLAND, names::PLAINS])
        .actions(&[
            ME.activates(names::JACE_WIELDER_OF_MYSTERIES, 1), // +1
            ME.targets_player(ME),
            ME.has_cards(Zone::Graveyard, 2),
            wins(ME),
        ]);

pub const JACE_MILLS_THEN_DRAWS: &Scenario =
    &Scenario::new("+1 mills the target player two, then draws")
        .battlefield(P0, &[names::JACE_WIELDER_OF_MYSTERIES])
        .hand(P0, &[names::PLAINS])
        .library(P0, &[names::ISLAND, names::ISLAND])
        .library(P1, &[names::PLAINS, names::PLAINS, names::ISLAND])
        .actions(&[
            ME.activates(names::JACE_WIELDER_OF_MYSTERIES, 1), // +1
            ME.targets_player(THEM),
            Resolve,
            THEM.has_cards(Zone::Graveyard, 2),
            THEM.library_top(&[names::ISLAND]),
            ME.has_cards(Zone::Hand, 2),
            ME.has_loyalty(names::JACE_WIELDER_OF_MYSTERIES, 5),
        ]);

pub const MANA_CRYPT: &Scenario =
    &Scenario::new("Mana Crypt's flip lost: 3 damage to its controller")
        .seed(2)
        .at(2, P1, TurnStep::PostcombatMain)
        .battlefield(P0, &[names::MANA_CRYPT])
        .actions(&[
            advance_to(3, TurnStep::Upkeep),
            ME.passes(),
            ME.life(17),
            THEM.life(20),
        ]);

pub const MANA_DRAIN: &Scenario = &beryl_cards::library::mana::SCENARIOS[1];

pub const TITHE_PAYMENT: &Scenario = &beryl_cards::library::actions::counter::UNLESS_PAID[1];

pub const MAP_EXPLORES_LAND: &Scenario = &explore_with_map(
    "Get Lost on a creature gives two Maps; one explores a land into hand",
    &[names::PLAINS, names::ISLAND],
)
.actions(&[
    THEM.passes(),
    ME.casts(names::GET_LOST),
    THEM.has(Zone::Graveyard, Prop::Flier, 1),
    THEM.has(Zone::Battlefield, tokens::MAP_TOKEN, 2),
    ME.has(Zone::Battlefield, tokens::MAP_TOKEN, 0),
    THEM.activates(tokens::MAP_TOKEN, 0),
    THEM.has(Zone::Battlefield, tokens::MAP_TOKEN, 1),
    THEM.has(Zone::Hand, names::PLAINS, 1),
    THEM.library_top(&[names::ISLAND]),
    THEM.counters(Prop::Flier, CounterKind::PlusOne, 0),
]);

pub const MAP_EXPLORES_NONLAND: &Scenario = &explore_with_map(
    "A Map explores a nonland into a counter, left on top",
    &[names::OUST, names::ISLAND],
)
.actions(&[
    THEM.passes(),
    ME.casts(names::GET_LOST),
    THEM.activates(tokens::MAP_TOKEN, 0),
    THEM.no(),
    THEM.counters(Prop::Flier, CounterKind::PlusOne, 1),
    THEM.has(Zone::Graveyard, names::OUST, 0),
    THEM.library_top(&[names::OUST, names::ISLAND]),
]);

pub const MINDSLAVER: &Scenario = &beryl_cards::library::turn::CONTROLLED_TURN[0];

pub const OUST: &Scenario = &beryl_cards::library::library::SECOND_FROM_TOP[0];

pub const PORTABLE_HOLE: &Scenario =
    &Scenario::new("Portable Hole exiles until it leaves the battlefield")
        .seed(12)
        .battlefield(P0, &[names::PLAINS])
        .permanents(P0, &[Permanent::new(Prop::Vanilla).sick()])
        .hand(P0, &[names::PORTABLE_HOLE])
        .library(P0, &[names::ISLAND])
        .battlefield(
            P1,
            &[
                Prop::Vanilla,
                Prop::Flier,
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
            ],
        )
        .hand(P1, &[Prop::Removal])
        .library(P1, &[names::ISLAND, names::ISLAND])
        .actions(&[
            ME.casts(names::PORTABLE_HOLE),
            Resolve,
            Resolve,
            THEM.has(Zone::Exile, Prop::Vanilla, 1),
            THEM.controls(Prop::Flier, 1),
            advance_to(2, TurnStep::PrecombatMain),
            THEM.casts(Prop::Removal),
            THEM.targets(names::PORTABLE_HOLE),
            Resolve,
            ME.has(Zone::Hand, names::PORTABLE_HOLE, 1),
            THEM.has(Zone::Exile, Prop::Vanilla, 0),
            THEM.controls(Prop::Vanilla, 1),
        ]);

pub const PORTABLE_HOLE_TARGET_GONE: &Scenario =
    &Scenario::new("Portable Hole's target gone before the trigger resolves")
        .battlefield(
            P0,
            &[names::PLAINS, names::ISLAND, names::ISLAND, names::ISLAND],
        )
        .hand(P0, &[names::PORTABLE_HOLE, Prop::Removal])
        .library(P0, &[names::ISLAND])
        .battlefield(P1, &[Prop::Vanilla])
        .actions(&[
            ME.casts(names::PORTABLE_HOLE),
            Resolve,
            stack(1),
            ME.casts(Prop::Removal),
            Resolve,
            THEM.has(Zone::Hand, Prop::Vanilla, 1),
            Resolve,
            stack(0),
            THEM.has(Zone::Exile, Prop::Vanilla, 0),
            ME.controls(names::PORTABLE_HOLE, 1),
        ]);

pub const REPRIEVE_COUNTERSPELL: &Scenario =
    &Scenario::new("Reprieve on a counterspell saves the spell it targets")
        .battlefield(P0, &[names::PLAINS; 3])
        .hand(P0, &[Prop::Artifact, names::REPRIEVE])
        .battlefield(P1, &[names::ISLAND; 2])
        .hand(P1, &[names::COUNTERSPELL])
        .library(P1, &[names::PLAINS])
        .actions(&[
            ME.casts(Prop::Artifact),
            ME.passes(),
            THEM.casts(names::COUNTERSPELL),
            ME.casts(names::REPRIEVE),
            ME.targets(names::COUNTERSPELL),
            Resolve,
            THEM.has(Zone::Hand, names::COUNTERSPELL, 1),
            ME.has(Zone::Hand, names::ISLAND, 1),
            Resolve,
            ME.has(Zone::Battlefield, Prop::Artifact, 1),
        ]);

pub const REPRIEVE_SPELL_WITH_X: &Scenario =
    &Scenario::new("Reprieve returns a spell with X to its owner's hand and draws")
        .battlefield(P0, &[names::ISLAND; 4])
        .hand(P0, &[names::THIEVING_SKYDIVER, names::PLAINS])
        .battlefield(P1, &[names::PLAINS, names::ISLAND])
        .hand(P1, &[names::REPRIEVE])
        .library(P1, &[names::PLAINS])
        .actions(&[
            Action::Cast {
                player: P0,
                card: names::THIEVING_SKYDIVER,
                face: FaceIx::FRONT,
                costs: Castings::only(0),
            },
            ME.x(1),
            THEM.casts(names::REPRIEVE),
            ME.has(Zone::Hand, names::THIEVING_SKYDIVER, 1),
            ME.has(Zone::Battlefield, names::THIEVING_SKYDIVER, 0),
            ME.tapped(names::ISLAND, 3),
            THEM.has(Zone::Hand, names::PLAINS, 1),
            THEM.has(Zone::Graveyard, names::REPRIEVE, 1),
        ]);

pub const SHOW_AND_TELL: &Scenario =
    &Scenario::new("Show and Tell puts both players' creatures onto the battlefield")
        .interaction(&[names::GRISELBRAND])
        .hand(P0, &[names::SHOW_AND_TELL, names::GRISELBRAND])
        .battlefield(P0, &[names::ISLAND; 3])
        .hand(P1, &[Prop::Vanilla])
        .actions(&[
            ME.casts(names::SHOW_AND_TELL),
            ME.picks_up_to(names::GRISELBRAND),
            THEM.picks_up_to(Prop::Vanilla),
            ME.controls(names::GRISELBRAND, 1),
            THEM.controls(Prop::Vanilla, 1),
            ME.has(Zone::Graveyard, names::SHOW_AND_TELL, 1),
        ]);

pub const SKYDIVER: &Scenario =
    &Scenario::new("Thieving Skydiver kicked steals Mana Crypt over a Map")
        .interaction(&[names::MANA_CRYPT, names::GET_LOST])
        .hand(P0, &[names::GET_LOST, names::THIEVING_SKYDIVER])
        .battlefield(
            P0,
            &[
                names::PLAINS,
                names::PLAINS,
                names::PLAINS,
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
            ],
        )
        .battlefield(P1, &[names::MANA_CRYPT, Prop::Flier])
        .library(P0, &[names::ISLAND])
        .library(P1, &[names::PLAINS])
        .actions(&[
            ME.casts(names::GET_LOST),
            Resolve,
            THEM.controls(tokens::MAP_TOKEN, 2),
            ME.tapped(names::PLAINS, 2),
            Action::Cast {
                player: P0,
                card: names::THIEVING_SKYDIVER,
                face: FaceIx::FRONT,
                costs: Castings::only(0),
            },
            ME.x(1),
            Resolve,
            ME.deciding(DecisionKind::Target, 2),
            ME.targets(names::MANA_CRYPT),
            Resolve,
            ME.controls(names::MANA_CRYPT, 1),
            advance_to(3, TurnStep::PrecombatMain),
            ME.controls(names::MANA_CRYPT, 1),
            THEM.controls(tokens::MAP_TOKEN, 2),
        ]);

pub const SNAPCASTER_MAGE: &Scenario =
    &beryl_cards::library::keywords::flashback::INSTANT_RESPONSE[0];

pub const SPELLBINDER: &Scenario = &Scenario::new("Spellbinder's card costs its owner {2} more")
    .hand(P0, &[names::ELITE_SPELLBINDER])
    .battlefield(P0, &[names::PLAINS; 3])
    .hand(P1, &[Prop::Artifact, names::ISLAND])
    .battlefield(P1, &[names::PLAINS; 3])
    .library(P1, &[names::ISLAND])
    .actions(&[
        ME.casts(names::ELITE_SPELLBINDER),
        Resolve,
        stack(1),
        Resolve,
        ME.yes(),
        THEM.has(Zone::Exile, Prop::Artifact, 1),
        THEM.has(Zone::Hand, Prop::Artifact, 0),
        advance_to(2, TurnStep::PrecombatMain),
        THEM.deciding(DecisionKind::Priority, 4),
        THEM.casts(Prop::Artifact),
        THEM.tapped(names::PLAINS, 3),
        Resolve,
        THEM.controls(Prop::Artifact, 1),
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

pub const STOCK_UP: &Scenario =
    &Scenario::new("Stock Up takes two of five and orders the rest on the bottom")
        .battlefield(P0, &[names::ISLAND; 3])
        .hand(P0, &[names::STOCK_UP])
        .library(
            P0,
            &[
                names::ISLAND,
                names::PLAINS,
                Prop::Artifact,
                names::COUNTERSPELL,
                Prop::Sorcery,
                names::TUNDRA,
            ],
        )
        .actions(&[
            ME.casts(names::STOCK_UP),
            Resolve,
            ME.deciding(DecisionKind::ChooseObject, 5),
            ME.picks(Prop::Artifact),
            ME.picks(names::COUNTERSPELL),
            ME.picks(Prop::Sorcery),
            ME.picks(names::PLAINS),
            ME.has(Zone::Hand, Prop::Artifact, 1),
            ME.has(Zone::Hand, names::COUNTERSPELL, 1),
            ME.library_top(&[names::TUNDRA, Prop::Sorcery, names::PLAINS, names::ISLAND]),
        ]);

pub const SUBTLETY_EVOKED: &Scenario =
    &Scenario::new("Subtlety evoked: sacrificed, its trigger still tucks a planeswalker spell")
        .interaction(&[names::TEFERI_TIME_RAVELER])
        .at(2, P1, TurnStep::PrecombatMain)
        .hand(P0, &[names::SUBTLETY, Prop::UntargetedSpell])
        .battlefield(P1, &[names::PLAINS, names::ISLAND, names::ISLAND])
        .hand(P1, &[names::TEFERI_TIME_RAVELER, names::PLAINS])
        .actions(&[
            THEM.casts(names::TEFERI_TIME_RAVELER),
            Action::Cast {
                player: P0,
                card: names::SUBTLETY,
                face: FaceIx::FRONT,
                costs: Castings::only(0),
            },
            ME.has(Zone::Exile, Prop::UntargetedSpell, 1),
            Resolve,
            ME.chooses(DecisionKind::TriggerOrder, names::SUBTLETY),
            ME.target_count(1),
            Resolve,
            stack(2),
            Resolve,
            THEM.no(),
            THEM.library_top(&[names::ISLAND, names::TEFERI_TIME_RAVELER]),
            ME.has(Zone::Graveyard, names::SUBTLETY, 1),
        ]);

pub const SUBTLETY_HARD_CAST: &Scenario =
    &Scenario::new("Subtlety hard cast: the creature spell's owner puts it on top")
        .at(2, P1, TurnStep::PrecombatMain)
        .battlefield(P0, &[names::ISLAND; 4])
        .hand(P0, &[names::SUBTLETY])
        .battlefield(P1, &[names::PLAINS; 3])
        .hand(P1, &[Prop::Flier, names::PLAINS])
        .actions(&[
            THEM.casts(Prop::Flier),
            ME.deciding(DecisionKind::Priority, 2),
            ME.casts(names::SUBTLETY),
            Resolve,
            ME.target_count(1),
            Resolve,
            THEM.yes(),
            THEM.library_top(&[Prop::Flier, names::ISLAND]),
            stack(0),
            ME.controls(names::SUBTLETY, 1),
        ]);

pub const SWIFT_RECONFIGURATION_ENTERS: &Scenario =
    &Scenario::new("a creature becomes a noncreature Vehicle that can't crew alone")
        .at(2, P1, TurnStep::PrecombatMain)
        .hand(P0, &[names::SWIFT_RECONFIGURATION])
        .battlefield(P0, &[names::PLAINS])
        .battlefield(P1, &[Prop::Vanilla])
        .hand(P1, &[names::PLAINS])
        .library(P0, &[names::PLAINS])
        .library(P1, &[names::PLAINS, names::PLAINS])
        .actions(&[
            THEM.passes(),
            ME.casts(names::SWIFT_RECONFIGURATION),
            THEM.creature(Prop::Vanilla, false),
            THEM.deciding(DecisionKind::Priority, 2),
            ME.controls(names::SWIFT_RECONFIGURATION, 1),
        ]);

pub const TEFERIS_PROTECTION: &Scenario =
    &Scenario::new("a creature phased out in response to Lightning Bolt survives")
        .at(2, P1, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[names::PLAINS, names::PLAINS, names::PLAINS, Prop::Vanilla],
        )
        .hand(P0, &[names::TEFERIS_PROTECTION])
        .battlefield(P1, &[names::MOUNTAIN, Prop::Flier])
        .hand(P1, &[names::LIGHTNING_BOLT])
        .library(P0, &[names::PLAINS; 3])
        .library(P1, &[names::PLAINS; 3])
        .actions(&[
            THEM.casts(names::LIGHTNING_BOLT),
            THEM.targets(Prop::Vanilla),
            ME.casts(names::TEFERIS_PROTECTION),
            Resolve,
            ME.has(Zone::Battlefield, Prop::Vanilla, 0),
            advance_to(2, TurnStep::BeginningOfCombat),
            THEM.has(Zone::Graveyard, names::LIGHTNING_BOLT, 1),
            THEM.passes(),
            THEM.done_attacking(),
            advance_to(3, TurnStep::Upkeep),
            ME.has(Zone::Battlefield, Prop::Vanilla, 1),
            ME.has(Zone::Graveyard, Prop::Vanilla, 0),
        ]);

pub const TEFERI_RETURNS_ARTIFACT: &Scenario =
    &Scenario::new("−3 returns an opposing artifact and draws a card")
        .battlefield(P0, &[names::TEFERI_TIME_RAVELER])
        .battlefield(P1, &[Prop::Artifact])
        .library(P1, &[names::PLAINS])
        .actions(&[
            ME.activates(names::TEFERI_TIME_RAVELER, 2), // −3
            ME.target_count(1),
            THEM.has(Zone::Hand, Prop::Artifact, 1),
            ME.has(Zone::Hand, names::ISLAND, 1),
            ME.has_loyalty(names::TEFERI_TIME_RAVELER, 1),
            ME.deciding(DecisionKind::Priority, 2),
        ]);

pub const THIRST: &Scenario =
    &Scenario::new("Thirst for Discovery: discarding two cards with a basic land in hand")
        .battlefield(P0, &[names::ISLAND; 3])
        .hand(P0, &[names::THIRST_FOR_DISCOVERY])
        .library(
            P0,
            &[
                names::PLAINS,
                Prop::Artifact,
                names::COUNTERSPELL,
                names::ISLAND,
            ],
        )
        .actions(&[
            ME.casts(names::THIRST_FOR_DISCOVERY),
            Resolve,
            ME.no(),
            ME.picks(Prop::Artifact),
            ME.picks(names::PLAINS),
            ME.has(Zone::Graveyard, Prop::Artifact, 1),
            ME.has(Zone::Graveyard, names::PLAINS, 1),
            ME.has(Zone::Hand, names::COUNTERSPELL, 1),
        ]);

pub const TIMETWISTER: &Scenario = &beryl_cards::library::actions::shuffle::SCENARIOS[1];

pub const TRINKET_MAGE_FINDS_MANA_CRYPT: &Scenario =
    &Scenario::new("Trinket Mage finds Mana Crypt")
        .interaction(&[names::MANA_CRYPT, names::STAFF_OF_THE_STORYTELLER])
        .hand(P0, &[names::TRINKET_MAGE])
        .battlefield(P0, &[names::ISLAND; 3])
        .library(
            P0,
            &[
                names::ISLAND,
                names::STAFF_OF_THE_STORYTELLER,
                names::PORTABLE_HOLE,
                names::OUST,
                names::MANA_CRYPT,
                names::PLAINS,
            ],
        )
        .library(P1, &[names::PLAINS])
        .actions(&[
            ME.casts(names::TRINKET_MAGE),
            Resolve,
            Resolve,
            ME.yes(),
            ME.deciding(DecisionKind::ChooseUpTo, 3),
            ME.picks_up_to(names::MANA_CRYPT),
            ME.has(Zone::Hand, names::MANA_CRYPT, 1),
            ME.has_cards(Zone::Library, 5),
            ME.has(Zone::Battlefield, names::TRINKET_MAGE, 1),
        ]);

pub const TRINKET_MAGE_FINDS_PORTABLE_HOLE: &Scenario =
    &beryl_cards::library::actions::search::FIND_AND_REVEAL[0];

const fn explore_with_map(name: &'static str, top: &'static [&'static str]) -> Scenario {
    Scenario::new(name)
        .at(2, P1, TurnStep::PrecombatMain)
        .battlefield(P0, &[names::PLAINS; 3])
        .hand(P0, &[names::GET_LOST])
        .library(P0, &[names::PLAINS])
        .battlefield(P1, &[Prop::Flier, Prop::Flier, names::PLAINS])
        .hand(P1, &[names::ISLAND])
        .library(P1, top)
}
