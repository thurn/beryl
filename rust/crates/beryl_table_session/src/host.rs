//! The authoritative session for a manual table shared over a link.

use std::sync::{
    Arc,
    mpsc::{Receiver, RecvTimeoutError},
};
use std::thread;
use std::time::Duration;

use beryl_link::host::{self, Host, Hosted, Sender, TableQueue};
use beryl_link::wire::RefusalReason;
use beryl_link::{Link, LinkError, PING_INTERVAL, PeerInfo, Route};
use beryl_session::ipc::LobbyUpdate;
use beryl_session::protocol::{LinkState, TableAction};
use beryl_session::{GameUpdate, SessionId, WIRE_HASH};
use beryl_table::{Action, Entry, Refusal, Seat, Table};
use serde_json::Value;

use crate::lobby;
use crate::prompt::{PromptState, Request};
use crate::publication::PublishedTable;
use crate::refusal::to_wire;
use crate::saves::Save;
use crate::sessions::{Emit, EmitEvent, TableEvent};

const LOCAL: Seat = Seat(0);
const GUEST: Seat = Seat(1);
const ACCEPT_POLL: Duration = Duration::from_millis(100);

pub(super) struct HostSession(Arc<TableQueue<HostedTable>>);

impl HostSession {
    pub(super) fn new(
        session: SessionId,
        mut table: PublishedTable,
        save: Save,
        emit: Emit,
        emit_event: EmitEvent,
    ) -> Result<Self, Refusal> {
        table.set_names(seat_names(&save));
        table.reset_seat(LOCAL)?;
        let link = if save.metadata.guest.is_some() {
            LinkState::Reconnecting
        } else {
            LinkState::Alone
        };
        Ok(Self(Arc::new(TableQueue::new(HostedTable {
            session,
            table,
            save,
            emit,
            emit_event,
            link,
            sender: None,
            stopped: false,
            show_script: true,
        }))))
    }

