use beryl_card::prelude::{Castings, FaceIx};
use beryl_cards::props::Prop;
use beryl_cards::{names, tokens};
use beryl_scenario::prelude::{
    Action, CounterKind, DecisionKind, ME, P0, P1, Permanent, Resolve, Scenario, THEM, TurnStep,
    Zone, advance_to,
};

pub const ICEBERG_BOTTOM: Scenario =
    Scenario::new("exiles a permanent until it leaves; waterbend taps a creature to pay")
        .at(2, P1, TurnStep::End)
        .hand(P0, &[names::AANGS_ICEBERG])
        .battlefield(
            P0,
            &[names::PLAINS, names::PLAINS, names::PLAINS, Prop::Flier],
        )
        .permanents(
            P1,
            &[Permanent::new(Prop::Planeswalker).counters(CounterKind::Loyalty, 5)],
        )
        .library(
            P0,
            &[names::ISLAND, Prop::Artifact, names::PLAINS, names::ISLAND],
        )
        .library(P1, &[names::PLAINS])
        .actions(&[
            ME.casts(names::AANGS_ICEBERG),
            Resolve,
            ME.target_count(1),
            ME.targets(Prop::Planeswalker),
            Resolve,
            THEM.has(Zone::Exile, Prop::Planeswalker, 1),
            advance_to(3, TurnStep::PrecombatMain),
            ME.activates(names::AANGS_ICEBERG, 1), // Waterbend {3}
            ME.chooses(DecisionKind::ManaPayment, Prop::Flier),
            ME.tapped(names::PLAINS, 2),
            ME.tapped(Prop::Flier, 1),
            ME.picks_up_to(Prop::Artifact),
            ME.done_picking(),
            ME.library_top(&[names::PLAINS, names::ISLAND, Prop::Artifact]),
            ME.has(Zone::Graveyard, names::AANGS_ICEBERG, 1),
            THEM.has_loyalty(Prop::Planeswalker, 3),
        ]);

pub const ICEBERG_SCRY: Scenario =
    Scenario::new("waterbend taps a creature and an artifact; scry reorders the top")
        .battlefield(
            P0,
            &[
                names::AANGS_ICEBERG,
                names::PLAINS,
                Prop::Flier,
                Prop::Artifact,
            ],
        )
        .library(P0, &[names::ISLAND, names::PLAINS])
        .library(P1, &[names::PLAINS])
        .actions(&[
            ME.activates(names::AANGS_ICEBERG, 1), // Waterbend {3}
            ME.deciding(DecisionKind::ManaPayment, 2),
            ME.chooses(DecisionKind::ManaPayment, Prop::Artifact),
            ME.tapped(Prop::Flier, 1),
            ME.tapped(names::PLAINS, 1),
            ME.done_picking(),
            ME.picks(names::PLAINS),
            ME.library_top(&[names::PLAINS, names::ISLAND]),
        ]);

pub const ICEBERG_MANA_SOURCES: Scenario =
    Scenario::new("waterbend uses mana sources without sacrificing them")
        .interaction(&[
            names::MANA_CRYPT,
            names::JACE_VRYNS_PRODIGY,
            names::CURRENCY_CONVERTER,
        ])
        .battlefield(
            P0,
            &[
                names::AANGS_ICEBERG,
                names::PLAINS,
                names::MANA_CRYPT,
                names::JACE_VRYNS_PRODIGY,
                names::CURRENCY_CONVERTER,
            ],
        )
        .hand(P0, &[names::PLAINS])
        .library(P0, &[names::ISLAND, names::PLAINS, names::ISLAND])
        .library(P1, &[names::PLAINS])
        .actions(&[
            ME.activates(names::JACE_VRYNS_PRODIGY, 0), // {T}: draw, then discard
            Resolve,
            ME.picks(names::PLAINS),
            Resolve,
            ME.yes(),
            ME.activates(names::CURRENCY_CONVERTER, 2), // {T}: put an exiled card into a graveyard
            Resolve,
            ME.activates(names::AANGS_ICEBERG, 1), // Waterbend {3}
            ME.chooses(DecisionKind::ManaPayment, tokens::TREASURE_TOKEN),
            ME.chooses(DecisionKind::ManaPayment, names::MANA_CRYPT),
            ME.picks_up_to(names::ISLAND),
            ME.done_picking(),
            ME.tapped(tokens::TREASURE_TOKEN, 1),
            ME.tapped(names::MANA_CRYPT, 1),
        ]);

