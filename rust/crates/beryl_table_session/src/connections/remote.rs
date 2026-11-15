//! Remote table transport and the profile identity it uses.

use std::path::Path;
use std::sync::mpsc::{TrySendError, sync_channel};
use std::thread;

use beryl_link::{Invite, Link, LinkError};
use beryl_link_iroh::{Identity, IrohHost, IrohLink, RelayChoice, RemoteInvite};

use super::incoming::{Incoming, Links};

fn identity(data_dir: &Path) -> Result<Identity, LinkError> {
    std::fs::create_dir_all(data_dir).map_err(|error| LinkError::Io(error.to_string()))?;
    Identity::open(data_dir.join("identity"))
}

pub(super) fn open(
    data_dir: &Path,
    relay: Option<String>,
) -> Result<(IrohHost, Invite), LinkError> {
    IrohHost::open(
        identity(data_dir)?,
        relay.map_or(RelayChoice::Default, RelayChoice::Url),
    )
}

pub(super) fn resume(data_dir: &Path, invite: &Invite) -> Result<IrohHost, LinkError> {
    IrohHost::resume(identity(data_dir)?, invite)
}

pub(super) fn secret(invite: &Invite) -> Result<String, LinkError> {
    Ok(RemoteInvite::decode(invite)?.secret())
}

pub(super) fn connector(
    data_dir: &Path,
    invite: Invite,
) -> Result<impl FnMut() -> Result<IrohLink, LinkError> + use<>, LinkError> {
    let remote = RemoteInvite::decode(&invite)?;
    // The guest can use the host's relay directly, including a custom URL.
    let relay = remote.relay().map_or(RelayChoice::Disabled, |url| {
        RelayChoice::Url(url.to_string())
    });
    let identity = identity(data_dir)?;
    Ok(move || IrohLink::connect(identity.clone(), &invite, relay.clone()))
}

pub(super) fn incoming(host: IrohHost) -> (Incoming, Links) {
    let (send, links) = sync_channel::<Box<dyn Link>>(1);
    let accepting = host.clone();
    let worker = thread::spawn(move || {
        loop {
            match accepting.accept() {
                Ok(link) => {
                    if matches!(
                        send.try_send(Box::new(link)),
                        Err(TrySendError::Disconnected(_))
                    ) {
                        break;
                    }
                }
                Err(LinkError::Closed) => break,
                Err(error) => eprintln!("Cannot accept a remote table connection: {error}"),
            }
        }
    });
    (
        Incoming::new(move || {
            let _ = host.close();
            let _ = worker.join();
        }),
        links,
    )
}
