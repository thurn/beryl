//! The generated names, the card table and the decklists.

use std::fmt::Write;
use std::path::PathBuf;

use beryl_engine::card::{AbilityKind, FaceIx, ManaColor, Subtype, Supertypes, Types};
use beryl_engine::rng::SplitMix64;
use beryl_engine::state::ObjectFlags;
use beryl_engine::{
    Choice, Decision, DecisionKind, GameTable, Observation, Opt, Outcome, Profile, Replay, Scratch,
    Seat, State, Step, TurnStep, Zone, ZoneKey, apply, next,
};

use crate::{CARDS, SCENARIOS, blue, decks, names};

mod effect_data;
mod knowledge;

/// This crate's directory, read at run time: a test binary from the shared target directory
/// may have been built in a worktree that no longer exists.
fn crate_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"))
}

/// The decklists whose cards have name constants, read at test time.
const DECKS: &str = "docs/sample-decks";
const BASIC_LANDS: [&str; 5] = ["Plains", "Island", "Swamp", "Mountain", "Forest"];

/// A card name as a Rust constant: "Jace, Vryn's Prodigy" is `JACE_VRYNS_PRODIGY`.
fn constant(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        let c = match c {
            'á' | 'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            c => c,
        };
        assert!(c.is_ascii(), "no ASCII spelling for {c:?} in {name:?}");
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_uppercase());
        } else if c != '\'' && !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').to_string();
    assert!(
        out.starts_with(|c: char| c.is_ascii_alphabetic()),
        "{name:?} gives no constant"
    );
    out
}

/// The card names of the decklists in `dir`, main deck and sideboard, each once, by file name
/// and then in each list's order.
fn deck_names(dir: &std::path::Path) -> Vec<String> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "txt"))
        .collect();
    paths.sort();
    let mut names = Vec::new();
    for path in paths {
        for line in std::fs::read_to_string(&path).unwrap().lines() {
            if let Some((count, name)) = line.split_once(' ')
                && count.parse::<u8>().is_ok()
                && !names.iter().any(|n| n == name.trim())
            {
                names.push(name.trim().to_string());
            }
        }
    }
    names
}

fn generate(decks: &[String]) -> String {
    let mut out = String::from(
        "//! Every card name as a constant, so a misspelled card is a compile error.\n\
         //!\n\
         //! Generated from the decklists in `docs/sample-decks` and the basic lands by the\n\
         //! `names_are_current` test. Regenerate with\n\
         //! `BERYL_UPDATE_NAMES=1 cargo test -p beryl_cards names_are_current`.\n\n",
    );
    let mut seen = std::collections::BTreeSet::new();
    let mut emit = |out: &mut String, name: &str| {
        let c = constant(name);
        assert!(seen.insert(c.clone()), "two names give {c}");
        writeln!(out, "pub const {c}: &str = {name:?};").unwrap();
    };
    let mut names: Vec<&str> = decks
        .iter()
        .map(String::as_str)
        .filter(|name| !BASIC_LANDS.contains(name))
        .collect();
    names.sort_unstable();
    for name in names {
        emit(&mut out, name);
    }
    out.push_str("\n// The basic lands.\n");
    for name in BASIC_LANDS {
        emit(&mut out, name);
    }
    out
}

#[test]
fn names_are_current() {
    let crate_dir = crate_dir();
    let decks = deck_names(&crate_dir.join("../../..").join(DECKS));
    let expected = generate(&decks);
    let path = crate_dir.join("src/names.rs");
    if std::env::var_os("BERYL_UPDATE_NAMES").is_some() {
        std::fs::write(&path, &expected).unwrap();
    }
    let actual = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        actual == expected,
        "src/names.rs is stale: run `BERYL_UPDATE_NAMES=1 cargo test -p beryl_cards names_are_current`"
    );
}

#[test]
fn constants_spell_names_plainly() {
    assert_eq!(constant("Jace, Vryn's Prodigy"), "JACE_VRYNS_PRODIGY");
    assert_eq!(constant("Life // Death"), "LIFE_DEATH");
    assert_eq!(constant("Troll of Khazad-dûm"), "TROLL_OF_KHAZAD_DUM");
    assert_eq!(
        names::JACE_WIELDER_OF_MYSTERIES,
        "Jace, Wielder of Mysteries"
    );
}

