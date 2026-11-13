//! Decklists.

use beryl_engine::Decklist;
use beryl_engine::card::{CardDef, Supertypes};
use beryl_engine::rng::SplitMix64;

use crate::{CARDS, names};

/// The deck a game starts with when its request names none, as owned name strings. Callers
/// outside the card crates use this instead of naming a deck.
#[must_use]
pub fn default_deck() -> Vec<(u8, String)> {
    WU_CONTROL
        .iter()
        .map(|&(count, name)| (count, name.to_owned()))
        .collect()
}

/// The 40-card main deck of `docs/sample-decks/2025-11-18-17lands-cb8d5d5f-wu-control.txt`, the
/// Milestone 1 mirror: a mythic drafter's deck from an Arena run of 2025-10-30.
pub const WU_CONTROL: &Decklist = &[
    (1, names::JACE_VRYNS_PRODIGY),
    (1, names::SNAPCASTER_MAGE),
    (1, names::TRINKET_MAGE),
    (1, names::ABHORRENT_OCULUS),
    (1, names::BEZA_THE_BOUNDING_SPRING),
    (1, names::SUBTLETY),
    (1, names::COUNTERSPELL),
    (1, names::NO_MORE_LIES),
    (1, names::REPRIEVE),
    (1, names::OUST),
    (1, names::THIRST_FOR_DISCOVERY),
    (1, names::STOCK_UP),
    (1, names::DAMN),
    (1, names::WINDS_OF_ABANDON),
    (1, names::FRACTURED_IDENTITY),
    (1, names::TEFERI_TIME_RAVELER),
    (1, names::THE_WANDERING_EMPEROR),
    (1, names::JACE_WIELDER_OF_MYSTERIES),
    (1, names::JACE_THE_MIND_SCULPTOR),
    (1, names::TEFERI_HERO_OF_DOMINARIA),
    (1, names::MANA_CRYPT),
    (1, names::PORTABLE_HOLE),
    (1, names::CURRENCY_CONVERTER),
    (1, names::STAFF_OF_THE_STORYTELLER),
    (1, names::TUNDRA),
    (1, names::SEACHROME_COAST),
    (1, names::FLOODFARM_VERGE),
    (1, names::SPARAS_HEADQUARTERS),
    (6, names::PLAINS),
    (6, names::ISLAND),
];

/// The 40-card main deck of `docs/sample-decks/2026-10-17-PrinceJools-wu-control.txt`, the
/// MTGO deck that was the mirror before. It plays implemented cards the mirror
/// lacks, which UI fixtures reach in play.
pub const VINTAGE_WU_CONTROL: &Decklist = &[
    (1, names::MANA_TITHE),
    (1, names::SWIFT_RECONFIGURATION),
    (1, names::OUST),
    (1, names::THUNDERTRAP_TRAINER),
    (1, names::JACE_VRYNS_PRODIGY),
    (1, names::THIEVING_SKYDIVER),
    (1, names::MALCOLM_ALLURING_SCOUNDREL),
    (1, names::OVERLORD_OF_THE_MISTMOORS),
    (1, names::ELITE_SPELLBINDER),
    (1, names::TEFERI_TIME_RAVELER),
    (1, names::AANGS_ICEBERG),
    (1, names::ADELINE_RESPLENDENT_CATHAR),
    (1, names::THE_WANDERING_EMPEROR),
    (1, names::JACE_THE_MIND_SCULPTOR),
    (1, names::SPHINX_OF_FALSE_CONCLUSIONS),
    (1, names::TIME_SPIRAL),
    (1, names::STRIP_MINE),
    (1, names::MANA_CRYPT),
    (1, names::SEACHROME_COAST),
    (1, names::SPARAS_HEADQUARTERS),
    (1, names::TUNDRA),
    (7, names::PLAINS),
    (6, names::ISLAND),
    (1, names::STAFF_OF_THE_STORYTELLER),
    (1, names::GET_LOST),
    (1, names::NO_MORE_LIES),
    (1, names::MANA_DRAIN),
    (1, names::SINK_INTO_STUPOR),
    (1, names::TIMETWISTER),
];

/// The size of a fuzz deck: the minimum for limited play
/// ([CR 100.2b](https://yawgatog.com/resources/magic-rules/#R1002b)): "each deck has a
/// minimum deck size of 40 cards."
const FUZZ_DECK: usize = 40;

