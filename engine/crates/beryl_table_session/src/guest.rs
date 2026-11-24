//! Relays the host's publications and matches action replies to local callers.

use std::collections::HashMap;
use std::sync::{
    Arc, Mutex, MutexGuard, PoisonError,
    mpsc::{Receiver, Sender, channel},
};
use std::thread;

use beryl_link::LinkError;
use beryl_link::guest::{Event, Guest};
use beryl_protocol::ipc::{LobbyUpdate, TableRequest};
use beryl_protocol::protocol::LinkState;
use beryl_protocol::{GameUpdate, SessionId};
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
    lobby: Option<LobbyUpdate>,
    link: Option<LinkState>,
    connection: u64,
    pending: HashMap<u64, Sender<Result<(), Refusal>>>,
}

impl GuestSession {
    pub(super) fn new(session: SessionId, guest: Guest, emit: Emit, emit_event: EmitEvent) -> Self {
        Self(Arc::new(Shared {
            session,
            emit,
            emit_event,
            state: Mutex::new(State {
                guest: Some(guest),
                current: None,
                lobby: None,
                link: None,
                connection: 0,
                pending: HashMap::new(),
            }),
        }))
    }

    pub(super) fn listen(&self, first: Option<Event>, events: Receiver<Event>) {
        let shared = Arc::clone(&self.0);
        thread::spawn(move || {
            let mut error = None;
            for event in first.into_iter().chain(events) {
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
                    Event::Lobby(payload) => match serde_json::from_value::<LobbyUpdate>(payload) {
                        Ok(update) => {
                            state.lobby = Some(update.clone());
                            (shared.emit_event)(TableEvent::Lobby(update));
                        }
                        Err(cause) => {
                            error = Some(format!("Could not read the host's lobby view: {cause}"));
                            break;
                        }
                    },
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

    pub(super) fn lobby(&self) -> Option<LobbyUpdate> {
        self.0.lock().lobby.clone()
    }

    pub(super) fn current(&self) -> Option<GameUpdate> {
        let state = self.0.lock();
        let mut update = state.current.clone()?;
        if matches!(state.link, Some(LinkState::Connected { .. }))
            && let Some(peer) = state.guest.as_ref().and_then(Guest::peer)
            && let Some(table) = &mut update.view.table
        {
            table.link = super::host::connected(&peer);
        }
        Some(update)
    }

    pub(super) fn local_action(&self) -> impl FnOnce(TableRequest) -> Result<(), Refusal> + use<> {
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
        if let Some(guest) = self.stop() {
            guest.disconnect();
        }
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn send(&self, action: TableRequest) -> Result<Receiver<Result<(), Refusal>>, Refusal> {
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
        // Locally detected disconnects have no publication from the host.
        // A page reloading in that interval still needs the current link state.
        if let Some(update) = &mut state.current
            && let Some(table) = &mut update.view.table
            && table.link != link
        {
            table.link = link;
            update.view.script = None;
        }
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
