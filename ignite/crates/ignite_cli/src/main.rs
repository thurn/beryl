//! The `ignite` command: parses the verb, then answers it locally, sends it to the hub (ticket
//! verbs, which work while the daemon is down) or sends it to the machine's daemon.

mod args;
mod client;
mod dispatch_allowed;
mod emergency;
mod fallback;
mod flaky;
mod import;
mod manifest;
mod plan;
mod pool_init;
mod reattach;
mod rejected;
mod replay;
mod restore;
mod ticket;
mod values;

use std::process::ExitCode;

use clap::Parser;

use ignite_core::protocol::client::Reply;

use crate::args::{Cli, Command, DispatchCommand, HubCommand, InternalCommand, PoolCommand};

fn main() -> ExitCode {
    let raw = with_shim_verb(std::env::args().collect());
    let cli = Cli::try_parse_from(&raw).unwrap_or_else(|error| rejected::exit(&error, &raw));
    let reply = answer(&cli, &raw);
    if cli.json && reply.json.is_none() && reply.text.is_empty() {
        // A command that printed its own output has nothing to wrap.
    } else if cli.json {
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

/// Cargo runs the compiler wrapper and the test runner as one program with no verb in front of
/// its arguments. On Windows the shims are copies of this executable named `rustc-wrap.exe`
/// and `test-runner.exe` (`ignite_daemon::wrap`), so a call by one of those names gets the
/// matching verb inserted after the program name.
fn with_shim_verb(mut raw: Vec<String>) -> Vec<String> {
    // Split on both separators by hand: `Path` only knows the host's separator, so a Windows path
    // would not be recognized when this runs elsewhere (as the unit test does).
    let stem = raw.first().map(|program| {
        let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
        name.strip_suffix(".exe").unwrap_or(name)
    });
    let verb = match stem {
        Some("rustc-wrap") => "rustc-wrap",
        Some("test-runner") => "runner",
        _ => return raw,
    };
    raw.insert(1, verb.to_owned());
    raw
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
        Command::Import { .. } => import::run(&cli.command, &current_dir()),
        Command::Gate { command } => replay::run(command, &current_dir()),
        // Answered here from the hub, like the ticket verbs.
        Command::Flaky { command } => flaky::run(command),
        // Answered here, so a worker can read the `machine` topic while the daemon is down.
        Command::Guide { topic } => ignite_core::guide::reply(topic.as_deref()),
        // Answered here from the nearest `ignite.toml`, so it works while the daemon is down.
        Command::Prime => prime(&current_dir()),
        // Answered here: no daemon runs before the first install.
        Command::Install => install(&current_dir()),
        // Answered here: it only writes files, and works before the daemon runs.
        Command::Pool {
            command: PoolCommand::Init { dir },
        } => pool_init(dir.as_deref(), &current_dir()),
        // Sent to the daemon here, so that a hook's answer can fall back on the client's own
        // hand-off while the daemon is down and block the call on any failure.
        Command::Dispatch {
            command: Some(DispatchCommand::Allowed { tool, session }),
            ..
        } => dispatch_allowed::run(tool.as_deref(), session.as_deref()),
        // Sent to the daemon here, so that the hook reads its input from stdin.
        Command::Dispatch {
            command: Some(DispatchCommand::Cleared { session }),
            ..
        } => dispatch_allowed::cleared(session.as_deref()),
        // Answered here, without the daemon: it is what looks after a daemon that is down.
        Command::Monitor => ignite_daemon::monitor::run(),
        Command::Daemon => exit_reply(ignite_daemon::main::run_daemon(&current_dir())),
        // Answered here: it is the hub, which runs beside the daemon, not in it.
        Command::Hub { command: None } => exit_reply(ignite_daemon::main::run_hub(&current_dir())),
        // Answered here from this machine's mirror, so it works while the daemon is down.
        Command::Hub {
            command:
                Some(HubCommand::Restore {
                    at, actor, dry_run, ..
                }),
        } => restore::run(at.as_deref(), actor.as_deref(), *dry_run),
        Command::Supervise { spec } => exit_reply(ignite_daemon::supervise::supervise(spec)),
        Command::RustcWrap(pass) => exit_reply(ignite_daemon::wrap::rustc_wrap(&pass.args)),
        Command::Runner(pass) => exit_reply(ignite_daemon::wrap::runner(&pass.args)),
        Command::Internal {
            command: InternalCommand::Selftest,
        } => exit_reply(ignite_daemon::update::selftest::command(&current_dir())),
        Command::Internal {
            command: InternalCommand::Verb { handle },
        } => exit_reply(ignite_daemon::verbproc::run(handle, &current_dir())),
        Command::Internal {
            command: InternalCommand::Restart,
        } => restart(current_dir()),
        _ => {
            let session = client::session_from_env(|name| std::env::var(name).ok())
                .filter(|_| !client::UNRECORDED_VERBS.contains(&verb));
            let mut request = client::request(verb, raw, current_dir(), session);
            if let Command::Dispatch {
                command: None,
                wait,
            } = &cli.command
            {
                request.wait_s = client::dispatch_request_wait_s(*wait);
            }
            if verb == "dispatch" {
                // The dispatcher keeps calling while the daemon is down; see `fallback`.
                let home = ignite_daemon::update::Home::from_env().root;
                let now = ignite_core::traits::Clock::now(&ignite_core::time::SystemClock);
                client::send_or(&request, |r| fallback::dispatch(r, &home, now))
            } else if !cli.json && client::SHOWS_STEPS.contains(&verb) {
                client::send_showing_steps(&request)
            } else {
                client::send(&request)
            }
        }
    }
}

fn install(cwd: &std::path::Path) -> Reply {
    match config_above(cwd) {
        Ok((repo, config)) => ignite_daemon::update::install_checkout(&repo, &config),
        Err(failure) => failure.into_reply(),
    }
}

/// `ignite pool init`. Workers start in the main checkout that `ignite install` recorded, or,
/// before any install, in the checkout around `cwd`.
fn pool_init(dir: Option<&std::path::Path>, cwd: &std::path::Path) -> Reply {
    let recorded = std::fs::read_to_string(ignite_daemon::update::Home::from_env().checkout_file())
        .ok()
        .map(|text| std::path::PathBuf::from(text.trim()))
        .filter(|path| path.is_dir());
    let checkout = match recorded {
        Some(checkout) => checkout,
        None => match config_above(cwd) {
            Ok((repo, _)) => repo,
            Err(failure) => return failure.into_reply(),
        },
    };
    let folder = dir.map_or_else(pool_init::default_folder, std::path::Path::to_path_buf);
    pool_init::run(&folder, &checkout)
}

/// The checkout holding the nearest `ignite.toml` above `cwd`, and the file parsed.
fn config_above(
    cwd: &std::path::Path,
) -> ticket::http::Result<(std::path::PathBuf, ignite_core::config::Config)> {
    let (file, text) = ticket::http::find_config(cwd)?;
    let config = ignite_core::config::Config::parse(&file.display().to_string(), &text)
        .map_err(ticket::http::usage)?;
    let repo = file
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();
    Ok((repo, config))
}

fn prime(cwd: &std::path::Path) -> Reply {
    use ignite_core::protocol::client::Exit;
    match config_above(cwd) {
        Ok((_, config)) => Reply::text(Exit::Success, ignite_core::prime::prime(&config)),
        Err(failure) => failure.into_reply(),
    }
}

/// `ignite internal restart`: asks the daemon to restart into its own executable (see
/// `ignite_daemon::main::drill`), then asks again until the new daemon answers, and says how
/// long after the request that was. The request is sent once: a connection that drops before
/// the reply means the daemon restarted before its reply was written.
fn restart(cwd: std::path::PathBuf) -> Reply {
    use ignite_core::protocol::client::{Exit, Request};
    use ignite_daemon::main::drill::{PROBE, RESTART_VERB};
    use ignite_daemon::server::{exchange, is_restarting};
    let socket = ignite_daemon::server::default_socket();
    let name = socket.to_string_lossy();
    let request = Request {
        verb: RESTART_VERB.to_owned(),
        args: Vec::new(),
        cwd,
        session: client::session_from_env(|name| std::env::var(name).ok()),
        wait_s: client::DEFAULT_WAIT_S,
        pid: Some(std::process::id()),
    };
    let pid_in = |reply: &Reply| {
        reply
            .json
            .as_ref()
            .and_then(|json| json.get("pid"))
            .and_then(serde_json::Value::as_u64)
    };
    let asked = std::time::Instant::now();
    let from = match exchange(&name, &request, None) {
        Ok(reply) if is_restarting(&reply) => None,
        Ok(reply) if reply.exit != Exit::Success => return reply,
        Ok(reply) => pid_in(&reply),
        Err(cut) if cut.sent => None,
        Err(cut) => {
            return Reply::text(
                Exit::Usage,
                format!(
                    "failed   the daemon at {} did not answer: {}",
                    socket.display(),
                    cut.error
                ),
            );
        }
    };
    let probe = Request {
        args: vec![PROBE.to_owned()],
        ..request
    };
    let pid = loop {
        match exchange(&name, &probe, None) {
            Ok(reply) if reply.exit == Exit::Success && !is_restarting(&reply) => {
                break pid_in(&reply);
            }
            _ if asked.elapsed() >= reattach::RETRY_FOR => {
                return Reply::text(
                    Exit::Unreachable,
                    format!(
                        "failed   no daemon answered at {} in the {} minutes after the restart\n\
                         next     ignite doctor",
                        socket.display(),
                        reattach::RETRY_FOR.as_secs() / 60
                    ),
                );
            }
            _ => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    };
    let after_ms = asked.elapsed().as_millis();
    let named = |pid: Option<u64>| pid.map_or_else(|| "?".to_owned(), |pid| pid.to_string());
    Reply {
        json: Some(serde_json::json!({
            "from_pid": from,
            "pid": pid,
            "answered_after_ms": after_ms,
        })),
        ..Reply::text(
            Exit::Success,
            format!(
                "restart  daemon {} restarted as {}; it answered {after_ms} ms after the request",
                named(from),
                named(pid)
            ),
        )
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

#[cfg(test)]
mod shim_tests {
    use super::with_shim_verb;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn a_shim_name_inserts_its_verb() {
        assert_eq!(
            with_shim_verb(args(&[r"C:\s\rustc-wrap.exe", "rustc.exe", "-vV"])),
            args(&[r"C:\s\rustc-wrap.exe", "rustc-wrap", "rustc.exe", "-vV"])
        );
        assert_eq!(
            with_shim_verb(args(&["/s/test-runner", "t"])),
            args(&["/s/test-runner", "runner", "t"])
        );
        assert_eq!(
            with_shim_verb(args(&["ignite", "ticket", "list"])),
            args(&["ignite", "ticket", "list"])
        );
    }
}