/// A decklist file's main deck: its `<count> <name>` lines up to the blank line before the
/// sideboard.
fn main_deck(path: &str) -> Vec<(u8, String)> {
    let text = std::fs::read_to_string(crate_dir().join(path)).unwrap();
    text.lines()
        .take_while(|line| !line.trim().is_empty())
        .map(|line| {
            let (count, name) = line.split_once(' ').expect("a <count> <name> line");
            (count.parse().unwrap(), name.trim().to_string())
        })
        .collect()
}

#[test]
fn the_decks_are_forty_cards_matching_their_decklist_files() {
    for (deck, path, names) in [
        (
            decks::WU_CONTROL,
            "../../../docs/sample-decks/2025-11-18-17lands-cb8d5d5f-wu-control.txt",
            30,
        ),
        (
            decks::VINTAGE_WU_CONTROL,
            "../../../docs/sample-decks/2026-10-17-PrinceJools-wu-control.txt",
            29,
        ),
    ] {
        assert_eq!(deck.len(), names, "{path}");
        let total: u32 = deck.iter().map(|&(n, _)| u32::from(n)).sum();
        assert_eq!(total, 40, "{path}");
        let mut ours: Vec<(u8, String)> = deck.iter().map(|&(n, c)| (n, c.to_string())).collect();
        let mut theirs = main_deck(path);
        ours.sort_unstable_by(|a, b| a.1.cmp(&b.1));
        theirs.sort_unstable_by(|a, b| a.1.cmp(&b.1));
        assert_eq!(ours, theirs, "{path}");
    }
}

/// The UI draws a card, token or emblem from `data/cards.json` by the name the engine gives
/// it, a face's name (`ui/src/cards.ts`): each must be there.
#[test]
fn every_face_token_and_emblem_name_is_in_the_card_data() {
    let path = crate_dir().join("../../../data/cards.json");
    let data: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut known = std::collections::BTreeSet::new();
    for entry in ["cards", "tokens"]
        .iter()
        .flat_map(|k| data[k].as_array().unwrap())
    {
        known.insert(entry["name"].as_str().unwrap());
        for face in entry["faces"].as_array().unwrap() {
            known.insert(face["name"].as_str().unwrap());
        }
    }
    let mut defs: Vec<_> = CARDS.cards().to_vec();
    let mut i = 0;
    while i < defs.len() {
        for &token in defs[i].tokens {
            if !defs.iter().any(|d| std::ptr::eq(*d, token)) {
                defs.push(token);
            }
        }
        i += 1;
    }
    let mut missing: Vec<&str> = defs
        .iter()
        .flat_map(|d| d.face_ixs().map(|f| d.face_name(f)))
        .filter(|name| !known.contains(name))
        .collect();
    missing.dedup();
    assert!(
        missing.is_empty(),
        "not in data/cards.json (run `just cards`): {missing:?}"
    );
}

#[test]
fn every_card_file_is_in_the_table_under_its_own_name() {
    let mut names: Vec<_> = CARDS.cards().iter().map(|c| c.name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), CARDS.cards().len());
    for card in CARDS.cards() {
        assert!(std::ptr::eq(CARDS.get(card.name).unwrap(), *card));
    }
}

#[test]
fn tundra_is_a_plains_island_with_both_mana_abilities() {
    let tundra = CARDS.get(names::TUNDRA).unwrap().front();
    assert_eq!(tundra.types, Types::LAND);
    assert!(!tundra.supertypes.contains(Supertypes::BASIC));
    assert!(tundra.subtypes.contains(Subtype::Plains));
    assert!(tundra.subtypes.contains(Subtype::Island));
    let produced: Vec<_> = tundra
        .abilities
        .iter()
        .map(|a| {
            assert_eq!(a.kind, AbilityKind::Mana);
            assert!(a.cost.tap);
            (
                a.produces.includes(ManaColor::White),
                a.produces.includes(ManaColor::Blue),
            )
        })
        .collect();
    assert_eq!(produced, [(true, false), (false, true)]);
    for (name, color) in [
        (names::PLAINS, ManaColor::White),
        (names::ISLAND, ManaColor::Blue),
    ] {
        let basic = CARDS.get(name).unwrap().front();
        assert!(basic.supertypes.contains(Supertypes::BASIC));
        assert_eq!(basic.abilities.len(), 1);
        assert!(basic.abilities[0].produces.includes(color));
    }
}

