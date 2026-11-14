use std::{sync::Arc, time::Duration};

use beryl_link::{Invite, Link, LinkError, LinkSender, MAX_FRAME_SIZE, PeerInfo, Route};
use iroh::{
    Endpoint,
    endpoint::{Connection, RecvStream, SendStream},
};
use tokio::sync::{Mutex, watch};

use crate::{
    ALPN, CONNECT_TIMEOUT, Identity, RelayChoice, RemoteInvite, bind, io_error, runtime, wait,
};

// QUIC only makes a newly opened stream visible when its first bytes are sent.
const STREAM_MARKER: [u8; 1] = [1];
const DRAIN_TIMEOUT: Duration = Duration::from_secs(15);

pub struct IrohLink {
    receive: RecvStream,
    shared: Arc<Shared>,
}

struct Sender(Arc<Shared>);

struct Shared {
    connection: Connection,
    send: Mutex<SendStream>,
    closed: watch::Sender<bool>,
    // Each accepted link keeps listening and dialing state alive independently of the host.
    _endpoint: Endpoint,
}

impl IrohLink {
    /// Dial the endpoint named by the invite. The table Hello verifies its secret.
    pub fn connect(
        identity: Identity,
        invite: &Invite,
        relay: RelayChoice,
    ) -> Result<Self, LinkError> {
        let invite = RemoteInvite::decode(invite)?;
        if matches!(relay, RelayChoice::Disabled) != invite.relay().is_none() {
            return Err(LinkError::Refused(
                "The invite does not match the relay setting.".into(),
            ));
        }
        wait(async move {
            let endpoint = bind(identity, relay).await?;
            tokio::time::timeout(CONNECT_TIMEOUT, async {
                let connection = endpoint
                    .connect(invite.endpoint_addr(), ALPN)
                    .await
                    .map_err(io_error)?;
                let (mut send, receive) = connection.open_bi().await.map_err(io_error)?;
                send.write_all(&STREAM_MARKER).await.map_err(io_error)?;
                Ok(Self::new(endpoint, connection, send, receive))
            })
            .await
            .map_err(|_| LinkError::Io("The Beryl host did not answer.".into()))?
        })
    }

    pub(crate) async fn accept_stream(
        endpoint: Endpoint,
        connection: Connection,
        send: SendStream,
        mut receive: RecvStream,
    ) -> Result<Self, LinkError> {
        let mut marker = [0];
        receive.read_exact(&mut marker).await.map_err(io_error)?;
        if marker != STREAM_MARKER {
            return Err(LinkError::Refused(
                "The guest opened an invalid Beryl stream.".into(),
            ));
        }
        Ok(Self::new(endpoint, connection, send, receive))
    }

    fn new(
        endpoint: Endpoint,
        connection: Connection,
        send: SendStream,
        receive: RecvStream,
    ) -> Self {
        let shared = Arc::new(Shared {
            connection,
            send: Mutex::new(send),
            closed: watch::Sender::new(false),
            _endpoint: endpoint,
        });
        Self { receive, shared }
    }
}

impl Drop for IrohLink {
    fn drop(&mut self) {
        let _ = self.shared.close();
    }
}

impl Link for IrohLink {
    fn send(&mut self, frame: &[u8]) -> Result<(), LinkError> {
        self.shared.send(frame)
    }

    fn receive(&mut self) -> Result<Vec<u8>, LinkError> {
        let result = wait(async {
            let mut closed = self.shared.closed.subscribe();
            tokio::select! {
                biased;
                _ = closed.wait_for(|closed| *closed) => Err(LinkError::Closed),
                result = read_frame(&mut self.receive) => result,
            }
        });
        if result.is_err() {
            self.shared.abort();
        }
        result
    }

    fn peer(&self) -> PeerInfo {
        let paths = self.shared.connection.paths();
        let selected = paths.iter().find(iroh::endpoint::Path::is_selected);
        PeerInfo {
            name: self.shared.connection.remote_id().to_string(),
            route: if selected
                .as_ref()
                .is_some_and(iroh::endpoint::Path::is_relay)
            {
                Route::Relayed
            } else {
                Route::Direct
            },
            round_trip: selected.map(|path| path.rtt()),
        }
    }

    fn try_clone_sender(&self) -> Result<Box<dyn LinkSender>, LinkError> {
        Ok(Box::new(Sender(Arc::clone(&self.shared))))
    }
}

impl Shared {
    fn send(self: &Arc<Self>, frame: &[u8]) -> Result<(), LinkError> {
        let header = checked_length(frame.len())?.to_be_bytes();
        let result = wait(async {
            let mut closed = self.closed.subscribe();
            tokio::select! {
                biased;
                _ = closed.wait_for(|closed| *closed) => Err(LinkError::Closed),
                result = async {
                    let mut send = self.send.lock().await;
                    send.write_all(&header).await.map_err(|_| LinkError::Closed)?;
                    send.write_all(frame).await.map_err(|_| LinkError::Closed)
                } => result,
            }
        });
        if result.is_err() {
            self.abort();
        }
        result
    }

    fn mark_closed(&self) -> bool {
        self.closed
            .send_if_modified(|closed| !std::mem::replace(closed, true))
    }

    fn abort(&self) {
        if self.mark_closed() {
            self.connection.close(0u8.into(), b"");
        }
    }

    fn close(self: &Arc<Self>) -> Result<(), LinkError> {
        if self.mark_closed() {
            let shared = Arc::clone(self);
            runtime()?.spawn(async move {
                // Close with a stream FIN so queued frames, including the last
                // table message, can reach the peer before the connection closes.
                let mut send = shared.send.lock().await;
                let _ = send.finish();
                drop(send);
                let _ = tokio::time::timeout(DRAIN_TIMEOUT, shared.connection.closed()).await;
                shared.connection.close(0u8.into(), b"");
            });
        }
        Ok(())
    }
}

impl LinkSender for Sender {
    fn send(&mut self, frame: &[u8]) -> Result<(), LinkError> {
        self.0.send(frame)
    }

    fn close(&self) -> Result<(), LinkError> {
        self.0.close()
    }
}

async fn read_frame(receive: &mut RecvStream) -> Result<Vec<u8>, LinkError> {
    let mut header = [0; 4];
    receive
        .read_exact(&mut header)
        .await
        .map_err(|_| LinkError::Closed)?;
    let length = u32::from_be_bytes(header) as usize;
    checked_length(length)?;
    let mut frame = vec![0; length];
    receive
        .read_exact(&mut frame)
        .await
        .map_err(|_| LinkError::Closed)?;
    Ok(frame)
}

fn checked_length(length: usize) -> Result<u32, LinkError> {
    if length > MAX_FRAME_SIZE {
        return Err(LinkError::Refused("The frame exceeds 8 MiB.".into()));
    }
    Ok(length as u32)
}
