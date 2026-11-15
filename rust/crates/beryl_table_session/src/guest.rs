//! Relays the host's publications and matches action replies to local callers.

use std::collections::HashMap;
use std::sync::{
    Arc, Mutex, MutexGuard, PoisonError,
    mpsc::{Receiver, Sender, channel},
};
use std::thread;

use beryl_link::guest::{self, Event, Guest};
use beryl_link::wire::Hello;
use beryl_link::{Link, LinkError, PING_INTERVAL};
use beryl_session::protocol::{LinkState, TableAction};
use beryl_session::{GameUpdate, SessionId, WIRE_HASH};
use beryl_table::Refusal;
use serde_json::Value;

use crate::refusal::from_wire;
use crate::sessions::{Emit, EmitEvent, TableEvent};

pub(super) struct GuestSession(Arc<Shared>);

struct Shared {
    session: SessionId,
    emit: Emit,
    emit_event: EmitEvent,
    state: Mutex<State>,
}

struct State {
    guest: Option<Guest>,
    current: Option<GameUpdate>,
    link: Option<LinkState>,
    connection: u64,
    pending: HashMap<u64, Sender<Result<(), Refusal>>>,
}

impl GuestSession {
    pub(super) fn new<L: Link + 'static>(
        session: SessionId,
        connect: impl FnMut() -> Result<L, LinkError> + Send + 'static,
        name: String,
        secret: String,
        emit: Emit,
        emit_event: EmitEvent,
    ) -> (Self, Receiver<Event>) {
        let hello = Hello {
            wire: WIRE_HASH.into(),
            name,
            secret,
        };
        let mut guest = guest::join(connect, hello, guest::REDIAL_DELAY, PING_INTERVAL);
        let events = guest.take_events().expect("new guest has events");
        let shared = Shared {
            session,
            emit,
            emit_event,
            state: Mutex::new(State {
                guest: Some(guest),
                current: None,
                link: None,
                connection: 0,
                pending: HashMap::new(),
            }),
        };
        (Self(Arc::new(shared)), events)
    }

    pub(super) fn listen(&self, events: Receiver<Event>) {
        let shared = Arc::clone(&self.0);
        thread::spawn(move || {
            let mut error = None;
            for event in events {
                let mut state = shared.lock();
                if state.guest.is_none() {
                    return;
                }
                match event {
                    Event::Joined { .. } | Event::Resumed { .. } => state.connection += 1,
                    Event::Done { n } => state.reply(n, Ok(())),
                    Event::Refused { n, reason } => state.reply(n, Err(from_wire(reason))),
                    Event::Update(payload) => {
                        if let Err(reason) = shared.update(&mut state, payload) {
                            error = Some(reason);
                            break;
                        }
                    }
                    Event::Lobby(payload) => (shared.emit_event)(TableEvent::Lobby(payload)),
                    Event::Reconnecting => {
                        state.pending.clear();
                        shared.link(&mut state, LinkState::Reconnecting);
                    }
                    Event::Left => break,
                    Event::Mismatch(reason) => {
                        error = Some(reason);
                        break;
                    }
                    Event::Failed(reason) => {
                        error = Some(reason.to_string());
                        break;
                    }
                }
            }
            let guest = {
                let mut state = shared.lock();
                let Some(guest) = state.guest.take() else {
                    return;
                };
                state.pending.clear();
                shared.link(&mut state, LinkState::Left);
                if let Some(reason) = error {
                    (shared.emit_event)(TableEvent::Error(reason));
                }
                guest
            };
            drop(guest);
        });
    }

    pub(super) fn current(&self) -> Option<GameUpdate> {
        self.0.lock().current.clone()
    }

    pub(super) fn local_action(&self) -> impl FnOnce(TableAction) -> Result<(), Refusal> + use<> {
        let shared = Arc::clone(&self.0);
        move |action| {
            let reply = shared.send(action)?;
            reply.recv().unwrap_or(Err(Refusal::Stale))
        }
    }

    pub(super) fn stop(&self) -> Option<Guest> {
        let mut state = self.0.lock();
        state.pending.clear();
        state.guest.take()
    }
}

impl Drop for GuestSession {
    fn drop(&mut self) {
        drop(self.stop());
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn send(&self, action: TableAction) -> Result<Receiver<Result<(), Refusal>>, Refusal> {
        let payload = serde_json::to_value(action).map_err(|_| Refusal::INVALID)?;
        let mut state = self.lock();
        if !matches!(state.link, Some(LinkState::Connected { .. })) {
            return Err(Refusal::Stale);
        }
        let number = state
            .guest
            .as_ref()
            .ok_or(Refusal::Stale)?
            .act_on(state.connection, payload)
            .map_err(|error| match error {
                LinkError::Refused(_) => Refusal::INVALID,
                _ => Refusal::Stale,
            })?;
        let (send, receive) = channel();
        state.pending.insert(number, send);
        Ok(receive)
    }

    fn update(&self, state: &mut State, payload: Value) -> Result<(), String> {
        let mut update: GameUpdate = serde_json::from_value(payload)
            .map_err(|error| format!("Could not read the host's table update: {error}"))?;
        let link = update
            .view
            .table
            .as_ref()
            .ok_or_else(|| "The host did not send a manual table.".to_owned())?
            .link;
        // The host owns the view, but session numbers order updates within
        // this app, including updates from local tables and engine games.
        update.session = self.session;
        state.current = Some(update.clone());
        (self.emit)(update);
        self.link(state, link);
        Ok(())
    }

    fn link(&self, state: &mut State, link: LinkState) {
        if state.link.replace(link) != Some(link) {
            (self.emit_event)(TableEvent::Link(link));
        }
    }
}

impl State {
    fn reply(&mut self, number: u64, result: Result<(), Refusal>) {
        if let Some(reply) = self.pending.remove(&number) {
            let _ = reply.send(result);
        }
    }
}
