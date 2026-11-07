//! Currency Converter's spec: its stops, its two abilities, the discard prompt, the cards it
//! exiles and what it makes of them.

use beryl_cards::names::{
    COUNTERSPELL, CURRENCY_CONVERTER, ISLAND, JACE_THE_MIND_SCULPTOR, JACE_VRYNS_PRODIGY,
    MANA_CRYPT, MANA_DRAIN, OUST, PLAINS, REPRIEVE, SEACHROME_COAST, SINK_INTO_STUPOR,
    STAFF_OF_THE_STORYTELLER, STOCK_UP, THE_WANDERING_EMPEROR, THIEVING_SKYDIVER,
    THIRST_FOR_DISCOVERY, TUNDRA,
};
use beryl_engine::state::CounterKind;
use beryl_engine::{DecisionKind, TurnStep};
use beryl_scenario::{Action, P0, P1, Permanent, Pick, Scenario};

use super::{Moment, Spec};

const CONVERTER: &str = CURRENCY_CONVERTER;

/// The Converter's abilities, by index among its own: its loot and its {T}.
const LOOT: u8 = 1;
const CONVERT: u8 = 2;

const LANDS: &[&str] = &[
    PLAINS, ISLAND, PLAINS, ISLAND, PLAINS, ISLAND, PLAINS, ISLAND,
];

/// The opponent's side of every moment: a mid-game WU table, three lands untapped, cards in
/// hand and a graveyard.
const fn table(
    name: &'static str,
    turn: u16,
    active: beryl_engine::Seat,
    step: TurnStep,
) -> Scenario {
    Scenario::new(name)
        .at(turn, active, step)
        .life(P0, 17)
        .life(P1, 14)
        .graveyard(P0, &[STOCK_UP, REPRIEVE])
        .battlefield(P1, THEIRS)
        .hand(P1, &[COUNTERSPELL, SINK_INTO_STUPOR, PLAINS])
        .graveyard(P1, &[OUST, MANA_DRAIN, STOCK_UP])
        .library(P1, LANDS)
}

const THEIRS: &[Permanent] = &[
    Permanent::new(THE_WANDERING_EMPEROR).counters(CounterKind::Loyalty, 3),
    Permanent::new(STAFF_OF_THE_STORYTELLER).counters(CounterKind::Story, 1),
    Permanent::new(PLAINS),
    Permanent::new(PLAINS),
    Permanent::new(ISLAND),
    Permanent::new(ISLAND),
    Permanent::new(SEACHROME_COAST),
];

/// The human's lands, all tapped: nothing pays for the loot.
const SPENT: [Permanent; 4] = [
    Permanent::new(ISLAND).tapped(),
    Permanent::new(ISLAND).tapped(),
    Permanent::new(PLAINS).tapped(),
    Permanent::new(TUNDRA).tapped(),
];

/// The human's lands, two untapped: the loot's {2}.
const TWO_OPEN: [Permanent; 4] = [
    Permanent::new(ISLAND),
    Permanent::new(ISLAND),
    Permanent::new(PLAINS).tapped(),
    Permanent::new(TUNDRA).tapped(),
];

const fn with(lands: [Permanent; 4], rest: [Permanent; 2]) -> [Permanent; 6] {
    [rest[0], rest[1], lands[0], lands[1], lands[2], lands[3]]
}

/// The Converter beside a tapped Thieving Skydiver, after combat, the lands spent.
const SPENT_TABLE: [Permanent; 6] = with(
    SPENT,
    [
        Permanent::new(CONVERTER),
        Permanent::new(THIEVING_SKYDIVER).tapped(),
    ],
);

/// The Converter beside Jace, Vryn's Prodigy, whose loot discards for it, the lands spent.
const JACE_TABLE: [Permanent; 6] = with(
    SPENT,
    [
        Permanent::new(CONVERTER),
        Permanent::new(JACE_VRYNS_PRODIGY),
    ],
);

/// The Converter with two lands open for its loot, beside a tapped Skydiver.
const OPEN_TABLE: [Permanent; 6] = with(
    TWO_OPEN,
    [
        Permanent::new(CONVERTER),
        Permanent::new(THIEVING_SKYDIVER).tapped(),
    ],
);

