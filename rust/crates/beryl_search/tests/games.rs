//! Random games over the cards implemented so far (architecture §17.4): fuzzing with the
//! invariants after every decision, determinism, information independence and the perf
//! counts. `BERYL_FUZZ_GAMES=<n>` plays more games and reports what they never reached; `just
//! fuzz <n>` does that in an optimized build that keeps the debug assertions.

mod common;

use std::collections::{BTreeSet, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};

use beryl_cards::CARDS;
use beryl_cards::decks::{VINTAGE_WU_CONTROL, WU_CONTROL, fuzz_deck};
use beryl_engine::card::{CardDef, FaceIx};
use beryl_engine::coverage;
use beryl_engine::rng::SplitMix64;
use beryl_engine::{
    CardId, CardIx, CardTable, Choice, Counts, Decision, GameTable, Large, LargeCardIx, LargeSlot,
    Layout, Misfit, Observation, Outcome, Profile, Relayout, Replay, Scratch, Seat, Slot, SlotId,
    Small, StallReason, State, Step, Zone, ZoneKey, apply, next,
};
use beryl_search::{Agent, AgentDecision, GreedyAgent, RandomAgent};
use common::{agents, fingerprint, fixed_game, game, games, mirror, play};

/// Every fourth game is played in the [`Large`] layout from its start, so the layout a game
/// outgrowing [`Small`](beryl_engine::Small) goes on in is fuzzed too. No capacity may run out
/// in any deck; how often games stall otherwise depends on the cards dealt, so the stall-rate
/// target is held on the fixed decks (`mirror_games_rarely_stall`) and only reported here.
#[test]
fn random_games_keep_the_invariants() {
    let n = games(40);
    let mut stalls = Vec::new();
    let mut kinds: HashMap<String, u64> = HashMap::new();
    coverage::start();
    for seed in 0..n {
        let (t, s) = game(seed);
        let outcome = if seed % 4 == 3 {
            fuzz(&t, s.widen(), seed, &mut kinds)
        } else {
            fuzz(&t, s, seed, &mut kinds)
        };
        match outcome {
            Outcome::Stalled(StallReason::Capacity) => panic!("seed {seed}: a capacity ran out"),
            Outcome::Stalled(reason) => stalls.push((seed, reason)),
            Outcome::Win(_) | Outcome::Draw => {}
        }
    }
    let hits = coverage::take();
    if std::env::var_os("BERYL_FUZZ_GAMES").is_some() {
        report(n, &kinds, &hits);
        println!("{} of {n} games stalled: {stalls:?}", stalls.len());
    }
}

/// Plays game `seed` from `s` with random agents, checking the reference implementations at
/// every decision and counting the decisions by kind. Debug assertions, which `just fuzz`
/// keeps on, check the invariants at every step.
fn fuzz<L: Layout>(
    t: &GameTable,
    mut s: State<L>,
    seed: u64,
    kinds: &mut HashMap<String, u64>,
) -> Outcome {
    /// Names the game when the engine panics during it.
    struct Game(u64);
    impl Drop for Game {
        fn drop(&mut self) {
            if std::thread::panicking() {
                eprintln!("the panic was in game {}", self.0);
            }
        }
    }
    let _game = Game(seed);
    play(t, &mut s, &mut Scratch::new(), &mut agents(seed), |s, d| {
        *kinds.entry(format!("{:?}", d.kind)).or_default() += 1;
        assert!(!d.is_empty(), "seed {seed}: {d:?}");
        if let Err(v) = beryl_engine::check_references(s, t) {
            panic!("a reference: {v:?}");
        }
    })
}

/// Widening is lossless: a game widened into [`Large`] at its start offers every decision
/// the [`Small`](beryl_engine::Small) game does, given the same answers, and stays the same
/// game as the `Small` one widened, decision after decision. Fuzz decks reach the most cards;
/// the fixed decks' games are long enough that the comparison covers a whole game.
#[test]
fn a_widened_game_plays_on_as_the_small_one() {
    for seed in 0..2 {
        widened_plays_on(game(seed), seed);
        let decisions = widened_plays_on(fixed_game(seed), seed);
        assert!(decisions > 40, "seed {seed}: a short game");
    }
}

