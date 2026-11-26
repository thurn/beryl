//! Decklists: the fixed ones, which `beryl_card_names` holds because they are only names, and
//! the fuzz decks dealt from the implemented cards.

use beryl_card::definition::Shape;
use beryl_engine::card::{CardDef, Supertypes};
use beryl_engine::rng::SplitMix64;

pub use beryl_card_names::decks::{
    FORGETFUL_FISH, KEYSTONE_FAMILIES, VINTAGE_WU_CONTROL, WU_CONTROL, default_deck,
};

use crate::CARDS;

/// The cards of the keystone family called `name` ([`KEYSTONE_FAMILIES`]), with every basic
/// land, as [`fuzz_deck_from`] takes them. A family's card may be one complete from the card
/// store with no file, which [`CardTable::cards`](beryl_engine::CardTable::cards) does not walk.
#[must_use]
pub fn keystone_family(name: &str) -> Option<Vec<&'static CardDef>> {
    let &(_, cards) = KEYSTONE_FAMILIES.iter().find(|(n, _)| *n == name)?;
    let basics = CARDS
        .cards()
        .iter()
        .copied()
        .filter(|c| c.front().supertypes.contains(Supertypes::BASIC));
    let family = cards.iter().map(|&card| {
        CARDS
            .get(card)
            .expect("a keystone family names implemented cards")
    });
    Some(basics.chain(family).collect())
}

/// Every card of every keystone family ([`KEYSTONE_FAMILIES`]), each once, with every basic
/// land: the pool [`pool_deck`] deals from. A family added to the list joins the pool with it.
/// The family fuzzers deal from one family at a time, so cards of different families rarely
/// meet there, as Humility and Gemrazer or Rest in Peace and Underworld Breach would here.
#[must_use]
pub fn keystone_pool() -> Vec<&'static CardDef> {
    let mut pool: Vec<&'static CardDef> = Vec::new();
    for &(name, _) in KEYSTONE_FAMILIES {
        for card in keystone_family(name).expect("a family of the list") {
            if !pool.iter().any(|c| c.name == card.name) {
                pool.push(card);
            }
        }
    }
    pool
}

/// Nonbasic cards in a pool deck, the same as a fuzz deck's.
const POOL_NONBASICS: usize = FUZZ_NONBASICS;

/// A 40-card deck from [`keystone_pool`] for `seed`: its nonbasic cards chosen as
/// [`fuzz_deck_from`] chooses them, and its basic lands split by the colored mana symbols of
/// those cards' costs rather than evenly. A deck of five colors split evenly casts few of its
/// spells, and the pool exists to put keystones of different families on the battlefield
/// together, two or three by the middle of a game.
#[must_use]
pub fn pool_deck(seed: u64) -> Vec<(u8, &'static str)> {
    pool_deck_from(seed, &keystone_pool())
}

