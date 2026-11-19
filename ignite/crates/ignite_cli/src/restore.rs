//! `ignite hub restore --from-mirror`: reads this machine's ticket mirror and sends the hub its
//! tickets through `POST /import` (see `ignite_daemon::mirror::restore` for what is sent). It
//! runs here, without the daemon, with the machine's token, so it works on any machine that has a
//! mirror, including while the daemon is down.

use std::fmt::Write as _;

use ignite_core::protocol::client::{Exit, Reply};
use ignite_core::protocol::tickets::ImportReply;
use ignite_core::time::Timestamp;
use ignite_core::traits::Error;
use ignite_daemon::mirror::{self, restore};
use serde_json::json;

use crate::client::session_from_env;
use crate::ticket::{Failure, Hub};

pub fn run(at: Option<&str>, actor: Option<&str>, dry_run: bool) -> Reply {
    let at = match at.map(Timestamp::parse).transpose() {
        Ok(at) => at,
        Err(error) => {
            return Reply::text(
                Exit::Usage,
                format!("--at {error}: give a UTC time as YYYY-MM-DDTHH:MM:SSZ"),
            );
        }
    };
    let path = ignite_daemon::update::Home::from_env()
        .root
        .join(mirror::FILE);
    if !path.exists() {
        return Reply::text(
            Exit::Usage,
            format!(
                "no ticket mirror at {}: this machine's daemon keeps one once it runs",
                path.display()
            ),
        );
    }
    let plan = match ignite_store::mirror::open(&path)
        .map_err(|e| Error::Io(e.to_string()))
        .and_then(|conn| restore::plan(&conn, at, actor))
    {
        Ok(plan) => plan,
        Err(error) => return Reply::text(Exit::Usage, error.to_string()),
    };
    let mut text = describe(&plan, at, actor);
    if dry_run {
        text.push_str("dry run: nothing was sent\n");
        return Reply::text(Exit::Success, text);
    }
    let env = |name: &str| std::env::var(name).ok();
    let writer = session_from_env(env).map_or_else(|| "ignite".to_owned(), |s| s.to_string());
    let cwd = std::env::current_dir().unwrap_or_default();
    let hub = match Hub::load(&cwd, writer, env) {
        Ok(hub) => hub,
        Err(failure) => return failure.into_reply(),
    };
    // A failed request keeps its own exit code (4 for an unreachable hub).
    let mut failed: Option<Failure> = None;
    let sent = restore::send(&plan, |request| {
        // An import that is repeated writes the same content again, so a retry is harmless.
        let reply = hub
            .post(&["import"], &json!(request), true)
            .and_then(|response| response.json());
        match reply {
            Ok(value) => serde_json::from_value::<ImportReply>(value)
                .map_err(|e| Error::Invalid(format!("/import: bad reply: {e}"))),
            Err(failure) => {
                let error = Error::Io(failure.message.clone());
                failed = Some(failure);
                Err(error)
            }
        }
    });
    match (sent, failed) {
        (Ok(imported), _) => {
            let _ = writeln!(text, "the hub changed {imported} tickets");
            Reply::text(Exit::Success, text)
        }
        (Err(_), Some(failure)) => failure.into_reply(),
        (Err(error), None) => Reply::text(Exit::Unreachable, error.to_string()),
    }
}

/// What the plan sends, in a few lines.
fn describe(plan: &restore::Plan, at: Option<Timestamp>, actor: Option<&str>) -> String {
    let mut text = match at {
        Some(at) => format!(
            "rolling {} tickets back to {at}{}, replacing later copies\n",
            plan.tickets.len(),
            actor.map_or_else(String::new, |actor| format!(" that {actor} changed since"))
        ),
        None => format!(
            "sending the newest version of {} tickets; the hub keeps any copy it updated later\n",
            plan.tickets.len()
        ),
    };
    if !plan.created_later.is_empty() {
        let ids: Vec<&str> = plan
            .created_later
            .iter()
            .map(ignite_core::ids::TicketId::as_str)
            .collect();
        let _ = writeln!(
            text,
            "left as they are, since they were created later: {}",
            ids.join(", ")
        );
    }
    text
}
