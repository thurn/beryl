//! Beryl's headless command line (architecture §16).
//!
//! `beryl_cli record-fixture <definitions.json> <out-dir> [name...]` plays the seeded games a
//! definitions file describes and writes, for each, the stream of messages the UI's fixture
//! transport replays (`ui/src/ipc.ts`) to `<out-dir>/<name>.json`. `just ui-shots` runs it
//! over `ui/fixtures.json`, so fixtures are generated, never committed.

mod fixture;

use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "usage: beryl_cli record-fixture <definitions.json> <out-dir> [name...]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [command, definitions, out, names @ ..] if command == "record-fixture" => {
            fixture::record_all(Path::new(definitions), Path::new(out), names)
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
