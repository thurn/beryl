//! Random games over one keystone family's cards at a time (`decks::KEYSTONE_FAMILIES`): fuzz
//! decks dealt from the family's keystones, its partners and the basic lands, so the cards of a
//! family meet each other in nearly every game, where decks drawn from every implemented card
//! rarely hold two of them. Each game starts in `Small` and goes on in `Large` from the engine
//! call that ran out of a capacity, as the app's games do; the test reports those games with
//! the list that filled. `just fuzz` plays these at length; no check runs them.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::common::widening::{Watch, Widened, compares_layouts, play_watched, play_widening};
use crate::common::{agents, games, seeds};
use beryl_cards::CARDS;
use beryl_cards::decks::{
    KEYSTONE_FAMILIES, fuzz_deck, fuzz_deck_from, keystone_family, keystone_pool, pool_deck,
    shared_deck,
};
use beryl_engine::card::Supertypes;
use beryl_engine::{Decklist, Format, GameTable, Outcome, Profile, StallReason, State};

/// Plays a family's games: two fuzz decks per game from its cards, a quarter as many games as
/// the whole fuzzer plays. No game may panic, break an invariant or stall for a reason other
/// than a budget; a game that leaves `Small` is reported, with the list that filled.
fn family(name: &str) {
    family_with_sideboard(name, &[]);
}

fn family_with_sideboard(name: &str, sideboard: &Decklist) {
    let cards = keystone_family(name).expect("a keystone family");
    let n = games(8).div_ceil(4).max(2);
    let played = play(
        name,
        n,
        |seed| per_player(|seed| fuzz_deck_from(seed, &cards), sideboard, seed),
        compares_layouts,
        None,
        |_| {},
    );
    println!("{name}: {played}");
}

/// The table of a per-player game: two decks dealt by `deal`, the second from the seed plus
/// 2^32, each with `sideboard`.
fn per_player(
    deal: impl Fn(u64) -> Vec<(u8, &'static str)>,
    sideboard: &Decklist,
    seed: u64,
) -> GameTable {
    let decks = [deal(seed), deal(seed.wrapping_add(1 << 32))];
    GameTable::with_sideboards(
        Format::CONSTRUCTED,
        &CARDS,
        decks.each_ref().map(Vec::as_slice),
        [sideboard; 2],
    )
    .unwrap()
}

/// The table of a shared-deck game (`Format::SHARED_DECK`): the two decks a per-player game of
/// the seed deals, joined into one library ([`shared_deck`]). The format has no sideboard.
fn shared(deal: impl Fn(u64) -> Vec<(u8, &'static str)>, seed: u64) -> GameTable {
    let deck = shared_deck(seed, deal);
    GameTable::new(Format::SHARED_DECK, &CARDS, [deck.as_slice(), &[]]).unwrap()
}

/// What a run of [`play`] saw.
#[derive(Default)]
struct Played {
    games: u64,
    decisions: u64,
    entered_subgame: u32,
    stalls: BTreeMap<String, u64>,
    widened: BTreeMap<&'static str, u64>,
}

impl Played {
    fn stalled(&self) -> u64 {
        self.stalls.values().sum()
    }
}

impl std::fmt::Display for Played {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} games, {} decisions; {} entered a subgame; stalls by reason {:?}; \
             {} left Small, by the list that filled: {:?}",
            self.games,
            self.decisions,
            self.entered_subgame,
            self.stalls,
            self.widened.values().sum::<u64>(),
            self.widened,
        )
    }
}

