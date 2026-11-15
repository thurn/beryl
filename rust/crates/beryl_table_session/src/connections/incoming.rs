//! Host listeners scoped to the lifetime of the current table.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, SyncSender, channel, sync_channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use beryl_link::{Link, LinkError, TcpHost, memory, secret};

type IncomingLink = Box<dyn Link>;
pub(super) type Links = Receiver<IncomingLink>;

/// Dropping a listener releases its address so a saved table can reopen it.
pub(super) struct Incoming(Option<Box<dyn FnOnce() + Send>>);

impl Incoming {
    pub(super) fn new(stop: impl FnOnce() + Send + 'static) -> Self {
        Self(Some(Box::new(stop)))
    }

    pub(super) fn tcp(host: TcpHost) -> Result<(Self, Links), LinkError> {
        host.set_nonblocking()?;
        let (send, links) = sync_channel::<IncomingLink>(1);
        let (stop, stopped) = channel::<()>();
        let worker = thread::spawn(move || {
            loop {
                match host.try_accept() {
                    Ok(Some(link)) => {
                        let _ = send.try_send(Box::new(link));
                    }
                    Ok(None) => {}
                    Err(error) => {
                        eprintln!("Cannot accept a local table connection: {error}");
                        break;
                    }
                }
                // A close wakes the worker immediately, including when idle.
                if !matches!(
                    stopped.recv_timeout(Duration::from_millis(25)),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                ) {
                    break;
                }
            }
        });
        Ok((
            Self::new(move || {
                drop(stop);
                let _ = worker.join();
            }),
            links,
        ))
    }
}

impl Drop for Incoming {
    fn drop(&mut self) {
        if let Some(stop) = self.0.take() {
            stop();
        }
    }
}

/// Local HTTP sessions share a registry for in-process connections.
#[derive(Clone, Default)]
pub struct MemoryInvites(Arc<Mutex<HashMap<String, SyncSender<IncomingLink>>>>);

impl MemoryInvites {
    pub(super) fn listen(&self, invite: &MemoryInvite) -> Result<(Incoming, Links), LinkError> {
        use std::collections::hash_map::Entry;

        let mut invites = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let Entry::Vacant(entry) = invites.entry(invite.0.clone()) else {
            return Err(LinkError::Refused("The table is already open.".into()));
        };
        let (sender, links) = sync_channel(1);
        entry.insert(sender);
        let registry = self.clone();
        let invite = invite.0.clone();
        let incoming = Incoming::new(move || {
            registry
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&invite);
        });
        Ok((incoming, links))
    }

    pub(super) fn connect(&self, invite: &MemoryInvite) -> Result<impl Link + use<>, LinkError> {
        let (host, guest) = memory::pair();
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&invite.0)
            .ok_or_else(|| LinkError::Refused("The in-process table is closed.".into()))?
            .try_send(Box::new(host))
            .map_err(|_| {
                LinkError::Refused("The in-process table did not accept the connection.".into())
            })?;
        Ok(guest)
    }
}

pub(super) struct MemoryInvite(String);

impl MemoryInvite {
    pub(super) fn new() -> Self {
        Self(secret())
    }

    pub(super) fn text(&self) -> String {
        format!("beryl-memory:{}", self.0)
    }

    pub(super) fn parse(text: &str) -> Result<Option<Self>, LinkError> {
        let Some(secret) = text.strip_prefix("beryl-memory:") else {
            return Ok(None);
        };
        if secret.len() != 8 || !secret.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return Err(LinkError::Refused(
                "The in-process invite is invalid.".into(),
            ));
        }
        Ok(Some(Self(secret.to_owned())))
    }

    pub(super) fn secret(&self) -> &str {
        &self.0
    }
}
