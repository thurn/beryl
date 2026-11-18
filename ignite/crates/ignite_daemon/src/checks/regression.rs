//! The regression checks that tip verification needs a pass for, keyed at one commit (see "Tip
//! verification" in `docs/ignite.md`).

use ignite_core::hash::{self, HashParts};
use ignite_core::ids::TreeId;
use ignite_core::manifest::{Kind, Platforms};
use ignite_core::protocol::Class;
use ignite_core::protocol::hub::{CheckState, ResultKey};
use ignite_core::traits::{Error, Result};

use super::{CheckRun, Checks, RunOptions};
use crate::server::Context;
use crate::tip::TipCheck;

impl Checks {
    /// Every regression check of the families this machine runs, keyed in `tree`. A check the
    /// hub switched off as flaky is left out. A check whose family lists other platforms also
    /// gets a key for each of them, marked as one this machine cannot run, so that `master`
    /// waits for their passes too.
    ///
    /// A family whose listing fails, or a check whose inputs cannot be resolved, is an error:
    /// the tip cannot be verified without it.
    pub(crate) fn regression_checks(&self, ctx: &Context, tree: &TreeId) -> Result<Vec<TipCheck>> {
        let tree = self.open(tree)?;
        let families: Vec<&str> = self
            .families()
            .filter(|(_, family)| family.kind == Kind::Regression)
            .map(|(name, _)| name)
            .collect();
        let options = RunOptions {
            verb: "tip".to_owned(),
            class: Class::Tip,
            rerun: false,
            ticket: None,
            session: None,
        };
        let mut run = CheckRun::default();
        let prepared = self.prepare(ctx, &tree, &families, &options, &mut run);
        if let Some((family, output)) = run.incomplete_families.first() {
            return Err(Error::Io(format!(
                "the family {family} could not be listed; see {}",
                output.display()
            )));
        }
        let mut checks = Vec::new();
        for prepared in prepared {
            if let Some(broken) = prepared.broken.first() {
                return Err(Error::Invalid(format!(
                    "the inputs of {} cannot be resolved: {}",
                    broken.check,
                    broken.message.as_deref().unwrap_or_default()
                )));
            }
            let Some(declared) = self.settings.manifest.families.get(&prepared.family) else {
                continue;
            };
            let family = &declared.family;
            // Without the hub's record every check counts as on, which can only hold `master`
            // longer, never move it past a check.
            let family_stats = self.history.stats(&prepared.family).ok();
            let commands = hash::commands(family, &self.settings.manifest);
            let other_platforms: Vec<_> = match &family.platforms {
                Platforms::Any(_) => Vec::new(),
                Platforms::Each(list) => list
                    .iter()
                    .copied()
                    .filter(|os| *os != self.settings.os)
                    .collect(),
            };
            for keyed in prepared.keyed {
                let state = family_stats
                    .as_ref()
                    .and_then(|s| s.checks.iter().find(|c| c.check == keyed.listed.check))
                    .map_or(CheckState::On, |c| c.state);
                if state == CheckState::Off {
                    continue;
                }
                let inputs: Vec<String> = keyed.inputs.iter().map(|(p, _)| p.clone()).collect();
                for os in &other_platforms {
                    let hash = hash::input_hash(&HashParts {
                        inputs: &keyed.inputs,
                        digest: keyed.listed.digest.as_deref(),
                        commands: &commands,
                        toolchain: &self.settings.toolchain,
                        platform: Some(*os),
                    });
                    checks.push(TipCheck {
                        check: keyed.listed.check.clone(),
                        family: prepared.family.clone(),
                        key: ResultKey {
                            check: keyed.listed.check.clone(),
                            hash,
                            platform: Some(*os),
                        },
                        inputs: inputs.clone(),
                        runnable: false,
                        reuse: state == CheckState::On,
                    });
                }
                checks.push(TipCheck {
                    check: keyed.listed.check,
                    family: prepared.family.clone(),
                    key: keyed.key,
                    inputs,
                    runnable: true,
                    reuse: state == CheckState::On,
                });
            }
        }
        Ok(checks)
    }
}