/// Plays `n` games from the run's first seed, each on the table `table` makes for its seed,
/// under the exact profile on even seeds and smart stops on odd ones, with
/// the layouts compared where `compare` says, every decision shown to `watch` and each game
/// to `finished` once it ends. No game may panic, break an invariant or run out of a capacity
/// in `Large`.
fn play(
    name: &str,
    n: u64,
    table: impl Fn(u64) -> GameTable,
    compare: impl Fn(u64) -> bool,
    mut watch: Option<Watch<'_>>,
    mut finished: impl FnMut(&Widened),
) -> Played {
    let mut played = Played {
        games: n,
        ..Played::default()
    };
    let mut kinds: HashMap<String, u64> = HashMap::new();
    for seed in seeds(n) {
        let t = table(seed);
        let profile = if seed.is_multiple_of(2) {
            Profile::EXACT
        } else {
            Profile::SMART
        };
        let game = play_watched(
            &t,
            State::new(&t, seed, profile).unwrap(),
            seed,
            &mut kinds,
            compare(seed),
            &mut agents(seed),
            watch.as_deref_mut().map(|w| w as Watch<'_>),
        );
        finished(&game);
        played.decisions += u64::from(game.decisions);
        played.entered_subgame += u32::from(game.entered_subgame);
        if let Some(list) = game.filled {
            *played.widened.entry(list).or_default() += 1;
        }
        match game.outcome {
            Outcome::Stalled(StallReason::Capacity) => {
                panic!("{name}, seed {seed}: a capacity ran out in Large");
            }
            Outcome::Stalled(reason) => {
                *played.stalls.entry(format!("{reason:?}")).or_default() += 1;
            }
            Outcome::Win(_) | Outcome::Draw => {}
        }
    }
    played
}

/// The pairs of pool cards seen together in a game, and the count in play at each decision.
type Seen = (BTreeSet<(&'static str, &'static str)>, Vec<usize>);

/// Random games with both decks dealt from every keystone and partner of every family
/// ([`pool_deck`]), so that cards of different families meet: Humility with Gemrazer, Rest in
/// Peace with Underworld Breach, Painter's Servant with Mother of Runes. Every game compares
/// both layouts at every decision, since the pool exists to find what the families alone do
/// not. It prints each pair of pool cards that were on the battlefield together with the
/// games they were in, how many pool cards were in play at each game's middle decision, and
/// the share of games that stalled. Burning Wish, a pool card, has itself in the sideboard to
/// fetch, as the outside-the-game family plays it.
#[test]
#[ignore = "random games of every family together: `just fuzz` plays them"]
fn every_family_together() {
    let pool: BTreeSet<&'static str> = keystone_pool()
        .iter()
        .filter(|c| !c.front().supertypes.contains(Supertypes::BASIC))
        .map(|c| c.name)
        .collect();
    let n = games(8).div_ceil(4).max(2);
    let current: RefCell<Seen> = RefCell::default();
    // Each pair with the games it was seen in, and each game's count at its middle decision.
    let mut pairs: BTreeMap<(&str, &str), u64> = BTreeMap::new();
    let mut middles: Vec<usize> = Vec::new();
    let mut watch = |names: &[&'static str]| {
        let mut here: Vec<&'static str> =
            names.iter().copied().filter(|n| pool.contains(n)).collect();
        here.sort_unstable();
        here.dedup();
        let (seen, counts) = &mut *current.borrow_mut();
        counts.push(here.len());
        for (i, &a) in here.iter().enumerate() {
            for &b in &here[i + 1..] {
                seen.insert((a, b));
            }
        }
    };
    let played = play(
        "every family",
        n,
        |seed| per_player(pool_deck, &[(1, beryl_cards::names::BURNING_WISH)], seed),
        |_| true,
        Some(&mut watch),
        |_| {
            let (seen, counts) = current.take();
            for pair in seen {
                *pairs.entry(pair).or_default() += 1;
            }
            middles.push(counts.get(counts.len() / 2).copied().unwrap_or(0));
        },
    );
    println!("every family together: {played}");
    #[allow(clippy::cast_precision_loss)] // A few thousand games at most.
    let share = |k: u64| 100.0 * k as f64 / played.games.max(1) as f64;
    println!(
        "stall rate {:.1}% ({} of {} games)",
        share(played.stalled()),
        played.stalled(),
        played.games
    );
    middles.sort_unstable();
    let at_least =
        |k: usize| u64::try_from(middles.iter().filter(|&&m| m >= k).count()).unwrap_or(u64::MAX);
    println!(
        "pool cards in play at a game's middle decision: median {}, two or more in {:.0}% of games, three or more in {:.0}%",
        middles.get(middles.len() / 2).copied().unwrap_or(0),
        share(at_least(2)),
        share(at_least(3)),
    );
    let mut by_games: Vec<_> = pairs.iter().collect();
    by_games.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    println!(
        "{} pairs of pool cards on the battlefield together, by the games they were in:",
        pairs.len()
    );
    for ((a, b), games) in by_games {
        println!("  {games:>5}  {a} + {b}");
    }
}

#[test]
#[ignore = "random games of one family: `just fuzz` plays them"]
fn attachments() {
    family("attachments");
}

#[test]
#[ignore = "random games of one family: `just fuzz` plays them"]
fn layers() {
    family("layers");
}

#[test]
#[ignore = "random games of one family: `just fuzz` plays them"]
fn replacement() {
    family("replacement");
}

#[test]
#[ignore = "random games of one family: `just fuzz` plays them"]
fn zones_and_text() {
    family("zones-and-text");
}

#[test]
#[ignore = "random games of one family: `just fuzz` plays them"]
fn copies() {
    family("copies");
}

#[test]
#[ignore = "random games of one family: `just fuzz` plays them"]
fn costs() {
    family("costs");
}

#[test]
#[ignore = "random games of one family: `just fuzz` plays them"]
fn identity() {
    family("identity");
}

#[test]
#[ignore = "random games of one family: `just fuzz` plays them"]
fn decisions() {
    family("decisions");
}

#[test]
#[ignore = "random games of one family: `just fuzz` plays them"]
fn turn_structure() {
    family("turn");
}

#[test]
#[ignore = "random games of one family: `just fuzz` plays them"]
fn outside_the_game() {
    family_with_sideboard("outside-the-game", &[(1, beryl_cards::names::BURNING_WISH)]);
}

#[test]
#[ignore = "random games of one family: `just fuzz` plays them"]
fn subgames() {
    family("subgames");
}

/// The keystone families whose shared-deck games are not played, each with why. Every family
/// of [`KEYSTONE_FAMILIES`] not named here is played by [`shared_games_of_every_family`].
const SHARED_LEFT_OUT: &[(&str, &str)] = &[];

/// Random shared-deck games (`Format::SHARED_DECK`) of each keystone family in turn: one
/// library and one graveyard for both players, dealt from the two decks a per-player game of
/// the family deals ([`shared_deck`]), with the invariants checked at every decision as in the
/// per-player games. No game may panic, break an invariant or run out of a capacity in
/// `Large`. A family that is added to the list is played here too.
#[test]
#[ignore = "random shared-deck games of every family: `just fuzz` plays them"]
fn shared_games_of_every_family() {
    let n = games(8).div_ceil(4).max(2);
    for &(name, _) in KEYSTONE_FAMILIES {
        if let Some((_, why)) = SHARED_LEFT_OUT.iter().find(|(n, _)| *n == name) {
            println!("shared {name}: left out, because {why}");
            continue;
        }
        let cards = keystone_family(name).expect("a keystone family");
        let played = play(
            name,
            n,
            |seed| shared(|seed| fuzz_deck_from(seed, &cards), seed),
            compares_layouts,
            None,
            |_| {},
        );
        println!("shared {name}: {played}");
    }
}

/// Random shared-deck games dealt from every keystone family together ([`pool_deck`]) and from
/// every implemented card ([`fuzz_deck`]), with the layouts compared at every decision of the
/// pool's games.
#[test]
#[ignore = "random shared-deck games of the pool and of every card: `just fuzz` plays them"]
fn shared_games_of_the_pool_and_every_card() {
    let n = games(8).div_ceil(4).max(2);
    let pool = play(
        "shared pool",
        n,
        |seed| shared(pool_deck, seed),
        |_| true,
        None,
        |_| {},
    );
    println!("shared pool: {pool}");
    let every = play(
        "shared fuzz",
        n,
        |seed| shared(fuzz_deck, seed),
        compares_layouts,
        None,
        |_| {},
    );
    println!("shared fuzz decks: {every}");
}

/// A deterministic board setup in which Worldgorger Dragon and Animate Dead enter a mandatory
/// loop, so the game ends in a draw (see CR 104.4b).
#[test]
fn a_loop_above_a_waiting_trigger_is_a_draw() {
    use beryl_cards::names::{ANIMATE_DEAD, PLAINS, SWAMP, WORLDGORGER_DRAGON};
    use beryl_engine::{Seat, TurnStep, Zone};
    let t = GameTable::new(
        Format::CONSTRUCTED,
        &CARDS,
        [
            &[
                (1, WORLDGORGER_DRAGON),
                (1, ANIMATE_DEAD),
                (2, SWAMP),
                (18, PLAINS),
            ],
            &[(20, PLAINS)],
        ],
    )
    .unwrap();
    let mut s: State = State::new(&t, 0, Profile::SMART).unwrap();
    s.board().begin_at(3, Seat::P0, TurnStep::PrecombatMain);
    for slot in s.library(Seat::P0).to_vec() {
        let zone = match s.definition_name(&t, slot) {
            WORLDGORGER_DRAGON => Zone::Graveyard,
            SWAMP => Zone::Battlefield,
            ANIMATE_DEAD => Zone::Hand,
            _ => continue,
        };
        s.board().place(slot, zone).unwrap();
    }
    let played = play_widening(&t, s, 0, &mut HashMap::new(), false, &mut agents(0));
    assert_eq!(played.outcome, Outcome::Draw);
}

/// Worldgorger Dragon's enters ability exiles 33 Mountains, linking each to the Dragon with a
/// memo, one more than the 32 memos `Small` holds. The game goes on in `Large`, naming the
/// memos as the list that filled, and plays on without running out of a capacity there.
#[test]
fn a_dragon_exiling_more_than_small_holds_plays_on_in_large() {
    use beryl_cards::names::{MOUNTAIN, PLAINS, WORLDGORGER_DRAGON};
    use beryl_engine::{Seat, TurnStep, Zone};
    let t = GameTable::new(
        Format::CONSTRUCTED,
        &CARDS,
        [&[(33, MOUNTAIN), (7, WORLDGORGER_DRAGON)], &[(20, PLAINS)]],
    )
    .unwrap();
    let mut s: State = State::new(&t, 0, Profile::EXACT).unwrap();
    s.board().begin_at(3, Seat::P0, TurnStep::PrecombatMain);
    for slot in s.library(Seat::P0).to_vec() {
        let zone = if s.definition_name(&t, slot) == MOUNTAIN {
            Zone::Battlefield
        } else {
            Zone::Hand
        };
        s.board().place(slot, zone).unwrap();
    }
    let played = play_widening(&t, s, 0, &mut HashMap::new(), true, &mut agents(0));
    assert_ne!(played.outcome, Outcome::Stalled(StallReason::Capacity));
    assert_eq!(played.filled, Some("the memos"), "{:?}", played.outcome);
}
