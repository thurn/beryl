//! `beryl_scenario render [--json] [NAME]...`: prints the rules text each named card's definition
//! renders to ([`beryl_card::render`]), each face under its name, or every card written in a file
//! when none is named, for comparing a definition with the card's Oracle text. With
//! `--json` it prints one JSON object per card and line, `{"name": .., "faces": [{"name": ..,
//! "text": ..}]}`, which `just card render` reads.
//!
//! `beryl_scenario dump [PATH] [--generated N] [--card NAME]...`: writes every card's scenarios
//! as JSON, to PATH or to standard output, for the oracle exporters (architecture §17.2), with
//! the scenarios generated for each card built from library effects ([`beryl_cards::derive`]),
//! followed by N generated scenarios ([`beryl_cards::generate`]), none by default. A card
//! complete from the card store, which has no file, gets its generated scenarios only when a
//! `--card` names it: there are hundreds, and no sweep plays them all. It lives here rather than
//! in `beryl_scenario` because it needs the cards, and the cards depend on that crate.
//!
//! `beryl_scenario run --card NAME`: plays one card's scenarios in the strict runner
//! ([`beryl_scenario::run`]), for `just card check`: the hand-written ones of its file, those of
//! the rules library's entries it is listed under, and the ones generated for it. It
//! prints a line per scenario and a count of each kind, and exits 1 when any fails or a
//! generated run could not be played, and 2 when no card has that name.

use std::fmt::Write as _;
use std::process::ExitCode;

use beryl_cards::{derive, generate};
use beryl_scenario::Scenario;

/// The seed of the first generated scenario: fixed, so a dump is the same each time.
const SEED: u64 = 1;

const USAGE: &str = "usage: beryl_scenario dump [PATH] [--generated N] [--card NAME]... | run \
                     --card NAME | render [--json] [NAME]...";

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "render") {
        return render(&args[1..]);
    }
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
    match (args.as_slice(), named.as_slice()) {
        ([run], [card]) if run == "run" => run_card(card),
        ([dump], _) if dump == "dump" => {
            println!("{}", dump_json(generated, &named));
            ExitCode::SUCCESS
        }
        ([dump, path], _) if dump == "dump" => {
            if let Err(e) = std::fs::write(path, dump_json(generated, &named) + "\n") {
                eprintln!("beryl_scenario: {path}: {e}");
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        _ => usage(),
    }
}

/// The dump: every card's scenarios, the generated ones of the cards built from library
/// effects and of the `named` cards complete from the store, then `generated` random ones.
fn dump_json(generated: usize, named: &[String]) -> String {
    let scenarios = generate::scenarios(generated, SEED);
    let mut cards = beryl_cards::scenarios().to_vec();
    // The rules library's entries' scenarios, each as its card's.
    for (card, of_entry) in beryl_cards::library::by_card() {
        cards.push((card, of_entry));
    }
    // The generated scenarios of the cards built from library effects, each as its
    // card's.
    cards.extend_from_slice(derive::all());
    // The named cards complete from the store, each with its generated scenarios.
    let from_store: Vec<_> = beryl_cards::CARDS
        .from_store()
        .iter()
        .filter(|def| named.iter().any(|n| n == def.name))
        .map(|&def| (def.name, &*derive::for_card(def).scenarios.leak()))
        .collect();
    cards.extend_from_slice(&from_store);
    cards.push((generate::CARD, scenarios.as_slice()));
    beryl_scenario::to_json(&beryl_cards::CARDS, &cards)
}

/// The card called `name`: by Beryl's name, or by the card store's, which names a double-faced
/// card "Front // Back" where Beryl names it by its front face (CR 712.6a).
fn find(name: &str) -> Option<&'static beryl_card::CardDef> {
    let front = name.split(" // ").next().unwrap_or(name);
    beryl_cards::CARDS
        .get(name)
        .or_else(|| beryl_cards::CARDS.get(front))
}

/// Plays the scenarios of the card called `name` in the strict runner.
fn run_card(name: &str) -> ExitCode {
    let Some(def) = find(name) else {
        eprintln!("beryl_scenario: no card named {name} is built");
        return ExitCode::from(2);
    };
    let written: Vec<&Scenario> = beryl_cards::scenarios()
        .iter()
        .copied()
        .chain(beryl_cards::library::by_card())
        .filter(|&(card, _)| card == def.name)
        .flat_map(|(_, scenarios)| scenarios)
        .collect();
    // A card with a Rust body gets no generated scenarios: nothing can read what it does.
    let derived = if derive::custom().contains(&def.name) {
        None
    } else {
        Some(derive::for_card(def))
    };
    let mut failed = 0;
    for (kind, scenario) in written.iter().map(|&s| ("hand-written", s)).chain(
        derived
            .iter()
            .flat_map(|d| &d.scenarios)
            .map(|s| ("generated", s)),
    ) {
        match beryl_scenario::run(&beryl_cards::CARDS, scenario) {
            Ok(()) => println!("pass {kind}: {}", scenario.name),
            Err(e) => {
                failed += 1;
                println!("fail {kind}: {e}");
            }
        }
    }
    let unplayable = derived.as_ref().map_or(&[][..], |d| &d.failures[..]);
    for why in unplayable {
        println!("fail generated: a run could not be played: {why}");
    }
    let generated = derived.as_ref().map_or(0, |d| d.scenarios.len());
    let custom = if derived.is_none() {
        " (a Rust body: none generated)"
    } else {
        ""
    };
    println!(
        "{} hand-written, {generated} generated{custom}, {} failed",
        written.len(),
        failed + unplayable.len()
    );
    if failed + unplayable.len() == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Prints the rendered rules text of the cards named, or of every card written in a file.
fn render(args: &[String]) -> ExitCode {
    let json = args.first().is_some_and(|a| a == "--json");
    let names = if json { &args[1..] } else { args };
    let defs: Vec<&'static beryl_card::CardDef> = if names.is_empty() {
        beryl_cards::CARDS.cards().to_vec()
    } else {
        let mut defs = Vec::new();
        for name in names {
            let Some(def) = find(name) else {
                eprintln!("beryl_scenario: no card named {name:?}");
                return ExitCode::FAILURE;
            };
            defs.push(def);
        }
        defs
    };
    if json {
        for def in defs {
            let faces: Vec<String> = def
                .face_ixs()
                .map(|face| {
                    let text = beryl_card::render::render_face(def, face);
                    format!(
                        "{{\"name\": {}, \"text\": {}}}",
                        json_string(def.face_name(face)),
                        json_string(&text)
                    )
                })
                .collect();
            println!(
                "{{\"name\": {}, \"faces\": [{}]}}",
                json_string(def.name),
                faces.join(", ")
            );
        }
        return ExitCode::SUCCESS;
    }
    let texts: Vec<String> = defs
        .iter()
        .map(|def| {
            def.face_ixs()
                .map(|face| {
                    let text = beryl_card::render::render_face(def, face);
                    format!("{}\n{text}", def.face_name(face))
                })
                .collect::<Vec<_>>()
                .join("\n\n")
        })
        .collect();
    println!("{}", texts.join("\n\n"));
    ExitCode::SUCCESS
}

/// `text` as a JSON string literal (RFC 8259 §7): quotes, backslashes and control characters
/// escaped, everything else as it is.
fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if u32::from(c) < 0x20 => {
                // Writing to a String cannot fail.
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
