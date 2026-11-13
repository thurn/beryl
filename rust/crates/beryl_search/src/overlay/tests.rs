//! The derived card knowledge, with the overlay on top, is what the hand-written tables gave
//! before derivation replaced them (bead by-w8gv.15.2).

use beryl_cards::{CARDS, names};
use beryl_engine::card::CardDef;

use crate::values::{CardInfo, info};
use crate::{CardValues, Knowledge};

/// The hand-written tables as they stood before derivation replaced them, kept as the
/// record the derived knowledge is checked against. Each holds a card name and, for an
/// ability, its index in the front face's abilities.
mod before {
    use beryl_cards::names;

    pub const WHEELS: &[&str] = &[names::TIMETWISTER, names::TIME_SPIRAL];
    pub const EMPTIES: &[(&str, u8)] = &[(names::JACE_WIELDER_OF_MYSTERIES, 2)];
    pub const NEVER_CAST: &[&str] = &[names::MANA_CRYPT];
    pub const FRIENDLY: &[(&str, u8)] = &[(names::THE_WANDERING_EMPEROR, 1)];
    pub const NEVER: &[(&str, u8)] = &[(names::AANGS_ICEBERG, 1)];
    pub const DRAWS: &[(&str, u8)] = &[
        (names::ABHORRENT_OCULUS, 0),
        (names::CURRENCY_CONVERTER, 1),
        (names::JACE_THE_MIND_SCULPTOR, 1),
        (names::JACE_VRYNS_PRODIGY, 0),
        (names::REPRIEVE, 0),
        (names::STAFF_OF_THE_STORYTELLER, 2),
        (names::STOCK_UP, 0),
        (names::TEFERI_HERO_OF_DOMINARIA, 0),
        (names::THIRST_FOR_DISCOVERY, 0),
    ];
}

/// Every card written in a file and every token or emblem it makes, once each.
fn every_card() -> Vec<&'static CardDef> {
    let mut defs: Vec<&'static CardDef> = Vec::new();
    for &def in CARDS.cards() {
        for &d in std::iter::once(&def).chain(def.tokens) {
            if !defs.iter().any(|&known| std::ptr::eq(known, d)) {
                defs.push(d);
            }
        }
    }
    defs
}

/// The flags the old tables gave `name`.
fn flags_before(name: &str) -> (bool, bool, u8, u8, u8, u8) {
    let bits = |list: &[(&str, u8)]| {
        list.iter()
            .filter(|(n, _)| *n == name)
            .fold(0, |bits, &(_, a)| bits | 1 << a)
    };
    (
        before::WHEELS.contains(&name),
        before::NEVER_CAST.contains(&name),
        bits(before::FRIENDLY),
        bits(before::NEVER),
        bits(before::DRAWS),
        bits(before::EMPTIES),
    )
}

fn flags(info: CardInfo) -> (bool, bool, u8, u8, u8, u8) {
    (
        info.wheel,
        info.never_cast,
        info.friendly,
        info.never,
        info.draws,
        info.empties,
    )
}

#[test]
fn tuned_knowledge_is_what_the_tables_gave() {
    for def in every_card() {
        assert_eq!(
            flags(info(def, Knowledge::Tuned)),
            flags_before(def.name),
            "{}: (wheel, never_cast, friendly, never, draws, empties)",
            def.name
        );
    }
}

/// The cards whose derived flags differ from the old tables, and how: each table left out an
/// ability that draws, and the overlay keeps the tuned decks playing as the table had them.
/// Any other difference is a derivation that disagrees with the tables, to be decided from
/// the card's Oracle text.
#[test]
fn derivation_differs_from_the_tables_only_where_the_overlay_says() {
    let expected: &[(&str, u8)] = &[
        (names::JACE_WIELDER_OF_MYSTERIES, 0b110),
        (names::TEFERI_TIME_RAVELER, 0b100),
        (names::SPARAS_HEADQUARTERS, 0b1_0000),
    ];
    for def in every_card() {
        let derived = info(def, Knowledge::Estimate);
        let (wheel, _, friendly, _, draws, empties) = flags_before(def.name);
        let extra = expected
            .iter()
            .find(|(name, _)| *name == def.name)
            .map_or(0, |&(_, bits)| bits);
        assert_eq!(
            (
                derived.wheel,
                derived.friendly,
                derived.draws,
                derived.empties
            ),
            (wheel, friendly, draws | extra, empties),
            "{}: (wheel, friendly, draws, empties)",
            def.name
        );
        assert!(!derived.never_cast && derived.never == 0, "{}", def.name);
    }
}

#[test]
fn tuned_values_are_the_hand_tuned_ones() {
    let t = beryl_engine::GameTable::new(
        &CARDS,
        [
            beryl_cards::decks::WU_CONTROL,
            beryl_cards::decks::VINTAGE_WU_CONTROL,
        ],
    )
    .unwrap();
    let tuned = CardValues::with(&t, Knowledge::Tuned);
    let estimated = CardValues::with(&t, Knowledge::Estimate);
    let card = |name: &str| {
        t.indices::<beryl_engine::LargeCardIx>()
            .find(|&c| t.def(c).name == name)
            .unwrap()
    };
    assert_eq!(tuned.get(card(names::JACE_THE_MIND_SCULPTOR)).value, 90);
    assert_eq!(tuned.get(card(names::ISLAND)).value, 10);
    assert!(tuned.get(card(names::MANA_CRYPT)).never_cast);
    assert!(!estimated.get(card(names::MANA_CRYPT)).never_cast);
    // Jace, the Mind Sculptor's loyalty of 3, estimated at 15 a loyalty counter.
    assert_eq!(estimated.get(card(names::JACE_THE_MIND_SCULPTOR)).value, 45);
}
