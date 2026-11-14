//! A length-prefixed frame stream over IPv4 loopback.

use std::{
    io::{self, Read, Write},
    net::{Ipv4Addr, Shutdown, TcpListener, TcpStream},
    sync::{Arc, Mutex},
};

use crate::{Invite, Link, LinkError, LinkSender, PeerInfo, Route, frame_length, secret};

pub struct TcpHost(TcpListener);

impl TcpHost {
    pub fn listen() -> Result<(Self, Invite), LinkError> {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|error| io_error(&error))?;
        let port = listener
            .local_addr()
            .map_err(|error| io_error(&error))?
            .port();
        Ok((
            Self(listener),
            Invite::Local {
                port,
                secret: secret(),
            },
        ))
    }

    pub fn accept(&self) -> Result<TcpLink, LinkError> {
        let (stream, _) = self.0.accept().map_err(|error| io_error(&error))?;
        TcpLink::from_stream(stream)
    }
}

pub struct TcpLink {
    shared: Arc<TcpConnection>,
}

struct TcpConnection {
    stream: TcpStream,
    // Hold the lock across both writes so another sender cannot split a frame.
    write_lock: Mutex<()>,
}

impl TcpLink {
    /// Use the invite's loopback port. The wire handshake checks its secret.
    pub fn connect(invite: &Invite) -> Result<Self, LinkError> {
        let Invite::Local { port, .. } = invite else {
            return Err(LinkError::Refused(
                "This connection needs a local invite.".into(),
            ));
        };
        let stream =
            TcpStream::connect((Ipv4Addr::LOCALHOST, *port)).map_err(|error| io_error(&error))?;
        Self::from_stream(stream)
    }

    fn from_stream(stream: TcpStream) -> Result<Self, LinkError> {
        stream.set_nodelay(true).map_err(|error| io_error(&error))?;
        Ok(Self {
            shared: Arc::new(TcpConnection {
                stream,
                write_lock: Mutex::new(()),
            }),
        })
    }

    fn read_frame(&self) -> Result<Vec<u8>, LinkError> {
        let mut stream = &self.shared.stream;
        let mut header = [0; 4];
        stream
            .read_exact(&mut header)
            .map_err(|error| io_error(&error))?;
        let length = u32::from_be_bytes(header) as usize;
        frame_length(length)?;
        let mut frame = vec![0; length];
        stream
            .read_exact(&mut frame)
            .map_err(|error| io_error(&error))?;
        Ok(frame)
    }
}

impl Drop for TcpLink {
    fn drop(&mut self) {
        let _ = self.shared.close();
    }
}

impl Link for TcpLink {
    fn send(&mut self, frame: &[u8]) -> Result<(), LinkError> {
        self.shared.send(frame)
    }

    fn receive(&mut self) -> Result<Vec<u8>, LinkError> {
        let result = self.read_frame();
        if result.is_err() {
            // After a partial or refused frame, the next byte may be payload.
            let _ = self.shared.close();
        }
        result
    }

    fn peer(&self) -> PeerInfo {
        PeerInfo {
            name: "Localhost".into(),
            route: Route::Local,
            round_trip: None,
        }
    }

    fn try_clone_sender(&self) -> Result<Box<dyn LinkSender>, LinkError> {
        Ok(Box::new(Arc::clone(&self.shared)))
    }
}

impl TcpConnection {
    fn send(&self, frame: &[u8]) -> Result<(), LinkError> {
        let header = frame_length(frame.len())?.to_be_bytes();
        let _guard = self.write_lock.lock().map_err(|_| LinkError::Closed)?;
        let mut stream = &self.stream;
        let result = stream
            .write_all(&header)
            .and_then(|()| stream.write_all(frame));
        if result.is_err() {
            let _ = self.close();
        }
        result.map_err(|error| io_error(&error))
    }

    fn close(&self) -> Result<(), LinkError> {
        // Some platforms report NotConnected if the socket was already shut down.
        match self.stream.shutdown(Shutdown::Both) {
            Err(error) if error.kind() != io::ErrorKind::NotConnected => Err(io_error(&error)),
            _ => Ok(()),
        }
    }
}

impl LinkSender for Arc<TcpConnection> {
    fn send(&mut self, frame: &[u8]) -> Result<(), LinkError> {
        TcpConnection::send(self, frame)
    }

    fn close(&self) -> Result<(), LinkError> {
        TcpConnection::close(self)
    }
}

fn io_error(error: &io::Error) -> LinkError {
    match error.kind() {
        io::ErrorKind::BrokenPipe
        | io::ErrorKind::ConnectionAborted
        | io::ErrorKind::ConnectionReset
        | io::ErrorKind::NotConnected
        | io::ErrorKind::UnexpectedEof => LinkError::Closed,
        _ => LinkError::Io(error.to_string()),
    }
}
