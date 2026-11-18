//! Write authorization: `Authorization: Bearer <token>` names a registered machine, and `X-Actor` the
//! session or person acting.
//!
//! A token is registered when its machine registers with the hub, which keeps only its SHA-256
//! (`ignite_store::hub::tokens`). Reads need no token; each route that writes refuses a request
//! without a [`Writer`] with HTTP 401.
//!
//! A token stands for its machine only: a route whose body names a machine refuses, with HTTP
//! 403, a token issued to another one (`routes::dispatch`). The `X-Actor` session or person is
//! taken on trust, a risk accepted because the hub is reachable only on the private network.

use rusqlite::Connection;

use ignite_core::ids::MachineId;
use ignite_store::hub::tokens;

/// Who is writing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Writer {
    pub machine: MachineId,
    pub actor: String,
}

/// Why a write was refused (HTTP 401).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unauthorized(pub String);

/// Checks the two headers of a write. The scheme name is matched without regard to case, as the
/// ticket service did. A known token without an `X-Actor` is refused too: every write records who
/// made it.
pub fn authorize(
    conn: &Connection,
    authorization: Option<&str>,
    actor: Option<&str>,
) -> Result<Writer, Unauthorized> {
    let unknown = || Unauthorized("Missing or unknown token".to_owned());
    let token = authorization
        .and_then(|value| value.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .map(|(_, token)| token.trim())
        .filter(|token| !token.is_empty() && !token.contains(char::is_whitespace))
        .ok_or_else(unknown)?;
    let machine = tokens::verify(conn, token)
        .map_err(|error| Unauthorized(format!("token check failed: {error}")))?
        .ok_or_else(unknown)?;
    let actor = actor
        .map(str::trim)
        .filter(|actor| !actor.is_empty())
        .ok_or_else(|| Unauthorized("Missing X-Actor".to_owned()))?;
    Ok(Writer {
        machine,
        actor: actor.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignite_store::hub;

    #[test]
    fn a_registered_token_with_an_actor_names_the_writer() {
        let conn = hub::open_in_memory().unwrap();
        let machine: MachineId = "m-1".parse().unwrap();
        let token = tokens::issue(&conn, &machine).unwrap();
        let writer = authorize(&conn, Some(&format!("bearer {token}")), Some(" s1 ")).unwrap();
        assert_eq!(
            writer,
            Writer {
                machine,
                actor: "s1".to_owned()
            }
        );
    }

    #[test]
    fn missing_unknown_or_malformed_headers_are_refused() {
        let conn = hub::open_in_memory().unwrap();
        let token = tokens::issue(&conn, &"m-1".parse().unwrap()).unwrap();
        let bearer = format!("Bearer {token}");
        for (authorization, actor) in [
            (None, Some("s1")),
            (Some("Bearer unknown"), Some("s1")),
            (Some(token.as_str()), Some("s1")),
            (Some("Basic abc"), Some("s1")),
            (Some(bearer.as_str()), None),
            (Some(bearer.as_str()), Some("  ")),
        ] {
            assert!(
                authorize(&conn, authorization, actor).is_err(),
                "{authorization:?} {actor:?}"
            );
        }
    }
}