/// [`pool_deck`] dealt from `cards`, a pool as [`keystone_pool`] gives it.
fn pool_deck_from(seed: u64, cards: &[&'static CardDef]) -> Vec<(u8, &'static str)> {
    let (basics, mut nonbasics): (Vec<&CardDef>, Vec<&CardDef>) = cards
        .iter()
        .copied()
        .filter(|c| c.shape != Shape::Melded)
        .partition(|c| c.front().supertypes.contains(Supertypes::BASIC));
    nonbasics.sort_unstable_by_key(|c| (rank(seed, c.name), c.name));
    nonbasics.truncate(POOL_NONBASICS);
    nonbasics.sort_unstable_by_key(|c| c.name);
    // The colored symbols the spells ask for, W, U, B, R, G.
    let mut pips = [0u32; 5];
    for card in &nonbasics {
        for face in card.faces {
            for (pip, &n) in pips.iter_mut().zip(&face.cost.symbols) {
                *pip += u32::from(n);
            }
        }
    }
    if pips == [0; 5] {
        pips = [1; 5];
    }
    // One basic land a color, the first by name of those whose mana ability makes it, so that a
    // deck plays Island and not also Snow-Covered Island.
    let mut by_color: [Option<&CardDef>; 5] = [None; 5];
    let mut basics = basics;
    basics.sort_unstable_by_key(|c| c.name);
    for card in basics {
        let makes = card
            .front()
            .abilities
            .iter()
            .fold(0, |colors, a| colors | a.produces().colors);
        for (color, slot) in by_color.iter_mut().enumerate() {
            if makes & (1 << color) != 0 && slot.is_none() {
                *slot = Some(card);
            }
        }
    }
    for (pip, land) in pips.iter_mut().zip(&by_color) {
        if land.is_none() {
            *pip = 0;
        }
    }
    let lands = u32::try_from(FUZZ_DECK - nonbasics.len()).expect("a deck size");
    let total: u32 = pips.iter().sum();
    let mut counts = pips.map(|p| p * lands / total.max(1));
    // The lands rounding left over, to the colors that lost the most to it (largest remainder).
    let mut order: Vec<usize> = (0..5).collect();
    order.sort_by_key(|&c| (std::cmp::Reverse(pips[c] * lands % total.max(1)), c));
    let short = lands - counts.iter().sum::<u32>();
    for &c in order
        .iter()
        .filter(|&&c| pips[c] > 0)
        .cycle()
        .take(short as usize)
    {
        counts[c] += 1;
    }
    let mut deck: Vec<(u8, &'static str)> = nonbasics.iter().map(|c| (1, c.name)).collect();
    for (count, land) in counts.iter().zip(by_color) {
        if let (Some(land), 1..) = (land, *count) {
            deck.push((u8::try_from(*count).expect("a land count"), land.name));
        }
    }
    deck
}

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
    // The combined back face of a meld pair is never a card in a deck (CR 713.1b).
    let (mut basics, mut nonbasics): (Vec<&CardDef>, Vec<&CardDef>) = cards
        .iter()
        .copied()
        .filter(|c| c.shape != Shape::Melded)
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

/// The one library of a shared-deck game (`Format::SHARED_DECK`) that `deal` gives for `seed`:
/// the two decks a per-player game of the same seed deals, from `seed` and from `seed + 2^32`,
/// one after the other. With 40-card fuzz decks it is 80 cards, the size of the format's own
/// deck. The fixture deck names `shared-fuzz:<seed>` and `shared-fuzz:<seed + 2^32>` give the
/// same list for `fuzz_deck`, so a fuzz game's seed replays in the app.
pub fn shared_deck(
    seed: u64,
    deal: impl Fn(u64) -> Vec<(u8, &'static str)>,
) -> Vec<(u8, &'static str)> {
    let mut deck = deal(seed);
    deck.extend(deal(seed.wrapping_add(1 << 32)));
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

    /// The shared deck is the 80 cards of the published list, 23 of them distinct.
    #[test]
    fn the_shared_deck_holds_eighty_cards() {
        let total: usize = FORGETFUL_FISH.iter().map(|&(n, _)| usize::from(n)).sum();
        assert_eq!(total, 80);
        let mut names: Vec<_> = FORGETFUL_FISH.iter().map(|&(_, name)| name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 23);
    }

    /// Each keystone family's cards are dealt from: every card it names, which a fuzz deck holds
    /// whole, since no family names more than a deck's nonbasic cards.
    #[test]
    fn keystone_family_decks_hold_every_card_of_the_family() {
        for &(name, cards) in KEYSTONE_FAMILIES {
            let pool = keystone_family(name).unwrap();
            let deck = fuzz_deck_from(0, &pool);
            for card in cards {
                assert!(deck.contains(&(1, card)), "{name}: {card}");
            }
        }
        assert!(keystone_family("no such family").is_none());
    }

    /// The pool holds every card of every family once, and its decks are legal, hold only pool
    /// cards and play lands of the colors their spells need.
    #[test]
    fn pool_decks_are_legal_and_follow_their_spells_colors() {
        let pool = keystone_pool();
        for &(name, cards) in KEYSTONE_FAMILIES {
            for card in cards {
                assert!(pool.iter().any(|c| c.name == *card), "{name}: {card}");
            }
        }
        for (i, a) in pool.iter().enumerate() {
            assert!(pool[i + 1..].iter().all(|b| b.name != a.name), "{}", a.name);
        }
        for seed in 0..50 {
            let deck = pool_deck(seed);
            let size: usize = deck.iter().map(|&(n, _)| usize::from(n)).sum();
            assert_eq!(size, FUZZ_DECK, "seed {seed}: {deck:?}");
            for (_, name) in &deck {
                assert!(pool.iter().any(|c| c.name == *name), "seed {seed}: {name}");
            }
            let lands: usize = deck
                .iter()
                .filter(|(_, n)| {
                    CARDS
                        .get(n)
                        .unwrap()
                        .front()
                        .supertypes
                        .contains(Supertypes::BASIC)
                })
                .map(|&(n, _)| usize::from(n))
                .sum();
            assert_eq!(lands, FUZZ_DECK - POOL_NONBASICS, "seed {seed}: {deck:?}");
        }
        assert_ne!(pool_deck(0), pool_deck(1));
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