/// Plays game `seed`, `(t, small)`, in both layouts at once, comparing them at every decision,
/// and returns how many decisions it had.
fn widened_plays_on((t, mut small): (GameTable, State), seed: u64) -> u32 {
    let mut large: State<Large> = small.widen();
    let (mut xs, mut xl) = (Scratch::new(), Scratch::<Large>::new());
    let mut agents = agents(seed);
    let mut decisions = 0;
    let mut asked = [AgentDecision(0); 2];
    loop {
        let step = next(&mut small, &t, &mut xs, &mut Replay);
        assert_eq!(
            next(&mut large, &t, &mut xl, &mut Replay),
            step.relayout(),
            "seed {seed}"
        );
        assert!(small.widen() == large, "seed {seed}, decision {decisions}");
        let Step::Decide(d) = step else { break };
        let c = agents[d.player.index()].choose(
            Observation::new(&small, d.player),
            &d,
            asked[d.player.index()].take(),
        );
        apply(&mut small, &t, &mut xs, &mut Replay, c);
        apply(&mut large, &t, &mut xl, &mut Replay, c);
        decisions += 1;
    }
    decisions
}

/// The 40-card decks a large deck of game `seed` is made of, one after another: fuzz decks
/// drawn from `seed`, or where `fixed`, the two fixed test decks in turn. Tests assert how a
/// game goes (that it is not short, that it reaches a card) only on the fixed decks, which do
/// not change as cards are written.
fn decks_of(seed: u64, fixed: bool) -> impl Iterator<Item = Vec<(u8, &'static str)>> {
    (0..).map(move |i: u64| {
        if fixed {
            [WU_CONTROL, VINTAGE_WU_CONTROL][(i % 2) as usize].to_vec()
        } else {
            fuzz_deck(seed.wrapping_add(i << 32))
        }
    })
}

/// The table of a game between two decks of `cards` cards each, from [`decks_of`].
fn table_of(cards: usize, seed: u64, fixed: bool) -> GameTable {
    let deck = |seed: u64, first: usize| -> Vec<(u8, &'static str)> {
        decks_of(seed, fixed)
            .skip(first)
            .flatten()
            .flat_map(|(count, name)| (0..count).map(move |_| (1, name)))
            .take(cards)
            .collect()
    };
    let decks = [deck(seed, 0), deck(seed.wrapping_add(1 << 16), 1)];
    GameTable::new(&CARDS, [decks[0].as_slice(), decks[1].as_slice()]).unwrap()
}

/// Two decks of 80 cards are more than [`Small`] has slots for, and no rule forbids them
/// ([CR 100.5](https://yawgatog.com/resources/magic-rules/#R1005): "There is no maximum deck
/// size for non-Commander decks."). `Small` refuses the game where its state is made;
/// [`Large`] deals it, and it plays to its end with the invariants holding at every step.
#[test]
fn eighty_card_decks_are_refused_by_small_and_play_in_large() {
    let too_large = Misfit::DeckTooLarge {
        player: Seat::P0,
        cards: 80,
        slots: 60,
    };
    for seed in 0..3 {
        let fixed = seed == 0;
        let t = table_of(80, seed, fixed);
        assert_eq!(State::new(&t, seed, Profile::SMART).err(), Some(too_large));
        let mut s: State<Large> = State::deal(&t, seed, Profile::SMART).unwrap();
        assert!(Seat::BOTH.iter().all(|&p| s.library(p).len() == 80));
        assert_eq!(s.fits::<Small>(&t), Err(too_large));
        let check = |s: &State<Large>| {
            s.check_invariants(&t)
                .unwrap_or_else(|v| panic!("seed {seed}: {v:?}"));
        };
        let mut decisions = 0;
        let mut x = Scratch::new();
        let outcome = play(&t, &mut s, &mut x, &mut agents(seed), |s, _| {
            check(s);
            decisions += 1;
        });
        check(&s);
        assert!(
            matches!(outcome, Outcome::Win(_) | Outcome::Draw),
            "seed {seed}: {outcome:?}"
        );
        assert!(!fixed || decisions > 40, "seed {seed}: a short game");
        // The cards a player owns are still their 80, wherever they went.
        for p in Seat::BOTH {
            let owned = [
                Zone::Library,
                Zone::Hand,
                Zone::Battlefield,
                Zone::Graveyard,
                Zone::Exile,
            ]
            .map(|zone| s.zone(ZoneKey::Of(p, zone)).count())
            .iter()
            .sum::<u32>();
            assert!(owned >= 80, "seed {seed}: player {p:?} owns {owned}");
        }
    }
}

