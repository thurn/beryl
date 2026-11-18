//! The `ignite` command: parses the verb, then answers it locally, sends it to the hub (ticket
//! verbs, which work while the daemon is down) or sends it to the machine's daemon.

mod args;
mod client;
mod manifest;
mod plan;
mod replay;
mod ticket;

use std::process::ExitCode;

use clap::Parser;

use ignite_core::protocol::client::Reply;

use crate::args::{Cli, Command};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let raw: Vec<String> = std::env::args().collect();
    let reply = answer(&cli, &raw);
    if cli.json {
        let json = reply
            .json
            .clone()
            .unwrap_or_else(|| serde_json::json!({"text": reply.text}));
        println!("{json}");
    } else if !reply.text.is_empty() {
        println!("{}", reply.text.trim_end());
    }
    ExitCode::from(reply.exit.code())
}

fn answer(cli: &Cli, raw: &[String]) -> Reply {
    let verb = cli.command.verb();
    if let Some(reply) = client::not_built(verb) {
        return reply;
    }
    match &cli.command {
        Command::Ticket { command } => ticket::run(command),
        Command::Plan { command } => plan::run(command),
        Command::Manifest { command } => manifest::run(command, &current_dir()),
        Command::Gate { command } => replay::run(command),
        Command::Daemon => exit_reply(ignite_daemon::main::run_daemon(&current_dir())),
        Command::Supervise { spec } => exit_reply(ignite_daemon::supervise::supervise(spec)),
        Command::RustcWrap(pass) => exit_reply(ignite_daemon::wrap::rustc_wrap(&pass.args)),
        Command::Runner(pass) => exit_reply(ignite_daemon::wrap::runner(&pass.args)),
        _ => {
            let session = client::session_from_env(|name| std::env::var(name).ok());
            client::send(&client::request(verb, raw, current_dir(), session))
        }
    }
}

fn current_dir() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_default()
}

/// A reply carrying only an exit code from a hidden subcommand, which prints its own output.
fn exit_reply(code: i32) -> Reply {
    use ignite_core::protocol::client::Exit;
    let exit = u8::try_from(code)
        .ok()
        .and_then(|c| Exit::try_from(c).ok())
        .unwrap_or(Exit::Usage);
    Reply::text(exit, "")
}
