//! The command to run again by hand when an express command fails (see [`super`]).

/// The command an agent runs in their tree to see a failed express command's errors: for
/// clippy, clippy on the crate that failed alone, which takes seconds; otherwise the command.
///
/// On 2026-11-18 one worker hit clippy's line limit on a function four times in a row through
/// `ignite test`, at 54 to 555 s a round, and a landing failed it once more, when clippy on the
/// one crate in the tree would have shown each error in seconds.
pub fn rerun_command(command: &str, output: &str) -> String {
    if !command.split_whitespace().any(|word| word == "clippy") {
        return command.to_owned();
    }
    match failed_crate(output) {
        Some(name) => format!("cargo clippy -p {name} --all-targets -- -D warnings"),
        None => "cargo clippy --all-targets -- -D warnings".to_owned(),
    }
}

/// The crate a Cargo failure names: ``could not compile `<crate>` `` or, failing that, the
/// first `crates/<crate>/` path of a diagnostic.
fn failed_crate(output: &str) -> Option<String> {
    let named = output.lines().find_map(|text| {
        let rest = text.split("could not compile `").nth(1)?;
        rest.split('`').next().map(str::to_owned)
    });
    named.or_else(|| {
        output.lines().find_map(|text| {
            let rest = text.trim_start().strip_prefix("--> ")?;
            let rest = rest.split("crates/").nth(1)?;
            rest.split('/').next().map(str::to_owned)
        })
    })
}
