use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use crate::{
    Link, LinkError, LinkSender, PeerInfo,
    wire::{Message, decode, encode_frame},
};

pub(crate) const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
pub const PING_INTERVAL: Duration = Duration::from_secs(5);
const MISSED_PINGS: usize = 3;

type Closer = Arc<Mutex<Box<dyn LinkSender>>>;
type Incoming = Result<(Vec<u8>, PeerInfo), LinkError>;

struct Outgoing {
    frame: Vec<u8>,
    finished: Option<mpsc::SyncSender<Result<(), LinkError>>>,
}

#[derive(Clone)]
pub(crate) struct Outbox {
    queue: Arc<Mutex<Option<mpsc::SyncSender<Outgoing>>>>,
    closer: Closer,
    peer: Arc<Mutex<PeerInfo>>,
    finishing: Arc<AtomicBool>,
}

impl Outbox {
    pub(crate) fn send(&self, message: &Message) -> Result<(), LinkError> {
        let frame = encode_frame(message)?;
        let result = self
            .queue
            .lock()
            .map_err(|_| LinkError::Closed)?
            .as_ref()
            .ok_or(LinkError::Closed)?
            .try_send(Outgoing {
                frame,
                finished: None,
            });
        if result.is_err() {
            self.close();
            return Err(LinkError::Closed);
        }
        Ok(())
    }

    // Seal the outbox and tell the writer to close only after delivering the
    // final frame. Closing immediately would discard the leave or refusal.
    pub(crate) fn finish(&self, message: &Message) -> Result<(), LinkError> {
        let frame = encode_frame(message)?;
        let (finished, completion) = mpsc::sync_channel(1);
        let queue = {
            let mut queue = self.queue.lock().map_err(|_| LinkError::Closed)?;
            let queue = queue.take().ok_or(LinkError::Closed)?;
            self.finishing.store(true, Ordering::Release);
            queue
        };
        if queue
            .try_send(Outgoing {
                frame,
                finished: Some(finished),
            })
            .is_err()
        {
            self.close();
            return Err(LinkError::Closed);
        }
        if let Ok(result) = completion.recv_timeout(HANDSHAKE_TIMEOUT) {
            result
        } else {
            self.close();
            Err(LinkError::Closed)
        }
    }

    pub(crate) fn is_finishing(&self) -> bool {
        self.finishing.load(Ordering::Acquire)
    }

    pub(crate) fn close(&self) {
        self.queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        close(&self.closer);
    }

