//! Guest-side event stream and automatic reconnection.

use std::{
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

use serde_json::Value;

use crate::{
    Link, LinkError, PeerInfo,
    connection::{Connection, Heartbeat, Outbox},
    wire::{Hello, Message, RefusalReason, TableStatus},
};

pub const REDIAL_DELAY: Duration = Duration::from_secs(3);
const EVENT_CAPACITY: usize = 32;

#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    Joined { seat: u8 },
    Done { n: u64 },
    Refused { n: u64, reason: RefusalReason },
    Update(Value),
    Lobby(Value),
    Left,
    Reconnecting,
    Resumed { seat: u8 },
    Mismatch(String),
    Failed(LinkError),
}

struct State {
    connection: Option<Outbox>,
    ready: bool,
    stopped: bool,
    next_action: u64,
}

pub struct Guest {
    state: Arc<Mutex<State>>,
    events: Option<mpsc::Receiver<Event>>,
    wake: mpsc::Sender<()>,
}

impl Guest {
    /// Number and send an action while connected. A reconnect never replays an
    /// unacknowledged action, since it may already have taken effect.
    pub fn act(&self, action: Value) -> Result<u64, LinkError> {
        let mut state = self.state.lock().map_err(|_| LinkError::Closed)?;
        let n = state.next_action;
        if !state.ready || state.stopped {
            return Err(LinkError::Closed);
        }
        state
            .connection
            .as_ref()
            .ok_or(LinkError::Closed)?
            .send(&Message::Act { n, action })?;
        state.next_action += 1;
        Ok(n)
    }

    /// Take the event stream, for example to forward events from another
    /// thread while the application keeps the action handle. Drain the stream
    /// continuously. If it fills, the guest disconnects and closes the stream
    /// rather than skipping updates that later updates depend on.
    pub fn take_events(&mut self) -> Option<mpsc::Receiver<Event>> {
        self.events.take()
    }

    pub fn peer(&self) -> Option<PeerInfo> {
        let state = self.state.lock().ok()?;
        if state.ready {
            state.connection.as_ref().map(Outbox::peer)
        } else {
            None
        }
    }

    /// Leave the current connection or cancel redial. A connect call already
    /// in progress can return later, but its link will be closed before hello.
    pub fn leave(&self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.stopped {
            return;
        }
        state.stopped = true;
        let _ = self.wake.send(());
        if let Some(connection) = state.connection.take() {
            // Hold the guard through the final write so the worker cannot drop
            // the live link before Leave has been sent.
            let _ = connection.finish(&Message::Leave);
        }
        state.ready = false;
    }
}

impl Drop for Guest {
    fn drop(&mut self) {
        self.leave();
    }
}

/// Start dialing on a worker. The connect callback may block and should use
/// the transport's own timeout. It is reused for each reconnect attempt.
pub fn join<L: Link + 'static>(
    mut connect: impl FnMut() -> Result<L, LinkError> + Send + 'static,
    hello: Hello,
    redial_delay: Duration,
    ping_interval: Duration,
) -> Guest {
    let state = Arc::new(Mutex::new(State {
        connection: None,
        ready: false,
        stopped: false,
        next_action: 1,
    }));
    let (event_sender, events) = mpsc::sync_channel(EVENT_CAPACITY);
    let (wake, cancellation) = mpsc::channel();
    let worker_state = Arc::clone(&state);
    thread::spawn(move || {
        let session = Session {
            state: &worker_state,
            events: &event_sender,
            ping_interval,
        };
        session.run(
            || Connection::new(connect()?),
            &hello,
            &cancellation,
            redial_delay,
        );
    });
    Guest {
        state,
        events: Some(events),
        wake,
    }
}

struct Session<'a> {
    state: &'a Mutex<State>,
    events: &'a mpsc::SyncSender<Event>,
    ping_interval: Duration,
}

enum End {
    Left,
    Mismatch(String),
    EventConsumerUnavailable,
    Disconnected(LinkError),
}

impl Session<'_> {
    fn run(
        &self,
        mut connect: impl FnMut() -> Result<Connection, LinkError>,
        hello: &Hello,
        cancellation: &mpsc::Receiver<()>,
        redial_delay: Duration,
    ) {
        let mut joined = false;
        let mut reconnecting = false;
        loop {
            let result = connect().and_then(|connection| {
                self.exchange(&connection, hello, &mut joined, &mut reconnecting)
            });
            {
                let Ok(mut state) = self.state.lock() else {
                    return;
                };
                state.connection = None;
                state.ready = false;
                if state.stopped {
                    return;
                }
            }
            match result.unwrap_or_else(End::Disconnected) {
                End::Left => {
                    let _ = self.events.try_send(Event::Left);
                    return;
                }
                End::Mismatch(reason) => {
                    let _ = self.events.try_send(Event::Mismatch(reason));
                    return;
                }
                End::EventConsumerUnavailable => return,
                End::Disconnected(error) if !joined => {
                    let _ = self.events.try_send(Event::Failed(error));
                    return;
                }
                End::Disconnected(_) => {
                    if !reconnecting && !self.emit(Event::Reconnecting) {
                        return;
                    }
                    reconnecting = true;
                }
            }
            if !matches!(
                cancellation.recv_timeout(redial_delay),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                return;
            }
            if self.state.lock().map_or(true, |state| state.stopped) {
                return;
            }
        }
    }

    fn exchange(
        &self,
        connection: &Connection,
        hello: &Hello,
        joined: &mut bool,
        reconnecting: &mut bool,
    ) -> Result<End, LinkError> {
        {
            let mut state = self.state.lock().map_err(|_| LinkError::Closed)?;
            if state.stopped {
                return Ok(End::Left);
            }
            connection.outbox.send(&Message::Hello(hello.clone()))?;
            state.connection = Some(connection.outbox.clone());
        }
        match connection.handshake()? {
            Message::Welcome { seat, table, host } => {
                connection.set_name(host);
                let state = self.state.lock().map_err(|_| LinkError::Closed)?;
                if state.stopped {
                    return Ok(End::Left);
                }
                *joined = true;
                *reconnecting = false;
                if !self.emit(match table {
                    TableStatus::New => Event::Joined { seat },
                    TableStatus::Resumed => Event::Resumed { seat },
                }) {
                    return Ok(End::EventConsumerUnavailable);
                }
            }
            Message::Mismatch { reason } => return Ok(End::Mismatch(reason)),
            _ => {
                return Err(LinkError::Refused(
                    "The host must send welcome first.".into(),
                ));
            }
        }
        let mut heartbeat = Heartbeat::new(self.ping_interval);
        loop {
            let event = match connection.receive(&mut heartbeat)? {
                Message::Done { n } => Event::Done { n },
                Message::Refused { n, reason } => Event::Refused { n, reason },
                Message::Update(payload) => {
                    let mut state = self.state.lock().map_err(|_| LinkError::Closed)?;
                    if state.stopped {
                        return Ok(End::Left);
                    }
                    state.ready = true;
                    if !self.emit(Event::Update(payload)) {
                        return Ok(End::EventConsumerUnavailable);
                    }
                    continue;
                }
                Message::Lobby(payload) => Event::Lobby(payload),
                Message::Leave => return Ok(End::Left),
                _ => {
                    return Err(LinkError::Refused(
                        "Unexpected message from the host.".into(),
                    ));
                }
            };
            if !self.emit(event) {
                return Ok(End::EventConsumerUnavailable);
            }
        }
    }

    fn emit(&self, event: Event) -> bool {
        self.events.try_send(event).is_ok()
    }
}