/// A table over clones of every implemented card, each a card of its own under its own name
/// (as the scale harness clones the card files: `tools/src/beryl_tools/scale.py`), and two
/// decks that share none: six of [`decks_of`]'s decks one after another for each player, every
/// one over a different set of clones.
fn table_of_clones(seed: u64, fixed: bool) -> GameTable {
    const DECKS: u64 = 6;
    let clone = |copy: u64, name: &str| -> &'static str {
        Box::leak(format!("{name} #{copy}").into_boxed_str())
    };
    let clones: Vec<&'static CardDef> = (0..2 * DECKS)
        .flat_map(|copy| CARDS.cards().iter().map(move |def| (copy, def)))
        .map(|(copy, def)| {
            &*Box::leak(Box::new(CardDef {
                name: clone(copy, def.name),
                faces: def.faces,
                tokens: def.tokens,
            }))
        })
        .collect();
    let cards: &'static CardTable = Box::leak(Box::new(CardTable::new(Box::leak(
        clones.into_boxed_slice(),
    ))));
    let deck = |player: u64| -> Vec<(u8, &'static str)> {
        (player * DECKS..)
            .zip(decks_of(seed, fixed).skip((player * DECKS) as usize))
            .take(DECKS as usize)
            .flat_map(|(copy, deck)| {
                deck.into_iter()
                    .map(move |(count, name)| (count, clone(copy, name)))
            })
            .collect()
    };
    let decks = [deck(0), deck(1)];
    GameTable::new(cards, [decks[0].as_slice(), decks[1].as_slice()]).unwrap()
}

/// A game of more distinct cards than the one byte of [`Small`]'s game index numbers is
/// refused there, where its state is made, and dealt in [`Large`], whose index is two bytes:
/// it plays to its end with the invariants holding at every step, cards past index 254 among
/// its objects, under agents that read their own tables by game index.
#[test]
fn a_table_past_255_cards_is_refused_by_small_and_plays_in_large() {
    for seed in 0..3 {
        let fixed = seed == 0;
        let t = table_of_clones(seed, fixed);
        assert!(t.len() > CardIx::COUNT, "seed {seed}: {} cards", t.len());
        let too_many = Misfit::TooManyCards {
            cards: t.len(),
            indices: 255,
        };
        assert_eq!(State::new(&t, seed, Profile::SMART).err(), Some(too_many));
        // The table's last card, which every table has, is one only the wide index names.
        assert_eq!(t.face_down_copy::<Large>().index(), t.len() - 1);
        let mut s: State<Large> = State::deal(&t, seed, Profile::SMART).unwrap();
        assert_eq!(s.fits::<Small>(&t), Err(too_many));
        let wide = |s: &State<Large>| {
            LargeSlot::all()
                .filter_map(|slot| s.card_behind(slot))
                .filter(|card| card.to::<CardIx>().is_none())
                .count()
        };
        assert!(wide(&s) > 50, "seed {seed}: {} wide cards", wide(&s));
        let check = |s: &State<Large>| {
            s.check_invariants(&t)
                .unwrap_or_else(|v| panic!("seed {seed}: {v:?}"));
        };
        let (mut decisions, mut seen_wide) = (0, false);
        let mut x = Scratch::new();
        let watch = |s: &State<Large>, d: &Decision<Large>| {
            check(s);
            decisions += 1;
            // A determinization deals the hidden cards again by their wide indices.
            if decisions % 64 == 0 {
                let det = Observation::new(s, d.player).determinize(&t, &mut SplitMix64::new(seed));
                check(&det);
            }
            // A wide card where every rule reads it: the battlefield, the stack.
            seen_wide |= s
                .battlefield()
                .iter()
                .copied()
                .chain(s.stack().iter().filter_map(|e| e.object().map(|r| r.slot)))
                .any(|slot| {
                    s.card_behind(slot)
                        .is_some_and(|c| c.to::<CardIx>().is_none())
                });
        };
        // The greedy agent's policy reads its card values by game index; the random one
        // reaches the plays a policy never makes.
        let outcome = if seed == 0 {
            let mut agents = [GreedyAgent::new(&t), GreedyAgent::new(&t)];
            play(&t, &mut s, &mut x, &mut agents, watch)
        } else {
            play(&t, &mut s, &mut x, &mut agents(seed), watch)
        };
        check(&s);
        assert!(
            matches!(outcome, Outcome::Win(_) | Outcome::Draw),
            "seed {seed}: {outcome:?}"
        );
        assert!(!fixed || decisions > 40, "seed {seed}: a short game");
        assert!(
            !fixed || seen_wide,
            "seed {seed}: no card past Small's index was played"
        );
    }
}

