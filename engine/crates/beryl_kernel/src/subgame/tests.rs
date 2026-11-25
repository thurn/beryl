//! What tells apart the histories of games that ended by restarting and of subgames.

use std::hash::{DefaultHasher, Hash, Hasher};

use crate::layout::Large;
use crate::profile::Profile;
use crate::state::State;
use crate::subgame::OuterGame;

fn hash(state: &State<Large>) -> u64 {
    let mut h = DefaultHasher::new();
    state.hash(&mut h);
    h.finish()
}

#[test]
fn turns_before_a_restart_and_turns_in_a_subgame_hash_apart() {
    let mut restarted = State::<Large>::empty(Profile::EXACT);
    restarted.restarted_turns = 5;
    let mut after_subgame = State::<Large>::empty(Profile::EXACT);
    after_subgame.subgame_turns = 5;
    assert_eq!(restarted.elapsed_turns(), after_subgame.elapsed_turns());
    assert_ne!(hash(&restarted), hash(&after_subgame));
}

#[test]
fn a_suspended_game_changes_the_hash() {
    let alone = State::<Large>::empty(Profile::EXACT);
    let mut inner = State::<Large>::empty(Profile::EXACT);
    inner.outer.put(State::<Large>::empty(Profile::EXACT));
    assert!(inner.in_subgame());
    assert_ne!(hash(&alone), hash(&inner));
}
