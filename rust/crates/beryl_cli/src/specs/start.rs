//! Starting a moment's game: live from one seat for the page ([`start`]), or as one shared
//! game publishing both seats for a review ([`start_review`]).

use std::sync::Arc;

use beryl_cards::CARDS;
use beryl_scenario::Action;
use beryl_search::GameAgent;
use beryl_session::{GameUpdate, Prepared, Seat, Sessions, Status, StopsSetting};

use super::{Moment, View};
use crate::boards::{self, Scripted, Shared};

/// Where a moment's game stood when it emitted a view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Still being set up: the page never shows it.
    Setup,
    /// Playing out the moment's own actions, which the page animates.
    Play,
    /// Every action taken: the human plays on.
    Live,
}

/// Starts `moment` in its own [`Sessions`], seen from `view`, each view sent to `emit` with the
/// phase it was emitted in. Scripts that fail report to the view's phase as `Live` with the
/// failure.
pub fn start(
    moment: &'static Moment,
    view: View,
    emit: impl Fn(GameUpdate, Phase) + Send + Sync + 'static,
) -> Result<(Sessions, Shared), String> {
    let progress = Shared::default();
    // An `AdvanceTo` holds the scripts rather than being taken.
    let setup = moment
        .board
        .actions
        .iter()
        .filter(|a| !matches!(a, Action::AdvanceTo { .. }))
        .count();
    // A trailing `AdvanceTo` holds the hand-over until the game reaches its turn.
    if let Some(Action::AdvanceTo { turn, .. }) =
        moment.board.actions.iter().chain(moment.play).last()
    {
        boards::lock(&progress).until = Some(*turn);
    }
    let watched = Arc::clone(&progress);
    let sessions = Sessions::new(None, move |update| {
        // The emitting thread is the one the scripts answer on, so this is the phase the view
        // was made in. Views are published before the stand-in answers, so the hand-over sees
        // the turn this one shows.
        let mut p = boards::lock(&watched);
        p.turn = update.view.turn.number;
        let phase = if p.taken < setup {
            Phase::Setup
        } else if p.done() {
            Phase::Live
        } else {
            Phase::Play
        };
        drop(p);
        let mut update = update;
        if phase != Phase::Live && matches!(update.view.status, Status::Deciding { .. }) {
            // A stand-in answers the viewer's decisions until the actions are taken, at once:
            // the viewer watches the game go on through them, each held as long as its events
            // take to follow (the opponent's ability on the stack, read before it resolves), as
            // the app holds the opponent's play.
            update.view.status = Status::Playing;
        }
        emit(update, phase);
    });
    let (table, state) =
        beryl_scenario::position(&CARDS, &moment.board).map_err(|e| e.to_string())?;
    let actions = || moment.board.actions.iter().chain(moment.play);
    let agent = |seat: Seat| {
        Scripted::with_actions(actions(), seat, &table, Arc::clone(&progress))
            .map(|a| Box::new(a) as Box<dyn GameAgent + Send>)
    };
    let handed = Arc::clone(&progress);
    sessions
        .start_prepared(Prepared {
            opponent: agent(view.other())?,
            stand_in: agent(view.seat())?,
            scripted: true,
            hand_over: Some(Box::new(move || boards::lock(&handed).done())),
            table,
            state,
            human: view.seat(),
            stops: StopsSetting::SMART,
        })
        .map_err(|e| e.to_string())?;
    Ok((sessions, progress))
}

/// Run the setup in one shared game, then publish both seats at every manual decision.
pub(super) fn start_review(
    moment: &Moment,
    emit: impl Fn([GameUpdate; 2], bool) + Send + 'static,
) -> Result<Sessions, String> {
    let progress = Shared::default();
    let actions = || moment.board.actions.iter().chain(moment.play);
    if let Some(Action::AdvanceTo { turn, .. }) = actions().last() {
        boards::lock(&progress).until = Some(*turn);
    }
    let (table, state) =
        beryl_scenario::position(&CARDS, &moment.board).map_err(|e| e.to_string())?;
    let agent = |seat| {
        Scripted::with_actions(actions(), seat, &table, Arc::clone(&progress))
            .map(|a| Box::new(a) as Box<dyn GameAgent + Send>)
    };
    let watched = Arc::clone(&progress);
    let sessions = Sessions::new(None, |_| {});
    sessions
        .start_review(
            Prepared {
                opponent: agent(Seat::P1)?,
                stand_in: agent(Seat::P0)?,
                scripted: true,
                hand_over: Some(Box::new(move || boards::lock(&watched).done())),
                table,
                state,
                human: Seat::P0,
                stops: StopsSetting::SMART,
            },
            Box::new(move |session, views| {
                let mut p = boards::lock(&progress);
                let live = p.done() && p.failure.is_none();
                p.turn = views[0].turn.number;
                drop(p);
                emit(views.map(|view| GameUpdate { session, view }), live);
            }),
        )
        .map_err(|e| e.to_string())?;
    Ok(sessions)
}

pub(super) fn at_checkpoint(updates: &[GameUpdate; 2], live: bool) -> bool {
    live && updates
        .iter()
        .any(|u| matches!(u.view.status, Status::Deciding { .. } | Status::Over { .. }))
}

pub(super) fn review_position(
    moment: &Moment,
) -> Result<([beryl_session::PlayerView; 2], Option<String>), String> {
    use beryl_session::protocol::{GameEvent, Why};

    let (sent, received) = std::sync::mpsc::channel();
    let _sessions = start_review(moment, move |updates, live| {
        let _ = sent.send((updates, live));
    })?;
    let mut countered = None;
    loop {
        let (updates, live) = received
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| "setup never reached a checkpoint".to_owned())?;
        if live {
            let view = &updates[0].view;
            for event in &view.events {
                if let GameEvent::ZoneChange {
                    slot,
                    reason: Why::Counter,
                    ..
                } = *event
                {
                    countered = view
                        .objects
                        .iter()
                        .find(|o| o.slot == slot)
                        .and_then(|o| o.name.clone())
                        .or(countered);
                }
            }
        }
        if at_checkpoint(&updates, live) {
            return Ok((updates.map(|u| u.view), countered));
        }
        if matches!(
            updates[0].view.status,
            Status::Over { .. } | Status::Stopped
        ) {
            return Err("game ended during setup".to_owned());
        }
    }
}
