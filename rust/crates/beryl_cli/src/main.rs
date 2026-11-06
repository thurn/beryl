//! Beryl's headless command line (architecture §16).
//!
//! - `beryl_cli play <agent> <agent> [--seed N] [--out FILE] [budget]` plays one seeded game
//!   of the WU Control mirror, the first agent in the first seat, and writes its transcript
//!   to `FILE`, or to standard output.
//! - `beryl_cli tournament <agent>... [--games N] [--seed N] [--json FILE] [--check]
//!   [budget]` plays a round robin, N games per pairing (an agent alone plays itself), and
//!   prints the table; `--json` also writes the results, `--check` checks the invariants at
//!   every decision. Games run in parallel, one thread each.
//! - The budget flags `--iterations N` and `--trees K` set a searching agent's iterations
//!   per decision and how many independent trees share them (`beryl_search::Budget`).
//! - `beryl_cli record-fixture <definitions.json> <out-dir> [name...]` plays the seeded games
//!   and boards a definitions file describes and writes, for each, the stream of messages the UI's
//!   fixture transport replays (`ui/src/ipc.ts`) to `<out-dir>/<name>.json`. `just ui-shots`
//!   runs it over `ui/fixtures.json`, so fixtures are generated, never committed.
//! - `beryl_cli longthink [--status | --upgrade | --add FAMILY] [--new | --run DIR] [--games
//!   N]` starts the long-think arena or resumes it ([`longthink`], `just longthink`), and
//!   supervises `beryl_cli longthink-run <dir> [--games N]`, the run's own copy of this
//!   binary playing it.
//! - `beryl_cli eval-fit [--games N] [--seed N]` fits the board evaluation's weights on
//!   seeded games and prints them ([`evalfit`]).
//! - `beryl_cli serve [--port N]` serves the app's game commands over HTTP for a UI in a plain
//!   browser to play live games through (`just ui-live`, [`serve`]).

mod boards;
mod evalfit;
mod fixture;
mod game;
mod longthink;
mod serve;
mod tournament;
mod transcript;

use std::path::Path;
use std::process::ExitCode;

use beryl_search::Budget;
use transcript::Transcript;

const USAGE: &str = "usage:
  beryl_cli play <agent> <agent> [--seed N] [--out FILE] [--iterations N] [--trees K]
  beryl_cli tournament <agent>... [--games N] [--seed N] [--json FILE] [--check]
      [--iterations N] [--trees K]
  beryl_cli record-fixture <definitions.json> <out-dir> [name...]
  beryl_cli longthink [--status | --upgrade | --add FAMILY] [--new | --run DIR] [--games N]
  beryl_cli eval-fit [--games N] [--seed N]
  beryl_cli serve [--port N]";

/// The port `serve` listens on unless told otherwise.
const DEFAULT_PORT: u16 = 1421;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [command, definitions, out, names @ ..] if command == "record-fixture" => {
            fixture::record_all(Path::new(definitions), Path::new(out), names)
        }
        [command, rest @ ..] if command == "longthink" => longthink::main(rest),
        [command, rest @ ..] if command == "eval-fit" => evalfit::main(rest),
        [command, dir] if command == "longthink-run" => longthink::run(Path::new(dir), None),
        [command, dir, flag, n] if command == "longthink-run" && flag == "--games" => {
            number(flag, n).and_then(|n| longthink::run(Path::new(dir), Some(n)))
        }
        [command] if command == "serve" => serve::serve(DEFAULT_PORT),
        [command, flag, port] if command == "serve" && flag == "--port" => {
            number(flag, port).and_then(serve::serve)
        }
        [command, rest @ ..] if command == "play" => Flags::parse(rest).and_then(|f| play(&f)),
        [command, rest @ ..] if command == "tournament" => {
            Flags::parse(rest).and_then(|f| tournament(&f))
        }
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("beryl_cli: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The positional agent names and the flags of `play` and `tournament`.
#[derive(Debug, Default)]
struct Flags {
    agents: Vec<String>,
    seed: u64,
    games: Option<u32>,
    out: Option<String>,
    json: Option<String>,
    check: bool,
    budget: Budget,
}

impl Flags {
    fn parse(args: &[String]) -> Result<Flags, String> {
        let mut f = Flags::default();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            let mut value = || {
                args.next()
                    .cloned()
                    .ok_or_else(|| format!("{arg} needs a value\n{USAGE}"))
            };
            match arg.as_str() {
                "--seed" => f.seed = number(arg, &value()?)?,
                "--games" => f.games = Some(number(arg, &value()?)?),
                "--out" => f.out = Some(value()?),
                "--json" => f.json = Some(value()?),
                "--check" => f.check = true,
                "--iterations" => f.budget.iterations = number(arg, &value()?)?,
                "--trees" => f.budget.trees = number(arg, &value()?)?,
                flag if flag.starts_with("--") => {
                    return Err(format!("unknown flag {flag}\n{USAGE}"));
                }
                name => f.agents.push(name.to_owned()),
            }
        }
        Ok(f)
    }
}

fn number<T: std::str::FromStr>(flag: &str, v: &str) -> Result<T, String> {
    v.parse()
        .map_err(|_| format!("{flag}: not a number: {v:?}"))
}

fn play(f: &Flags) -> Result<(), String> {
    let [a, b] = f.agents.as_slice() else {
        return Err(format!("play takes two agents\n{USAGE}"));
    };
    let names = [a.as_str(), b.as_str()];
    let t = game::mirror();
    let mut transcript = Transcript::new(names);
    let played = game::play(&t, f.seed, names, f.budget, true, Some(&mut transcript))?;
    match &f.out {
        Some(path) => {
            std::fs::write(path, transcript.text()).map_err(|e| format!("{path}: {e}"))?;
            println!(
                "{:?} on turn {} after {} decisions; transcript in {path}",
                played.outcome, played.turns, played.decisions
            );
        }
        None => print!("{}", transcript.text()),
    }
    Ok(())
}

fn tournament(f: &Flags) -> Result<(), String> {
    if f.agents.is_empty() {
        return Err(format!("tournament takes at least one agent\n{USAGE}"));
    }
    let results = tournament::run(&f.agents, f.games.unwrap_or(100), f.seed, f.budget, f.check)?;
    print!("{}", tournament::table(&results));
    if let Some(path) = &f.json {
        let json = serde_json::to_string_pretty(&results).map_err(|e| e.to_string())?;
        std::fs::write(path, json + "\n").map_err(|e| format!("{path}: {e}"))?;
    }
    Ok(())
}
