//! Host-side handshake and dispatch into a table that owns the game state.

use std::time::Duration;

/// Shared by local callers and the link loop so requests wait in FIFO order.
pub use parking_lot::FairMutex as TableQueue;

use serde_json::Value;

use crate::{
    Link, LinkError, PeerInfo,
    connection::{Connection, Heartbeat, Outbox},
    wire::{Message, RefusalReason, TableStatus, VERSION_MISMATCH},
};

pub trait Hosted {
    fn seat_for(&mut self, name: &str) -> Option<u8>;

    /// Apply the action and send both seats their updates before returning.
    /// A refused action emits no updates.
    /// Local actions should hold the same table queue for the same sequence of work.
    fn act(&mut self, seat: u8, action: Value) -> Result<(), RefusalReason>;

    /// Prepare the first seat snapshot before admitting the connection.
    fn update_for(&mut self, seat: u8, peer: &PeerInfo) -> Result<Value, LinkError>;
    fn lobby_for(&mut self, seat: u8) -> Option<Value>;

    /// Install the sender for subsequent actions. The initial snapshot is
    /// already queued when this runs.
    fn connected(&mut self, _seat: u8, _sender: Sender) {}

    /// Report a wire mismatch to the local player after authenticating the invite.
    fn version_mismatch(&mut self, _reason: &str) {}

    fn left(&mut self, seat: u8);
    fn dropped(&mut self, seat: u8);
}

/// The host's handshake state. Reuse it for consecutive links to one table.
pub struct Host {
    wire: String,
    secret: String,
    name: String,
    guest: Option<(String, u8)>,
}

impl Host {
    pub fn new(
        wire: impl Into<String>,
        secret: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        Self {
            wire: wire.into(),
            secret: secret.into(),
            name: name.into(),
            guest: None,
        }
    }

    /// Restore the guest's reservation when reopening a saved table.
    pub fn restore_guest(&mut self, name: impl Into<String>, seat: u8) {
        self.guest = Some((name.into(), seat));
    }
}

#[derive(Clone)]
pub struct Sender(Outbox);

impl Sender {
    pub fn update(&self, payload: Value) -> Result<(), LinkError> {
        self.0.send(&Message::Update(payload))
    }

    pub fn lobby(&self, payload: Value) -> Result<(), LinkError> {
        self.0.send(&Message::Lobby(payload))
    }

    pub fn leave(&self) -> Result<(), LinkError> {
        self.0.finish(&Message::Leave)
    }

    /// Interrupt the link so a guest can reconnect.
    pub fn disconnect(&self) {
        self.0.close();
    }

    pub fn peer(&self) -> PeerInfo {
        self.0.peer()
    }
}

/// Serve one accepted link. The caller can accept another after this returns.
/// A failed handshake never invokes table callbacks.
pub fn serve(
    link: impl Link + 'static,
    host: &mut Host,
    hosted: &TableQueue<impl Hosted>,
    ping_interval: Duration,
) -> Result<(), LinkError> {
    let connection = Connection::new(link)?;
    let Message::Hello(hello) = connection.handshake()? else {
        return Err(LinkError::Refused(
            "The guest must send hello first.".into(),
        ));
    };
    if hello.secret != host.secret {
        return Err(LinkError::Refused("The invite secret is incorrect.".into()));
    }
    if hello.wire != host.wire {
        let reason = VERSION_MISMATCH.to_owned();
        hosted.lock().version_mismatch(&reason);
        let _ = connection.outbox.finish(&Message::Mismatch {
            reason: reason.clone(),
        });
        return Err(LinkError::Refused(reason));
    }
    connection.set_name(hello.name.clone());

    let seat;
    {
        let mut table = hosted.lock();
        let status;
        (seat, status) = match &host.guest {
            Some((name, seat)) if name == &hello.name => (*seat, TableStatus::Resumed),
            Some(_) => return Err(LinkError::Refused("The guest seat is reserved.".into())),
            None => (
                table.seat_for(&hello.name).ok_or_else(|| {
                    LinkError::Refused("There is no guest seat available.".into())
                })?,
                TableStatus::New,
            ),
        };
        let mut initial = || {
            let sender = Sender(connection.outbox.clone());
            let update = table.update_for(seat, &sender.peer())?;
            connection.outbox.send(&Message::Welcome {
                seat,
                table: status,
                host: host.name.clone(),
            })?;
            sender.update(update)?;
            if let Some(lobby) = table.lobby_for(seat) {
                sender.lobby(lobby)?;
            }
            table.connected(seat, sender);
            Ok(())
        };
        host.guest = Some((hello.name, seat));
        if let Err(error) = initial() {
            table.dropped(seat);
            return Err(error);
        }
    }

    let result = serve_actions(&connection, hosted, seat, ping_interval);
    let mut table = hosted.lock();
    if result.is_ok() || connection.outbox.is_finishing() {
        table.left(seat);
        host.guest = None;
        Ok(())
    } else {
        table.dropped(seat);
        result
    }
}

fn serve_actions(
    connection: &Connection,
    hosted: &TableQueue<impl Hosted>,
    seat: u8,
    ping_interval: Duration,
) -> Result<(), LinkError> {
    let mut heartbeat = Heartbeat::new(ping_interval);
    loop {
        match connection.receive(&mut heartbeat)? {
            Message::Act { n, action } => {
                let mut table = hosted.lock();
                if connection.outbox.is_finishing() {
                    return Ok(());
                }
                let reply = match table.act(seat, action) {
                    Ok(()) => Message::Done { n },
                    Err(reason) => Message::Refused { n, reason },
                };
                connection.outbox.send(&reply)?;
            }
            Message::Leave => return Ok(()),
            _ => {
                return Err(LinkError::Refused(
                    "Unexpected message from the guest.".into(),
                ));
            }
        }
    }
}
