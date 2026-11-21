//! `record`: builds the recordings pack for the present sources if it is missing, and prints
//! its path. It is the one program of Beryl 3D that plays the engine, kept apart from `b3d` so
//! that `b3d` links neither the engine nor the cards (docs/beryl-3d.md, "Edit-to-output time").
//! `just b3d` runs it when `b3d` exits with [`beryl_inspect::recordings::MISSING`], and before
//! the tests and the gate.

mod recorder;

use std::process::ExitCode;

fn main() -> ExitCode {
    if let Err(why) = beryl_inspect::data::checkout() {
        eprintln!("record: {why}");
        return ExitCode::FAILURE;
    }
    let root = beryl_inspect::recordings::root();
    match beryl_inspect::recordings::build(&recorder::Cli { root }) {
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
