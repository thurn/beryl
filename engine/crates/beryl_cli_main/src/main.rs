//! Beryl's headless command line (architecture §16).
//!
//! - `beryl_cli play <agent> <agent> [--seed N] [--out FILE] [budget]` plays one seeded game
//!   of the WU Control mirror, the first agent in the first seat, and writes its transcript
//!   to `FILE`, or to standard output.
//! - `beryl_cli tournament <agent>... [--games N] [--seed N] [--json FILE] [--check]
//!   [--family NAME] [budget]` plays a round robin, N games per pairing (an agent alone plays
//!   itself), and prints the table; `--json` also writes the results, `--check` checks the
//!   invariants at every decision, and `--family` deals each game two fuzz decks of one
//!   keystone family's cards (`beryl_cards::decks::KEYSTONE_FAMILIES`) in place of the WU
//!   Control mirror. Games run in parallel, one thread each.
//! - `beryl_cli ladder <challenger> [--vs NAME] [--work 3M] [--bar stronger|plausible|audit]
//!   [--cap N] [--seed N] [--positions DIR] [--out DIR]` plays a paired match against the
//!   champion up to the cap, with a sequential test for stronger and plausible, and prints a
//!   `docs/ladder.md` row ([`ladder`], `just ladder`).
//! - `beryl_cli positions build [--games N] [--seed N] [--work 3M] [--out DIR]` cuts the
//!   ladder's balanced start positions from champion games; `beryl_cli positions sample
//!   [--count N] DIR` prints some as transcripts ([`positions`], `just positions`).
//! - `beryl_cli blunders [agent] [--work 3M,30M] [--seeds 5] [--only NAME,...] [--json FILE]`
//!   runs the blunder suite of `data/blunders.json` for an agent, the champion by default
//!   ([`blunders`], `just blunders`).
//! - `beryl_cli why ENTRY --a LABEL --b LABEL [--rollouts 2000]` compares the champion's
//!   paired rollouts after two choices at a blunder-suite position ([`why`], `just why`).
//! - `beryl_cli gate <challenger> --claims NAME,...|none [--vs NAME]` reports the suite's
//!   results for two search families ([`gate`], `just gate`).
//! - `beryl_cli screen <challenger> <match-dir> [--vs NAME] [--recheck N] [--work 3M]
//!   [--games N]` replays the champion's match games, asks a challenger each of the
//!   champion's decisions and re-searches where they differ ([`screen`], `just screen`).
//! - The budget flags `--iterations N` and `--trees K` set a searching agent's iterations
//!   per decision and how many independent trees share them (`beryl_search::Budget`).
//! - `beryl_cli record-fixture <definitions.json> <out-dir> [name...]` plays the seeded games,
//!   boards and manual tables in a definitions file such as `data/client/fixtures.json`.
//!   It writes a stream of recorded session messages to `<out-dir>/<name>.json`.
//! - `beryl_cli eval-fit [--games N] [--seed N]` fits the board evaluation's weights on
//!   seeded games and prints them ([`evalfit`]).
//! - `beryl_cli review <replay.json> [--agent NAME] [--turn N] [--out FILE]` replays an app game's replay
//!   file and writes its transcript; `--agent recorded` or another agent adds its search at
//!   each of the opponent's decisions ([`review`]).
//! - `beryl_cli freeze-saves <dir> [--games N] [--seed N] [--replay FILE]` adds saves to the
//!   frozen corpus at `<dir>`: a small set of seeded games between them covering the implemented cards'
//!   cards and every kind of decision, or the given replay file ([`freeze`]).
//! - `beryl_cli specs-check [card...]` lists
//!   each generated moment and the candidates generation skipped;
//!   `beryl_cli specs-describe [--views] <card> [moment]` prints what each moment shows from
//!   each seat, each view whole with `--views`; `beryl_cli specs-script <card> [moment]`
//!   prints each seat's scripts, view by view ([`specs`]).

use std::path::Path;
use std::process::ExitCode;

use beryl_cli::transcript::Transcript;
use beryl_cli::{
    Catalog, agreement, anchor_replay, blunders, evalfit, fit, fixture, freeze, game, gate, hunt,
    ladder, number, positions, review, screen, script_check, specs, tournament, why,
};
use beryl_search::Budget;

