//! `beryl3d`, the app (docs/beryl-3d.md, "Driving the running app"). `just b3d app` runs it.
//! At the end of a run it prints the frame log's summary and the log's path.

use std::process::ExitCode;

use beryl_3d::app;
use beryl_3d::options::{Options, TOOLS, usage};

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.iter().any(|argument| argument == "--help") {
        println!("{}", usage(TOOLS));
        return ExitCode::SUCCESS;
    }
    let options = match Options::parse(&arguments, TOOLS) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("beryl3d: {error}");
            return ExitCode::from(2);
        }
    };
    #[cfg(feature = "tools")]
    if let Some(path) = &options.write_fixture {
        let Some(name) = &options.fixture else {
            eprintln!("beryl3d: --write-fixture needs --fixture");
            return ExitCode::from(2);
        };
        return match beryl_3d::fixture::write(name, std::path::Path::new(path)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("beryl3d: {error}");
                ExitCode::FAILURE
            }
        };
    }
    match app::run(&options) {
        Ok(finished) => {
            println!("{}", finished.summary.print(&finished.run));
            println!("frame log: {}", finished.path.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("beryl3d: {error}");
            ExitCode::FAILURE
        }
    }
}
