//! `b3d-render`: the picture commands of `just b3d`, `png` and `film` (docs/beryl-3d.md, "The
//! agent interface"). Everything is in the library's `pictures` module, which the tests call
//! too.

use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ran = beryl_3d::pictures::main(&args);
    // A closed pipe (`| head`) is not an error of the command's.
    let _ = std::io::stdout().write_all(ran.stdout.as_bytes());
    let _ = std::io::stderr().write_all(ran.stderr.as_bytes());
    ExitCode::from(ran.code)
}
