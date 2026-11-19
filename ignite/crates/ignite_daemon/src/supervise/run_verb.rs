//! `ignite run`: runs any heavy command with cores from the daemon, and records it.
//!
//! The command gets a hang limit like the checks do ([`DEFAULT_HANG_LIMIT_S`], or `--hang-limit`),
//! so a command that never ends does not keep its cores forever. It is also ended when the
//! client that asked for it disconnects before the command finishes, as when an agent's tool
//! call is killed: nobody is left to read the result, and the command would otherwise run on
//! with its cores.

use std::fmt::Write as _;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::thread;

use ignite_core::ids::TicketId;
use ignite_core::protocol::Class;
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::traits::{CommandSpec, RunResult};

use crate::server::{Context, Verb};

/// Seconds after which `ignite run` ends a command that is still running, unless `--hang-limit`
/// says otherwise. Generous, because `run` is used for long builds and test runs: it is there
/// so that a command that hangs does not keep its cores for days.
pub const DEFAULT_HANG_LIMIT_S: u64 = 3 * 3600;

/// `ignite run`: runs any heavy command with cores from the daemon, and records it.
pub struct RunVerb;

/// The lines of a command's output that `ignite run` prints, from its end.
const TAIL_LINES: usize = 30;

/// `ignite run`'s arguments:
/// `[--class C] [--ticket ID] [--cores N] [--grows] [--hang-limit S] -- <command>`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct RunArgs {
    class: Class,
    ticket: Option<TicketId>,
    cores: Option<u32>,
    /// The command rereads `IGNITE_CORES_FILE` while it runs, so its grant may grow.
    grows: bool,
    /// Seconds after which the command is ended as hung.
    hang_limit_s: u64,
    command: Vec<String>,
}

fn parse_run_args(args: &[String]) -> Result<RunArgs, String> {
    let mut parsed = RunArgs {
        class: Class::Agent,
        ticket: None,
        cores: None,
        grows: false,
        hang_limit_s: DEFAULT_HANG_LIMIT_S,
        command: Vec::new(),
    };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = || {
            rest.next()
                .cloned()
                .ok_or_else(|| format!("{arg} needs a value"))
        };
        match arg.as_str() {
            "--" => {
                parsed.command = rest.cloned().collect();
                break;
            }
            "--class" => {
                let name = value()?;
                parsed.class = serde_json::from_value(serde_json::Value::String(name.clone()))
                    .map_err(|_| format!("unknown class: {name}"))?;
            }
            "--ticket" => {
                let id = value()?;
                parsed.ticket = Some(TicketId::new(id).map_err(|e| e.to_string())?);
            }
            "--cores" => {
                let n = value()?;
                parsed.cores = Some(n.parse().map_err(|_| format!("not a core count: {n}"))?);
            }
            "--grows" => parsed.grows = true,
            "--hang-limit" => {
                let n = value()?;
                parsed.hang_limit_s = n
                    .parse()
                    .ok()
                    .filter(|limit| *limit > 0)
                    .ok_or_else(|| format!("not a number of seconds: {n}"))?;
            }
            other => return Err(format!("unexpected argument: {other}")),
        }
    }
    if parsed.command.is_empty() {
        return Err(
            "usage: ignite run [--class C] [--ticket ID] [--cores N] [--grows] [--hang-limit S] -- \
             <command>"
                .into(),
        );
    }
    Ok(parsed)
}

impl Verb for RunVerb {
    fn name(&self) -> &'static str {
        "run"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        let args = match parse_run_args(&req.args) {
            Ok(args) => args,
            Err(message) => return Reply::text(Exit::Usage, message),
        };
        // The scheduler holds a command to two thirds of the machine and shares what is free,
        // so asking for every core means "as many as I may have".
        let cores = args.cores.unwrap_or_else(|| {
            thread::available_parallelism().map_or(1, |n| u32::try_from(n.get()).unwrap_or(1))
        });
        let spec = CommandSpec {
            args: args.command.clone(),
            cwd: req.cwd.clone(),
            env: Vec::new(),
            class: args.class,
            cores,
            resources: Vec::new(),
            grows: args.grows,
            tree: None,
            ticket: args.ticket,
            hang_limit_s: Some(args.hang_limit_s),
        };
        // The client's connection is probed until its request is handed to a handle.
        let result = match crate::server::caller_probe() {
            Some(gone) => ctx.launcher.run_until(&spec, &*gone),
            None => ctx.launcher.run(&spec),
        };
        let mut reply = render(&args.command, &result);
        if let Some(hint) = express_hint(ctx, &req.cwd) {
            let _ = writeln!(reply.text, "hint     {hint}");
        }
        reply
    }
}

/// A hint naming `ignite test` of the express families, for a command run in the express
/// commands' directory of a build tree. Such a command asks the scheduler for cores like any
/// build, and on 2026-11-18 Ignite's own test build waited minutes for them behind other
/// builds, while `ignite test` of those families runs on the express cores the scheduler never
/// hands out (see `crate::checks::express`).
fn express_hint(ctx: &Context, cwd: &Path) -> Option<String> {
    let express = ctx.config.express.as_ref()?;
    if express.families.is_empty() {
        return None;
    }
    let trees = ctx.repo.join(&ctx.config.repository.trees);
    let in_dir = cwd.strip_prefix(&trees).is_ok_and(|rest| {
        let in_tree: std::path::PathBuf = rest.components().skip(1).collect();
        in_tree.starts_with(&express.dir)
    });
    let names: Vec<String> = express.families.iter().map(|f| format!("{f}:")).collect();
    in_dir.then(|| {
        format!(
            "`ignite test {}` runs Ignite's checks on reserved cores when your tree changes \
             only Ignite",
            names.join(" ")
        )
    })
}

/// The reply for a finished command: the verdict, the end of its output and where the rest is.
fn render(command: &[String], result: &RunResult) -> Reply {
    let shown = command.join(" ");
    let shown = if shown.chars().count() > 60 {
        format!("{}...", shown.chars().take(57).collect::<String>())
    } else {
        shown
    };
    let status = match (result.hung, result.exit) {
        (true, _) => "hung, and was ended at its hang limit".to_owned(),
        (_, Some(code)) => format!("exit {code}"),
        (_, None) => "ended by a signal".to_owned(),
    };
    let (verdict, exit) = if result.exit == Some(0) && !result.hung {
        ("ran", Exit::Success)
    } else {
        ("failed", Exit::CheckFailed)
    };
    let mut text = format!(
        "{verdict:<8} {shown} in {:.0} s: {status}\n",
        result.usage.wall_s
    );
    for line in tail(&result.output, TAIL_LINES) {
        text.push_str(&line);
        text.push('\n');
    }
    let _ = writeln!(text, "output   {}", result.output.display());
    Reply {
        exit,
        text,
        json: Some(serde_json::json!({
            "exit": result.exit,
            "output": result.output,
            "usage": result.usage,
        })),
        handle: None,
    }
}

/// The last `lines` lines of a file, read from at most its last 64 KiB.
fn tail(path: &Path, lines: usize) -> Vec<String> {
    const WINDOW: u64 = 64 * 1024;
    let Ok(mut file) = File::open(path) else {
        return Vec::new();
    };
    let len = file.metadata().map_or(0, |m| m.len());
    if file
        .seek(SeekFrom::Start(len.saturating_sub(WINDOW)))
        .is_err()
    {
        return Vec::new();
    }
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&bytes);
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..]
        .iter()
        .map(|line| (*line).to_owned())
        .collect()
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