pub const THUNDERTRAP_TRAINER: Scenario = Scenario::new("Thundertrap Trainer takes a spell")
    .hand(P0, &[names::THUNDERTRAP_TRAINER])
    .battlefield(P0, &[names::ISLAND; 2])
    .library(
        P0,
        &[
            names::PLAINS,
            Prop::Instant,
            Prop::Flier,
            names::OUST,
            Prop::Artifact,
        ],
    )
    .library(P1, &[names::PLAINS])
    .actions(&[
        ME.casts(names::THUNDERTRAP_TRAINER),
        Resolve,
        Resolve,
        ME.yes(),
        ME.deciding(DecisionKind::ChooseObject, 2),
        ME.picks(names::OUST),
        ME.has(Zone::Hand, names::OUST, 1),
        ME.has_cards(Zone::Library, 4),
        advance_to(3, TurnStep::PrecombatMain),
        ME.has(Zone::Hand, Prop::Artifact, 1),
    ]);

pub const TRINKET_KNOWN_TOP: Scenario =
    Scenario::new("Trinket Mage searches with a known library top")
        .hand(P0, &[names::TRINKET_MAGE])
        .battlefield(
            P0,
            &[
                Prop::Planeswalker,
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
            ],
        )
        .library(
            P0,
            &[names::ISLAND, Prop::Artifact, names::OUST, names::PLAINS],
        )
        .library(P1, &[names::PLAINS])
        .actions(&[
            ME.activates(Prop::Planeswalker, 0), // the planeswalker's first ability
            ME.targets_player(ME),
            ME.no(),
            ME.casts(names::TRINKET_MAGE),
            Resolve,
            Resolve,
            ME.yes(),
            ME.picks_up_to(Prop::Artifact),
            ME.has(Zone::Hand, Prop::Artifact, 1),
        ]);

pub const WINDS_SINGLE: Scenario =
    Scenario::new("Winds of Abandon exiles a creature; its controller fetches a tapped basic")
        .battlefield(P0, &[names::PLAINS, names::PLAINS, Prop::Vanilla])
        .hand(P0, &[names::WINDS_OF_ABANDON, names::PLAINS])
        .battlefield(P1, &[Prop::Flier])
        .library(P1, &[names::TUNDRA, names::PLAINS, names::ISLAND])
        .actions(&[
            ME.casts(names::WINDS_OF_ABANDON),
            Resolve,
            THEM.picks_up_to(names::PLAINS),
            THEM.has(Zone::Exile, Prop::Flier, 1),
            ME.controls(Prop::Vanilla, 1),
            THEM.controls(names::PLAINS, 1),
            THEM.tapped(names::PLAINS, 1),
            THEM.has_cards(Zone::Library, 2),
        ]);

pub const WINDS_OVERLOAD: Scenario =
    Scenario::new("Winds of Abandon overloaded: a basic per creature exiled")
        .battlefield(
            P0,
            &[
                names::PLAINS,
                names::PLAINS,
                names::PLAINS,
                names::PLAINS,
                names::PLAINS,
                names::PLAINS,
                Prop::Vanilla,
            ],
        )
        .hand(P0, &[names::WINDS_OF_ABANDON, names::PLAINS])
        .battlefield(P1, &[Prop::Flier, Prop::Vanilla])
        .library(P1, &[names::ISLAND, names::PLAINS, names::TUNDRA])
        .actions(&[
            Action::Cast {
                player: P0,
                card: names::WINDS_OF_ABANDON,
                face: FaceIx::FRONT,
                costs: Castings::only(0),
            },
            Resolve,
            THEM.deciding(DecisionKind::ChooseUpTo, 3),
            THEM.picks_up_to(names::ISLAND),
            THEM.done_picking(),
            THEM.has_cards(Zone::Exile, 2),
            ME.controls(Prop::Vanilla, 1),
            THEM.controls(names::ISLAND, 1),
            THEM.controls(names::PLAINS, 0),
            THEM.tapped(names::ISLAND, 1),
        ]);

pub const SUBTLETY_PITCH: Scenario =
    Scenario::new("Subtlety with mana and blue cards: cast or evoke, then which card to exile")
        .at(2, P1, TurnStep::PrecombatMain)
        .battlefield(P0, &[names::ISLAND; 4])
        .hand(
            P0,
            &[names::SUBTLETY, Prop::UntargetedSpell, Prop::Planeswalker],
        )
        .battlefield(P1, &[names::PLAINS; 3])
        .hand(P1, &[Prop::Flier, names::PLAINS])
        .actions(&[
            THEM.casts(Prop::Flier),
            ME.deciding(DecisionKind::Priority, 3),
            Action::Cast {
                player: P0,
                card: names::SUBTLETY,
                face: FaceIx::FRONT,
                costs: Castings::only(0),
            },
            ME.deciding(DecisionKind::ChooseObject, 2),
            ME.picks(Prop::UntargetedSpell),
            ME.has(Zone::Exile, Prop::UntargetedSpell, 1),
            Resolve,
            ME.chooses(DecisionKind::TriggerOrder, names::SUBTLETY),
            ME.target_count(1),
            Resolve,
            Resolve,
            THEM.yes(),
            THEM.library_top(&[Prop::Flier, names::ISLAND]),
            ME.has(Zone::Graveyard, names::SUBTLETY, 1),
        ]);
