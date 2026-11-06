//! What the rollout policy knows about each card of a game (architecture §19.1, decision
//! D21): a value, hand-tuned for the cards of the WU Control mirror and the Vintage deck
//! before it and estimated from mana value and stats for the rest and for tokens, and a few
//! flags of card knowledge. Built once per game, indexed by game-table index (§6.4), so a
//! decision reads it with one load.

use beryl_cards::names;
use beryl_engine::card::{Face, Keywords, Replacement, Types};
use beryl_engine::{CardIx, GameTable};

/// Hand-tuned values: roughly how much a strong player would rather have the card than not,
/// on a scale where a game-winning planeswalker is about 90 and a basic land 10.
const VALUES: &[(&str, i16)] = &[
    (names::JACE_THE_MIND_SCULPTOR, 90),
    (names::TEFERI_HERO_OF_DOMINARIA, 85),
    (names::TEFERI_TIME_RAVELER, 80),
    (names::THE_WANDERING_EMPEROR, 80),
    (names::OVERLORD_OF_THE_MISTMOORS, 75),
    (names::FRACTURED_IDENTITY, 70),
    (names::ADELINE_RESPLENDENT_CATHAR, 70),
    (names::BEZA_THE_BOUNDING_SPRING, 65),
    (names::JACE_WIELDER_OF_MYSTERIES, 65),
    (names::SPHINX_OF_FALSE_CONCLUSIONS, 65),
    (names::ABHORRENT_OCULUS, 60),
    (names::MANA_DRAIN, 60),
    (names::DAMN, 55),
    (names::COUNTERSPELL, 55),
    (names::ELITE_SPELLBINDER, 55),
    (names::GET_LOST, 55),
    (names::AANGS_ICEBERG, 55),
    (names::WINDS_OF_ABANDON, 50),
    (names::MALCOLM_ALLURING_SCOUNDREL, 50),
    (names::THIEVING_SKYDIVER, 50),
    (names::JACE_VRYNS_PRODIGY, 50),
    (names::OUST, 50),
    (names::NO_MORE_LIES, 50),
    (names::SUBTLETY, 45),
    (names::SWIFT_RECONFIGURATION, 45),
    (names::SINK_INTO_STUPOR, 45),
    (names::PORTABLE_HOLE, 40),
    (names::STOCK_UP, 40),
    (names::THUNDERTRAP_TRAINER, 40),
    (names::TIME_SPIRAL, 40),
    (names::TIMETWISTER, 40),
    (names::CURRENCY_CONVERTER, 35),
    (names::REPRIEVE, 35),
    (names::SNAPCASTER_MAGE, 35),
    (names::STAFF_OF_THE_STORYTELLER, 35),
    (names::THIRST_FOR_DISCOVERY, 35),
    (names::TRINKET_MAGE, 30),
    (names::MANA_TITHE, 30),
    (names::STRIP_MINE, 15),
    (names::TUNDRA, 14),
    (names::FLOODFARM_VERGE, 13),
    (names::SEACHROME_COAST, 13),
    (names::SPARAS_HEADQUARTERS, 12),
    (names::PLAINS, 10),
    (names::ISLAND, 10),
    (names::MANA_CRYPT, 0),
];

/// Wheels: each player shuffles their hand away and draws seven, worth casting only from a
/// nearly empty hand.
const WHEELS: &[&str] = &[names::TIMETWISTER, names::TIME_SPIRAL];

/// Abilities that draw seven and win the game if that empties the library, as Jace, Wielder
/// of Mysteries' −8 does: worth activating only on a library of seven cards or fewer.
const EMPTIES: &[(&str, u8)] = &[(names::JACE_WIELDER_OF_MYSTERIES, 2)];

/// Cards the policy never casts: Mana Crypt's coin flips, 3 damage on half its controller's
/// upkeeps, cost more life over the mirror's long games than its mana is worth to a policy
/// that does not plan its turns. On the battlefield they are worth nothing either: no removal
/// is spent on them.
const NEVER_CAST: &[&str] = &[names::MANA_CRYPT];

/// Abilities whose targets help their controller although their filters allow either side,
/// by card name and index in the face's abilities: The Wandering Emperor's +1 puts a +1/+1
/// counter on a creature.
const FRIENDLY: &[(&str, u8)] = &[(names::THE_WANDERING_EMPEROR, 1)];

/// Abilities the policy never activates: Aang's Iceberg's waterbend sacrifices it, which
/// returns the permanent it exiled.
const NEVER: &[(&str, u8)] = &[(names::AANGS_ICEBERG, 1)];

/// Abilities and spells that draw or take cards from their controller's library by choice,
/// by card name and index in the face's abilities: worth nothing while the library runs low
/// and an empty library would lose (`Policy::may_draw`). A triggered one makes the permanent
/// not worth casting then: Abhorrent Oculus manifests dread, two cards off the top, each
/// opponent's upkeep.
const DRAWS: &[(&str, u8)] = &[
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

/// One card's entry.
#[derive(Clone, Copy, Debug, Default)]
pub struct CardInfo {
    pub value: i16,
    pub wheel: bool,
    pub never_cast: bool,
    /// Bit `i`: ability `i` of the front face targets to help its controller.
    pub friendly: u8,
    /// Bit `i`: never activate ability `i` of the front face.
    pub never: u8,
    /// Bit `i`: ability `i` of the front face draws its controller cards.
    pub draws: u8,
    /// Bit `i`: ability `i` of the front face wins by drawing the library empty.
    pub empties: u8,
    /// Whether a face of it wins instead of losing by drawing from an empty library, as
    /// Jace, Wielder of Mysteries does.
    pub wins_instead: bool,
}

impl CardInfo {
    pub fn friendly(self, ability: u8) -> bool {
        ability < 8 && self.friendly & (1 << ability) != 0
    }

    pub fn never(self, ability: u8) -> bool {
        ability < 8 && self.never & (1 << ability) != 0
    }

    pub fn draws(self, ability: u8) -> bool {
        ability < 8 && self.draws & (1 << ability) != 0
    }

    pub fn empties(self, ability: u8) -> bool {
        ability < 8 && self.empties & (1 << ability) != 0
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
                never_cast: NEVER_CAST.contains(&def.name),
                friendly: bits(FRIENDLY),
                never: bits(NEVER),
                draws: bits(DRAWS),
                empties: bits(EMPTIES),
                wins_instead: def.faces.iter().any(|f| {
                    f.abilities
                        .iter()
                        .any(|a| a.replacement == Replacement::WinInsteadOfDrawingFromEmptyLibrary)
                }),
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
