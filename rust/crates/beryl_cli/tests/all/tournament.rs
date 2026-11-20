use beryl_search::Budget;

use crate::common::TEST;
use beryl_cli::tournament::*;

/// Seats alternate and each win is credited to the agent that won it, whichever seat it
/// held: greedy beats random from both.
#[test]
fn wins_go_to_the_agent_not_the_seat() {
    let agents = ["greedy".to_owned(), "random".to_owned()];
    let r = run(&TEST, &agents, 8, 3, Budget::DEFAULT, false, false, None).unwrap();
    let p = &r.pairings[0];
    assert_eq!(
        (p.a.as_str(), p.b.as_str(), p.games),
        ("greedy", "random", 8)
    );
    assert!(p.a_wins >= 6, "{p:?}");
}

/// A paired tournament plays each seed from both seats and sorts its seeds by how the
/// first agent fared over the two.
#[test]
fn paired_games_share_a_seed_from_both_seats() {
    let agents = ["greedy".to_owned(), "random".to_owned()];
    let r = run(&TEST, &agents, 6, 5, Budget::DEFAULT, false, true, None).unwrap();
    let q = r.pairings[0].paired.expect("paired seeds");
    assert_eq!(q.a_swept + q.split + q.b_swept, 3);
    assert!(q.a_swept >= 2, "{q:?}");
    assert!(r.pairings[0].paired.is_some());
    let unpaired = run(&TEST, &agents, 2, 5, Budget::DEFAULT, false, false, None).unwrap();
    assert!(unpaired.pairings[0].paired.is_none());
}
