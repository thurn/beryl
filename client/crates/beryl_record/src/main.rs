//! `record`: builds the recordings pack for the present sources if it is missing, and prints
//! its path. It is the one program of the client that plays the engine, kept apart from `client` so
//! that `client` links neither the engine nor the cards (docs/beryl-3d.md, "Edit-to-output time").
//! `just client` runs it when `client` exits with [`beryl_inspect::recordings::MISSING`], and before
//! the tests and the gate.

mod recorder;

use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments == ["--help"] || arguments == ["-h"] {
        println!("just client record [--all | --prefix <recording> | --cards <card>...]");
        println!("With no arguments, record the checked-in corpus and every fixture.");
        return ExitCode::SUCCESS;
    }
    if let Err(why) = beryl_inspect::data::checkout() {
        eprintln!("record: {why}");
        return ExitCode::FAILURE;
    }
    let root = beryl_inspect::recordings::root();
    let selection = match recorder::Selection::parse(&root, &arguments) {
        Ok(selection) => selection,
        Err(why) => {
            eprintln!("record: {why}");
            return ExitCode::FAILURE;
        }
    };
    match beryl_inspect::recordings::build(&arguments, &recorder::Cli { root, selection }) {
        Ok(path) => {
            println!("{}", path.display());
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("record: {why}");
            ExitCode::FAILURE
        }
    }
}
