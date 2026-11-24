use std::{
    fmt,
    hash::{BuildHasher, RandomState},
    str::FromStr,
};

use crate::LinkError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invite {
    Local {
        port: u16,
        secret: String,
    },
    /// The complete remote code, including the `beryl:` prefix.
    Remote(String),
}

impl fmt::Display for Invite {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local { port, secret } => write!(formatter, "beryl-local:{port}:{secret}"),
            Self::Remote(code) => formatter.write_str(code),
        }
    }
}

impl FromStr for Invite {
    type Err = LinkError;

    fn from_str(code: &str) -> Result<Self, Self::Err> {
        if code.starts_with("beryl:") {
            return Ok(Self::Remote(code.into()));
        }
        let local = code.strip_prefix("beryl-local:").ok_or_else(|| {
            LinkError::Refused("The invite must start with beryl-local: or beryl:.".into())
        })?;
        let (port, secret) = local.split_once(':').ok_or_else(|| {
            LinkError::Refused("The local invite needs a port and a secret.".into())
        })?;
        let port = port
            .parse::<u16>()
            .ok()
            .filter(|&port| port != 0)
            .filter(|_| port.bytes().all(|byte| byte.is_ascii_digit()))
            .ok_or_else(|| LinkError::Refused("The local invite has an invalid port.".into()))?;
        if secret.is_empty() || !secret.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return Err(LinkError::Refused(
                "The local invite has an invalid secret.".into(),
            ));
        }
        Ok(Self::Local {
            port,
            secret: secret.into(),
        })
    }
}

/// Generate the eight characters for a local invite.
pub fn secret() -> String {
    // Use the standard library's randomly seeded hasher so local invites
    // need no random-number dependency.
    const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
    let bits = RandomState::new().hash_one(0_u8);
    (0..8)
        .map(|index| char::from(ALPHABET[((bits >> (5 * index)) & 31) as usize]))
        .collect()
}