    pub(crate) fn peer(&self) -> PeerInfo {
        self.peer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

fn close(closer: &Closer) {
    let _ = closer
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .close();
}

pub(crate) struct Connection {
    pub(crate) outbox: Outbox,
    inbox: mpsc::Receiver<Incoming>,
}

impl Connection {
    pub(crate) fn new(mut link: impl Link + 'static) -> Result<Self, LinkError> {
        let closer = Arc::new(Mutex::new(link.try_clone_sender()?));
        let mut writer = link.try_clone_sender()?;
        let peer = Arc::new(Mutex::new(link.peer()));
        let (incoming, inbox) = mpsc::sync_channel(1);
        let (queue, outgoing) = mpsc::sync_channel::<Outgoing>(32);
        // A separate writer lets the protocol close a transport that has
        // stopped reading while a write is blocked.
        let writer_closer = Arc::clone(&closer);
        thread::spawn(move || {
            for next in outgoing {
                let result = writer.send(&next.frame);
                if let Some(finished) = next.finished {
                    let _ = finished.send(result);
                    break;
                }
                if result.is_err() {
                    break;
                }
            }
            close(&writer_closer);
        });
        thread::spawn(move || {
            loop {
                let result = link.receive().map(|frame| (frame, link.peer()));
                let failed = result.is_err();
                if incoming.send(result).is_err() || failed {
                    break;
                }
            }
        });
        Ok(Self {
            outbox: Outbox {
                queue: Arc::new(Mutex::new(Some(queue))),
                closer,
                peer,
                finishing: Arc::new(AtomicBool::new(false)),
            },
            inbox,
        })
    }

    pub(crate) fn handshake(&self) -> Result<Message, LinkError> {
        self.read(HANDSHAKE_TIMEOUT)?.ok_or(LinkError::Closed)
    }

    fn read(&self, timeout: Duration) -> Result<Option<Message>, LinkError> {
        let (frame, peer) = match self.inbox.recv_timeout(timeout) {
            Ok(result) => result?,
            Err(mpsc::RecvTimeoutError::Timeout) => return Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(LinkError::Closed),
        };
        let mut current = self.outbox.peer.lock().map_err(|_| LinkError::Closed)?;
        current.route = peer.route;
        current.round_trip = current.round_trip.or(peer.round_trip);
        drop(current);
        decode(&frame).map(Some)
    }

    pub(crate) fn set_name(&self, name: String) {
        self.outbox
            .peer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .name = name;
    }

    pub(crate) fn receive(&self, heartbeat: &mut Heartbeat) -> Result<Message, LinkError> {
        loop {
            if self.outbox.is_finishing() {
                // The writer closes the transport after it sends the final frame.
                self.read(HANDSHAKE_TIMEOUT)?.ok_or(LinkError::Closed)?;
                continue;
            }
            if let Some(ping) = heartbeat.tick(Instant::now())? {
                self.send_heartbeat(&ping)?;
            }
            match self.read(heartbeat.wait(Instant::now()))? {
                Some(Message::Ping { n }) => self.send_heartbeat(&Message::Pong { n })?,
                Some(Message::Pong { n }) => {
                    if let Some(round_trip) = heartbeat.answered(n, Instant::now()) {
                        self.outbox
                            .peer
                            .lock()
                            .map_err(|_| LinkError::Closed)?
                            .round_trip = Some(round_trip);
                    }
                }
                Some(message) => return Ok(message),
                None => {}
            }
        }
    }

    fn send_heartbeat(&self, message: &Message) -> Result<(), LinkError> {
        match self.outbox.send(message) {
            Err(LinkError::Closed) if self.outbox.is_finishing() => Ok(()),
            result => result,
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // When another thread is sending a final frame, that writer owns
        // closure. The final send also closes the transport on timeout.
        if !self.outbox.is_finishing() {
            self.outbox.close();
        }
    }
}

pub(crate) struct Heartbeat {
    interval: Duration,
    next_ping: Instant,
    next_number: u64,
    pending: VecDeque<(u64, Instant)>,
}

impl Heartbeat {
    pub(crate) fn new(interval: Duration) -> Self {
        Self {
            interval,
            next_ping: Instant::now() + interval,
            next_number: 0,
            pending: VecDeque::new(),
        }
    }

    fn tick(&mut self, now: Instant) -> Result<Option<Message>, LinkError> {
        if now < self.next_ping {
            return Ok(None);
        }
        if self.pending.len() == MISSED_PINGS {
            return Err(LinkError::Closed);
        }
        let n = self.next_number;
        self.next_number += 1;
        self.pending.push_back((n, now));
        self.next_ping = now + self.interval;
        Ok(Some(Message::Ping { n }))
    }

    fn wait(&self, now: Instant) -> Duration {
        self.next_ping.saturating_duration_since(now)
    }

    fn answered(&mut self, n: u64, now: Instant) -> Option<Duration> {
        let position = self.pending.iter().position(|&(number, _)| number == n)?;
        let (_, sent) = self.pending[position];
        self.pending.drain(..=position);
        Some(now.duration_since(sent))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartbeat_measures_replies_and_expires_after_three_misses() {
        let interval = Duration::from_secs(5);
        let mut heartbeat = Heartbeat::new(interval);
        let first_tick = heartbeat.next_ping;
        assert_eq!(heartbeat.tick(first_tick), Ok(Some(Message::Ping { n: 0 })));
        let latency = Duration::from_millis(20);
        assert_eq!(heartbeat.answered(1, first_tick + latency), None);
        assert_eq!(heartbeat.answered(0, first_tick + latency), Some(latency));
        assert_eq!(heartbeat.answered(0, first_tick + latency), None);
        for n in 1..=3 {
            assert_eq!(
                heartbeat.tick(first_tick + interval * n),
                Ok(Some(Message::Ping { n: u64::from(n) }))
            );
        }
        assert_eq!(
            heartbeat.tick(first_tick + interval * 4),
            Err(LinkError::Closed)
        );
    }
}
