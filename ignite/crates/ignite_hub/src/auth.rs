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
//!
//! The dashboard's edit controls write without a token. On the private listener, `tailscale
//! serve` replaces any `Tailscale-User-Login` header a client sent with the login of the
//! Tailscale user making the request, and Funnel requests from the public internet carry none,
//! so the header names a person on the private network ([`dashboard_editor`]). A browser sends
//! [`DASHBOARD_HEADER`] with each edit: a page on another site cannot add a custom header to a
//! cross-origin request without a CORS preflight, which the hub never approves, so the header
//! stops cross-site request forgery.

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

/// The header the dashboard's script sends with every edit (see the module comment).
pub const DASHBOARD_HEADER: &str = "x-ignite-dashboard";

/// The machine a dashboard edit is recorded on.
pub const DASHBOARD_MACHINE: &str = "dashboard";

/// The name a dashboard edit is recorded under, from the `Tailscale-User-Login` header: the part
/// of the login before any `@`, so the changelog, which the public listener shows, never shows
/// an email address. `None` without a login.
pub fn dashboard_editor(login: Option<&str>) -> Option<String> {
    let login = login.map(str::trim).filter(|login| !login.is_empty())?;
    let name = login.split('@').next().unwrap_or(login);
    (!name.is_empty() && !name.contains(char::is_whitespace)).then(|| name.to_owned())
}

/// The writer of a dashboard edit: a Tailscale user (`login`) whose request carries
/// [`DASHBOARD_HEADER`] (`marker`). The caller allows this only on the private listener and only
/// for ticket edits.
pub fn dashboard_writer(login: Option<&str>, marker: Option<&str>) -> Option<Writer> {
    marker?;
    Some(Writer {
        machine: DASHBOARD_MACHINE.parse().ok()?,
        actor: dashboard_editor(login)?,
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

    #[test]
    fn a_dashboard_edit_needs_a_tailscale_login_and_the_marker_header() {
        let writer = dashboard_writer(Some("someone@example.com"), Some("1")).unwrap();
        assert_eq!(writer.actor, "someone");
        assert_eq!(writer.machine.as_str(), DASHBOARD_MACHINE);
        assert_eq!(dashboard_writer(Some("someone@example.com"), None), None);
        assert_eq!(dashboard_writer(None, Some("1")), None);
        assert_eq!(dashboard_writer(Some("  "), Some("1")), None);
        assert_eq!(dashboard_editor(Some("@example.com")), None);
    }
}