#[test]
fn a_game_of_the_three_lands_runs_to_decking() {
    let deck = &[
        (14, names::PLAINS),
        (13, names::ISLAND),
        (13, names::TUNDRA),
    ];
    let t = GameTable::new(&CARDS, [deck, deck]).unwrap();
    let mut s = State::new(&t, 42, Profile::SMART);
    let mut x = Scratch::new();
    let choose = |d: &Decision| match d.kind {
        DecisionKind::Priority => Choice((d.len() - 1) as u16),
        _ => Choice(0),
    };
    let outcome = loop {
        match next(&mut s, &t, &mut x, &mut Replay) {
            Step::Over(outcome) => break outcome,
            Step::Decide(d) => apply(&mut s, &t, &mut x, &mut Replay, choose(&d)),
        }
    };
    assert_eq!(outcome, Outcome::Win(s.turn().starting_player));
    assert_eq!(s.battlefield().len(), 67);
}

/// Every card's scenarios pass, under each numbering of the decks' slots, and play out the
/// same under each: slot numbers are random handles the rules must never read (docs/bugs.md
/// has several bugs of that class). Debug builds check the engine's invariants at every
/// position, so a scenario that breaks one fails here too.
#[test]
fn every_card_has_scenarios_and_they_pass() {
    use beryl_scenario::Slots;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let mut cards: Vec<_> = SCENARIOS.iter().map(|&(name, _)| name).collect();
    cards.sort_unstable();
    let mut table: Vec<_> = CARDS.cards().iter().map(|c| c.name).collect();
    table.sort_unstable();
    assert_eq!(cards, table, "one SCENARIOS entry per card");
    let mut failures = Vec::new();
    for &(card, scenarios) in SCENARIOS {
        if scenarios.is_empty() {
            failures.push(format!("{card} has no scenario"));
        }
        for scenario in scenarios {
            let mut dealt = None;
            for slots in Slots::ALL {
                // An invariant violation panics; report it with the rest.
                let run = catch_unwind(AssertUnwindSafe(|| {
                    beryl_scenario::trace_with(&CARDS, scenario, slots)
                }));
                let positions = match run {
                    Ok(Ok(positions)) => format!("{positions:?}"),
                    Ok(Err(e)) => {
                        failures.push(format!("{card} ({slots:?} slots): {e}"));
                        break;
                    }
                    Err(_) => {
                        let name = scenario.name;
                        failures.push(format!("{card} ({slots:?} slots): {name} panicked"));
                        break;
                    }
                };
                match &dealt {
                    None => dealt = Some(positions),
                    Some(d) if *d != positions => failures.push(format!(
                        "{card} ({slots:?} slots): {} plays out otherwise than with the \
                         dealt slots",
                        scenario.name
                    )),
                    Some(_) => {}
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The bar for a supported card (architecture §17.3): "every one of its abilities is exercised
/// by at least one scenario", and so is every way to cast it. Any card's scenarios may
/// exercise another's: the basic lands' mana abilities pay for everything.
#[test]
fn every_ability_and_casting_option_is_exercised_by_a_scenario() {
    use beryl_engine::coverage;

    coverage::start();
    for &(_, scenarios) in SCENARIOS {
        for scenario in scenarios {
            // `every_card_has_scenarios_and_they_pass` reports failures.
            let _ = beryl_scenario::run(&CARDS, scenario);
        }
    }
    let hits = coverage::take();
    // What no scenario can exercise yet, and why. An entry a scenario exercises fails too, so
    // the list stays current.
    let unreachable = [(
        "Spara's Headquarters: ability 0 (Mana)",
        "no implemented card asks for {G}, and automatic payment spends a unit that may be \
         several kinds as the lowest of them, {W}",
    )];
    let mut missed = Vec::new();
    let mut reached = Vec::new();
    for (what, address) in crate::parts() {
        let known = unreachable.iter().any(|&(u, _)| u == what);
        match (hits.contains(&address), known) {
            (false, false) => missed.push(what),
            (true, true) => reached.push(what),
            _ => {}
        }
    }
    assert!(
        missed.is_empty(),
        "no scenario exercises:\n{}",
        missed.join("\n")
    );
    assert!(
        reached.is_empty(),
        "a scenario now exercises, so drop it from `unreachable`:\n{}",
        reached.join("\n")
    );
}

#[test]
fn the_dump_is_valid_json_with_every_scenario() {
    let json: serde_json::Value =
        serde_json::from_str(&beryl_scenario::to_json(&CARDS, SCENARIOS)).unwrap();
    let total: usize = SCENARIOS.iter().map(|(_, s)| s.len()).sum();
    assert_eq!(json.as_array().unwrap().len(), total);
    assert_eq!(json[0]["card"], names::PLAINS);
}

/// A paused body's looks stay known (architecture §13): while Elite Spellbinder asks whether
/// to exile a card, its controller knows the hand it looked at, so a state determinized from
/// what they know replays to the same choice of cards.
#[test]
fn a_question_after_a_look_keeps_the_look() {
    let p0 = &[(1, names::ELITE_SPELLBINDER), (3, names::PLAINS)];
    let p1 = &[
        (1, names::THIEVING_SKYDIVER),
        (1, names::SPHINX_OF_FALSE_CONCLUSIONS),
        (1, names::ISLAND),
    ];
    let t = GameTable::new(&CARDS, [p0, p1]).unwrap();
    let mut s = State::new(&t, 1, Profile::EXACT);
    s.board().begin_at(1, Seat::P0, TurnStep::PrecombatMain);
    for player in Seat::BOTH {
        for slot in s.library(player).to_vec() {
            let land = s.copiable(slot).face(&t).types.contains(Types::LAND);
            let zone = if player == Seat::P0 && land {
                Zone::Battlefield
            } else {
                Zone::Hand
            };
            s.board().place(slot, zone).unwrap();
        }
    }
    let mut x = Scratch::new();
    let question = loop {
        match next(&mut s, &t, &mut x, &mut Replay) {
            Step::Decide(d) if d.kind == DecisionKind::YesNo => break d,
            Step::Decide(d) => {
                let cast = (0..d.len() as u16)
                    .map(Choice)
                    .find(|&c| matches!(d.candidates.option(c), Some(Opt::Cast { .. })));
                apply(&mut s, &t, &mut x, &mut Replay, cast.unwrap_or(Choice(0)));
            }
            Step::Over(o) => panic!("the game ended: {o:?}"),
        }
    };
    assert_eq!(question.player, Seat::P0);
    for slot in s.zone(ZoneKey::Of(Seat::P1, Zone::Hand)).iter() {
        assert!(
            s.knows(Seat::P0, slot),
            "player 0 forgot the card in slot {slot}"
        );
    }
    let choices = |s: &mut State| {
        let mut x = Scratch::new();
        apply(s, &t, &mut x, &mut Replay, Choice(0));
        match next(s, &t, &mut x, &mut Replay) {
            Step::Decide(d) => d,
            Step::Over(o) => panic!("the game ended: {o:?}"),
        }
    };
    let mut det = Observation::new(&s, Seat::P0).determinize(&t, &mut SplitMix64::new(9));
    let real = choices(&mut s);
    assert_eq!(real.kind, DecisionKind::ChooseObject);
    assert_eq!(choices(&mut det), real);
}

/// Trinket Mage's search shows its controller the whole library
/// ([CR 701.23a](https://yawgatog.com/resources/magic-rules/#R70123a)), and the shuffle after
/// it, though nothing is found, leaves none of it known
/// ([CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a)): what the oracles,
/// shuffling on their own, cannot check.
#[test]
fn a_search_that_finds_nothing_still_shuffles() {
    use beryl_scenario::{Action, P0, P1, Permanent, Pick, Scenario};
    const ISLANDS: &[Permanent] = &[Permanent::new(names::ISLAND); 3];
    const SEARCH: Scenario = Scenario::new("Trinket Mage finds nothing")
        .hand(P0, &[names::TRINKET_MAGE])
        .battlefield(P0, ISLANDS)
        .library(P0, &[names::MANA_CRYPT, names::OUST, names::PLAINS])
        .library(P1, &[names::PLAINS])
        .actions(&[
            Action::cast(P0, names::TRINKET_MAGE),
            Action::Resolve,
            Action::Resolve,
            Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
            Action::answer(P0, DecisionKind::ChooseUpTo, Pick::Done),
            // Player 0 has nothing more to do: the next decision is player 1's turn.
            Action::pass(P1),
        ]);
    let mut known = Vec::new();
    beryl_scenario::visit(&CARDS, &SEARCH, |_, s, d| {
        let library = s.zone(ZoneKey::Of(Seat::P0, Zone::Library));
        let count = library
            .iter()
            .filter(|&slot| s.knows(Seat::P0, slot))
            .count();
        known.push((d.kind, count));
    })
    .unwrap();
    let searching = known
        .iter()
        .find(|&&(kind, _)| kind == DecisionKind::ChooseUpTo);
    assert_eq!(searching, Some(&(DecisionKind::ChooseUpTo, 3)));
    assert_eq!(known.last(), Some(&(DecisionKind::Priority, 0)));
}

/// A discard to hand size in the cleanup step triggers Currency Converter, so the active
/// player gets priority there and, once the trigger has resolved, another cleanup step begins
/// ([CR 514.3a](https://yawgatog.com/resources/magic-rules/#R5143a)): "If so, those
/// state-based actions are performed, then those triggered abilities are put on the stack,
/// then the player whose turn it is receives priority. ... Once the stack is empty and all
/// players pass in succession, another cleanup step begins." Not an oracle scenario: the Forge
/// harness cannot script cleanup discards.
#[test]
fn a_cleanup_discard_triggers_currency_converter() {
    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};
    const HAND: &[&str] = &[names::ISLAND; 8];
    const DISCARD: Scenario = Scenario::new("Currency Converter exiles a cleanup discard")
        .at(3, P0, TurnStep::End)
        .battlefield(P0, &[Permanent::new(names::CURRENCY_CONVERTER)])
        .hand(P0, HAND)
        .library(P0, &[names::PLAINS])
        .library(P1, &[names::PLAINS, names::PLAINS])
        .actions(&[
            Action::pass(P0),
            // Eight Islands: discarding one is no question.
            Action::assert(Assertion::Stack { count: 1 }),
            Action::Resolve,
            Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
            Action::assert(Assertion::Count {
                player: P0,
                zone: Zone::Exile,
                card: Some(names::ISLAND),
                count: 1,
            }),
            Action::pass(P0),
            // Player 1's turn: the second cleanup step had nothing to do.
            Action::pass(P0),
        ]);
    let mut seen = Vec::new();
    beryl_scenario::visit(&CARDS, &DISCARD, |_, s, d| {
        seen.push((s.turn().number, s.turn().step, d.player, d.kind));
    })
    .unwrap();
    let cleanup = |player, kind| (3, TurnStep::Cleanup, player, kind);
    assert_eq!(
        seen,
        [
            (3, TurnStep::End, Seat::P0, DecisionKind::Priority),
            // The trigger on the stack, in the cleanup step.
            cleanup(Seat::P0, DecisionKind::Priority),
            cleanup(Seat::P0, DecisionKind::YesNo),
            cleanup(Seat::P0, DecisionKind::Priority),
            // The next cleanup step ends the turn.
            (4, TurnStep::Upkeep, Seat::P0, DecisionKind::Priority),
        ]
    );
}

/// A manifested card is known to its controller alone while it is face down (CR 708.5), and
/// to both players once it leaves the battlefield
/// ([CR 708.9](https://yawgatog.com/resources/magic-rules/#R7089)): "If a face-down permanent
/// ... moves from the battlefield to any other zone, its owner must reveal it to all players as
/// they move it." Here it goes to a library, a hidden zone, where only that reveal tells the
/// other player. A state determinized for them meanwhile keeps the permanent face down,
/// whatever card it deals there.
#[test]
fn a_face_down_card_is_hidden_until_it_leaves() {
    let scenario = blue::abhorrent_oculus::SCENARIOS
        .iter()
        .find(|s| s.name == "A face-down permanent leaving the battlefield is revealed")
        .expect("the scenario");
    let mut snapcaster = None;
    let mut known = Vec::new();
    beryl_scenario::visit(&CARDS, scenario, |t, s, d| {
        let face_down = s
            .battlefield()
            .iter()
            .copied()
            .find(|&slot| s.object(slot).face == FaceIx::DOWN);
        if let Some(slot) = face_down {
            snapcaster = Some(slot);
            let det = Observation::new(s, Seat::P1).determinize(t, &mut SplitMix64::new(3));
            assert_eq!(det.object(slot).face, FaceIx::DOWN);
            det.check_invariants(t).unwrap();
        }
        if let Some(slot) = snapcaster {
            known.push((d.kind, s.knows(Seat::P0, slot), s.knows(Seat::P1, slot)));
        }
    })
    .unwrap();
    let face_down = &known[..known.len() - 1];
    assert!(!face_down.is_empty());
    assert!(face_down.iter().all(|&(_, p0, p1)| p0 && !p1), "{known:?}");
    assert_eq!(known.last(), Some(&(DecisionKind::Priority, true, true)));
}

/// A token copying a face-down permanent is a face-up 2/2 with no text and no name: face down
/// is a status, which isn't copied (CR 707.2), and the face-down values are what is
/// ([CR 708.2](https://yawgatog.com/resources/magic-rules/#R7082)). It is a token of no card in
/// either deck, so neither player learns the card behind the original from it, and it can't be
/// turned face up (CR 701.40b).
#[test]
fn a_copy_of_a_face_down_permanent_hides_its_card() {
    let scenario = blue::abhorrent_oculus::SCENARIOS
        .iter()
        .find(|s| s.name == "Fractured Identity copies a face-down permanent as a 2/2")
        .expect("the scenario");
    let mut last = None;
    beryl_scenario::visit(&CARDS, scenario, |t, s, d| {
        last = Some((t.clone(), *s, *d));
    })
    .unwrap();
    let (t, s, d) = last.expect("a decision");
    let copy = s
        .battlefield()
        .iter()
        .copied()
        .find(|&slot| s.object(slot).flags.has(ObjectFlags::TOKEN))
        .expect("the copy");
    assert_eq!(s.controller(copy), Seat::P1);
    assert_eq!(s.object(copy).face, FaceIx::FRONT);
    assert_eq!(s.copiable(copy).record(&t).0, t.face_down_copy());
    assert_eq!(s.card_behind(copy), None);
    assert!(s.knows(Seat::P0, copy) && s.knows(Seat::P1, copy));
    let derived = s.derived(copy);
    assert_eq!((derived.power, derived.toughness), (2, 2));
    assert!(
        !(0..d.len() as u16)
            .any(|i| matches!(d.candidates.option(Choice(i)), Some(Opt::TurnFaceUp(_)))),
        "{d:?}"
    );
}

/// Turning a face-down permanent face up (CR 116.2b) asks how to pay its mana cost when the
/// payments differ in what they leave floating, and turns it face up once paid: two Islands,
/// or an Island and Mana Crypt with {C} left over. Not an oracle scenario: the exporters script
/// no payment plans.
#[test]
fn turning_face_up_asks_for_a_payment_plan() {
    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};
    const PLAN: Scenario = Scenario::new("A face-down Snapcaster Mage turns up with Mana Crypt")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(names::SNAPCASTER_MAGE).face_down(),
                Permanent::new(names::ISLAND),
                Permanent::new(names::ISLAND),
                Permanent::new(names::MANA_CRYPT),
            ],
        )
        .library(P0, &[names::PLAINS])
        .library(P1, &[names::PLAINS])
        .actions(&[
            Action::TurnFaceUp {
                player: P0,
                card: names::SNAPCASTER_MAGE,
            },
            Action::assert(Assertion::Decision {
                player: P0,
                kind: DecisionKind::PaymentPlan,
                candidates: 2,
            }),
            Action::answer(P0, DecisionKind::PaymentPlan, Pick::Plan { i: 1 }),
            Action::assert(Assertion::FaceDown {
                player: P0,
                card: names::SNAPCASTER_MAGE,
                count: 0,
            }),
            Action::assert(Assertion::Tapped {
                player: P0,
                card: names::MANA_CRYPT,
                count: 1,
            }),
            Action::assert(Assertion::Tapped {
                player: P0,
                card: names::ISLAND,
                count: 1,
            }),
            Action::assert(Assertion::Pt {
                player: P0,
                card: names::SNAPCASTER_MAGE,
                token: false,
                power: 2,
                toughness: 1,
            }),
        ]);
    beryl_scenario::run(&CARDS, &PLAN).unwrap_or_else(|e| panic!("{e}"));
}

/// The slot of the permanent named `name`.
fn permanent_named(t: &GameTable, s: &State, name: &str) -> beryl_engine::Slot {
    s.battlefield()
        .iter()
        .copied()
        .find(|&slot| s.definition_name(t, slot) == name)
        .unwrap()
}

/// Currency Converter's "{T}: Put a card exiled with this artifact into its owner's
/// graveyard" does nothing until a card is exiled with it (CR 607.2a), and then does.
#[test]
fn currency_converters_return_is_pointless_until_it_exiles_a_card() {
    use crate::colorless::currency_converter::SCENARIOS as CONVERTER;
    use beryl_engine::card::ObjectAbility;
    let scenario = CONVERTER
        .iter()
        .find(|s| s.name == "Currency Converter chooses among its exiled cards")
        .unwrap();
    let mut seen = Vec::new();
    beryl_scenario::visit(&CARDS, scenario, |t, s, d| {
        if d.kind == DecisionKind::Priority && d.player == Seat::P0 {
            let converter = permanent_named(t, s, names::CURRENCY_CONVERTER);
            seen.push((
                beryl_engine::pointless(s, t, converter, ObjectAbility::own(2)),
                s.exiled_with(converter).count(),
            ));
        }
    })
    .unwrap();
    assert_eq!(seen.first(), Some(&(true, 0)));
    assert_eq!(seen.last(), Some(&(false, 2)));
    // Its loot is never pointless.
    assert!(
        seen.iter()
            .all(|&(pointless, exiled)| pointless == (exiled == 0))
    );
}

/// A profile that hides activations that would do nothing from a player leaves this one out
/// for them; every other profile offers it, as the rules do (CR 602.2).
#[test]
fn a_hidden_pointless_activation_is_left_out() {
    use beryl_engine::card::ObjectAbility;
    use beryl_scenario::{P0, Permanent, Scenario};
    const SETUP: Scenario = Scenario::new("Currency Converter with nothing exiled")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &[Permanent::new(names::CURRENCY_CONVERTER)])
        .hand(P0, &[names::ISLAND])
        .library(P0, &[names::PLAINS])
        .library(beryl_scenario::P1, &[names::PLAINS]);
    let offers_return = |profile: Profile| {
        let (t, mut s) = beryl_scenario::position(&CARDS, &SETUP).unwrap();
        s.set_profile(profile);
        let converter = permanent_named(&t, &s, names::CURRENCY_CONVERTER);
        let Step::Decide(d) = next(&mut s, &t, &mut Scratch::new(), &mut Replay) else {
            panic!("a decision");
        };
        assert_eq!(d.kind, DecisionKind::Priority);
        (0..d.len() as u16).any(|i| {
            d.candidates.option(Choice(i))
                == Some(Opt::Activate {
                    slot: converter,
                    ability: ObjectAbility::own(2),
                })
        })
    };
    assert!(!offers_return(Profile::SMART));
    assert!(offers_return(Profile::EXACT));
    assert!(offers_return(Profile {
        hides_pointless: [false, true],
        ..Profile::SMART
    }));
    assert!(!offers_return(Profile {
        hides_pointless: [true, false],
        ..Profile::SMART
    }));
}

