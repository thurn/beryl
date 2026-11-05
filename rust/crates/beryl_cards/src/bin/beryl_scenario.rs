//! `beryl_scenario dump [PATH]`: writes every card's scenarios as JSON, to PATH or to standard
//! output, for the oracle exporters (architecture §17.2). It lives here rather than in
//! `beryl_scenario` because it needs the cards, and the cards depend on that crate.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = beryl_scenario::to_json(beryl_cards::SCENARIOS);
    match args.as_slice() {
        [dump] if dump == "dump" => println!("{json}"),
        [dump, path] if dump == "dump" => {
            if let Err(e) = std::fs::write(path, json + "\n") {
                eprintln!("beryl_scenario: {path}: {e}");
                return ExitCode::FAILURE;
            }
        }
        _ => {
            eprintln!("usage: beryl_scenario dump [PATH]");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}
