//! `beryl_scenario dump [PATH] [--generated N]`: writes every card's scenarios as JSON, to PATH
//! or to standard output, for the oracle exporters (architecture §17.2), followed by N
//! generated scenarios ([`beryl_cards::generate`]), none by default. It lives here rather than
//! in `beryl_scenario` because it needs the cards, and the cards depend on that crate.

use std::process::ExitCode;

use beryl_cards::generate;

/// The seed of the first generated scenario: fixed, so a dump is the same each time.
const SEED: u64 = 1;

fn main() -> ExitCode {
    let usage = || {
        eprintln!("usage: beryl_scenario dump [PATH] [--generated N]");
        ExitCode::from(2)
    };
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut generated = 0;
    if let Some(i) = args.iter().position(|a| a == "--generated") {
        let Some(Ok(n)) = args.get(i + 1).map(|n| n.parse()) else {
            return usage();
        };
        generated = n;
        args.drain(i..i + 2);
    }
    let scenarios = generate::scenarios(generated, SEED);
    let mut cards = beryl_cards::SCENARIOS.to_vec();
    cards.push((generate::CARD, scenarios.as_slice()));
    let json = beryl_scenario::to_json(&beryl_cards::CARDS, &cards);
    match args.as_slice() {
        [dump] if dump == "dump" => println!("{json}"),
        [dump, path] if dump == "dump" => {
            if let Err(e) = std::fs::write(path, json + "\n") {
                eprintln!("beryl_scenario: {path}: {e}");
                return ExitCode::FAILURE;
            }
        }
        _ => return usage(),
    }
    ExitCode::SUCCESS
}