const USAGE: &str = "usage:
  beryl_cli play <agent> <agent> [--seed N] [--out FILE] [--iterations N] [--trees K]
  beryl_cli tournament <agent>... [--games N] [--seed N] [--json FILE] [--check]
      [--paired] [--family NAME] [--iterations N] [--trees K]
      (an agent is a registered one or a long-think entrant, family@budget)
  beryl_cli ladder <challenger> [--vs NAME] [--work 3M] [--bar stronger|plausible|audit]
      [--cap N] [--seed N] [--positions DIR] [--out DIR]
  beryl_cli positions build [--games N] [--seed N] [--work 3M] [--out DIR]
  beryl_cli positions sample [--count N] DIR
  beryl_cli screen <challenger> <match-dir> [--vs NAME] [--recheck N] [--work 3M] [--games N]
  beryl_cli blunders [agent] [--work 3M,30M] [--seeds 5] [--only NAME,...] [--json FILE]
  beryl_cli why ENTRY --a LABEL --b LABEL [--rollouts 2000]
  beryl_cli gate <challenger> --claims NAME,...|none [--vs NAME]
  beryl_cli record-fixture <definitions.json> <out-dir> [name...]
  beryl_cli eval-fit [--games N] [--seed N]
  beryl_cli fit <match-dir>... [--seed N] [--out FILE]
  beryl_cli specs-check [card...]
  beryl_cli script-check <definitions.json> [--dump DIR] [check...]
  beryl_cli specs-describe [--views] <card> [moment]
  beryl_cli specs-script <card> [moment]
  beryl_cli blunder-hunt [--hours 6] [--out DIR] [--seed N] [--inspect CASE.json]
  beryl_cli review <replay.json> [--agent NAME] [--turn N] [--out FILE]
  beryl_cli freeze-saves <dir> [--games N] [--seed N] [--replay FILE]
  beryl_cli anchor-replay <run-dir> <entrant> [--examples N] [--recheck N] [--guarded]
  beryl_cli agreement <run-dir> <entrant> [--positions N] [--budget B]";

/// Every implemented card and what is built from their files, which every command plays with.
/// This binary is the only part of `beryl_cli` that links the cards: the library takes them
/// from here, so an edit to a card file relinks this binary and recompiles no library code.
static CATALOG: Catalog = Catalog {
    cards: &beryl_cards::CARDS,
    scenarios: beryl_cards::scenarios,
    library_scenarios: || beryl_cards::library::by_card().collect(),
    derived_scenarios: |def| beryl_cards::derive::for_card(def).scenarios,
    fuzz_deck_from: beryl_cards::decks::fuzz_deck_from,
    keystone_family: beryl_cards::decks::keystone_family,
};

fn main() -> ExitCode {
    let c = &CATALOG;
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let [command, rest @ ..] = args.as_slice()
        && command == "gate"
    {
        return match gate::main(c, rest) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("beryl_cli: {error}");
                ExitCode::from(2)
            }
        };
    }
    let result = match args.as_slice() {
        [command, definitions, out, names @ ..] if command == "record-fixture" => {
            fixture::record_all(c, Path::new(definitions), Path::new(out), names)
        }
        [command, rest @ ..] if command == "eval-fit" => evalfit::main(c, rest),
        [command, rest @ ..] if command == "fit" => fit::main(c, rest),
        [command, rest @ ..] if command == "review" => review::main(c, rest),
        [command, rest @ ..] if command == "freeze-saves" => freeze::main(c, rest),
        [command, rest @ ..] if command == "anchor-replay" => anchor_replay::main(c, rest),
        [command, rest @ ..] if command == "agreement" => agreement::main(c, rest),
        [command, rest @ ..] if command == "ladder" => ladder::main(c, rest),
        [command, rest @ ..] if command == "blunder-hunt" => hunt::main(c, rest),
        [command, rest @ ..] if command == "blunders" => blunders::main(c, rest),
        [command, rest @ ..] if command == "why" => why::main(c, rest),
        [command, rest @ ..] if command == "screen" => screen::main(c, rest),
        [command, rest @ ..] if command == "positions" => positions::main(c, rest),
        [command, cards @ ..] if command == "specs-check" => specs::check_all(c, cards),
        [command, definitions, checks @ ..] if command == "script-check" => {
            script_check::main(c, Path::new(definitions), checks)
        }
        [command, flag, card] if command == "specs-describe" && flag == "--views" => {
            specs::describe(c, std::slice::from_ref(card), None, true)
        }
        [command, flag, card, moment] if command == "specs-describe" && flag == "--views" => {
            specs::describe(c, std::slice::from_ref(card), Some(moment), true)
        }
        [command, card] if command == "specs-describe" => {
            specs::describe(c, std::slice::from_ref(card), None, false)
        }
        [command, card, moment] if command == "specs-describe" => {
            specs::describe(c, std::slice::from_ref(card), Some(moment), false)
        }
        [command, card] if command == "specs-script" => {
            specs::scripts(c, std::slice::from_ref(card), None)
        }
        [command, card, moment] if command == "specs-script" => {
            specs::scripts(c, std::slice::from_ref(card), Some(moment))
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
    paired: bool,
    family: Option<String>,
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
                "--paired" => f.paired = true,
                "--family" => f.family = Some(value()?),
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

fn play(f: &Flags) -> Result<(), String> {
    let [a, b] = f.agents.as_slice() else {
        return Err(format!("play takes two agents\n{USAGE}"));
    };
    let names = [a.as_str(), b.as_str()];
    let t = game::mirror(&CATALOG);
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
    let results = tournament::run(
        &CATALOG,
        &f.agents,
        f.games.unwrap_or(100),
        f.seed,
        f.budget,
        f.check,
        f.paired,
        f.family.as_deref(),
    )?;
    print!("{}", tournament::table(&results));
    if let Some(path) = &f.json {
        let json = serde_json::to_string_pretty(&results).map_err(|e| e.to_string())?;
        std::fs::write(path, json + "\n").map_err(|e| format!("{path}: {e}"))?;
    }
    Ok(())
}
