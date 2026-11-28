//! Reading the arguments of the ticket verbs, and the reply for a failure behind a trait.

use std::collections::BTreeMap;

use ignite_core::ids::{SessionId, TicketId};
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::traits::Error;

use super::{NEEDS_PERSON, Target};

/// The reply for a failure behind one of the traits, with the exit code its kind calls for.
pub(super) fn failure(error: &Error, what: &str) -> Reply {
    let exit = match error {
        Error::Unreachable(_) => Exit::Unreachable,
        Error::Conflict(_) => Exit::Conflict,
        Error::NotFound(_) | Error::Invalid(_) | Error::Io(_) => Exit::Usage,
    };
    Reply::text(exit, format!("failed   {what}: {error}"))
}

pub(super) fn usage(text: &str) -> Reply {
    Reply::text(Exit::Usage, format!("usage    {text}"))
}

pub(super) fn session_of(req: &Request) -> std::result::Result<SessionId, Reply> {
    req.session.clone().ok_or_else(|| {
        usage("no session: run this from a desktop-app session, or set IGNITE_SESSION")
    })
}

/// Reads `--name value` and `--name=value` options (and `-m value` for `--message`) and the
/// positional words, rejecting any other option.
pub(super) fn options(
    args: &[String],
    known: &[(&str, Option<&str>)],
) -> std::result::Result<(BTreeMap<String, String>, Vec<String>), Reply> {
    let mut values = BTreeMap::new();
    let mut words = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if !arg.starts_with('-') || arg == "-" {
            words.push(arg.clone());
            continue;
        }
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) => (flag, Some(value.to_owned())),
            None => (arg.as_str(), None),
        };
        let Some((name, _)) = known
            .iter()
            .find(|(long, short)| flag == format!("--{long}") || Some(flag) == *short)
        else {
            return Err(usage(&format!("unknown option {flag}")));
        };
        let Some(value) = inline.or_else(|| iter.next().cloned()) else {
            return Err(usage(&format!("{flag} needs a value")));
        };
        values.insert((*name).to_owned(), value);
    }
    Ok((values, words))
}

pub(super) fn parse_start(req: &Request) -> std::result::Result<(Target, SessionId), Reply> {
    let (mut values, words) = options(&req.args, &[("reservation", None)])?;
    let session = session_of(req)?;
    let text = words.join(" ");
    let code = values.remove("reservation");
    if text.trim().is_empty() {
        return Err(usage(
            "ignite start <ticket> [--reservation <code>] | ignite start \"<summary>\"",
        ));
    }
    // A summary is several words; a ticket ID is one.
    if text.contains(char::is_whitespace) {
        if code.is_some() {
            return Err(usage("--reservation goes with a ticket ID, not a summary"));
        }
        return Ok((Target::Adhoc(text.trim().to_owned()), session));
    }
    match TicketId::new(text.clone()) {
        Ok(id) => Ok((Target::Ticket { id, code }, session)),
        Err(error) => Err(usage(&format!(
            "{error}; quote a summary of several words to file a ticket"
        ))),
    }
}

pub(super) fn parse_release(req: &Request) -> std::result::Result<(String, Option<String>), Reply> {
    let (mut values, words) = options(&req.args, &[("message", Some("-m")), ("needs", None)])?;
    if !words.is_empty() {
        return Err(usage("ignite release -m \"<note>\" [--needs <what>]"));
    }
    let note = values.remove("message").unwrap_or_default();
    let needs = values.remove("needs");
    if needs.as_deref() == Some(NEEDS_PERSON) && note.trim().is_empty() {
        return Err(usage(
            "--needs person needs a note (-m) stating the exact need and what was tried",
        ));
    }
    Ok((note, needs))
}

/// The reason of `ignite close`, and whether it passed `--allow-off-checks`
/// (see [`super::off_checks`]).
pub(super) fn parse_close(req: &Request) -> std::result::Result<(String, bool), Reply> {
    let mut args = req.args.clone();
    let allow = super::off_checks::take_allow(&mut args);
    let (mut values, words) = options(&args, &[("message", Some("-m"))])?;
    match values.remove("message") {
        Some(reason) if words.is_empty() && !reason.trim().is_empty() => Ok((reason, allow)),
        _ => Err(usage("ignite close -m \"<reason>\" [--allow-off-checks]")),
    }
}
