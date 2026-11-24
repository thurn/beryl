use beryl_cli::match_log::GameRecord;
use beryl_cli::match_log::files::{self};
use beryl_cli::tournament::game_seed;
use beryl_search::Settings;
use std::fs;
use std::path::PathBuf;

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
    assert!(
        run(&TEST, &m, |_, _| {}).is_err(),
        "a second match in one directory"
    );
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
