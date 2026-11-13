//! `beryl_scenario dump [PATH] [--generated N] [--card NAME]...`: writes every card's scenarios
//! as JSON, to PATH or to standard output, for the oracle exporters (architecture §17.2), with
//! the scenarios generated for each card built from library effects ([`beryl_cards::derive`]),
//! followed by N generated scenarios ([`beryl_cards::generate`]), none by default. A card
//! complete from the card store, which has no file, gets its generated scenarios only when a
//! `--card` names it: there are hundreds, and no sweep plays them all. It lives here rather than
//! in `beryl_scenario` because it needs the cards, and the cards depend on that crate.

use std::process::ExitCode;

use beryl_cards::generate;

/// The seed of the first generated scenario: fixed, so a dump is the same each time.
const SEED: u64 = 1;

fn main() -> ExitCode {
    let usage = || {
        eprintln!("usage: beryl_scenario dump [PATH] [--generated N] [--card NAME]...");
        ExitCode::from(2)
    };
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut named = Vec::new();
    while let Some(i) = args.iter().position(|a| a == "--card") {
        let Some(name) = args.get(i + 1).cloned() else {
            return usage();
        };
        named.push(name);
        args.drain(i..i + 2);
    }
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
    // The rules library's entries' scenarios, each as its card's.
    for (card, of_entry) in beryl_cards::library::by_card() {
        cards.push((card, of_entry));
    }
    // The generated scenarios of the cards built from library effects (plan K3), each as its
    // card's.
    cards.extend_from_slice(beryl_cards::derive::all());
    // The named cards complete from the store, each with its generated scenarios.
    let from_store: Vec<_> = beryl_cards::CARDS
        .from_store()
        .iter()
        .filter(|def| named.iter().any(|n| n == def.name))
        .map(|&def| {
            (
                def.name,
                &*beryl_cards::derive::for_card(def).scenarios.leak(),
            )
        })
        .collect();
    cards.extend_from_slice(&from_store);
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
