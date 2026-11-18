//! The compiler wrapper, the test runner and the `cargo` shim.
//!
//! - `ignite rustc-wrap`, set as Cargo's `RUSTC_WRAPPER`, runs the compiler and appends one
//!   JSON line per compile unit to the build's units file: crate, mode, instructions, CPU
//!   seconds and whether the unit was rebuilt.
//! - `ignite runner`, set as Cargo's target runner, does the same for each test process.
//! - The `cargo` shim, on each build tree's `PATH`, sends a plain `cargo` call through
//!   `ignite run`, so it runs with cores from the daemon.
//!
//! Cargo runs `RUSTC_WRAPPER` and the runner as one program path with no arguments of its own,
//! so [`write_shims`] writes a small script for each beside the `cargo` shim, and
//! [`cargo_env`] points Cargo at them.

use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use ignite_core::protocol::hub::CompileUnit;
use ignite_os::Native;
use ignite_os::usage::{ProcessUsage, UsageReader};

/// The variable that names the build's units file. The wrapper and the runner record nothing
/// when it is unset.
pub const UNITS_ENV: &str = "IGNITE_UNITS";

/// A file in a shim directory that marks it as one, so a shim skips every shim directory on
/// `PATH` (another tree's included) when it looks for the real `cargo`. Two shims that each
/// took the other for the real one would start each other forever.
pub const SHIM_MARKER: &str = ".ignite-shims";

/// The compiler wrapper script's name in a shim directory.
pub const RUSTC_WRAP_SCRIPT: &str = "rustc-wrap";
/// The test runner script's name in a shim directory.
pub const RUNNER_SCRIPT: &str = "test-runner";

/// The mode recorded for a test process run by `ignite runner`.
pub const RUN_MODE: &str = "run";

/// Writes the `cargo` shim, the compiler wrapper script and the test runner script into `dir`,
/// each calling the `ignite` executable at `ignite`.
pub fn write_shims(dir: &Path, ignite: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    fs::write(dir.join(SHIM_MARKER), "")?;
    let ignite = shell_quote(&ignite.display().to_string());
    let scripts = [
        ("cargo", CARGO_SHIM.replace("@IGNITE@", &ignite)),
        (
            RUSTC_WRAP_SCRIPT,
            format!("#!/bin/sh\nexec {ignite} rustc-wrap \"$@\"\n"),
        ),
        (
            RUNNER_SCRIPT,
            format!("#!/bin/sh\nexec {ignite} runner \"$@\"\n"),
        ),
    ];
    for (name, text) in scripts {
        let path = dir.join(name);
        fs::write(&path, text)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

/// The variables that make Cargo compile through the wrapper and run tests through the runner
/// in the shim directory `shims`, recording into `units`.
pub fn cargo_env(shims: &Path, units: &Path) -> Vec<(String, String)> {
    vec![
        (
            "RUSTC_WRAPPER".to_owned(),
            shims.join(RUSTC_WRAP_SCRIPT).display().to_string(),
        ),
        (
            runner_variable(),
            shims.join(RUNNER_SCRIPT).display().to_string(),
        ),
        (UNITS_ENV.to_owned(), units.display().to_string()),
    ]
}

/// Cargo's runner variable for the host, such as `CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER`.
fn runner_variable() -> String {
    let os = match std::env::consts::OS {
        "macos" => "apple_darwin",
        "linux" => "unknown_linux_gnu",
        "windows" => "pc_windows_msvc",
        other => other,
    };
    format!("CARGO_TARGET_{}_{}_RUNNER", std::env::consts::ARCH, os).to_uppercase()
}

/// Quotes `text` for a POSIX shell.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The `cargo` shim. Inside a command that already holds a grant (`IGNITE_CORES` is set), or
/// with `IGNITE_SHIM=0`, it runs the real `cargo` as it is.
const CARGO_SHIM: &str = r#"#!/bin/sh
# Sends a plain `cargo` call through `ignite run`, so it runs with cores from the daemon.
real=""
old_ifs=$IFS
IFS=:
for dir in $PATH; do
    if [ -e "$dir/.ignite-shims" ]; then
        continue
    fi
    if [ -x "$dir/cargo" ]; then
        real="$dir/cargo"
        break
    fi
done
IFS=$old_ifs
if [ -z "$real" ]; then
    echo "ignite cargo shim: no cargo on PATH outside the shim directories" >&2
    exit 127
fi
if [ -n "$IGNITE_CORES" ] || [ "$IGNITE_SHIM" = "0" ]; then
    exec "$real" "$@"
fi
exec @IGNITE@ run -- "$real" "$@"
"#;

/// The hidden `ignite rustc-wrap <rustc> <args...>` subcommand.
pub fn rustc_wrap(args: &[String]) -> i32 {
    let Some((program, rest)) = args.split_first() else {
        eprintln!("ignite rustc-wrap: no compiler given");
        return 1;
    };
    let unit = CompileArgs::parse(args);
    let rebuilt = unit.as_ref().is_some_and(CompileArgs::built_before);
    let (code, usage) = match run_measured(program, rest) {
        Ok(ran) => ran,
        Err(error) => {
            eprintln!("ignite rustc-wrap: cannot run {program}: {error}");
            return 1;
        }
    };
    if let Some(unit) = unit {
        record(&CompileUnit {
            crate_name: unit.crate_name,
            mode: unit.mode.to_owned(),
            instructions: usage.instructions.map(|n| n as f64),
            cpu_s: usage.cpu_s,
            rebuilt,
        });
    }
    code
}

/// The hidden `ignite runner <test binary> <args...>` subcommand.
pub fn runner(args: &[String]) -> i32 {
    let Some((program, rest)) = args.split_first() else {
        eprintln!("ignite runner: no test binary given");
        return 1;
    };
    let (code, usage) = match run_measured(program, rest) {
        Ok(ran) => ran,
        Err(error) => {
            eprintln!("ignite runner: cannot run {program}: {error}");
            return 1;
        }
    };
    record(&CompileUnit {
        crate_name: test_binary_crate(Path::new(program)),
        mode: RUN_MODE.to_owned(),
        instructions: usage.instructions.map(|n| n as f64),
        cpu_s: usage.cpu_s,
        rebuilt: false,
    });
    code
}

/// Runs a program with this process's standard streams, which Cargo reads, and returns its exit
/// code with its use. A program ended by a signal returns 128 plus the signal, as a shell does.
fn run_measured(program: &str, args: &[String]) -> io::Result<(i32, ProcessUsage)> {
    use std::os::unix::process::ExitStatusExt;
    let mut child = Command::new(program).args(args).spawn()?;
    let (status, usage) = Native.reap(&mut child)?;
    let code = status
        .code()
        .or_else(|| status.signal().map(|s| 128 + s))
        .unwrap_or(1);
    Ok((code, usage))
}

/// Appends one unit line to the file `IGNITE_UNITS` names. A failed write loses the line and
/// never fails the compile.
fn record(unit: &CompileUnit) {
    let Some(path) = std::env::var_os(UNITS_ENV) else {
        return;
    };
    let Ok(mut line) = serde_json::to_vec(unit) else {
        return;
    };
    line.push(b'\n');
    // One `write` call on a file opened to append, so lines from parallel compilers never
    // interleave.
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(&line);
    }
}

/// What the wrapper learns about a compile unit from the compiler's arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CompileArgs {
    crate_name: String,
    mode: &'static str,
    out_dir: Option<PathBuf>,
    /// The `-C extra-filename` suffix Cargo gives each unit's files, such as `-1a2b3c4d`.
    extra_filename: String,
}

