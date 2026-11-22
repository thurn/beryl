//! Random games over the cards implemented so far (architecture §17.4): fuzzing with the
//! invariants after every decision, determinism, information independence and the perf
//! counts. `BERYL_FUZZ_GAMES=<n>` plays more games and reports what they never reached, from the
//! seed `BERYL_FUZZ_FIRST_SEED` names (0 if unset); `just
//! fuzz <n>` does that in an optimized build that keeps the debug assertions.

use std::collections::{BTreeSet, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};

use crate::common::widening::{compares_layouts, play_widening};
use crate::common::{agents, fingerprint, fixed_game, game, games, mirror, play, redeal, seeds};
use beryl_cards::decks::{VINTAGE_WU_CONTROL, WU_CONTROL, fuzz_deck};
use beryl_cards::{CARDS, names};
use beryl_engine::card::{CardDef, Colors, FaceIx, Words};
use beryl_engine::coverage;
use beryl_engine::filter::Filter;
use beryl_engine::rng::SplitMix64;
use beryl_engine::state::Pending;
use beryl_engine::{
    CardId, CardIx, CardTable, Choice, Counts, Decision, Format, GameTable, Large, LargeCardIx,
    LargeSlot, Layout, Misfit, Observation, Outcome, Profile, Relayout, Replay, Scratch, Seat,
    Slot, SlotId, Small, StallReason, State, Step, Zone, ZoneKey, apply, next, observe,
};
use beryl_search::{Agent, AgentDecision, GreedyAgent, RandomAgent};

