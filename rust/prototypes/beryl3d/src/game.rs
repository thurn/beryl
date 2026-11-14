//! The live game: the WU Control mirror between the mcts-hidden agent and an expert agent
//! standing in for the human seat, run by the real session on its own threads. Every view goes
//! down a channel to the thread that draws.

use std::sync::mpsc::{Receiver, channel};

use beryl_session::protocol::SessionId;
use beryl_session::protocol::StopsSetting;
use beryl_session::{GameUpdate, Seat, Sessions, StartGame};

pub const SEED: u64 = 20_261_114;

/// Starts the game. `wake` is called after each view is sent, so a waiting window wakes up.
/// The returned `Sessions` owns the game; dropping it ends it.
pub fn start(
    wake: impl Fn() + Send + Sync + 'static,
) -> (Sessions, SessionId, Receiver<GameUpdate>) {
    let (sender, receiver) = channel();
    let sessions = Sessions::new(None, move |update| {
        let _ = sender.send(update);
        wake();
    });
    let deck = beryl_cards::decks::default_deck();
    let start = StartGame {
        seed: SEED,
        decks: [deck.clone(), deck],
        human: Seat::P0,
        opponent: "mcts-hidden".to_owned(),
        stops: StopsSetting::SMART,
    };
    let id = sessions
        .start_game(&start, Some("expert"))
        .expect("the game starts");
    (sessions, id, receiver)
}