/// A game between 60-card decks, the most [`Small`] holds, is the same game in either
/// layout: the same seed deals it the same, and at every decision the `Small` game widened
/// and narrowed again is itself, slot for slot, as is each decision it is asked.
#[test]
fn a_sixty_card_game_round_trips_between_the_layouts() {
    for seed in 0..3 {
        let fixed = seed == 0;
        let t = table_of(60, seed, fixed);
        let mut small = State::new(&t, seed, Profile::SMART).unwrap();
        let dealt_wide: State<Large> = State::deal(&t, seed, Profile::SMART).unwrap();
        assert!(dealt_wide == small.widen(), "seed {seed}");
        let (mut x, mut agents) = (Scratch::new(), agents(seed));
        let mut decisions = 0;
        let mut asked = [AgentDecision(0); 2];
        loop {
            let wide = small.widen();
            assert!(wide.try_relayout::<Small>(&t) == Ok(small), "seed {seed}");
            assert!(wide.try_relayout::<Large>(&t) == Ok(wide), "seed {seed}");
            let Step::Decide(d) = next(&mut small, &t, &mut x, &mut Replay) else {
                break;
            };
            assert_eq!(d.relayout::<Large>().relayout::<Small>(), d, "seed {seed}");
            let c = agents[d.player.index()].choose(
                Observation::new(&small, d.player),
                &d,
                asked[d.player.index()].take(),
            );
            apply(&mut small, &t, &mut x, &mut Replay, c);
            decisions += 1;
        }
        assert!(!fixed || decisions > 40, "seed {seed}: a short game");
    }
}

/// A game that has outgrown [`Small`] is refused there by name, not by a panic: one token
/// more than its battlefield lists, then one more than its pool has slots for.
#[test]
fn a_game_past_small_does_not_narrow() {
    let (t, small) = game(0);
    let mut s = small.widen();
    assert!(
        s.try_relayout::<Small>(&t) == Ok(small),
        "a Small game fits"
    );
    let token = |s: &mut State<Large>| {
        s.board()
            .place_token(Seat::P0, LargeCardIx::FIRST, FaceIx::FRONT, false)
            .unwrap();
    };
    for _ in 0..129 {
        token(&mut s);
    }
    let battlefield = Misfit::ListTooLong {
        list: "the battlefield",
        len: 129,
        capacity: 128,
    };
    assert_eq!(s.try_relayout::<Small>(&t).err(), Some(battlefield));
    for _ in 129..136 {
        token(&mut s);
    }
    assert_eq!(
        s.try_relayout::<Small>(&t).err(),
        Some(Misfit::PoolTooSmall { slots: 135 })
    );
}

/// What a long run reached: how often each kind of decision came up, and the abilities and
/// casting options it never exercised, which random play needs help to find.
fn report(games: u64, kinds: &HashMap<String, u64>, hits: &BTreeSet<usize>) {
    let mut kinds: Vec<_> = kinds.iter().collect();
    kinds.sort_by_key(|&(k, n)| (std::cmp::Reverse(*n), k.clone()));
    println!("fuzz: {games} games; decisions by kind:");
    for (kind, n) in kinds {
        println!("  {kind}: {n}");
    }
    let never: Vec<String> = beryl_cards::parts()
        .into_iter()
        .filter(|(_, address)| !hits.contains(address))
        .map(|(what, _)| what)
        .collect();
    println!(
        "fuzz: {} abilities and casting options never exercised:",
        never.len()
    );
    for what in never {
        println!("  {what}");
    }
}