/// A player offered pointless activations quietly may still tap Currency Converter with nothing
/// exiled, but it makes no stop of its own: with nothing else to do, their turn plays on to
/// their next one, where a drawn land stops them. Once a card is exiled with it, it does stop
/// them (CR 602.2, CR 117.3d).
#[test]
fn a_quiet_pointless_activation_makes_no_stop() {
    use beryl_engine::card::ObjectAbility;
    use beryl_scenario::{P0, P1, Permanent, Scenario};
    const SETUP: Scenario = Scenario::new("Currency Converter alone")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &[Permanent::new(names::CURRENCY_CONVERTER)])
        .library(P0, &[names::PLAINS, names::PLAINS])
        .library(P1, &[names::PLAINS, names::PLAINS]);
    // The turn and step of P0's first priority decision, and whether it offered the {T}.
    let first_stop = |profile: Profile| {
        let (t, mut s) = beryl_scenario::position(&CARDS, &SETUP).unwrap();
        s.set_profile(profile);
        let converter = permanent_named(&t, &s, names::CURRENCY_CONVERTER);
        let mut x = Scratch::new();
        loop {
            let Step::Decide(d) = next(&mut s, &t, &mut x, &mut Replay) else {
                panic!("a decision");
            };
            if d.player == Seat::P0 && d.kind == DecisionKind::Priority {
                let offered = (0..d.len() as u16).any(|i| {
                    d.candidates.option(Choice(i))
                        == Some(Opt::Activate {
                            slot: converter,
                            ability: ObjectAbility::own(2),
                        })
                });
                return (s.turn().number, s.turn().step, offered);
            }
            apply(&mut s, &t, &mut x, &mut Replay, Choice(0));
        }
    };
    let offered = Profile {
        hides_pointless: [false, true],
        ..Profile::SMART
    };
    assert_eq!(
        first_stop(offered),
        (3, TurnStep::PrecombatMain, true),
        "offered loudly, it stops"
    );
    assert_eq!(
        first_stop(Profile {
            quiet_pointless: Some(Seat::P0),
            ..offered
        }),
        (5, TurnStep::PrecombatMain, true),
        "offered quietly, the turn plays on to a drawn land"
    );
}

