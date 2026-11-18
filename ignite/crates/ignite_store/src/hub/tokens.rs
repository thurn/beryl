//! Per-machine write tokens. A machine gets a token when it registers and sends it as a bearer
//! token on every write. The hub keeps only each token's SHA-256, in `meta` under
//! `token_hashes`, as a JSON object from hash to machine: the shape the ticket service used for
//! its registered machines, so its hashes carry over at the switch.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use rusqlite::{Connection, OptionalExtension};
use sha2::{Digest, Sha256};

use ignite_core::ids::MachineId;

use crate::{Result, StoreError};

/// Issues a new token for `machine`, replacing any earlier one, and returns it in clear. The
/// hub never stores it in clear, so this is the only time anyone sees it.
pub fn issue(conn: &Connection, machine: &MachineId) -> Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| StoreError::Invalid(format!("no random numbers: {error}")))?;
    let token = hex(&bytes);
    let mut hashes = load(conn)?;
    hashes.retain(|_, owner| owner != machine.as_str());
    hashes.insert(hash(&token), machine.as_str().to_owned());
    let text = serde_json::to_string(&hashes)
        .map_err(|error| StoreError::Invalid(format!("token_hashes: {error}")))?;
    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('token_hashes', ?1) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [text],
    )?;
    Ok(token)
}

/// The machine a token belongs to, or `None` for a token the hub never issued or has replaced.
pub fn verify(conn: &Connection, token: &str) -> Result<Option<MachineId>> {
    let Some(machine) = load(conn)?.remove(&hash(token)) else {
        return Ok(None);
    };
    machine
        .parse()
        .map(Some)
        .map_err(|error| StoreError::Invalid(format!("token_hashes: {error}")))
}

fn load(conn: &Connection) -> Result<BTreeMap<String, String>> {
    let text: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'token_hashes'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    match text {
        None => Ok(BTreeMap::new()),
        Some(text) => serde_json::from_str(&text).map_err(|error| {
            StoreError::Invalid(format!("meta.token_hashes is not an object: {error}"))
        }),
    }
}

/// The lowercase hex SHA-256 of a token, as the ticket service stored it.
fn hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::open_in_memory;

    fn machine(name: &str) -> MachineId {
        name.parse().unwrap()
    }

    #[test]
    fn an_issued_token_names_its_machine_and_nothing_else_does() {
        let conn = open_in_memory().unwrap();
        let first = issue(&conn, &machine("m-1")).unwrap();
        let second = issue(&conn, &machine("m-2")).unwrap();
        assert_ne!(first, second);
        assert_eq!(verify(&conn, &first).unwrap(), Some(machine("m-1")));
        assert_eq!(verify(&conn, &second).unwrap(), Some(machine("m-2")));
        assert_eq!(verify(&conn, "unknown").unwrap(), None);
        let stored: String = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'token_hashes'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            !stored.contains(&first),
            "the token is stored only as a hash"
        );
    }

    #[test]
    fn issuing_again_replaces_the_machines_earlier_token() {
        let conn = open_in_memory().unwrap();
        let old = issue(&conn, &machine("m-1")).unwrap();
        let new = issue(&conn, &machine("m-1")).unwrap();
        assert_eq!(verify(&conn, &old).unwrap(), None);
        assert_eq!(verify(&conn, &new).unwrap(), Some(machine("m-1")));
    }

    #[test]
    fn hashes_match_the_ticket_service() {
        // The ticket service hashed tokens as lowercase hex SHA-256 of their UTF-8 bytes.
        assert_eq!(
            hash("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
