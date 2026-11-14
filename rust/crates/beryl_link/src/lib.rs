//! Ordered, bidirectional frame transport for a multiplayer table.

mod connection;
pub mod guest;
pub mod host;
mod invite;
pub mod memory;
pub mod tcp;
pub mod wire;

pub use connection::PING_INTERVAL;

pub use invite::{Invite, secret};
pub use memory::MemoryLink;
pub use tcp::{TcpHost, TcpLink};

use std::{fmt, time::Duration};

/// Maximum size of the payload in a length-prefixed frame.
pub const MAX_FRAME_SIZE: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    Closed,
    Refused(String),
    Io(String),
}

impl fmt::Display for LinkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => formatter.write_str("The connection closed."),
            Self::Refused(reason) | Self::Io(reason) => formatter.write_str(reason),
        }
    }
}

impl std::error::Error for LinkError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerInfo {
    pub name: String,
    pub route: Route,
    pub round_trip: Option<Duration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Memory,
    Local,
    Direct,
    Relayed,
}

/// One end of an ordered, reliable connection. Dropping the end closes it.
///
/// Keep the link on the thread that receives frames. A send handle can send
/// frames or close the connection while that thread is blocked in
/// [`Link::receive`]. Sends made through the link and its handles are serialized
/// as whole frames. Dropping a send handle leaves the link open.
pub trait Link: Send {
    fn send(&mut self, frame: &[u8]) -> Result<(), LinkError>;
    fn receive(&mut self) -> Result<Vec<u8>, LinkError>;
    fn peer(&self) -> PeerInfo;
    fn try_clone_sender(&self) -> Result<Box<dyn LinkSender>, LinkError>;
}

pub trait LinkSender: Send {
    fn send(&mut self, frame: &[u8]) -> Result<(), LinkError>;

    /// Close both directions and unblock a receiver waiting for a frame.
    fn close(&self) -> Result<(), LinkError>;
}

fn frame_length(length: usize) -> Result<u32, LinkError> {
    if length > MAX_FRAME_SIZE {
        return Err(LinkError::Refused("The frame exceeds 8 MiB.".into()));
    }
    Ok(length as u32)
}

impl Link for Box<dyn Link> {
    fn send(&mut self, frame: &[u8]) -> Result<(), LinkError> {
        (**self).send(frame)
    }

    fn receive(&mut self) -> Result<Vec<u8>, LinkError> {
        (**self).receive()
    }

    fn peer(&self) -> PeerInfo {
        (**self).peer()
    }

    fn try_clone_sender(&self) -> Result<Box<dyn LinkSender>, LinkError> {
        (**self).try_clone_sender()
    }
}