    /// Accept links in order so the handshake retains one guest reservation.
    pub(super) fn listen(&self, links: Receiver<impl Link + 'static>) {
        let hosted = Arc::clone(&self.0);
        let mut host = {
            let table = self.0.lock();
            let metadata = &table.save.metadata;
            let settings = &metadata.settings;
            let mut host = Host::new(WIRE_HASH, &settings.secret, &settings.name);
            if let Some(guest) = &metadata.guest {
                host.restore_guest(guest, GUEST.0);
            }
            host
        };
        thread::spawn(move || {
            loop {
                if hosted.lock().stopped {
                    break;
                }
                match links.recv_timeout(ACCEPT_POLL) {
                    Ok(link) => {
                        if hosted.lock().stopped {
                            break;
                        }
                        let _ = host::serve(link, &mut host, &hosted, PING_INTERVAL);
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        });
    }

    pub(super) fn is_table(&self, id: &str) -> bool {
        self.0.lock().save.id() == id
    }

    pub(super) fn local_action(&self) -> impl FnOnce(Request) -> Result<(), Refusal> + use<> {
        let table = Arc::clone(&self.0);
        move |action| table.lock().apply(LOCAL, action)
    }

    pub(super) fn open_prompt(
        &self,
        seat: Seat,
        build: impl FnOnce(&Table) -> Result<PromptState, Refusal>,
        on_done: impl Fn(&PromptState) -> Result<Action, Refusal> + Send + Sync + 'static,
    ) -> Result<(), Refusal> {
        let mut host = self.0.lock();
        host.check_available(LOCAL)?;
        let state = build(host.table.table())?;
        let all = state.observer_lift().is_some();
        let HostedTable { table, save, .. } = &mut *host;
        table.open_prompt(seat, state, on_done, |entry| save_entry(save, entry))?;
        host.publish((!all).then_some(seat));
        Ok(())
    }

    pub(super) fn lobby(&self) -> LobbyUpdate {
        self.0.lock().lobby(LOCAL)
    }

    pub(super) fn current(&self) -> GameUpdate {
        let table = self.0.lock();
        table.update(LOCAL, table.link_state(), table.show_script)
    }

    pub(super) fn link_state(&self) -> LinkState {
        self.0.lock().link_state()
    }

    #[cfg(debug_assertions)]
    pub(super) fn cut_link_for_test(&self) {
        let mut table = self.0.lock();
        if let Some(sender) = table.sender.take() {
            sender.disconnect();
            table.link_changed(LinkState::Reconnecting);
        }
    }

    pub(super) fn stop(&self) -> Option<Sender> {
        let mut table = self.0.lock();
        table.stopped = true;
        table.sender.take()
    }
}

impl Drop for HostSession {
    fn drop(&mut self) {
        // Closing the app interrupts the link so the guest can redial. Only
        // the explicit Leave command sends a deliberate departure.
        if let Some(sender) = self.stop() {
            sender.disconnect();
        }
    }
}

struct HostedTable {
    session: SessionId,
    table: PublishedTable,
    save: Save,
    emit: Emit,
    emit_event: EmitEvent,
    link: LinkState,
    sender: Option<Sender>,
    stopped: bool,
    show_script: bool,
}

impl HostedTable {
    fn check_available(&self, seat: Seat) -> Result<(), Refusal> {
        if self.stopped
            || (self.sender.is_none() && (seat != LOCAL || self.link != LinkState::Alone))
        {
            return Err(Refusal::Stale);
        }
        Ok(())
    }

    // Both IPC and the authenticated link call this with the same table
    // queue lock held until both views have been sent or queued.
    fn apply(&mut self, seat: Seat, request: Request) -> Result<(), Refusal> {
        self.check_available(seat)?;
        let was_playing = self.table.table().game_match.in_game;
        let all = match request {
            Request::Action(action) => {
                check_lobby_actor(seat, &action)?;
                self.table
                    .dispatch_action(seat, action, |entry| save_entry(&mut self.save, entry))?;
                true
            }
            Request::Lobby(request) => {
                self.table
                    .apply_lobby_checked(seat, request.change, |entry| {
                        save_entry(&mut self.save, entry)
                    })?;
                true
            }
            Request::Answer(request) => self.table.answer(seat, request.answer, |entry| {
                save_entry(&mut self.save, entry)
            })?,
        };
        self.publish((!all).then_some(seat));
        if !was_playing || !self.table.table().game_match.in_game {
            self.publish_lobby();
        }
        Ok(())
    }

    fn publish(&mut self, only: Option<Seat>) {
        if only != Some(LOCAL)
            && let Some(sender) = &self.sender
        {
            let guest = self.update(GUEST, self.link_state(), true);
            if sender.update(serialize(&guest)).is_err() {
                // The guest will get the current state in the next snapshot.
                sender.disconnect();
                self.sender = None;
                if only == Some(GUEST) {
                    self.link_changed(LinkState::Reconnecting);
                    return;
                }
                self.link = LinkState::Reconnecting;
                (self.emit_event)(TableEvent::Link(self.link));
            }
        }
        if only != Some(GUEST) {
            self.show_script = true;
            self.emit_local();
        }
    }

    fn update(&self, seat: Seat, link: LinkState, show_script: bool) -> GameUpdate {
        let mut view = self
            .table
            .publication(seat)
            .expect("hosted seat")
            .view()
            .clone();
        let table = view.table.as_mut().expect("manual table");
        table.link = link;
        table.peer_name = if seat == LOCAL {
            self.save.metadata.guest.clone()
        } else {
            Some(self.save.metadata.settings.name.clone())
        };
        if !show_script {
            view.script = None;
        }
        GameUpdate {
            session: self.session,
            view,
        }
    }

    fn link_state(&self) -> LinkState {
        self.sender
            .as_ref()
            .map_or(self.link, |sender| connected(&sender.peer()))
    }

    fn lobby(&self, seat: Seat) -> LobbyUpdate {
        lobby::project(
            self.table.table(),
            seat,
            &seat_names(&self.save),
            Some(&self.save.metadata.settings.invite),
            self.table.choosing_first(),
        )
    }

    fn publish_lobby(&mut self) {
        if let Some(sender) = &self.sender
            && sender.lobby(serde_json::json!(self.lobby(GUEST))).is_err()
        {
            sender.disconnect();
            self.sender = None;
            self.link_changed(LinkState::Reconnecting);
        }
        (self.emit_event)(TableEvent::Lobby(self.lobby(LOCAL)));
    }

    fn emit_local(&self) {
        (self.emit)(self.update(LOCAL, self.link_state(), self.show_script));
    }

    fn link_changed(&mut self, link: LinkState) {
        if !self.stopped && self.link != link {
            self.link = link;
            (self.emit_event)(TableEvent::Link(link));
            self.show_script = false;
            self.emit_local();
        }
    }
}

impl Hosted for HostedTable {
    fn seat_for(&mut self, name: &str) -> Option<u8> {
        if self.stopped || name.trim().is_empty() {
            return None;
        }
        if let Err(error) = self.save.reserve_guest(name) {
            eprintln!(
                "Cannot reserve the guest seat at table {}: {error}",
                self.save.id()
            );
            return None;
        }
        self.table.set_names(seat_names(&self.save));
        Some(GUEST.0)
    }

    fn act(&mut self, seat: u8, action: Value) -> Result<(), RefusalReason> {
        let action = serde_json::from_value(action).map_err(|_| to_wire(Refusal::INVALID))?;
        self.apply(Seat(seat), action).map_err(to_wire)
    }

    fn update_for(&mut self, seat: u8, peer: &PeerInfo) -> Result<Value, LinkError> {
        if self.stopped {
            return Err(LinkError::Closed);
        }
        let seat = Seat(seat);
        self.table
            .reset_seat(seat)
            .map_err(|_| LinkError::Refused("Could not prepare the table view.".into()))?;
        Ok(serialize(&self.update(seat, connected(peer), true)))
    }

    fn lobby_for(&mut self, seat: u8) -> Option<Value> {
        Some(serde_json::json!(self.lobby(Seat(seat))))
    }

    fn connected(&mut self, _: u8, sender: Sender) {
        let link = connected(&sender.peer());
        self.sender = Some(sender);
        self.link_changed(link);
        (self.emit_event)(TableEvent::Lobby(self.lobby(LOCAL)));
    }

    fn version_mismatch(&mut self, reason: &str) {
        if !self.stopped {
            (self.emit_event)(TableEvent::Error(reason.into()));
        }
    }

    fn left(&mut self, _: u8) {
        self.sender = None;
        self.link_changed(LinkState::Left);
        self.stopped = true;
    }

    fn dropped(&mut self, _: u8) {
        self.sender = None;
        self.link_changed(LinkState::Reconnecting);
    }
}

fn save_entry(save: &mut Save, entry: &Entry) -> Result<(), Refusal> {
    save.append(entry).map_err(|error| {
        eprintln!("Cannot save table {}: {error}", save.id());
        Refusal::INVALID
    })
}

fn check_lobby_actor(seat: Seat, action: &TableAction) -> Result<(), Refusal> {
    match action {
        Action::LoadDeck { seat: owner, .. }
        | Action::Sideboard { seat: owner, .. }
        | Action::Ready { seat: owner, .. }
            if *owner != seat =>
        {
            Err(Refusal::Permission)
        }
        Action::Start { .. } if seat != LOCAL => Err(Refusal::Permission),
        Action::Batch { actions } => actions
            .iter()
            .try_for_each(|action| check_lobby_actor(seat, action)),
        _ => Ok(()),
    }
}

fn seat_names(save: &Save) -> Vec<Option<String>> {
    vec![
        Some(save.metadata.settings.name.clone()),
        save.metadata.guest.clone(),
    ]
}

pub(super) fn connected(peer: &PeerInfo) -> LinkState {
    LinkState::Connected {
        direct: peer.route != Route::Relayed,
        round_trip_ms: peer
            .round_trip
            .map(|rtt| u32::try_from(rtt.as_millis()).unwrap_or(u32::MAX)),
    }
}

fn serialize(update: &GameUpdate) -> Value {
    serde_json::to_value(update).expect("a game update serializes to JSON")
}
