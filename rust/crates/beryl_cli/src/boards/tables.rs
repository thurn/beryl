//! The tables more than one board starts from.

use beryl_cards::names::{
    ABHORRENT_OCULUS, BEZA_THE_BOUNDING_SPRING, COUNTERSPELL, CURRENCY_CONVERTER, DAMN,
    FLOODFARM_VERGE, FRACTURED_IDENTITY, ISLAND, JACE_THE_MIND_SCULPTOR, JACE_VRYNS_PRODIGY,
    JACE_WIELDER_OF_MYSTERIES, MANA_CRYPT, NO_MORE_LIES, OUST, PLAINS, PORTABLE_HOLE, REPRIEVE,
    SEACHROME_COAST, SNAPCASTER_MAGE, SPARAS_HEADQUARTERS, STAFF_OF_THE_STORYTELLER, STOCK_UP,
    SUBTLETY, TEFERI_HERO_OF_DOMINARIA, TEFERI_TIME_RAVELER, THE_WANDERING_EMPEROR,
    THIRST_FOR_DISCOVERY, TRINKET_MAGE, TUNDRA, WINDS_OF_ABANDON,
};
use beryl_engine::state::CounterKind;
use beryl_engine::{Seat, TurnStep};
use beryl_scenario::{Action, P0, P1, Permanent, Scenario};

use super::shorthand::{FISH_TOKEN, ISLAND_, LANDS, PLAINS_, SAMURAI_TOKEN, TREASURE_TOKEN};

/// Currency Converter, untapped, beside the mana for Thirst for Discovery, which draws Tundra,
/// Mana Crypt and Counterspell: `actions` play on from there.
pub(super) const fn converter(name: &'static str, actions: &'static [Action]) -> Scenario {
    Scenario::new(name)
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(P0, CONVERTER_TABLE)
        .hand(P0, &[THIRST_FOR_DISCOVERY])
        .library(P0, &[TUNDRA, MANA_CRYPT, COUNTERSPELL, ISLAND, PLAINS])
        .battlefield(P1, &[ISLAND_, PLAINS_])
        .library(P1, LANDS)
        .actions(actions)
}

/// The phone prototype's WU mirror at turn 14 in `active`'s precombat main phase, from which
/// `actions` play on. The opponent's two manifested dread are face-down Counterspell and Oust.
pub(super) const fn wu_mirror(
    name: &'static str,
    active: Seat,
    actions: &'static [Action],
) -> Scenario {
    Scenario::new(name)
        .at(14, active, TurnStep::PrecombatMain)
        .life(P0, 14)
        .battlefield(P0, MIRROR_YOURS)
        .hand(
            P0,
            &[
                COUNTERSPELL,
                NO_MORE_LIES,
                REPRIEVE,
                OUST,
                STOCK_UP,
                DAMN,
                ISLAND,
                PLAINS,
            ],
        )
        .graveyard(
            P0,
            &[
                PLAINS,
                ABHORRENT_OCULUS,
                CURRENCY_CONVERTER,
                JACE_THE_MIND_SCULPTOR,
                TEFERI_TIME_RAVELER,
                JACE_VRYNS_PRODIGY,
                SUBTLETY,
                FRACTURED_IDENTITY,
                WINDS_OF_ABANDON,
                THIRST_FOR_DISCOVERY,
            ],
        )
        .library(P0, LANDS)
        .life(P1, 11)
        .battlefield(P1, MIRROR_THEIRS)
        .hand(P1, &[FRACTURED_IDENTITY, COUNTERSPELL, REPRIEVE])
        .graveyard(
            P1,
            &[
                ISLAND,
                REPRIEVE,
                OUST,
                PORTABLE_HOLE,
                JACE_WIELDER_OF_MYSTERIES,
                TEFERI_HERO_OF_DOMINARIA,
                THE_WANDERING_EMPEROR,
                BEZA_THE_BOUNDING_SPRING,
                DAMN,
                WINDS_OF_ABANDON,
                THIRST_FOR_DISCOVERY,
                STOCK_UP,
                NO_MORE_LIES,
                COUNTERSPELL,
            ],
        )
        .exile(P1, &[SNAPCASTER_MAGE, SUBTLETY])
        .library(P1, LANDS)
        .actions(actions)
}

/// The human's side of the WU mirror: their Snapcaster Mage came down this turn.
pub(super) const MIRROR_YOURS: &[Permanent] = &[
    Permanent::new(TUNDRA),
    Permanent::new(SEACHROME_COAST),
    Permanent::new(FLOODFARM_VERGE),
    Permanent::new(SPARAS_HEADQUARTERS),
    ISLAND_,
    ISLAND_,
    ISLAND_,
    ISLAND_,
    PLAINS_,
    PLAINS_,
    Permanent::new(THE_WANDERING_EMPEROR).counters(CounterKind::Loyalty, 5),
    Permanent::new(TEFERI_HERO_OF_DOMINARIA),
    Permanent::new(JACE_WIELDER_OF_MYSTERIES),
    Permanent::new(PORTABLE_HOLE),
    Permanent::new(STAFF_OF_THE_STORYTELLER).counters(CounterKind::Story, 4),
    Permanent::new(MANA_CRYPT),
    Permanent::new(BEZA_THE_BOUNDING_SPRING),
    Permanent::new(SUBTLETY),
    Permanent::new(SNAPCASTER_MAGE).sick(),
    Permanent::new(TRINKET_MAGE),
    Permanent::token(SAMURAI_TOKEN),
    Permanent::token(SAMURAI_TOKEN),
    Permanent::token(FISH_TOKEN),
    Permanent::token(FISH_TOKEN),
];

/// The opponent's side of the WU mirror.
pub(super) const MIRROR_THEIRS: &[Permanent] = &[
    Permanent::new(TUNDRA),
    Permanent::new(SEACHROME_COAST),
    Permanent::new(FLOODFARM_VERGE),
    Permanent::new(SPARAS_HEADQUARTERS),
    ISLAND_,
    ISLAND_,
    ISLAND_,
    PLAINS_,
    PLAINS_,
    Permanent::new(JACE_THE_MIND_SCULPTOR).counters(CounterKind::Loyalty, 6),
    Permanent::new(TEFERI_TIME_RAVELER).counters(CounterKind::Loyalty, 5),
    Permanent::new(STAFF_OF_THE_STORYTELLER).counters(CounterKind::Story, 2),
    Permanent::new(MANA_CRYPT),
    Permanent::new(CURRENCY_CONVERTER),
    Permanent::token(TREASURE_TOKEN),
    Permanent::new(ABHORRENT_OCULUS),
    Permanent::new(JACE_VRYNS_PRODIGY),
    Permanent::new(TRINKET_MAGE),
    Permanent::new(COUNTERSPELL).face_down(),
    Permanent::new(OUST).face_down(),
    Permanent::token(FISH_TOKEN),
    Permanent::token(FISH_TOKEN),
];

pub(super) const CONVERTER_TABLE: &[Permanent] = &[
    Permanent::new(CURRENCY_CONVERTER),
    ISLAND_,
    ISLAND_,
    ISLAND_,
];
