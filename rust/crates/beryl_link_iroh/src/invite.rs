use std::net::{Ipv4Addr, SocketAddr};

use beryl_link::{Invite, LinkError};
use data_encoding::BASE32_NOPAD;
use iroh::{EndpointAddr, EndpointId, RelayUrl};

const PREFIX: &str = "beryl:";
const VERSION: u8 = 1;
const KEY_BYTES: usize = 32;
const SECRET_BYTES: usize = 16;
const MAX_RELAY_BYTES: usize = 2048;

pub(crate) enum Address {
    Relay(RelayUrl),
    Loopback(u16),
}

/// The address and table secret carried by a remote invite.
pub struct RemoteInvite {
    key: EndpointId,
    address: Address,
    secret: [u8; SECRET_BYTES],
}

impl RemoteInvite {
    pub(crate) fn new(key: EndpointId, address: Address) -> Result<Self, LinkError> {
        let mut secret = [0; SECRET_BYTES];
        getrandom::fill(&mut secret).map_err(crate::io_error)?;
        Ok(Self {
            key,
            address,
            secret,
        })
    }

    pub fn decode(invite: &Invite) -> Result<Self, LinkError> {
        let Invite::Remote(code) = invite else {
            return Err(LinkError::Refused(
                "This connection needs a remote invite.".into(),
            ));
        };
        let code = code.strip_prefix(PREFIX).ok_or_else(invalid)?;
        // One version byte, key, secret, one address tag, then address bytes.
        let max_bytes = 1 + KEY_BYTES + SECRET_BYTES + 1 + MAX_RELAY_BYTES;
        if code.len() > (max_bytes * 8).div_ceil(5)
            || !code
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || (b'2'..=b'7').contains(&byte))
        {
            return Err(invalid());
        }
        let bytes = BASE32_NOPAD
            .decode(code.to_ascii_uppercase().as_bytes())
            .map_err(|_| invalid())?;
        let [VERSION, rest @ ..] = bytes.as_slice() else {
            return Err(invalid());
        };
        let (key, rest) = rest.split_first_chunk::<KEY_BYTES>().ok_or_else(invalid)?;
        let (secret, rest) = rest
            .split_first_chunk::<SECRET_BYTES>()
            .ok_or_else(invalid)?;
        let address = match rest {
            [0, high, low] if (*high, *low) != (0, 0) => {
                Address::Loopback(u16::from_be_bytes([*high, *low]))
            }
            [1, url @ ..] => {
                let url = std::str::from_utf8(url).map_err(|_| invalid())?;
                Address::Relay(relay_url(url).ok_or_else(invalid)?)
            }
            _ => return Err(invalid()),
        };
        Ok(Self {
            key: EndpointId::from_bytes(key).map_err(|_| invalid())?,
            address,
            secret: *secret,
        })
    }

    pub fn encode(&self) -> Invite {
        let mut bytes = vec![VERSION];
        bytes.extend_from_slice(self.key.as_bytes());
        bytes.extend_from_slice(&self.secret);
        match &self.address {
            Address::Loopback(port) => {
                bytes.push(0);
                bytes.extend_from_slice(&port.to_be_bytes());
            }
            Address::Relay(url) => {
                bytes.push(1);
                bytes.extend_from_slice(url.as_str().as_bytes());
            }
        }
        Invite::Remote(format!(
            "{PREFIX}{}",
            BASE32_NOPAD.encode(&bytes).to_ascii_lowercase()
        ))
    }

    pub fn endpoint_id(&self) -> EndpointId {
        self.key
    }

    pub fn relay(&self) -> Option<&RelayUrl> {
        match &self.address {
            Address::Relay(url) => Some(url),
            Address::Loopback(_) => None,
        }
    }

    /// Pass this value to the table's Hello handshake.
    pub fn secret(&self) -> String {
        BASE32_NOPAD.encode(&self.secret).to_ascii_lowercase()
    }

    pub(crate) fn endpoint_addr(&self) -> EndpointAddr {
        let address = EndpointAddr::new(self.key);
        match &self.address {
            Address::Relay(url) => address.with_relay_url(url.clone()),
            Address::Loopback(port) => {
                address.with_ip_addr(SocketAddr::from((Ipv4Addr::LOCALHOST, *port)))
            }
        }
    }
}

pub(crate) fn relay_url(text: &str) -> Option<RelayUrl> {
    let url = text.parse::<RelayUrl>().ok()?;
    (matches!(url.scheme(), "http" | "https")
        && url.host().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.as_str().len() <= MAX_RELAY_BYTES)
        .then_some(url)
}

fn invalid() -> LinkError {
    LinkError::Refused("The Beryl remote invite is invalid.".into())
}
