//! The tables of `ignite.toml` that say how each app's workers run on a machine (see "CLI mode"
//! in the design document).
//!
//! ```toml
//! # The program and the arguments every turn of the app gets.
//! [pool.cli.claude]
//! program = 'claude'
//! dialect = 'claude' # Optional when it matches the provider name.
//! args = ["--dangerously-skip-permissions"]
//! ```

use serde::{Deserialize, Serialize};

/// `[pool.cli.<app>]`: the program every CLI turn of the app runs and the arguments it always
/// gets, before the ones each turn adds (model, effort, transcript, prompt).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliProgram {
    /// The executable, looked up on `PATH` when it has no directory.
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// The adapter to use when the provider name differs from its CLI dialect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dialect: Option<String>,
}

impl CliProgram {
    /// The adapter name, defaulting to the provider's own name.
    pub fn dialect<'a>(&'a self, provider: &'a str) -> &'a str {
        self.dialect.as_deref().unwrap_or(provider)
    }
}

impl super::Config {
    /// The program and arguments of `app`'s CLI turns, if `[pool.cli.<app>]` declares them.
    pub fn cli_program(&self, app: &str) -> Option<&CliProgram> {
        self.pool.cli.get(app)
    }

    /// Validates provider names, executable names, and CLI dialects.
    pub(super) fn validate_cli(&self) -> Result<(), (String, String)> {
        let fail = |key: String, message: &str| Err((key, message.to_owned()));
        for (app, program) in &self.pool.cli {
            // Provider names become part of session IDs and their on-disk folder names.
            if app.is_empty()
                || app.ends_with("-slot")
                || !app
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
            {
                return fail(
                    format!("pool.cli.{app}"),
                    "provider names use ASCII letters, digits, hyphens and underscores, and cannot end in -slot",
                );
            }
            if program.program.trim().is_empty() {
                return fail(format!("pool.cli.{app}.program"), "must not be empty");
            }
            let dialect = program.dialect(app);
            if !crate::cli::dialects().any(|name| name == dialect) {
                return Err((
                    format!("pool.cli.{app}.dialect"),
                    format!(
                        "unknown CLI dialect {dialect:?}; choose {}",
                        crate::cli::dialects().collect::<Vec<_>>().join(", ")
                    ),
                ));
            }
        }
        Ok(())
    }
}
