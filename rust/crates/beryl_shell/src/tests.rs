//! A game saved through one app's [`Dirs`] resumes in another's.

use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use beryl_cards::decks::{VINTAGE_WU_CONTROL, WU_CONTROL};
use beryl_session::{GameUpdate, PlayerView, Seat, StartGame, Status, StopsSetting};

use crate::{Dirs, sessions};

fn owned(deck: &[(u8, &str)]) -> Vec<(u8, String)> {
    deck.iter().map(|&(n, name)| (n, name.to_owned())).collect()
}

/// The next view that asks the human to decide, or shows the game over.
fn to_decision(views: &Receiver<GameUpdate>) -> PlayerView {
    loop {
        let view = views
            .recv_timeout(Duration::from_secs(120))
            .expect("the session appears hung: it emitted no view within the hang limit")
            .view;
        if matches!(view.status, Status::Deciding { .. } | Status::Over { .. }) {
            return view;
        }
    }
}

#[test]
fn a_game_dropped_mid_game_resumes_from_a_second_sessions_on_the_same_dirs() {
    let temp = tempfile::tempdir().unwrap();
    let dirs = Dirs {
        data: temp.path().join("data"),
        logs: temp.path().join("logs"),
        bugs: temp.path().join("bugs"),
    };
    let start = StartGame {
        seed: 3,
        decks: [owned(WU_CONTROL), owned(VINTAGE_WU_CONTROL)],
        human: Seat::P0,
        opponent: "random".to_owned(),
        stops: StopsSetting::EXACT,
    };

    let (sent, views) = channel();
    let first = sessions(&dirs, move |update| sent.send(update).unwrap());
    first.start_game(&start, None).unwrap();
    let stopped = to_decision(&views);
    assert!(matches!(stopped.status, Status::Deciding { .. }));
    // The app quits at the human's first decision.
    drop(first);
    assert!(
        dirs.data.join("replays").is_dir(),
        "the replay is in the data folder"
    );

    let (sent, views) = channel();
    let second = sessions(&dirs, move |update| sent.send(update).unwrap());
    assert!(
        crate::resume_latest(&second).is_some(),
        "the unfinished game resumes"
    );
    assert_eq!(to_decision(&views), stopped, "it resumes where it stopped");
}
