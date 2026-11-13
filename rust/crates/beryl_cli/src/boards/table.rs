//! Boards that show the table itself: its piles, marks, signals and permanents.

use beryl_cards::names::{
    ABHORRENT_OCULUS, BEZA_THE_BOUNDING_SPRING, COUNTERSPELL, CURRENCY_CONVERTER, DAMN,
    ELITE_SPELLBINDER, FRACTURED_IDENTITY, GET_LOST, ISLAND, JACE_THE_MIND_SCULPTOR,
    JACE_VRYNS_PRODIGY, MALCOLM_ALLURING_SCOUNDREL, MANA_CRYPT, MANA_DRAIN, NO_MORE_LIES, OUST,
    OVERLORD_OF_THE_MISTMOORS, PLAINS, PORTABLE_HOLE, REPRIEVE, SEACHROME_COAST, SNAPCASTER_MAGE,
    STAFF_OF_THE_STORYTELLER, STOCK_UP, SUBTLETY, SWIFT_RECONFIGURATION, TEFERI_HERO_OF_DOMINARIA,
    TEFERI_TIME_RAVELER, THE_WANDERING_EMPEROR, THIEVING_SKYDIVER, THIRST_FOR_DISCOVERY,
    THUNDERTRAP_TRAINER, TIME_SPIRAL, TRINKET_MAGE, TUNDRA, WINDS_OF_ABANDON,
};
use beryl_engine::state::CounterKind;
use beryl_engine::{DecisionKind, TurnStep};
use beryl_scenario::{Action, P0, P1, Permanent, Pick, Scenario};

use super::shorthand::{
    ISLAND_, JACE_TELEPATH_UNBOUND, LANDS, PLAINS_, cast_with_option, choose, play_land,
};
use super::tables::wu_mirror;

