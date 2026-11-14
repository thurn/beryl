//! The blocking multiplayer frame link over a single iroh QUIC stream.

mod identity;
mod invite;
mod transport;

pub use identity::Identity;
pub use invite::RemoteInvite;
pub use transport::IrohLink;

use std::{future::Future, net::Ipv4Addr, sync::OnceLock, time::Duration};

use beryl_link::{Invite, LinkError};
use iroh::{
    Endpoint, RelayMode, Watcher,
    endpoint::{NetReportConfig, PortmapperConfig, presets},
};
use tokio::runtime::{Builder, Handle, Runtime};

use crate::invite::{Address, relay_url};

const ALPN: &[u8] = b"beryl/table/1";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
static RUNTIME: OnceLock<Result<Runtime, LinkError>> = OnceLock::new();

#[derive(Clone, Default)]
pub enum RelayChoice {
    #[default]
    Default,
    Url(String),
    /// Bind only on IPv4 loopback and disable all Internet discovery.
    Disabled,
}

pub struct IrohHost {
    endpoint: Endpoint,
}

impl IrohHost {
    pub fn open(identity: Identity, relay: RelayChoice) -> Result<(Self, Invite), LinkError> {
        wait(async move {
            let offline = matches!(relay, RelayChoice::Disabled);
            let endpoint = bind(identity, relay, 0).await?;
            let address = if offline {
                let port = endpoint
                    .bound_sockets()
                    .first()
                    .ok_or(LinkError::Closed)?
                    .port();
                Address::Loopback(port)
            } else {
                wait_for_relay(&endpoint).await?;
                let status = endpoint.home_relay_status().get();
                let relay = status
                    .iter()
                    .find(|relay| relay.is_connected())
                    .ok_or_else(|| LinkError::Io("Beryl lost its relay connection.".into()))?;
                Address::Relay(relay.url().clone())
            };
            let invite = RemoteInvite::new(endpoint.id(), address)?.encode();
            Ok((Self { endpoint }, invite))
        })
    }

    /// Restore the endpoint at the address in its saved invite.
    pub fn resume(identity: Identity, invite: &Invite) -> Result<Self, LinkError> {
        let invite = RemoteInvite::decode(invite)?;
        if invite.endpoint_id() != identity.endpoint_id() {
            return Err(LinkError::Refused(
                "The saved invite belongs to a different Beryl identity.".into(),
            ));
        }
        let (relay, port) = match invite.address() {
            Address::Relay(url) => (RelayChoice::Url(url.to_string()), 0),
            Address::Loopback(port) => (RelayChoice::Disabled, *port),
        };
        wait(async move {
            let online = !matches!(relay, RelayChoice::Disabled);
            let endpoint = bind(identity, relay, port).await?;
            if online {
                wait_for_relay(&endpoint).await?;
            }
            Ok(Self { endpoint })
        })
    }

    /// Stop accepting and release the endpoint, including its bound socket.
    pub fn close(self) -> Result<(), LinkError> {
        wait(async {
            self.endpoint.close().await;
            Ok(())
        })
    }

    /// Accept one connection. A client must open its table stream promptly.
    pub fn accept(&self) -> Result<IrohLink, LinkError> {
        wait(async {
            let incoming = self.endpoint.accept().await.ok_or(LinkError::Closed)?;
            tokio::time::timeout(CONNECT_TIMEOUT, async {
                let connection = incoming.await.map_err(io_error)?;
                let (send, receive) = connection.accept_bi().await.map_err(io_error)?;
                // Read the stream marker, which makes open_bi visible before Hello.
                IrohLink::accept_stream(self.endpoint.clone(), connection, send, receive).await
            })
            .await
            .map_err(|_| LinkError::Refused("The guest did not open a Beryl stream.".into()))?
        })
    }
}

async fn wait_for_relay(endpoint: &Endpoint) -> Result<(), LinkError> {
    tokio::time::timeout(CONNECT_TIMEOUT, endpoint.online())
        .await
        .map_err(|_| LinkError::Io("Beryl could not reach its relay.".into()))
}

async fn bind(
    identity: Identity,
    relay: RelayChoice,
    local_port: u16,
) -> Result<Endpoint, LinkError> {
    // Minimal supplies TLS without publishing endpoint addresses to a DNS service.
    let builder = Endpoint::builder(presets::Minimal)
        .secret_key(identity.0)
        .alpns(vec![ALPN.to_vec()]);
    let builder = match relay {
        RelayChoice::Default => builder.relay_mode(RelayMode::Default),
        RelayChoice::Url(url) => {
            let url = relay_url(&url)
                .ok_or_else(|| LinkError::Refused("The Beryl relay URL is invalid.".into()))?;
            builder.relay_mode(RelayMode::custom([url]))
        }
        RelayChoice::Disabled => builder
            .relay_mode(RelayMode::Disabled)
            .portmapper_config(PortmapperConfig::Disabled)
            .net_report_config(NetReportConfig::minimal())
            .clear_ip_transports()
            .bind_addr((Ipv4Addr::LOCALHOST, local_port))
            .map_err(io_error)?,
    };
    builder.bind().await.map_err(io_error)
}

fn runtime() -> Result<&'static Runtime, LinkError> {
    RUNTIME
        .get_or_init(|| {
            Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("beryl-iroh")
                .enable_all()
                .build()
                .map_err(io_error)
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn wait<T: Send>(
    future: impl Future<Output = Result<T, LinkError>> + Send,
) -> Result<T, LinkError> {
    let runtime = runtime()?;
    if Handle::try_current().is_ok() {
        // The app may call us from Tauri's Tokio runtime. Tokio does not let
        // that thread enter a second runtime while it is already driving a task.
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .spawn_scoped(scope, || runtime.block_on(future))
                .map_err(io_error)?
                .join()
                .map_err(|_| LinkError::Closed)?
        })
    } else {
        runtime.block_on(future)
    }
}

fn io_error(error: impl std::fmt::Display) -> LinkError {
    LinkError::Io(error.to_string())
}