/// Every fourth game is played in the [`Large`] layout from its start, so the layout a game
/// outgrowing [`Small`](beryl_engine::Small) goes on in is fuzzed too. The others start in
/// `Small` and go on in `Large` from the engine call that ran out of a capacity, as the app's
/// games do: a card that exiles every other permanent its controller has, keeping a memo for
/// each, can outgrow `Small`'s lists in a long game. No capacity may
/// run out in `Large`. How often games stall otherwise depends on the cards dealt, so the
/// stall-rate target is held on the fixed decks (`mirror_games_rarely_stall`) and only reported
/// here, as are the games that left `Small`. The nightly's `just fuzz` plays it at length; no
/// check runs it.
#[test]
#[ignore = "40 random games, about 40 CPU seconds in a debug build: `just fuzz` plays it"]
fn random_games_keep_the_invariants() {
    let n = games(40);
    let mut stalls = Vec::new();
    let mut widened = Vec::new();
    let mut kinds: HashMap<String, u64> = HashMap::new();
    coverage::start();
    for seed in seeds(n) {
        let (t, s) = game(seed);
        let outcome = if seed % 4 == 3 {
            fuzz(&t, s.widen(), seed, &mut kinds)
        } else {
            let played = play_widening(&t, s, seed, &mut kinds, compares_layouts(seed));
            widened.extend(played.filled.map(|list| (seed, list)));
            played.outcome
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
        println!("{} games left Small: {widened:?}", widened.len());
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
        let (t, s) = game(seed);
        let fuzz = play_widening(&t, s, seed, &mut HashMap::new(), true);
        assert_eq!(fuzz.filled, None, "fuzz seed {seed}");

        let (t, s) = fixed_game(seed);
        let fixed = play_widening(&t, s, seed, &mut HashMap::new(), true);
        assert_eq!(fixed.filled, None, "fixed seed {seed}");
        assert!(fixed.decisions > 40, "fixed seed {seed}: a short game");
    }
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
    GameTable::new(
        Format::CONSTRUCTED,
        &CARDS,
        [decks[0].as_slice(), decks[1].as_slice()],
    )
    .unwrap()
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
                ..**def
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
    GameTable::new(
        Format::CONSTRUCTED,
        cards,
        [decks[0].as_slice(), decks[1].as_slice()],
    )
    .unwrap()
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
                let det =
                    Observation::new(s, d.decider).determinize(&t, &mut SplitMix64::new(seed));
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
            let c = agents[d.decider.index()].choose(
                Observation::new(&small, d.decider),
                &d,
                asked[d.decider.index()].take(),
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
            .place_token(Seat::P0, LargeCardIx::FIRST, FaceIx::FRONT, &[])
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
                let c = agents[d.decider.index()].choose(
                    Observation::new(&s, d.decider),
                    &d,
                    asked[d.decider.index()].take(),
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
                let viewer = d.decider;
                let mut det = redeal(s, viewer, &t, &mut rng);
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

/// Show and Tell's picks are hidden until both players have made one (CR 101.4a). At the second
/// player's question, a state redealt from what they know deals the first player's picked
/// slot a card the pick could have taken, never the instant each deck also holds, and the
/// second player sees and is asked the same there: the view of the paused resolution, which
/// replays the first pick, and the decision itself.
#[test]
fn a_hidden_pick_redealt_shows_and_asks_the_same() {
    let deck: &[(u8, &str)] = &[
        (12, names::SHOW_AND_TELL),
        (16, names::ISLAND),
        (6, names::GRISELBRAND),
        (6, names::COUNTERSPELL),
    ];
    let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [deck, deck]).unwrap();
    let mut rng = SplitMix64::new(7);
    let (mut reached, mut redealt) = (0, 0);
    for seed in 0..games(6) {
        let mut s = State::new(&t, seed, Profile::EXACT).unwrap();
        play(
            &t,
            &mut s,
            &mut Scratch::new(),
            &mut agents(seed),
            |s, d| {
                let Pending::Resolving(r) = s.pending() else {
                    return;
                };
                let picks: Vec<Slot> = r.hidden_picks().collect();
                let viewer = d.decider;
                if !r.asks_hidden() || picks.iter().all(|&slot| s.knows(viewer, slot)) {
                    return;
                }
                reached += 1;
                let mut x = Scratch::new();
                let seen = Observer.choose(observe(s, &t, &mut x, viewer), d, AgentDecision(0));
                for _ in 0..4 {
                    let mut det = redeal(s, viewer, &t, &mut rng);
                    det.check_invariants(&t).unwrap();
                    for &slot in &picks {
                        let card = det.card_behind(slot).expect("a card picked");
                        assert_ne!(t.def(card).name, names::COUNTERSPELL, "seed {seed}");
                        redealt += usize::from(det.card_behind(slot) != s.card_behind(slot));
                    }
                    let theirs =
                        Observer.choose(observe(&det, &t, &mut x, viewer), d, AgentDecision(0));
                    assert_eq!(seen, theirs, "seed {seed}: the paused view differs");
                    match next(&mut det, &t, &mut Scratch::new(), &mut Replay) {
                        Step::Decide(again) => assert_eq!(again, *d, "seed {seed}"),
                        Step::Over(o) => panic!("seed {seed}: the redealt game ended: {o:?}"),
                    }
                }
            },
        );
    }
    assert!(reached > 0, "no second player's hidden pick was asked");
    // The cards picked really were dealt anew: the test has something to catch.
    assert!(redealt > 0, "no picked card was redealt");
}

/// What a view shows of colors: for every slot, whether it is each color as the viewer may
/// ask ([`Observation::matches`]) and its text changes, with the identities and zones
/// [`Observer`] hashes.
fn colors_seen(t: &GameTable, obs: Observation<'_>) -> u64 {
    let mut h = DefaultHasher::new();
    for slot in Slot::all() {
        (
            obs.knows(slot),
            obs.card(slot),
            obs.object(slot),
            obs.words_of(slot),
        )
            .hash(&mut h);
        for color in Colors::EACH {
            obs.matches(t, &Filter::Colors(color), slot, &Words::NONE)
                .hash(&mut h);
        }
    }
    for p in Seat::BOTH {
        for zone in [Zone::Library, Zone::Hand, Zone::Graveyard, Zone::Exile] {
            obs.zone(ZoneKey::Of(p, zone)).hash(&mut h);
        }
    }
    h.finish()
}

/// A color that an effect gives cards in every zone, as Painter's Servant's chosen color,
/// betrays nothing hidden: redealt, the hidden cards are all that color too, and the view of
/// colors is the same. Each fuzz deck here ends in Painter's Servants.
#[test]
fn a_color_given_in_every_zone_shows_nothing_hidden() {
    let mut rng = SplitMix64::new(7);
    let mut reached = 0;
    for seed in 0..games(20) {
        let deck = |seed: u64| {
            let mut deck = fuzz_deck(seed);
            deck.truncate(deck.len() - 2);
            let cards: u8 = deck.iter().map(|&(count, _)| count).sum();
            deck.push((40 - cards, names::PAINTERS_SERVANT));
            deck
        };
        let decks = [deck(seed), deck(seed.wrapping_add(1 << 32))];
        let t = GameTable::new(
            Format::CONSTRUCTED,
            &CARDS,
            [decks[0].as_slice(), decks[1].as_slice()],
        )
        .unwrap();
        let mut s = State::new(&t, seed, Profile::EXACT).unwrap();
        play(
            &t,
            &mut s,
            &mut Scratch::new(),
            &mut agents(seed),
            |s, d| {
                if !s.reaches_elsewhere() {
                    return;
                }
                reached += 1;
                let det = Observation::new(s, d.decider).determinize(&t, &mut rng);
                assert_eq!(
                    colors_seen(&t, Observation::new(s, d.decider)),
                    colors_seen(&t, Observation::new(&det, d.decider)),
                    "seed {seed}"
                );
            },
        );
    }
    assert!(
        reached > 0,
        "no game had Painter's Servant on the battlefield"
    );
}

#[test]
fn determinization_keeps_the_size_of_every_hidden_group() {
    let (t, mut s) = game(3);
    play(&t, &mut s, &mut Scratch::new(), &mut agents(3), |s, d| {
        let det = Observation::new(s, d.decider).determinize(&t, &mut SplitMix64::new(1));
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
        // No two effects of one layer in the mirror read what the other writes, so the
        // layers never try one against another (CR 613.8a): the dependency search costs
        // these games nothing.
        assert_eq!(c.dependency_trials, 0, "trials in game ({greedy}, {seed})");
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
