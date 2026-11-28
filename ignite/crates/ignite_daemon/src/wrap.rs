//! The compiler wrapper and the test runner.
//!
//! - `ignite rustc-wrap`, set as Cargo's `RUSTC_WRAPPER`, runs the compiler and appends one
//!   JSON line per compile unit to the build's units file: crate, mode, instructions, CPU
//!   seconds, whether the unit was rebuilt, and when it started and ended.
//! - `ignite runner`, set as Cargo's target runner, does the same for each test process.
//!
//! Cargo runs `RUSTC_WRAPPER` and the runner as one program path with no arguments of its own,
//! so [`write_shims`] writes a small script for each into a shim directory, and [`cargo_env`]
//! points Cargo at them. The scripts are POSIX shell scripts. On Windows each shim is instead a
//! hard link (or a copy) of `ignite.exe` that picks its verb from its own file name, because a
//! `.cmd` file runs through `cmd.exe`, which refuses a command line over 8191 characters, and
//! the compile line of a crate such as `windows-sys` is longer than that.
//!
//! The shim directory holds no `cargo` and never goes on a session's `PATH`. A `cargo` that
//! sent each call through `ignite run` would break the cargo calls agents and recipes make: the
//! daemon runs the command with its own environment (so `RUSTFLAGS` and the like are lost), no
//! standard input, and a reply that holds only the end of the output and Ignite's exit code, or
//! a handle once the call outlives the client's wait.

use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use ignite_core::protocol::hub::CompileUnit;
use ignite_core::time::{SystemClock, Timestamp};
use ignite_core::traits::Clock;
use ignite_os::Native;
use ignite_os::usage::{ProcessUsage, UsageReader};
use serde::Serialize;

/// The variable that names the build's units file. The wrapper and the runner record nothing
/// when it is unset.
pub const UNITS_ENV: &str = "IGNITE_UNITS";

/// Files that earlier releases wrote into a shim directory, for a `cargo` shim that is gone.
/// [`write_shims`] deletes them, so no `cargo` is left in a shim directory to divert a call.
const RETIRED: [&str; 5] = [
    ".ignite-shims",
    "cargo",
    "cargo.cmd",
    // The batch shims that Windows releases wrote before the shims became executables.
    "rustc-wrap.cmd",
    "test-runner.cmd",
];

/// The compiler wrapper script's name in a shim directory.
#[cfg(unix)]
pub const RUSTC_WRAP_SCRIPT: &str = "rustc-wrap";
/// The test runner script's name in a shim directory.
#[cfg(unix)]
pub const RUNNER_SCRIPT: &str = "test-runner";
/// The compiler wrapper script's name in a shim directory.
#[cfg(windows)]
pub const RUSTC_WRAP_SCRIPT: &str = "rustc-wrap.exe";
/// The test runner script's name in a shim directory.
#[cfg(windows)]
pub const RUNNER_SCRIPT: &str = "test-runner.exe";

/// The mode recorded for a test process run by `ignite runner`.
pub const RUN_MODE: &str = "run";

