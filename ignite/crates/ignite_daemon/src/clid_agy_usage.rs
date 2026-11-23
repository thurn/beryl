//! Polls `agy -p /usage --output-format json` after an Antigravity CLI turn or probe and
//! stores the resulting quota windows for every configured `agy` provider that shares the
//! same executable.

use ignite_core::cli::agy::{Agy, quota_group_for_model, read_usage};
use ignite_core::cli::{CliApp, UsageSample};
use ignite_core::protocol::Class;
use ignite_core::traits::CommandSpec;

use super::{Health, default_model};
use crate::server::Context;

/// Seconds `agy -p /usage --output-format json` may run before its supervisor ends it.
const USAGE_HANG_S: u64 = 30;

impl Health {
    /// Runs `<program> -p /usage --output-format json` when `provider` uses the `agy` dialect,
    /// stores usage rows for `provider` (and any sibling `agy` provider sharing the same
    /// executable), and returns the samples recorded for `provider`.
    pub(super) fn record_agy_usage(&self, ctx: &Context, provider: &str) -> Vec<UsageSample> {
        if ctx.config.cli_dialect(provider) != Agy.name() {
            return Vec::new();
        }
        let Some(program) = ctx.config.cli_program(provider) else {
            return Vec::new();
        };
        let cwd = self.home.join("probe");
        if std::fs::create_dir_all(&cwd).is_err() {
            return Vec::new();
        }
        let result = ctx.launcher.run(&CommandSpec {
            args: vec![
                program.program.clone(),
                "-p".to_owned(),
                "/usage".to_owned(),
                "--output-format".to_owned(),
                "json".to_owned(),
            ],
            cwd,
            env: Vec::new(),
            class: Class::Agent,
            cores: 0,
            resources: Vec::new(),
            grows: false,
            tree: None,
            ticket: None,
            hang_limit_s: Some(USAGE_HANG_S),
            reserve: None,
        });
        if result.hung || result.exit != Some(0) {
            return Vec::new();
        }
        let output = std::fs::read_to_string(&result.output).unwrap_or_default();
        let now = ctx.clock.now();
        let mut own = Vec::new();
        for (name, other) in &ctx.config.pool.cli {
            if other.dialect(name) != Agy.name() || other.program != program.program {
                continue;
            }
            let model = default_model(&ctx.config, name).unwrap_or_default();
            let samples = read_usage(&output, quota_group_for_model(&model));
            if !samples.is_empty()
                && let Err(error) = self.record_usage(ctx, name, &samples, now)
            {
                eprintln!("ignite: agy usage samples for {name} not stored: {error}");
            }
            if name == provider {
                own = samples;
            }
        }
        own
    }
}