/// Plays a game with random agents, recording every choice and the state hash at every
/// decision.
fn record((t, mut s): (GameTable, State), seed: u64) -> (Vec<Choice>, Vec<u64>) {
    let mut x = Scratch::new();
    let mut agents = agents(seed);
    let (mut choices, mut hashes) = (Vec::new(), Vec::new());
    let mut asked = [AgentDecision(0); 2];
    loop {
        match next(&mut s, &t, &mut x, &mut Replay) {
            Step::Over(_) => return (choices, hashes),
            Step::Decide(d) => {
                hashes.push(fingerprint(&s));
                let c = agents[d.player.index()].choose(
                    Observation::new(&s, d.player),
                    &d,
                    asked[d.player.index()].take(),
                );
                choices.push(c);
                apply(&mut s, &t, &mut x, &mut Replay, c);
            }
        }
    }
}

/// Replaying a game's choices from its seed reproduces it hash for hash, in fuzz decks' games
/// and the fixed decks'. On the fixed decks, the games are long and two seeds deal different
/// games, so the hashes are not trivially equal.
#[test]
fn a_seed_and_its_choices_reproduce_every_state_hash() {
    for (seed, fixed) in [(0, false), (1, false), (0, true), (1, true)] {
        let deal = |seed| if fixed { fixed_game(seed) } else { game(seed) };
        let (choices, hashes) = record(deal(seed), seed);
        let (t, mut s) = deal(seed);
        let mut x = Scratch::new();
        let mut replayed = Vec::new();
        let mut choices = choices.into_iter();
        while let Step::Decide(_) = next(&mut s, &t, &mut x, &mut Replay) {
            replayed.push(fingerprint(&s));
            apply(&mut s, &t, &mut x, &mut Replay, choices.next().unwrap());
        }
        assert_eq!(replayed, hashes, "seed {seed}");
        if fixed {
            assert!(hashes.len() > 40, "seed {seed}: a short game");
            let other = record(deal(seed + 4), seed + 4).1;
            assert_ne!(other, hashes, "seeds {seed} and {}", seed + 4);
        }
    }
}

/// Decides by hashing everything its Observation shows, so a view that leaks anything hidden
/// decides differently on two states that differ only in what is hidden.
struct Observer;

impl Agent for Observer {
    fn choose(&mut self, obs: Observation<'_>, d: &Decision, _at: AgentDecision) -> Choice {
        let mut h = DefaultHasher::new();
        for slot in Slot::all() {
            (obs.knows(slot), obs.card(slot), obs.object(slot)).hash(&mut h);
        }
        for p in Seat::BOTH {
            obs.player(p).hash(&mut h);
            obs.library(p).hash(&mut h);
            obs.graveyard(p).hash(&mut h);
            for zone in [
                Zone::Library,
                Zone::Hand,
                Zone::Battlefield,
                Zone::Graveyard,
                Zone::Exile,
            ] {
                obs.zone(ZoneKey::Of(p, zone)).hash(&mut h);
            }
        }
        (obs.viewer(), obs.turn(), obs.battlefield()).hash(&mut h);
        Choice((h.finish() % d.len() as u64) as u16)
    }
}

#[test]
fn agents_decide_the_same_on_a_redeterminized_state() {
    let mut rng = SplitMix64::new(7);
    let (mut decisions, mut redealt) = (0, 0);
    for seed in 0..games(10) {
        let (t, mut s) = game(seed);
        play(
            &t,
            &mut s,
            &mut Scratch::new(),
            &mut agents(seed),
            |s, d| {
                let viewer = d.player;
                let mut det = Observation::new(s, viewer).determinize(&t, &mut rng);
                det.check_invariants(&t).unwrap();
                for slot in Slot::all() {
                    if s.knows(viewer, slot) {
                        assert_eq!(det.object(slot), s.object(slot), "slot {slot}");
                    }
                }
                let ours = Observer.choose(Observation::new(s, viewer), d, AgentDecision(0));
                let theirs = Observer.choose(Observation::new(&det, viewer), d, AgentDecision(0));
                assert_eq!(
                    ours, theirs,
                    "seed {seed}: the view of a determinized state differs"
                );
                // The engine offers the viewer the same decision there.
                match next(&mut det, &t, &mut Scratch::new(), &mut Replay) {
                    Step::Decide(again) => assert_eq!(again, *d, "seed {seed}"),
                    Step::Over(o) => panic!("seed {seed}: the determinized game ended: {o:?}"),
                }
                decisions += 1;
                if Slot::all().any(|slot| det.card_behind(slot) != s.card_behind(slot)) {
                    redealt += 1;
                }
            },
        );
    }
    // Hidden cards really were dealt anew: the test has something to catch.
    assert!(redealt * 2 > decisions, "{redealt} of {decisions}");
}

