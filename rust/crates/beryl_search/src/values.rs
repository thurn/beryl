//! What the rollout policy knows about each card of a game (architecture §19.1, decision
//! D21): a value, hand-tuned for the WU Control mirror's 29 cards and estimated from mana
//! value and stats for the rest and for tokens, and a few flags of card knowledge. Built once
//! per game, indexed by game-table index (§6.4), so a decision reads it with one load.

use beryl_cards::names;
use beryl_engine::card::{Face, Keywords, Types};
use beryl_engine::{CardIx, GameTable};

/// Hand-tuned values: roughly how much a strong player would rather have the card than not,
/// on a scale where a game-winning planeswalker is about 90 and a basic land 10.
const VALUES: &[(&str, i16)] = &[
    (names::JACE_THE_MIND_SCULPTOR, 90),
    (names::TEFERI_TIME_RAVELER, 80),
    (names::THE_WANDERING_EMPEROR, 80),
    (names::OVERLORD_OF_THE_MISTMOORS, 75),
    (names::ADELINE_RESPLENDENT_CATHAR, 70),
    (names::MANA_CRYPT, 70),
    (names::SPHINX_OF_FALSE_CONCLUSIONS, 65),
    (names::MANA_DRAIN, 60),
    (names::ELITE_SPELLBINDER, 55),
    (names::GET_LOST, 55),
    (names::AANGS_ICEBERG, 55),
    (names::MALCOLM_ALLURING_SCOUNDREL, 50),
    (names::THIEVING_SKYDIVER, 50),
    (names::JACE_VRYNS_PRODIGY, 50),
    (names::OUST, 50),
    (names::NO_MORE_LIES, 50),
    (names::SWIFT_RECONFIGURATION, 45),
    (names::SINK_INTO_STUPOR, 45),
    (names::THUNDERTRAP_TRAINER, 40),
    (names::TIME_SPIRAL, 40),
    (names::TIMETWISTER, 40),
    (names::STAFF_OF_THE_STORYTELLER, 35),
    (names::MANA_TITHE, 30),
    (names::STRIP_MINE, 15),
    (names::TUNDRA, 14),
    (names::SEACHROME_COAST, 13),
    (names::SPARAS_HEADQUARTERS, 12),
    (names::PLAINS, 10),
    (names::ISLAND, 10),
];

/// Wheels: each player shuffles their hand away and draws seven, worth casting only from a
/// nearly empty hand.
const WHEELS: &[&str] = &[names::TIMETWISTER, names::TIME_SPIRAL];

/// Abilities whose targets help their controller although their filters allow either side,
/// by card name and index in the face's abilities: The Wandering Emperor's +1 puts a +1/+1
/// counter on a creature.
const FRIENDLY: &[(&str, u8)] = &[(names::THE_WANDERING_EMPEROR, 1)];

/// Abilities the policy never activates: Aang's Iceberg's waterbend sacrifices it, which
/// returns the permanent it exiled.
const NEVER: &[(&str, u8)] = &[(names::AANGS_ICEBERG, 1)];

/// One card's entry.
#[derive(Clone, Copy, Debug, Default)]
pub struct CardInfo {
    pub value: i16,
    pub wheel: bool,
    /// Bit `i`: ability `i` of the front face targets to help its controller.
    pub friendly: u8,
    /// Bit `i`: never activate ability `i` of the front face.
    pub never: u8,
}

impl CardInfo {
    pub fn friendly(self, ability: u8) -> bool {
        ability < 8 && self.friendly & (1 << ability) != 0
    }

    pub fn never(self, ability: u8) -> bool {
        ability < 8 && self.never & (1 << ability) != 0
    }
}

/// Every card's entry, by game index.
#[derive(Clone, Debug)]
pub struct CardValues {
    cards: [CardInfo; CardIx::COUNT],
}

impl CardValues {
    pub fn new(t: &GameTable) -> CardValues {
        let mut cards = [CardInfo::default(); CardIx::COUNT];
        for (i, info) in cards.iter_mut().enumerate().take(t.len()) {
            let def = t.def(CardIx::new(i as u8));
            let bits = |list: &[(&str, u8)]| {
                list.iter()
                    .filter(|(name, _)| *name == def.name)
                    .fold(0, |bits, &(_, a)| bits | 1 << a)
            };
            *info = CardInfo {
                value: VALUES
                    .iter()
                    .find(|(name, _)| *name == def.name)
                    .map_or_else(|| estimate(def.front()), |&(_, v)| v),
                wheel: WHEELS.contains(&def.name),
                friendly: bits(FRIENDLY),
                never: bits(NEVER),
            };
        }
        CardValues { cards }
    }

    #[inline]
    pub fn get(&self, card: CardIx) -> CardInfo {
        self.cards[card.index()]
    }
}

/// A value from what the face shows, for cards and tokens without a hand-tuned one: a
/// creature by its stats and evasion, a planeswalker by its loyalty, anything else by its
/// mana value.
fn estimate(face: &Face) -> i16 {
    let mana_value = i16::from(face.cost.mana_value());
    if face.types.intersects(Types::CREATURE) {
        let stats = i16::from(face.power.unwrap_or(0)) + i16::from(face.toughness.unwrap_or(0));
        let flying = if face.keywords.intersects(Keywords::FLYING) {
            5
        } else {
            0
        };
        4 * stats + 3 * mana_value + flying
    } else if face.types.intersects(Types::PLANESWALKER) {
        15 * i16::from(face.loyalty.unwrap_or(0))
    } else if face.types.intersects(Types::LAND) {
        10
    } else {
        5 + 4 * mana_value
    }
}
