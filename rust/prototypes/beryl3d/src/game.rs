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
    // A macOS thread inherits the scheduling class of the thread that creates it, and neither
    // the session nor the search pool sets one. Started from the frame thread, whose class is
    // user-interactive, every search thread ran level with drawing. Started from a thread in the
    // utility class, the session thread and the search threads it creates run one class below
    // drawing, as decision C9 in docs/beryl-3d.md says.
    let (sessions, id) = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                crate::measure::utility();
                let id = sessions
                    .start_game(&start, Some("expert"))
                    .expect("the game starts");
                (sessions, id)
            })
            .join()
            .expect("the game starts")
    });
    (sessions, id, receiver)
}