#[test]
fn determinization_keeps_the_size_of_every_hidden_group() {
    let (t, mut s) = game(3);
    play(&t, &mut s, &mut Scratch::new(), &mut agents(3), |s, d| {
        let det = Observation::new(s, d.player).determinize(&t, &mut SplitMix64::new(1));
        for p in Seat::BOTH {
            let cards = |s: &State| {
                let mut v: Vec<CardIx> = Slot::all()
                    .filter(|&slot| {
                        s.object(slot).owner() == p && s.object(slot).zone() != Zone::Nowhere
                    })
                    .filter_map(|slot| s.card_behind(slot))
                    .collect();
                v.sort_unstable();
                v
            };
            assert_eq!(cards(&det), cards(s), "player {p} has the same deck");
        }
    });
}

/// The agents of a seeded WU Control mirror game: both random or both greedy.
fn mirror_agents(t: &GameTable, greedy: bool, seed: u64) -> [Box<dyn Agent>; 2] {
    if greedy {
        [Box::new(GreedyAgent::new(t)), Box::new(GreedyAgent::new(t))]
    } else {
        [
            Box::new(RandomAgent::new(seed * 2)),
            Box::new(RandomAgent::new(seed * 2 + 1)),
        ]
    }
}

/// The perf counts of one seeded WU Control mirror game under smart stops, and its outcome.
fn mirror_game(t: &GameTable, greedy: bool, seed: u64) -> (Counts, Outcome) {
    let mut s = State::new(t, seed, Profile::SMART).unwrap();
    let mut x = Scratch::new();
    let outcome = play(
        t,
        &mut s,
        &mut x,
        &mut mirror_agents(t, greedy, seed),
        |_, _| {},
    );
    (x.counts, outcome)
}

/// The perf counts of seeded games (architecture §17.4), each with the value it is pinned at,
/// within a tolerance band of 5%: a change that moves one out of its band either fixes a
/// regression or updates the pin on purpose.
#[test]
fn perf_counts_stay_in_their_bands() {
    // (greedy, seed): decisions, silent, state-based checks, state copies, layer recomputes,
    // solver runs, events.
    let pinned: [(bool, u64, [u64; 7]); 6] = [
        // The random games, as the random agent draws each decision from its seed and the
        // decision's index among its own.
        (false, 0, [173, 534, 659, 92, 55, 61, 744]),
        (false, 1, [225, 698, 838, 178, 66, 65, 886]),
        (false, 2, [178, 581, 720, 96, 59, 73, 770]),
        (true, 0, [115, 442, 527, 62, 41, 41, 555]),
        (true, 1, [138, 531, 604, 66, 52, 43, 665]),
        (true, 2, [406, 913, 1205, 206, 101, 91, 1366]),
    ];
    let t = mirror();
    let mut outside = Vec::new();
    for (greedy, seed, pinned) in pinned {
        let (c, _) = mirror_game(&t, greedy, seed);
        let actual = [
            c.decisions,
            c.silent,
            c.state_based_checks,
            c.state_copies,
            c.layer_recomputes,
            c.solver_runs,
            c.events,
        ];
        let names = [
            "decisions",
            "silent",
            "state_based_checks",
            "state_copies",
            "layer_recomputes",
            "solver_runs",
            "events",
        ];
        for ((name, actual), pinned) in names.into_iter().zip(actual).zip(pinned) {
            if actual.abs_diff(pinned) * 100 > pinned * 5 {
                outside.push((greedy, seed, name, actual, pinned));
            }
        }
    }
    assert!(
        outside.is_empty(),
        "outside their bands (greedy, seed, name, actual, pinned): {outside:?}"
    );
}

/// The stall-rate target (architecture §1): under 0.5% of seeded WU Control mirror games,
/// random and greedy, stall.
#[test]
fn mirror_games_rarely_stall() {
    let t = mirror();
    let n = games(25);
    let mut stalls = Vec::new();
    for greedy in [false, true] {
        for seed in 0..n {
            if let (_, Outcome::Stalled(reason)) = mirror_game(&t, greedy, seed) {
                stalls.push((greedy, seed, reason));
            }
        }
    }
    assert!(
        stalls.len() * 200 < 2 * n as usize,
        "{} of {} games stalled: {stalls:?}",
        stalls.len(),
        2 * n
    );
}