/// The Converter, three Islands for Thirst for Discovery, and Mana Crypt to pay the loot after.
const THIRST_TABLE: &[Permanent] = &[
    Permanent::new(CONVERTER),
    Permanent::new(MANA_CRYPT),
    Permanent::new(ISLAND),
    Permanent::new(ISLAND),
    Permanent::new(ISLAND),
    Permanent::new(PLAINS).tapped(),
];

/// Jace loots Oust away and the Converter is asked whether to exile it.
const JACE_LOOTS: &[Action] = &[
    Action::activate(P0, JACE_VRYNS_PRODIGY, 0),
    Action::choose(P0, DecisionKind::Discard, OUST),
];

/// Jace loots Oust away and the Converter exiles it.
const JACE_FEEDS: &[Action] = &[
    Action::activate(P0, JACE_VRYNS_PRODIGY, 0),
    Action::choose(P0, DecisionKind::Discard, OUST),
    Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
];

/// Thirst for Discovery discards Tundra and Counterspell, and the Converter exiles both.
const THIRST_FEEDS: &[Action] = &[
    Action::cast(P0, THIRST_FOR_DISCOVERY),
    Action::choose(P0, DecisionKind::Discard, TUNDRA),
    Action::choose(P0, DecisionKind::Discard, COUNTERSPELL),
    Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
    Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
];

const fn jace(name: &'static str) -> Scenario {
    table(name, 5, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &JACE_TABLE)
        .hand(P0, &[OUST])
        .library(
            P0,
            &[JACE_THE_MIND_SCULPTOR, PLAINS, ISLAND, PLAINS, ISLAND],
        )
}

const fn thirst(name: &'static str) -> Scenario {
    table(name, 5, P0, TurnStep::PrecombatMain)
        .battlefield(P0, THIRST_TABLE)
        .hand(P0, &[THIRST_FOR_DISCOVERY])
        .library(
            P0,
            &[TUNDRA, JACE_THE_MIND_SCULPTOR, COUNTERSPELL, ISLAND, PLAINS],
        )
        .actions(THIRST_FEEDS)
}