impl CompileArgs {
    /// Reads `args` (the compiler first). `None` for Cargo's probes, which ask the compiler for
    /// its version or its configuration and compile no crate.
    fn parse(args: &[String]) -> Option<Self> {
        let mut crate_name = None;
        let mut out_dir = None;
        let mut extra_filename = String::new();
        let mut emit = String::new();
        let mut test = false;
        let mut iter = args.iter().skip(1);
        while let Some(arg) = iter.next() {
            let mut value = |flag: &str| -> Option<String> {
                if arg == flag {
                    iter.next().cloned()
                } else {
                    arg.strip_prefix(flag)
                        .and_then(|rest| rest.strip_prefix('='))
                        .map(str::to_owned)
                }
            };
            if let Some(name) = value("--crate-name") {
                crate_name = Some(name);
            } else if let Some(dir) = value("--out-dir") {
                out_dir = Some(PathBuf::from(dir));
            } else if let Some(kinds) = value("--emit") {
                emit = kinds;
            } else if arg == "--test" {
                test = true;
            } else if let Some(codegen) = value("-C")
                && let Some(suffix) = codegen.strip_prefix("extra-filename=")
            {
                suffix.clone_into(&mut extra_filename);
            }
        }
        let crate_name = crate_name.filter(|name| name != "___")?;
        // With `cargo clippy`, Cargo runs `RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER rustc ...`, so
        // the compiler this wrapper is given is clippy's driver.
        let clippy = Path::new(&args[0])
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.starts_with("clippy-driver"));
        let mode = if clippy {
            "clippy"
        } else if test {
            "test"
        } else if !emit.is_empty() && !emit.split(',').any(|kind| kind.starts_with("link")) {
            "check"
        } else {
            "build"
        };
        Some(Self {
            crate_name,
            mode,
            out_dir,
            extra_filename,
        })
    }

    /// Whether the unit's output directory already holds a file of this unit, such as
    /// `libname-hash.rlib` or `name-hash.d`: then this compile rebuilds it.
    fn built_before(&self) -> bool {
        let Some(dir) = &self.out_dir else {
            return false;
        };
        let stem = format!("{}{}", self.crate_name, self.extra_filename);
        let lib = format!("lib{stem}");
        let Ok(entries) = fs::read_dir(dir) else {
            return false;
        };
        entries.filter_map(Result::ok).any(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            [&stem, &lib].iter().any(|prefix| {
                name.strip_prefix(prefix.as_str())
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
            })
        })
    }
}

/// The crate a Cargo test binary belongs to: its file name without Cargo's `-<hash>` suffix.
fn test_binary_crate(binary: &Path) -> String {
    let name = binary
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    match name.rsplit_once('-') {
        Some((stem, hash)) if !hash.is_empty() && hash.chars().all(|c| c.is_ascii_hexdigit()) => {
            stem.to_owned()
        }
        _ => name,
    }
}

#[cfg(test)]
mod tests;