/// Boards that show the table itself: its piles, marks, signals and permanents.
pub(super) const TABLE: &[Scenario] = &[
    // The human casts Thundertrap Trainer and takes nothing; in the opponent's turn they play
    // a land and Stock Up, and the human has nothing to do until their next main phase.
    Scenario::new("opponent-turn")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(P0, &[ISLAND_, ISLAND_, PLAINS_])
        .hand(P0, &[THUNDERTRAP_TRAINER])
        .library(
            P0,
            &[
                COUNTERSPELL,
                DAMN,
                OUST,
                JACE_THE_MIND_SCULPTOR,
                TEFERI_HERO_OF_DOMINARIA,
                TEFERI_HERO_OF_DOMINARIA,
                JACE_THE_MIND_SCULPTOR,
            ],
        )
        .permanents(P1, &[ISLAND_, ISLAND_, ISLAND_])
        .hand(P1, &[PLAINS, STOCK_UP])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P0, THUNDERTRAP_TRAINER),
            Action::answer(P0, DecisionKind::YesNo, Pick::No),
            play_land(P1, PLAINS),
            Action::cast(P1, STOCK_UP),
        ]),
    // An opening hand holding two Plains.
    Scenario::new("hand-pair")
        .hand(
            P0,
            &[
                SNAPCASTER_MAGE,
                PLAINS,
                ISLAND,
                MANA_CRYPT,
                PLAINS,
                ABHORRENT_OCULUS,
                TEFERI_TIME_RAVELER,
            ],
        )
        .library(P0, LANDS)
        .hand(P1, &[ISLAND, PLAINS, ISLAND, PLAINS, ISLAND, PLAINS])
        .library(P1, LANDS),
    // Twins with one mark between them: +1/+1 counters, a story counter, and a Thieving
    // Skydiver just cast beside one that has been there.
    Scenario::new("marks")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(
            P0,
            &[
                Permanent::new(TRINKET_MAGE).counters(CounterKind::PlusOne, 2),
                Permanent::new(TRINKET_MAGE),
                Permanent::new(STAFF_OF_THE_STORYTELLER).counters(CounterKind::Story, 1),
                Permanent::new(STAFF_OF_THE_STORYTELLER),
                Permanent::new(THIEVING_SKYDIVER),
                ISLAND_,
                ISLAND_,
                PLAINS_,
                PLAINS_,
            ],
        )
        .hand(P0, &[THIEVING_SKYDIVER])
        .library(P0, LANDS)
        .permanents(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[Action::cast(P0, THIEVING_SKYDIVER)]),
    // A Snapcaster Mage flashed in beside one that has been there, and a Thundertrap Trainer
    // cast with offspring beside its token copy; a land to play keeps the human's priority.
    Scenario::new("signals")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(
            P0,
            &[
                Permanent::new(SNAPCASTER_MAGE),
                ISLAND_,
                ISLAND_,
                ISLAND_,
                ISLAND_,
                ISLAND_,
                ISLAND_,
                ISLAND_,
                ISLAND_,
            ],
        )
        .hand(P0, &[SNAPCASTER_MAGE, THUNDERTRAP_TRAINER, ISLAND])
        .library(P0, LANDS)
        .permanents(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P0, SNAPCASTER_MAGE),
            cast_with_option(P0, THUNDERTRAP_TRAINER),
        ]),
    // Subtlety blocks one of two Abhorrent Oculi and leaves it damaged; a land to play keeps
    // the human's priority after combat.
    Scenario::new("marks-damage")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(
            P0,
            &[
                Permanent::new(ABHORRENT_OCULUS),
                Permanent::new(ABHORRENT_OCULUS),
                ISLAND_,
                ISLAND_,
                PLAINS_,
            ],
        )
        .hand(P0, &[PLAINS])
        .library(P0, LANDS)
        .permanents(P1, &[Permanent::new(SUBTLETY), ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[
            choose(P0, DecisionKind::Attacker, ABHORRENT_OCULUS),
            Action::answer(P0, DecisionKind::Attacker, Pick::Done),
            choose(P1, DecisionKind::Blocker, SUBTLETY),
        ]),
    // Get Lost on the human's own Snapcaster Mage makes two Map tokens; Overlord of the
    // Mistmoors' Insects, one of them under Swift Reconfiguration; and a kicked Thieving
    // Skydiver steals Mana Crypt.
    Scenario::new("tokens")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(
            P0,
            &[
                Permanent::new(SNAPCASTER_MAGE),
                PLAINS_,
                PLAINS_,
                PLAINS_,
                PLAINS_,
                PLAINS_,
                PLAINS_,
                ISLAND_,
                ISLAND_,
                ISLAND_,
                ISLAND_,
                ISLAND_,
            ],
        )
        .hand(
            P0,
            &[
                GET_LOST,
                OVERLORD_OF_THE_MISTMOORS,
                SWIFT_RECONFIGURATION,
                THIEVING_SKYDIVER,
            ],
        )
        .library(P0, LANDS)
        .permanents(P1, &[Permanent::new(MANA_CRYPT), ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(&[
            Action::cast(P0, GET_LOST),
            cast_with_option(P0, OVERLORD_OF_THE_MISTMOORS),
            Action::cast(P0, SWIFT_RECONFIGURATION),
            cast_with_option(P0, THIEVING_SKYDIVER),
            Action::answer(P0, DecisionKind::X, Pick::Number { n: 1 }),
            choose(P0, DecisionKind::Target, MANA_CRYPT),
        ]),
    // Teferi, Time Raveler at 1 loyalty: its +1 affordable, its −3 not, though the opponent's
    // Snapcaster Mage could be its target.
    Scenario::new("walker-low")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(
            P0,
            &[
                Permanent::new(TEFERI_TIME_RAVELER).counters(CounterKind::Loyalty, 1),
                ISLAND_,
                PLAINS_,
            ],
        )
        .library(P0, LANDS)
        .permanents(P1, &[Permanent::new(SNAPCASTER_MAGE), ISLAND_, PLAINS_])
        .library(P1, LANDS),
    // A crowded table, nine and ten nonland permanents a side, among them planeswalkers, a pile
    // of twins, counters and a tapped one: the opponent casts Stock Up, which the human may
    // counter.
    Scenario::new("crowded")
        .at(4, P1, TurnStep::PrecombatMain)
        .permanents(
            P0,
            &[
                Permanent::new(SNAPCASTER_MAGE),
                Permanent::new(TRINKET_MAGE),
                Permanent::new(JACE_VRYNS_PRODIGY),
                Permanent::new(THUNDERTRAP_TRAINER),
                Permanent::new(STAFF_OF_THE_STORYTELLER).counters(CounterKind::Story, 2),
                Permanent::new(CURRENCY_CONVERTER).tapped(),
                Permanent::new(MANA_CRYPT),
                Permanent::new(JACE_THE_MIND_SCULPTOR),
                Permanent::new(THE_WANDERING_EMPEROR),
                ISLAND_,
                ISLAND_,
                ISLAND_,
                PLAINS_,
                PLAINS_,
                Permanent::new(TUNDRA),
            ],
        )
        .hand(P0, &[COUNTERSPELL, MANA_DRAIN, OUST, ISLAND])
        .library(P0, LANDS)
        .permanents(
            P1,
            &[
                Permanent::new(BEZA_THE_BOUNDING_SPRING),
                Permanent::new(SUBTLETY),
                Permanent::new(TRINKET_MAGE),
                Permanent::new(SNAPCASTER_MAGE),
                Permanent::new(ELITE_SPELLBINDER),
                Permanent::new(MALCOLM_ALLURING_SCOUNDREL),
                Permanent::new(THIEVING_SKYDIVER),
                Permanent::new(STAFF_OF_THE_STORYTELLER),
                Permanent::new(STAFF_OF_THE_STORYTELLER),
                Permanent::new(TEFERI_HERO_OF_DOMINARIA),
                ISLAND_,
                ISLAND_,
                ISLAND_,
                PLAINS_,
                PLAINS_,
                Permanent::new(SEACHROME_COAST),
            ],
        )
        .hand(P1, &[STOCK_UP])
        .library(P1, LANDS)
        .actions(&[Action::cast(P1, STOCK_UP)]),
    // More permanents than the phone's rows can show at full width: fourteen creatures and
    // twelve other nonland permanents on each side, with tapped copies that cannot stack.
    Scenario::new("crowded-max")
        .at(4, P0, TurnStep::PrecombatMain)
        .permanents(
            P0,
            &[
                Permanent::new(SNAPCASTER_MAGE),
                Permanent::new(SNAPCASTER_MAGE).tapped(),
                Permanent::new(TRINKET_MAGE),
                Permanent::new(TRINKET_MAGE).tapped(),
                Permanent::new(JACE_VRYNS_PRODIGY),
                Permanent::new(THUNDERTRAP_TRAINER),
                Permanent::new(THUNDERTRAP_TRAINER).tapped(),
                Permanent::new(BEZA_THE_BOUNDING_SPRING),
                Permanent::new(SUBTLETY),
                Permanent::new(SUBTLETY).tapped(),
                Permanent::new(ELITE_SPELLBINDER),
                Permanent::new(MALCOLM_ALLURING_SCOUNDREL),
                Permanent::new(THIEVING_SKYDIVER),
                Permanent::new(ABHORRENT_OCULUS),
                Permanent::new(STAFF_OF_THE_STORYTELLER),
                Permanent::new(STAFF_OF_THE_STORYTELLER).counters(CounterKind::Story, 2),
                Permanent::new(STAFF_OF_THE_STORYTELLER).tapped(),
                Permanent::new(CURRENCY_CONVERTER),
                Permanent::new(CURRENCY_CONVERTER).tapped(),
                Permanent::new(MANA_CRYPT),
                Permanent::new(MANA_CRYPT).tapped(),
                Permanent::new(PORTABLE_HOLE),
                Permanent::new(PORTABLE_HOLE).tapped(),
                Permanent::new(TEFERI_HERO_OF_DOMINARIA),
                Permanent::new(JACE_THE_MIND_SCULPTOR),
                Permanent::new(THE_WANDERING_EMPEROR),
                ISLAND_,
                ISLAND_,
                PLAINS_,
                PLAINS_,
            ],
        )
        .hand(P0, &[COUNTERSPELL])
        .library(P0, LANDS)
        .permanents(
            P1,
            &[
                Permanent::new(SNAPCASTER_MAGE),
                Permanent::new(SNAPCASTER_MAGE).tapped(),
                Permanent::new(TRINKET_MAGE),
                Permanent::new(TRINKET_MAGE).tapped(),
                Permanent::new(JACE_VRYNS_PRODIGY),
                Permanent::new(THUNDERTRAP_TRAINER),
                Permanent::new(THUNDERTRAP_TRAINER).tapped(),
                Permanent::new(BEZA_THE_BOUNDING_SPRING),
                Permanent::new(SUBTLETY),
                Permanent::new(SUBTLETY).tapped(),
                Permanent::new(ELITE_SPELLBINDER),
                Permanent::new(MALCOLM_ALLURING_SCOUNDREL),
                Permanent::new(THIEVING_SKYDIVER),
                Permanent::new(ABHORRENT_OCULUS),
                Permanent::new(STAFF_OF_THE_STORYTELLER),
                Permanent::new(STAFF_OF_THE_STORYTELLER).counters(CounterKind::Story, 2),
                Permanent::new(STAFF_OF_THE_STORYTELLER).tapped(),
                Permanent::new(CURRENCY_CONVERTER),
                Permanent::new(CURRENCY_CONVERTER).tapped(),
                Permanent::new(MANA_CRYPT),
                Permanent::new(MANA_CRYPT).tapped(),
                Permanent::new(PORTABLE_HOLE),
                Permanent::new(PORTABLE_HOLE).tapped(),
                Permanent::new(TEFERI_HERO_OF_DOMINARIA),
                Permanent::new(JACE_THE_MIND_SCULPTOR),
                Permanent::new(THE_WANDERING_EMPEROR),
                ISLAND_,
                ISLAND_,
                PLAINS_,
                PLAINS_,
            ],
        )
        .library(P1, LANDS),
    // Six other permanents a side, a back row reaching past the opponent's piles in the far
    // corner, and thick libraries, graveyards and exiles on both sides: the piles lie clear of
    // every permanent (by-lomm).
    Scenario::new("piles-crowded")
        .at(5, P0, TurnStep::PrecombatMain)
        .permanents(
            P0,
            &[
                Permanent::new(SNAPCASTER_MAGE),
                Permanent::new(STAFF_OF_THE_STORYTELLER),
                Permanent::new(CURRENCY_CONVERTER),
                Permanent::new(JACE_THE_MIND_SCULPTOR),
                Permanent::new(THE_WANDERING_EMPEROR),
                Permanent::new(TEFERI_HERO_OF_DOMINARIA),
                Permanent::new(TEFERI_TIME_RAVELER),
                ISLAND_,
                ISLAND_,
                PLAINS_,
                Permanent::new(TUNDRA),
                Permanent::new(SEACHROME_COAST),
            ],
        )
        .hand(P0, &[COUNTERSPELL, OUST])
        .graveyard(
            P0,
            &[
                MANA_DRAIN,
                STOCK_UP,
                DAMN,
                REPRIEVE,
                THIRST_FOR_DISCOVERY,
                NO_MORE_LIES,
            ],
        )
        .exile(
            P0,
            &[TIME_SPIRAL, SUBTLETY, MALCOLM_ALLURING_SCOUNDREL, GET_LOST],
        )
        .library(P0, &[ISLAND; 30])
        .permanents(
            P1,
            &[
                Permanent::new(TRINKET_MAGE),
                Permanent::new(STAFF_OF_THE_STORYTELLER),
                Permanent::new(CURRENCY_CONVERTER),
                Permanent::new(JACE_THE_MIND_SCULPTOR),
                Permanent::new(THE_WANDERING_EMPEROR),
                Permanent::new(TEFERI_HERO_OF_DOMINARIA),
                Permanent::new(TEFERI_TIME_RAVELER),
                ISLAND_,
                ISLAND_,
                PLAINS_,
                Permanent::new(TUNDRA),
            ],
        )
        .graveyard(
            P1,
            &[
                SWIFT_RECONFIGURATION,
                STOCK_UP,
                DAMN,
                COUNTERSPELL,
                FRACTURED_IDENTITY,
                REPRIEVE,
            ],
        )
        .exile(P1, &[SNAPCASTER_MAGE, OUST, TIME_SPIRAL, WINDS_OF_ABANDON])
        .library(P1, &[PLAINS; 30]),
    // Four planeswalkers and an emblem a side, and a Portable Hole a side holding the other's
    // Snapcaster Mage, beside a full hand for the opponent: the human's Teferi, Hero of
    // Dominaria −8 and Hole in their turn, then the opponent's Jace, Telepath Unbound −9 and
    // Hole in theirs.
    Scenario::new("walkers")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(
            P0,
            &[
                Permanent::new(TEFERI_HERO_OF_DOMINARIA).counters(CounterKind::Loyalty, 9),
                Permanent::new(JACE_THE_MIND_SCULPTOR),
                Permanent::new(THE_WANDERING_EMPEROR),
                Permanent::new(JACE_VRYNS_PRODIGY).transformed(),
                Permanent::new(SNAPCASTER_MAGE),
                ISLAND_,
                ISLAND_,
                PLAINS_,
                PLAINS_,
            ],
        )
        .hand(P0, &[PORTABLE_HOLE, COUNTERSPELL, OUST, ISLAND])
        .library(P0, LANDS)
        .permanents(
            P1,
            &[
                Permanent::new(JACE_VRYNS_PRODIGY)
                    .transformed()
                    .counters(CounterKind::Loyalty, 10),
                Permanent::new(JACE_THE_MIND_SCULPTOR),
                Permanent::new(THE_WANDERING_EMPEROR),
                Permanent::new(TEFERI_HERO_OF_DOMINARIA),
                Permanent::new(SNAPCASTER_MAGE),
                ISLAND_,
                ISLAND_,
                PLAINS_,
                PLAINS_,
            ],
        )
        .hand(
            P1,
            &[
                PORTABLE_HOLE,
                COUNTERSPELL,
                MANA_DRAIN,
                OUST,
                REPRIEVE,
                STOCK_UP,
                ISLAND,
                PLAINS,
                ISLAND,
                PLAINS,
                ISLAND,
                PLAINS,
            ],
        )
        .library(P1, LANDS)
        .actions(&[
            Action::activate(P0, TEFERI_HERO_OF_DOMINARIA, 2),
            Action::cast(P0, PORTABLE_HOLE),
            choose(P0, DecisionKind::Target, SNAPCASTER_MAGE),
            Action::activate(P1, JACE_TELEPATH_UNBOUND, 2),
            Action::cast(P1, PORTABLE_HOLE),
            choose(P1, DecisionKind::Target, SNAPCASTER_MAGE),
        ]),
    // Each player has an emblem: the human's Teferi, Hero of Dominaria −8 in their turn, then
    // the opponent's Jace, Telepath Unbound −9 in theirs, both left on the battlefield.
    Scenario::new("emblems")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(
            P0,
            &[
                Permanent::new(TEFERI_HERO_OF_DOMINARIA).counters(CounterKind::Loyalty, 9),
                ISLAND_,
                PLAINS_,
            ],
        )
        .library(P0, LANDS)
        .permanents(
            P1,
            &[
                Permanent::new(JACE_VRYNS_PRODIGY)
                    .transformed()
                    .counters(CounterKind::Loyalty, 10),
                ISLAND_,
                PLAINS_,
            ],
        )
        .library(P1, LANDS)
        .actions(&[
            Action::activate(P0, TEFERI_HERO_OF_DOMINARIA, 2),
            Action::activate(P1, JACE_TELEPATH_UNBOUND, 2),
        ]),
    // The phone prototype's late-game WU mirror at turn 14, about 30 permanents (DESIGN.md
    // in the mobile prototype): the human's precombat main phase, with a land drop and their
    // sorceries castable, and then their declaration of attackers.
    wu_mirror("wu-mirror", P0, &[]),
    // The same table in the opponent's turn: their Fractured Identity targets the human's
    // The Wandering Emperor.
    wu_mirror(
        "wu-mirror-respond",
        P1,
        &[
            Action::cast(P1, FRACTURED_IDENTITY),
            choose(P1, DecisionKind::Target, THE_WANDERING_EMPEROR),
        ],
    ),
];
