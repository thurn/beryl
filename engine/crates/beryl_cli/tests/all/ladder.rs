use beryl_cli::match_log::files::{self};
use beryl_cli::match_log::{GameRecord, Verdict as GameVerdict};
use beryl_cli::tournament::game_seed;
use beryl_search::Settings;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::common::TEST;
use beryl_cli::ladder::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("beryl-ladder-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

/// A pair's two games share their seed and swap the seats; every game leaves its log and
/// transcript, and `match.json` the result.
#[test]
fn a_pair_shares_its_seed_and_swaps_seats() {
    let out = scratch("pairs");
    let m = Match {
        challenger: "greedy".to_owned(),
        champion: "random".to_owned(),
        work: PROXY,
        bar: Bar::Stronger,
        cap: 6,
        seed: 11,
        out: out.clone(),
        positions: Vec::new(),
    };
    let r = run(&TEST, &m, |_, _| {}).unwrap();
    assert_eq!(
        (r.games, r.pairs.total(), r.verdict),
        (6, 3, Verdict::Undecided)
    );
    assert!(r.wins >= 4, "{r:?}");
    let games: Vec<GameRecord> = files::read(&out.join("games.jsonl")).unwrap();
    assert_eq!(games.len(), 6);
    for pair in 0..3 {
        let mut both: Vec<&GameRecord> = games.iter().filter(|g| g.pair == pair).collect();
        both.sort_by_key(|g| g.seat);
        assert_eq!(both.len(), 2);
        assert_eq!(both[0].seed, both[1].seed);
        assert_eq!(both[0].seed, game_seed(11, 0, pair));
        assert_eq!((both[0].seat, both[1].seat), (0, 1));
        for g in both {
            assert!(out.join("logs").join(format!("{}.jsonl", g.game)).exists());
            assert!(
                out.join("transcripts")
                    .join(format!("{}.txt", g.game))
                    .exists()
            );
        }
    }
    assert!(out.join("match.json").exists());
    // The same seed plays the same games.
    let again = run(
        &TEST,
        &Match {
            out: scratch("again"),
            ..m.clone()
        },
        |_, _| {},
    )
    .unwrap();
    assert_eq!(
        (again.wins, again.losses, again.pairs),
        (r.wins, r.losses, r.pairs)
    );
    // Run again, the finished match plays no game and reaches the same result.
    let finished = games_in(&out);
    let rerun = run(&TEST, &m, |_, _| {}).unwrap();
    assert_eq!((rerun.wins, rerun.pairs), (r.wins, r.pairs));
    assert_eq!(games_in(&out), finished);
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&again.out);
}

/// With positions, four pairs in five start from one, both games from the same, the set
/// cycled through; the fifth starts from the shuffle. The games replay from their records.
#[test]
fn positions_start_four_pairs_in_five() {
    let set = beryl_cli::positions::build(
        &TEST,
        &beryl_cli::positions::Build {
            games: 1,
            seed: 2,
            settings: Settings {
                family: "pimc2/hidden/rollout/tuned".parse().unwrap(),
                work: 2_000,
                threads: 1,
            },
            band: [0.0, 1.0],
        },
        |_| {},
    )
    .unwrap()
    .positions;
    assert!(set.len() >= 2, "{}", set.len());
    let out = scratch("positions");
    let m = Match {
        challenger: "greedy".to_owned(),
        champion: "random".to_owned(),
        work: PROXY,
        bar: Bar::Stronger,
        cap: 12,
        seed: 4,
        out: out.clone(),
        positions: set.clone(),
    };
    let names: Vec<Option<&str>> = (0..6)
        .map(|p| m.position(p).map(|(n, _)| n.as_str()))
        .collect();
    let name = |k: usize| Some(set[k % set.len()].0.as_str());
    assert_eq!(names, [name(0), name(1), name(2), name(3), None, name(4)]);
    let r = run(&TEST, &m, |_, _| {}).unwrap();
    assert_eq!((r.from_positions.total(), r.from_shuffle.total()), (5, 1));
    assert!(r.text().contains("from positions 5"));
    let games: Vec<GameRecord> = files::read(&out.join("games.jsonl")).unwrap();
    for g in &games {
        let expected = m.position(g.pair).map(|(n, _)| n.clone());
        assert_eq!(g.position, expected, "{}", g.game);
    }
    let mut visited = 0;
    let walked =
        beryl_cli::anchor_replay::walk(&TEST, &out, "greedy", |_, _, _, _, _, _| visited += 1);
    assert_eq!(walked.unwrap(), 12);
    let _ = fs::remove_dir_all(&out);
}

/// A match of `challenger` against `champion` to `cap` games in `out`.
fn small(challenger: &str, champion: &str, cap: u32, out: PathBuf) -> Match {
    Match {
        challenger: challenger.to_owned(),
        champion: champion.to_owned(),
        work: PROXY,
        bar: Bar::Stronger,
        cap,
        seed: 5,
        out,
        positions: Vec::new(),
    }
}

/// The two matchups a continued match is tested with: fixed agents, and a search whose
/// trees carry from one decision to the next.
const MATCHUPS: [(&str, &str); 2] = [
    ("greedy", "greedy+hold"),
    (
        "pimc18/hidden/rollout/tuned@3k",
        "pimc18/hidden/rollout/tuned@3k",
    ),
];