/// A token sacrificed for mana is still in the graveyard during casting, but
/// [CR 111.6](https://yawgatog.com/resources/magic-rules/#R1116) says "A token isn't a card".
/// This transient-state fixture stays in Rust: the oracle exporters cannot seed its token.
#[test]
fn sacrificed_mana_token_cannot_pay_a_graveyard_card_cost() {
    use beryl_scenario::{Action, Assertion, P0, Permanent, Scenario};
    const SCENARIO: Scenario =
        Scenario::new("Abhorrent Oculus pays with Treasure without exiling the token")
            .battlefield(
                P0,
                &[
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                    Permanent::token(crate::tokens::TREASURE_TOKEN),
                    Permanent::new(names::CURRENCY_CONVERTER).tapped(),
                ],
            )
            .hand(P0, &[names::ABHORRENT_OCULUS])
            .graveyard(P0, &[names::ISLAND; 6])
            .actions(&[
                Action::cast(P0, names::ABHORRENT_OCULUS),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Exile,
                    card: None,
                    count: 6,
                }),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Exile,
                    card: Some(crate::tokens::TREASURE_TOKEN),
                    count: 0,
                }),
                Action::Resolve,
                Action::assert(Assertion::Pt {
                    player: P0,
                    card: names::ABHORRENT_OCULUS,
                    token: false,
                    power: 5,
                    toughness: 5,
                }),
            ]);
    beryl_scenario::run(&CARDS, &SCENARIO).unwrap();
}

