//! Telling the hub about a command line the parser refused, so the `rejected-command`
//! improvement rule can find the verbs and flags agents look for and do not find.
//!
//! The report never changes what the user sees: the parser's error prints and the command exits
//! with the parser's code whether or not the hub answers. It waits at most [`TIMEOUT`] for the
//! hub, and sends nothing outside a checkout with an `ignite.toml` or without a write token.

use std::time::Duration;

use clap::CommandFactory;
use clap::error::{ContextKind, ContextValue, ErrorKind};

use ignite_core::ids::SessionId;
use ignite_core::protocol::client::Exit;
use ignite_core::protocol::hub::RejectedCommand;

use crate::args::Cli;
use crate::client::session_from_env;
use crate::ticket::Hub;

/// The longest the report may hold up the parser's error.
const TIMEOUT: Duration = Duration::from_secs(2);

/// Prints the parser's error and exits, after reporting it to the hub when it is a verb or flag
/// the executable does not have.
pub fn exit(error: &clap::Error, raw: &[String]) -> ! {
    let env = |name: &str| std::env::var(name).ok();
    let session = session_from_env(env);
    if let Some(command) = describe(error, raw, session.clone()) {
        let actor = session.map_or_else(|| "ignite".to_owned(), |s| s.to_string());
        let cwd = std::env::current_dir().unwrap_or_default();
        if let Ok(hub) = Hub::load(&cwd, actor, env) {
            send(&hub, &command);
        }
    }
    if let Some(hint) = owner_replacement(error) {
        let _ = error.print();
        eprintln!("{hint}");
        std::process::exit(i32::from(Exit::Usage.code()));
    }
    error.exit()
}

pub(crate) fn owner_replacement(error: &clap::Error) -> Option<&'static str> {
    match error.get(ContextKind::InvalidArg) {
        Some(ContextValue::String(arg))
            if error.kind() == ErrorKind::UnknownArgument
                && arg.split('=').next() == Some("--owner") =>
        {
            Some("Use machine:<name> and provider:<name> labels to pin a ticket.")
        }
        _ => None,
    }
}

/// Sends `command` once, and ignores whatever happens.
pub fn send(hub: &Hub, command: &RejectedCommand) {
    if let Ok(body) = serde_json::to_value(command) {
        hub.post_once(&["commands", "rejected"], &body, TIMEOUT);
    }
}

/// The report for `error`, if it rejected a subcommand or a flag. `raw` is the whole command
/// line, the executable's name first.
pub fn describe(
    error: &clap::Error,
    raw: &[String],
    session: Option<SessionId>,
) -> Option<RejectedCommand> {
    let (kind, word, suggested) = match error.kind() {
        ErrorKind::InvalidSubcommand => (
            "unknown subcommand",
            ContextKind::InvalidSubcommand,
            ContextKind::SuggestedSubcommand,
        ),
        ErrorKind::UnknownArgument => (
            "unknown argument",
            ContextKind::InvalidArg,
            ContextKind::SuggestedArg,
        ),
        _ => return None,
    };
    let rejected = match error.get(word)? {
        ContextValue::String(word) => word.clone(),
        _ => return None,
    };
    let args: Vec<String> = raw.iter().skip(1).cloned().collect();
    let mut verb = subcommand_path(&args, &rejected);
    // A flag given a value with `=` is reported by its name alone.
    verb.push(rejected.split('=').next().unwrap_or(&rejected).to_owned());
    let suggestion = match error.get(suggested) {
        Some(ContextValue::String(text)) => Some(text.clone()),
        Some(ContextValue::Strings(texts)) => texts.first().cloned(),
        _ => None,
    };
    Some(RejectedCommand {
        verb: verb.join(" "),
        args,
        kind: kind.to_owned(),
        suggestion,
        session,
    })
}

/// The names of the subcommands among `args` before `rejected`, such as `ticket dep`, with each
/// alias spelled as the subcommand it stands for.
fn subcommand_path(args: &[String], rejected: &str) -> Vec<String> {
    let mut command = Cli::command();
    let mut path = vec![];
    for arg in args {
        if arg == rejected {
            break;
        }
        let Some(sub) = command.find_subcommand(arg).cloned() else {
            continue;
        };
        path.push(sub.get_name().to_owned());
        command = sub;
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn describe_line(line: &str) -> Option<RejectedCommand> {
        let raw: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
        let error = Cli::try_parse_from(&raw).expect_err("a usage error");
        describe(&error, &raw, None)
    }

    #[test]
    fn an_unknown_subcommand_is_named_after_the_subcommands_before_it() {
        let command = describe_line("ignite ticket dep frob Ta.1").unwrap();
        assert_eq!(command.verb, "ticket dep frob");
        assert_eq!(command.kind, "unknown subcommand");
        assert_eq!(command.args, ["ticket", "dep", "frob", "Ta.1"]);
    }

    #[test]
    fn an_unknown_flag_is_named_after_its_subcommands_without_its_value() {
        let command = describe_line("ignite ticket children Ta --closed=yes").unwrap();
        assert_eq!(command.verb, "ticket children --closed");
        assert_eq!(command.kind, "unknown argument");
    }

    #[test]
    fn the_parsers_suggestion_is_kept() {
        let command = describe_line("ignite ticket shwo Ta").unwrap();
        assert_eq!(command.suggestion.as_deref(), Some("show"));
    }

    #[test]
    fn a_missing_argument_is_not_reported() {
        assert!(describe_line("ignite ticket show").is_none());
    }
}