pub const SPEC: Spec = Spec {
    card: CONVERTER,
    moments: &[
        Moment::new(
            "quiet-when-empty",
            "Nothing exiled: no stop",
            table("quiet-when-empty", 5, P0, TurnStep::PostcombatMain)
                .battlefield(P0, &SPENT_TABLE)
                .hand(P0, &[JACE_THE_MIND_SCULPTOR])
                .library(P0, LANDS),
        )
        .look("Your turn ends unasked; the next stop is your next turn.")
        .expect(&["turn 7", "your turn", "step precombatMain"]),
        Moment::new(
            "tap-anyway",
            "You can still tap it",
            table("tap-anyway", 5, P0, TurnStep::PrecombatMain)
                .battlefield(P0, &SPENT_TABLE)
                .hand(P0, &[ISLAND, JACE_THE_MIND_SCULPTOR])
                .library(P0, LANDS),
        )
        .look("Click it: both abilities, the loot greyed. Pick the other.")
        .expect(&[
            "decision priority",
            "offers Currency Converter ability 2",
            "not offers Currency Converter ability 1",
        ]),
        Moment::new(
            "tapped-for-nothing",
            "Tapped with nothing exiled",
            table("tapped-for-nothing", 5, P0, TurnStep::PrecombatMain)
                .battlefield(P0, &SPENT_TABLE)
                .hand(P0, &[ISLAND, JACE_THE_MIND_SCULPTOR])
                .library(P0, LANDS),
        )
        .play(&[Action::activate(P0, CONVERTER, CONVERT)])
        .look("It taps and resolves; nothing is made.")
        .expect(&[
            "tapped Currency Converter",
            "controls 0 Rogue Token",
            "controls 0 Treasure Token",
        ]),
        Moment::new(
            "loot-stops-main",
            "Loot open: it stops, hand empty",
            table("loot-stops-main", 5, P0, TurnStep::PostcombatMain)
                .battlefield(P0, &OPEN_TABLE)
                .library(P0, LANDS),
        )
        .look("Your second main stops for the loot.")
        .expect(&[
            "turn 5",
            "step postcombatMain",
            "offers Currency Converter ability 1",
        ]),
        Moment::new(
            "loot-stops-their-turn",
            "Loot open: their end step stops",
            table("loot-stops-their-turn", 6, P1, TurnStep::PrecombatMain)
                .battlefield(P0, &OPEN_TABLE)
                .library(P0, LANDS),
        )
        .look("Their turn plays to its end step, where you may loot.")
        .expect(&[
            "their turn",
            "step end",
            "offers Currency Converter ability 1",
        ]),
        Moment::new(
            "loot-empty-hand",
            "Loot with an empty hand",
            table("loot-empty-hand", 5, P0, TurnStep::PostcombatMain)
                .battlefield(P0, &OPEN_TABLE)
                .library(P0, LANDS),
        )
        .play(&[Action::activate(P0, CONVERTER, LOOT)])
        .look("It draws, discards that card, and asks to exile it.")
        .expect(&[
            "decision yesNo",
            "ask mayMove",
            "resolving Currency Converter",
        ]),
        Moment::new(
            "discard-prompt",
            "Discarding asks to exile",
            jace("discard-prompt").actions(JACE_LOOTS),
        )
        .look("Center question; Decline and Exile bottom right.")
        .expect(&[
            "decision yesNo",
            "ask mayMove",
            "resolving Currency Converter",
        ]),
        Moment::new(
            "stops-with-exile",
            "Something exiled: it stops",
            jace("stops-with-exile").actions(JACE_FEEDS),
        )
        .play(&[Action::pass(P0)])
        .look("Passing your first main stops again in your second.")
        .expect(&[
            "turn 5",
            "step postcombatMain",
            "1 exiled with Currency Converter",
            "offers Currency Converter ability 2",
        ]),
        Moment::new(
            "declined",
            "Declined: nothing exiled",
            jace("declined").actions(&[
                Action::activate(P0, JACE_VRYNS_PRODIGY, 0),
                Action::choose(P0, DecisionKind::Discard, OUST),
                Action::answer(P0, DecisionKind::YesNo, Pick::No),
            ]),
        )
        .look("Oust stays in the graveyard; your turn ends unasked.")
        .expect(&["turn 7", "0 exiled with Currency Converter"]),
        Moment::new(
            "exiled-pile",
            "Exiled cards sit behind it",
            thirst("exiled-pile"),
        )
        .look("Two cards peek out down-left. Click them to browse.")
        .expect(&["decision priority", "2 exiled with Currency Converter"]),
        Moment::new(
            "both-abilities",
            "Both abilities, both affordable",
            thirst("both-abilities"),
        )
        .look("Click or drag it out: two side-by-side choices.")
        .expect(&[
            "offers Currency Converter ability 1",
            "offers Currency Converter ability 2",
        ]),
        Moment::new(
            "choose-exiled",
            "Pick which card to return",
            thirst("choose-exiled"),
        )
        .play(&[Action::activate(P0, CONVERTER, CONVERT)])
        .look("A chooser of the two exiled cards; click one.")
        .expect(&[
            "decision chooseObject",
            "2 candidates",
            "resolving Currency Converter",
        ]),
        Moment::new(
            "land-to-treasure",
            "A land makes a Treasure",
            thirst("land-to-treasure"),
        )
        .play(&[
            Action::activate(P0, CONVERTER, CONVERT),
            Action::choose(P0, DecisionKind::ChooseObject, TUNDRA),
        ])
        .look("Tundra to the graveyard, a Treasure appears.")
        .expect(&[
            "controls 1 Treasure Token",
            "1 exiled with Currency Converter",
        ]),
        Moment::new(
            "nonland-to-rogue",
            "A nonland card makes a Rogue",
            thirst("nonland-to-rogue"),
        )
        .play(&[
            Action::activate(P0, CONVERTER, CONVERT),
            Action::choose(P0, DecisionKind::ChooseObject, COUNTERSPELL),
        ])
        .look("Counterspell to the graveyard, a 2/2 Rogue appears.")
        .expect(&["controls 1 Rogue Token", "1 exiled with Currency Converter"]),
        Moment::new(
            "bounced-in-response",
            "Bounced in response, still returns",
            jace("bounced-in-response").actions(JACE_FEEDS),
        )
        .play(&[
            Action::activate(P0, CONVERTER, CONVERT),
            Action::cast(P1, SINK_INTO_STUPOR),
            Action::choose(P1, DecisionKind::Target, CONVERTER),
        ])
        .look("Sink into Stupor bounces it; its ability still returns Oust.")
        .expect(&["controls 0 Currency Converter", "controls 1 Rogue Token"]),
    ],
};