/// Every engine payment mode completes mana before an additional card cost (CR 601.2h).
#[test]
fn each_payment_mode_finishes_mana_before_exile_choices() {
    use beryl_engine::Payment;
    use beryl_engine::event::Event;
    use beryl_scenario::{P0, Permanent, Scenario};
    const SETUP: Scenario = Scenario::new("payment modes before exile")
        .battlefield(P0, &[Permanent::new(names::ISLAND); 3])
        .hand(P0, &[names::ABHORRENT_OCULUS])
        .graveyard(
            P0,
            &[
                names::ISLAND,
                names::PLAINS,
                names::OUST,
                names::REPRIEVE,
                names::COUNTERSPELL,
                names::STOCK_UP,
                names::THIRST_FOR_DISCOVERY,
            ],
        );
    for payment in [Payment::KeepMostPayable, Payment::Greedy, Payment::Manual] {
        let (t, mut s) = beryl_scenario::position(&CARDS, &SETUP).unwrap();
        s.set_profile(Profile {
            payment,
            stops: beryl_engine::Stops::Forced,
            full_control: Some(P0),
            ..Profile::EXACT
        });
        let mut x = Scratch::recording();
        let Step::Decide(d) = next(&mut s, &t, &mut x, &mut Replay) else {
            panic!("cast offered")
        };
        let cast = (0..d.len())
            .map(|i| Choice(u16::try_from(i).unwrap()))
            .find(|&c| matches!(d.candidates.option(c), Some(Opt::Cast { .. })))
            .unwrap();
        apply(&mut s, &t, &mut x, &mut Replay, cast);
        let mut manual = 0;
        loop {
            let Step::Decide(d) = next(&mut s, &t, &mut x, &mut Replay) else {
                panic!("cost choice")
            };
            if d.kind == DecisionKind::ExileCost {
                break;
            }
            assert_eq!(d.kind, DecisionKind::ManaPayment);
            manual += 1;
            apply(&mut s, &t, &mut x, &mut Replay, Choice(0));
        }
        assert_eq!(manual, if payment == Payment::Manual { 3 } else { 0 });
        assert_eq!(
            s.battlefield()
                .iter()
                .filter(|&&slot| s.object(slot).flags.has(ObjectFlags::TAPPED))
                .count(),
            3
        );
        assert_eq!(s.graveyard(P0).len(), 7);
        x.events.drain().for_each(drop);
        loop {
            apply(&mut s, &t, &mut x, &mut Replay, Choice(0));
            let Step::Decide(after) = next(&mut s, &t, &mut x, &mut Replay) else {
                panic!("cast finishes")
            };
            assert!(
                !x.events
                    .drain()
                    .any(|e| matches!(e, Event::Mana { .. } | Event::Tap { .. })),
                "cost completion never pays twice"
            );
            if after.kind != DecisionKind::ExileCost {
                break;
            }
        }
        assert_eq!(s.graveyard(P0).len(), 1);
        assert_eq!(s.stack().len(), 1);
    }
}
