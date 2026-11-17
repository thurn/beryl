//! `beryl3d`, the app (docs/beryl-3d.md, "Driving the running app"). `just b3d app` runs it.
//! At the end of a run it prints the frame log's summary and the log's path.

use std::process::ExitCode;

use beryl_3d::app;
use beryl_3d::options::{Options, TOOLS, usage};

fn main() -> ExitCode {
    beryl_session::bugs::set_build_commit(env!("BERYL_COMMIT"));
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
            Err(error) => failed(&error),
        };
    }
    match app::run(&options) {
        Ok(finished) => {
            println!("{}", finished.summary.print(&finished.run));
            println!("frame log: {}", finished.path.display());
            ExitCode::SUCCESS
        }
        Err(error) => failed(&error),
    }
}

/// Prints `error` and gives the status to exit with: `beryl_inspect`'s
/// `recordings::MISSING_STATUS` when the recordings pack a `--fixture` reads is missing, so
/// that `just b3d app` and `just b3d smoke` build the pack and start the app again, and 1
/// otherwise.
fn failed(error: &str) -> ExitCode {
    eprintln!("beryl3d: {error}");
    #[cfg(feature = "tools")]
    let status = beryl_inspect::recordings::status(error, 1);
    #[cfg(not(feature = "tools"))]
    let status = 1;
    ExitCode::from(status)
}