/// The lines of `games.jsonl` in `out`, as a set: games finish in any order.
fn games_in(out: &Path) -> BTreeSet<String> {
    fs::read_to_string(out.join("games.jsonl"))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// `match.json` in `out`, without what names the directory or the day.
fn result_in(out: &Path) -> serde_json::Value {
    let text = fs::read_to_string(out.join("match.json")).unwrap();
    let mut json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let fields = json.as_object_mut().unwrap();
    fields.remove("out");
    fields.remove("date");
    json
}

/// A match played to cap 4 and run again at cap 8 is the match played to cap 8 at once.
#[test]
fn a_match_extended_to_a_higher_cap_equals_one_played_at_once() {
    for (i, (challenger, champion)) in MATCHUPS.into_iter().enumerate() {
        let whole = small(challenger, champion, 8, scratch(&format!("whole{i}")));
        run(&TEST, &whole, |_, _| {}).unwrap();
        let part = small(challenger, champion, 4, scratch(&format!("part{i}")));
        let first = run(&TEST, &part, |_, _| {}).unwrap();
        assert_eq!(first.games, 4, "{challenger}");
        run(
            &TEST,
            &Match {
                cap: 8,
                ..part.clone()
            },
            |_, _| {},
        )
        .unwrap();
        assert_eq!(games_in(&part.out), games_in(&whole.out), "{challenger}");
        assert_eq!(result_in(&part.out), result_in(&whole.out), "{challenger}");
        let _ = fs::remove_dir_all(&whole.out);
        let _ = fs::remove_dir_all(&part.out);
    }
}

/// The lines of `file` in `out`, sorted.
fn lines_in(out: &Path, file: &str) -> Vec<String> {
    let mut lines: Vec<String> = fs::read_to_string(out.join(file))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect();
    lines.sort();
    lines
}

/// Drops the lines of game `game` from `file` in `out`.
fn drop_game(out: &Path, file: &str, game: &str) {
    let path = out.join(file);
    let mut kept = String::new();
    for line in fs::read_to_string(&path).unwrap().lines() {
        if !line.contains(&format!("\"game\":\"{game}\"")) {
            kept += line;
            kept.push('\n');
        }
    }
    fs::write(&path, kept).unwrap();
}

/// A game cut off halfway, its log holding the first half of its decisions and
/// `games.jsonl` no line for it, continues from its log to the record it first reached. A
/// game stopped after its decisions were written but before its record replays to the same
/// record without writing its decisions twice.
#[test]
fn a_half_logged_game_finishes_to_the_same_record() {
    for (i, (challenger, champion)) in MATCHUPS.into_iter().enumerate() {
        let m = small(challenger, champion, 2, scratch(&format!("half{i}")));
        run(&TEST, &m, |_, _| {}).unwrap();
        let finished = games_in(&m.out);
        let decisions = lines_in(&m.out, "decisions.jsonl");
        let (whole, half) = ("pair0000-seat0", "pair0000-seat1");
        let log = m.out.join("logs").join(format!("{half}.jsonl"));
        let lines: Vec<String> = fs::read_to_string(&log)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        assert!(lines.len() > 10, "{challenger}: {} decisions", lines.len());
        fs::write(&log, lines[..lines.len() / 2].join("\n") + "\n").unwrap();
        drop_game(&m.out, "decisions.jsonl", half);
        drop_game(&m.out, "games.jsonl", half);
        // The other game had its decisions written, and then the run stopped.
        drop_game(&m.out, "games.jsonl", whole);
        let left = files::read::<GameRecord>(&m.out.join("games.jsonl"));
        assert!(left.unwrap().is_empty());
        run(&TEST, &m, |_, _| {}).unwrap();
        assert_eq!(games_in(&m.out), finished, "{challenger}");
        assert_eq!(
            lines_in(&m.out, "decisions.jsonl"),
            decisions,
            "{challenger}"
        );
        let logged = fs::read_to_string(&log).unwrap().lines().count();
        assert_eq!(logged, lines.len(), "{challenger}");
        let _ = fs::remove_dir_all(&m.out);
    }
}

/// A directory holding another match is refused, naming the field that differs.
#[test]
fn another_challenger_is_refused_by_name() {
    let out = scratch("other");
    run(&TEST, &small("greedy", "random", 2, out.clone()), |_, _| {}).unwrap();
    let e = run(
        &TEST,
        &small("greedy+hold", "random", 2, out.clone()),
        |_, _| {},
    )
    .unwrap_err();
    assert!(
        e.contains("challenger is \"greedy\", not \"greedy+hold\""),
        "{e}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// A match whose sequential test has decided plays no more games when run again, at any
/// cap.
#[test]
fn a_decided_match_plays_nothing_when_run_again() {
    let out = scratch("decided");
    let m = small("greedy", "random", 2, out.clone());
    run(&TEST, &m, |_, _| {}).unwrap();
    // Thirty swept pairs after the first, more than the test needs to pass.
    let first: GameRecord = files::read::<GameRecord>(&out.join("games.jsonl")).unwrap()[0].clone();
    let mut games = files::Log::open(&out.join("games.jsonl")).unwrap();
    for pair in 1..=30 {
        for seat in 0..2 {
            games
                .append(&GameRecord {
                    game: format!("pair{pair:04}-seat{seat}"),
                    pair,
                    seat,
                    outcome: GameVerdict::Win,
                    ..first.clone()
                })
                .unwrap();
        }
    }
    drop(games);
    let before = games_in(&out);
    let r = run(&TEST, &Match { cap: 200, ..m }, |_, _| {}).unwrap();
    assert_eq!(r.verdict, Verdict::Pass);
    assert!(r.games <= 62, "{}", r.games);
    assert_eq!(games_in(&out), before);
    assert_eq!(fs::read_dir(out.join("logs")).unwrap().count(), 2);
    let _ = fs::remove_dir_all(&out);
}