/// Nonbasic cards in a fuzz deck at most, about a deck's spells and nonbasic lands.
const FUZZ_NONBASICS: usize = 23;

/// A legal 40-card deck of the cards implemented so far, for random play: up to 23 nonbasic
/// cards chosen by `seed`, each at most once as in a draft, and basic lands in turn for
/// the rest. A limited deck holds "as many duplicates of a card as are included with the
/// product" (CR 100.2b), and the card pool holds one of each.
pub fn fuzz_deck(seed: u64) -> Vec<(u8, &'static str)> {
    fuzz_deck_from(seed, CARDS.cards())
}

/// [`fuzz_deck`] drawn from `cards` instead of every implemented card.
///
/// Each nonbasic card gets a rank from a hash of `seed` and its name, and the deck takes the
/// lowest-ranked ones (rendezvous hashing). A card's rank does not depend on which other cards
/// exist, so adding or removing a card file changes a seed's deck only when that card ranks
/// into it, which keeps tests that play fuzz decks stable as cards are written.
pub fn fuzz_deck_from(seed: u64, cards: &[&CardDef]) -> Vec<(u8, &'static str)> {
    let (mut basics, mut nonbasics): (Vec<&CardDef>, Vec<&CardDef>) = cards
        .iter()
        .copied()
        .partition(|c| c.front().supertypes.contains(Supertypes::BASIC));
    basics.sort_unstable_by_key(|c| c.name);
    nonbasics.sort_unstable_by_key(|c| (rank(seed, c.name), c.name));
    nonbasics.truncate(FUZZ_NONBASICS);
    // By name, so that the deck's order depends only on which cards it holds.
    nonbasics.sort_unstable_by_key(|c| c.name);
    let mut deck: Vec<(u8, &'static str)> = nonbasics.iter().map(|c| (1, c.name)).collect();
    let lands = FUZZ_DECK - nonbasics.len();
    for (i, c) in basics.iter().enumerate() {
        let count = lands / basics.len() + usize::from(i < lands % basics.len());
        deck.push((count as u8, c.name));
    }
    deck
}

/// A card's place in `seed`'s fuzz deck order: a 64-bit FNV-1a hash of its name, mixed with
/// `seed` through `SplitMix64` so that every seed orders the cards differently.
fn rank(seed: u64, name: &str) -> u64 {
    let hash = name.bytes().fold(0xCBF2_9CE4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3)
    });
    SplitMix64::new(seed ^ hash).next_u64()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writing or deleting one card file changes a seed's fuzz deck only when that card ranks
    /// into it, and then by that one card alone, so tests that play fuzz decks do not depend on
    /// which other cards exist. With the pool at a few dozen nonbasic cards a given card lands
    /// in about 23 of every N decks, so this checks the swap itself rather than a share of seeds.
    #[test]
    fn one_card_more_or_less_changes_only_decks_it_joins() {
        let all = CARDS.cards();
        for (ix, card) in all.iter().enumerate() {
            if card.front().supertypes.contains(Supertypes::BASIC) {
                continue;
            }
            let mut fewer = all.to_vec();
            fewer.remove(ix);
            for seed in 0..100 {
                let with = fuzz_deck_from(seed, all);
                let without = fuzz_deck_from(seed, &fewer);
                if with.contains(&(1, card.name)) {
                    let dropped: Vec<_> = without.iter().filter(|c| !with.contains(c)).collect();
                    let added: Vec<_> = with.iter().filter(|c| !without.contains(c)).collect();
                    assert_eq!(added, [&(1, card.name)], "seed {seed}, {}", card.name);
                    assert!(dropped.len() <= 1, "seed {seed}, {}", card.name);
                } else {
                    assert_eq!(with, without, "seed {seed}, {}", card.name);
                }
            }
        }
    }

    #[test]
    fn fuzz_decks_are_legal_and_vary_by_seed() {
        for seed in 0..20 {
            let deck = fuzz_deck(seed);
            assert_eq!(
                deck.iter().map(|&(n, _)| usize::from(n)).sum::<usize>(),
                FUZZ_DECK
            );
        }
        assert_ne!(fuzz_deck(0), fuzz_deck(1));
    }
}
