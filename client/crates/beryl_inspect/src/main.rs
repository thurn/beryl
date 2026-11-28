//! `client`: the text commands of `just client` (docs/beryl-3d.md, "The agent interface"). Everything
//! is in the library's `cli` module, which the tests call too.

use std::io::Write;
use std::process::ExitCode;

/// Windows gives the main thread a 1 MiB stack, and `spec`, `info`, `tree` and `film` recurse
/// deeper than that, so the command runs on a thread with a stack the size macOS gives the main
/// thread (8 MiB) or more.
const STACK_BYTES: usize = 256 * 1024 * 1024;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let worker = std::thread::Builder::new()
        .stack_size(STACK_BYTES)
        .spawn(move || beryl_inspect::cli::main(&args))
        .expect("spawn the command thread");
    let ran = match worker.join() {
        Ok(ran) => ran,
        Err(panic) => std::panic::resume_unwind(panic),
    };
    // A closed pipe (`| head`) is not an error of the command's.
    let _ = std::io::stdout().write_all(ran.stdout.as_bytes());
    let _ = std::io::stderr().write_all(ran.stderr.as_bytes());
    ExitCode::from(ran.code)
}