/// Writes the compiler wrapper script and the test runner script into `dir`, each calling the
/// `ignite` executable at `ignite`. A script that already holds the same text is left alone,
/// and one that changes is replaced whole, so this is safe while they run.
pub fn write_shims(dir: &Path, ignite: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    for name in RETIRED {
        match fs::remove_file(dir.join(name)) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    install(dir, ignite)
}

/// Writes the two scripts. A script that already holds the same text is left alone.
#[cfg(unix)]
fn install(dir: &Path, ignite: &Path) -> io::Result<()> {
    for (name, text) in scripts(&ignite.display().to_string()) {
        // Written beside the script and renamed over it, so a build that starts the script
        // while it is being rewritten runs either the old text or the new, never a part.
        let path = dir.join(name);
        if fs::read_to_string(&path).is_ok_and(|old| old == text) {
            continue;
        }
        let partial = dir.join(format!("{name}.{}.partial", std::process::id()));
        fs::write(&partial, text)?;
        make_executable(&partial)?;
        fs::rename(&partial, &path)?;
    }
    Ok(())
}

/// Makes each shim a hard link to `ignite`, or a copy where a link is impossible. The
/// executable starts as `rustc-wrap.exe` or `test-runner.exe` and `ignite` reads the verb from
/// that name (`ignite_cli`'s `main`). A link keeps the release's file alive after the updater
/// deletes its directory, so a shim never names a missing executable.
#[cfg(windows)]
fn install(dir: &Path, ignite: &Path) -> io::Result<()> {
    for name in [RUSTC_WRAP_SCRIPT, RUNNER_SCRIPT] {
        let path = dir.join(name);
        if same_file(&path, ignite) {
            continue;
        }
        let partial = dir.join(format!("{name}.{}.partial", std::process::id()));
        let _ = fs::remove_file(&partial);
        if fs::hard_link(ignite, &partial).is_err() {
            fs::copy(ignite, &partial)?;
        }
        if let Err(error) = fs::rename(&partial, &path) {
            let _ = fs::remove_file(&partial);
            // Windows will not replace a running executable. The shim that is running is a
            // complete older release, which does the same work, so keep it.
            if !path.exists() {
                return Err(error);
            }
        }
    }
    Ok(())
}

/// Whether `a` and `b` have the same size and modification time, as a hard link or a copy of
/// a file does.
#[cfg(windows)]
fn same_file(a: &Path, b: &Path) -> bool {
    let (Ok(a), Ok(b)) = (fs::metadata(a), fs::metadata(b)) else {
        return false;
    };
    a.len() == b.len() && a.modified().ok() == b.modified().ok()
}

/// The two scripts' names and texts, each calling the `ignite` executable at `ignite`.
#[cfg(unix)]
fn scripts(ignite: &str) -> [(&'static str, String); 2] {
    let ignite = shell_quote(ignite);
    [
        (
            RUSTC_WRAP_SCRIPT,
            format!("#!/bin/sh\nexec {ignite} rustc-wrap \"$@\"\n"),
        ),
        (
            RUNNER_SCRIPT,
            format!("#!/bin/sh\nexec {ignite} runner \"$@\"\n"),
        ),
    ]
}

#[cfg(unix)]
fn make_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
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
#[cfg(unix)]
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The hidden `ignite rustc-wrap <rustc> <args...>` subcommand.
pub fn rustc_wrap(args: &[String]) -> i32 {
    let Some((program, rest)) = args.split_first() else {
        eprintln!("ignite rustc-wrap: no compiler given");
        return 1;
    };
    let unit = CompileArgs::parse(args);
    let rebuilt = unit.as_ref().is_some_and(CompileArgs::built_before);
    let started = SystemClock.now();
    let (code, usage) = match run_measured(program, rest) {
        Ok(ran) => ran,
        Err(error) => {
            eprintln!("ignite rustc-wrap: cannot run {program}: {error}");
            return 1;
        }
    };
    if let Some(unit) = unit {
        let compiled = CompileUnit {
            crate_name: recorded_name(
                &unit.crate_name,
                unit.mode == "test",
                std::env::var("CARGO_PKG_NAME").ok().as_deref(),
            ),
            mode: unit.mode.to_owned(),
            instructions: usage.instructions.map(|n| n as f64),
            cpu_s: usage.cpu_s,
            rebuilt,
        };
        record(&compiled, started);
    }
    code
}

/// The hidden `ignite runner <test binary> <args...>` subcommand.
pub fn runner(args: &[String]) -> i32 {
    let Some((program, rest)) = args.split_first() else {
        eprintln!("ignite runner: no test binary given");
        return 1;
    };
    let started = SystemClock.now();
    let (code, usage) = match run_measured(program, rest) {
        Ok(ran) => ran,
        Err(error) => {
            eprintln!("ignite runner: cannot run {program}: {error}");
            return 1;
        }
    };
    let ran = CompileUnit {
        crate_name: recorded_name(
            &test_binary_crate(Path::new(program)),
            true,
            std::env::var("CARGO_PKG_NAME").ok().as_deref(),
        ),
        mode: RUN_MODE.to_owned(),
        instructions: usage.instructions.map(|n| n as f64),
        cpu_s: usage.cpu_s,
        rebuilt: false,
    };
    record(&ran, started);
    code
}

/// Runs a program with this process's standard streams, which Cargo reads, and returns its exit
/// code with its use. A program ended by a signal returns 128 plus the signal, as a shell does.
fn run_measured(program: &str, args: &[String]) -> io::Result<(i32, ProcessUsage)> {
    let mut child = Command::new(program).args(args).spawn()?;
    let (status, usage) = Native.reap(&mut child)?;
    Ok((exit_code(status), usage))
}

#[cfg(unix)]
fn exit_code(status: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status
        .code()
        .or_else(|| status.signal().map(|s| 128 + s))
        .unwrap_or(1)
}

/// Every Windows process exits with a code.
#[cfg(windows)]
fn exit_code(status: std::process::ExitStatus) -> i32 {
    status.code().unwrap_or(1)
}

/// One line of a units file: the unit, with when it started and ended in milliseconds since the
/// Unix epoch, so a reader can tell how much of a command's time went to compiling.
#[derive(Serialize)]
struct UnitLine<'a> {
    #[serde(flatten)]
    unit: &'a CompileUnit,
    started_ms: i64,
    ended_ms: i64,
}

/// Appends one unit line, for a unit that started at `started` and ends now, to the file
/// `IGNITE_UNITS` names. A failed write loses the line and never fails the compile.
fn record(unit: &CompileUnit, started: Timestamp) {
    let Some(path) = std::env::var_os(UNITS_ENV) else {
        return;
    };
    let line = UnitLine {
        unit,
        started_ms: started.millis(),
        ended_ms: SystemClock.now().millis(),
    };
    let Ok(mut line) = serde_json::to_vec(&line) else {
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

/// The name a unit is recorded under. An integration test binary is named for its target, and
/// many packages name that target `all` (`tests/all/main.rs`), so recording `all` added every
/// package's test binary into one entry, which the `slow-crate` rule then reported as a single
/// crate. A test target is recorded as `package::target` instead. A unit test of a library or
/// binary has the package's own crate name, and every other unit keeps its crate name.
fn recorded_name(crate_name: &str, test: bool, package: Option<&str>) -> String {
    match package {
        Some(package) if test && crate_name != package.replace('-', "_") => {
            format!("{package}::{crate_name}")
        }
        _ => crate_name.to_owned(),
    }
}

/// The crate a Cargo test binary belongs to: its file name without Cargo's `-<hash>` suffix.
fn test_binary_crate(binary: &Path) -> String {
    // The stem, not the whole name: a test binary on Windows ends in `.exe`.
    let name = binary
        .file_stem()
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
