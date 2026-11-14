//! A pair of links joined by channels within one process.

use std::sync::{Arc, Mutex, mpsc};

use crate::{Link, LinkError, LinkSender, PeerInfo, Route, frame_length};

type Frame = Vec<u8>;
type Outboxes = Arc<Mutex<Option<[mpsc::Sender<Frame>; 2]>>>;

#[derive(Clone)]
struct MemorySender {
    outboxes: Outboxes,
    destination: usize,
}

pub struct MemoryLink {
    sender: MemorySender,
    inbox: mpsc::Receiver<Frame>,
}

pub fn pair() -> (MemoryLink, MemoryLink) {
    let (first_outbox, first_inbox) = mpsc::channel();
    let (second_outbox, second_inbox) = mpsc::channel();
    let outboxes = Arc::new(Mutex::new(Some([first_outbox, second_outbox])));
    (
        MemoryLink {
            sender: MemorySender {
                outboxes: Arc::clone(&outboxes),
                destination: 1,
            },
            inbox: first_inbox,
        },
        MemoryLink {
            sender: MemorySender {
                outboxes,
                destination: 0,
            },
            inbox: second_inbox,
        },
    )
}

impl MemoryLink {
    /// Disconnect both ends. Receivers can still drain frames already queued.
    pub fn sever(&self) {
        self.sender.sever();
    }
}

impl Drop for MemoryLink {
    fn drop(&mut self) {
        self.sever();
    }
}

impl Link for MemoryLink {
    fn send(&mut self, frame: &[u8]) -> Result<(), LinkError> {
        self.sender.send(frame)
    }

    fn receive(&mut self) -> Result<Vec<u8>, LinkError> {
        self.inbox.recv().map_err(|_| LinkError::Closed)
    }

    fn peer(&self) -> PeerInfo {
        PeerInfo {
            name: "In memory".into(),
            route: Route::Memory,
            round_trip: None,
        }
    }

    fn try_clone_sender(&self) -> Result<Box<dyn LinkSender>, LinkError> {
        Ok(Box::new(self.sender.clone()))
    }
}

impl MemorySender {
    fn sever(&self) {
        // Closing drops the only channel senders, waking either blocked reader
        // even when another thread still owns a handle.
        self.outboxes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }
}

impl LinkSender for MemorySender {
    fn send(&mut self, frame: &[u8]) -> Result<(), LinkError> {
        frame_length(frame.len())?;
        let frame = frame.to_vec();
        self.outboxes
            .lock()
            .map_err(|_| LinkError::Closed)?
            .as_ref()
            .ok_or(LinkError::Closed)?[self.destination]
            .send(frame)
            .map_err(|_| LinkError::Closed)
    }

    fn close(&self) -> Result<(), LinkError> {
        self.sever();
        Ok(())
    }
}
