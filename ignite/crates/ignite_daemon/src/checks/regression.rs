//! The regression checks that tip verification needs a pass for, keyed at one commit (see "Tip
//! verification" in `docs/ignite.md`).

use ignite_core::config::Os;
use ignite_core::hash::{self, HashParts};
use ignite_core::ids::{CheckName, TreeId};
use ignite_core::manifest::{Family, Kind, Platforms};
use ignite_core::protocol::Class;
use ignite_core::protocol::hub::{CheckState, IncompleteReason, ResultKey};
use ignite_core::traits::Result;

use super::{CheckRun, Checks, LISTING_BUILD_FAILED, RunOptions, UNRESOLVED};
use crate::server::Context;
use crate::tip::{TipCheck, TipSet, Unlisted};

impl Checks {
    /// Every regression check of every regression family, keyed in `tree`, as a tip needs a
    /// pass for it. A check the hub switched off as flaky is left out.
    ///
    /// A family this machine does not run (it is not in the machine's list, or the family does
    /// not name this operating system) is listed all the same, and its checks are keyed as
    /// ones only another machine can pass: [`TipCheck::runnable`] is false. So are the keys of a
    /// check whose family lists other platforms, for each of them. Every such key is the key the
    /// machine that runs it computes, so that machine's pass satisfies it here.
    ///
    /// A family that cannot be listed, one whose products cannot be built for its listing, and
    /// a check whose inputs cannot be resolved are not errors: they come back in
    /// [`TipSet::unlisted`], so the other families still verify and the tip waits for these.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn regression_checks(&self, ctx: &Context, tree: &TreeId) -> Result<TipSet> {
        let tree = self.open(tree)?;
        let runs_here: Vec<&str> = self
            .families()
            .filter(|(_, family)| family.kind == Kind::Regression)
            .map(|(name, _)| name)
            .collect();
        let among: Vec<(&str, &Family)> = self
            .settings
            .manifest
            .families
            .iter()
            .filter(|(_, declared)| declared.family.kind == Kind::Regression)
            .map(|(name, declared)| (name.as_str(), &declared.family))
            .collect();
        let options = RunOptions {
            verb: "tip".to_owned(),
            class: Class::Tip,
            rerun: false,
            ticket: None,
            session: None,
        };
        let mut run = CheckRun::default();
        let prepared = self.prepare_among(ctx, &tree, &among, &options, &mut run);
        let mut set = TipSet::default();
        for &(name, family) in &among {
            let reportable = runs_here.contains(&name);
            let unlisted = |message: String| Unlisted {
                name: CheckName::new(format!("{name}:listing"))
                    .unwrap_or_else(|_| unreachable!("a family name and a word make a check name")),
                family: name.to_owned(),
                message,
                reportable,
                platforms: other_platforms(family, reportable),
            };
            if let Some((_, output)) = run.incomplete_families.iter().find(|(f, _)| f == name) {
                set.unlisted.push(unlisted(format!(
                    "the family {name} could not be listed; see {}",
                    output.display()
                )));
            } else if !prepared.iter().any(|p| p.family == name) {
                let products: Vec<String> =
                    run.build_failures.iter().map(|(p, _)| p.clone()).collect();
                set.unlisted.push(unlisted(format!(
                    "a product the listing of {name}{LISTING_BUILD_FAILED}{}",
                    products.join(", ")
                )));
            }
        }
        for prepared in prepared {
            let Some(declared) = self.settings.manifest.families.get(&prepared.family) else {
                continue;
            };
            let family = &declared.family;
            let runs_here = runs_here.contains(&prepared.family.as_str());
            for broken in &prepared.broken {
                set.unlisted.push(Unlisted {
                    name: broken.check.clone(),
                    family: prepared.family.clone(),
                    // Only an invalid manifest entry uses the words the report groups by.
                    message: format!(
                        "the inputs of {}{}{}",
                        broken.check,
                        if broken.reason == Some(IncompleteReason::InvalidEntry) {
                            UNRESOLVED
                        } else {
                            " could not be read: "
                        },
                        broken.message.as_deref().unwrap_or_default()
                    ),
                    reportable: runs_here,
                    platforms: other_platforms(family, runs_here),
                });
            }
            // Without the hub's record every check counts as on, which can only hold `master`
            // longer, never move it past a check.
            let family_stats = self.history.stats(&prepared.family).ok();
            let commands = hash::commands(family, &self.settings.manifest);
            let platforms: Vec<Option<Os>> = match &family.platforms {
                Platforms::Any(_) => vec![None],
                Platforms::Each(list) => list.iter().copied().map(Some).collect(),
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
                let mut push = |key: ResultKey, runnable: bool| {
                    set.checks.push(TipCheck {
                        check: keyed.listed.check.clone(),
                        family: prepared.family.clone(),
                        key,
                        inputs: inputs.clone(),
                        runnable,
                        reuse: state == CheckState::On,
                    });
                };
                let mut mine = false;
                for platform in &platforms {
                    let mine_here = runs_here && platform.is_none_or(|os| os == self.settings.os);
                    if mine_here {
                        mine = true;
                        continue;
                    }
                    let hash = hash::input_hash(&HashParts {
                        inputs: &keyed.inputs,
                        digest: keyed.listed.digest.as_deref(),
                        commands: &commands,
                        toolchain: &self.settings.toolchain,
                        platform: *platform,
                    });
                    push(
                        ResultKey {
                            check: keyed.listed.check.clone(),
                            hash,
                            platform: *platform,
                        },
                        false,
                    );
                }
                if mine {
                    push(keyed.key.clone(), true);
                }
            }
        }
        Ok(set)
    }
}

/// The operating systems whose machines can run a family this machine does not run: those it
/// names, when it names some. Empty for a family that runs here, whose trouble is this
/// machine's own, and for a family that passes on any platform.
fn other_platforms(family: &Family, runs_here: bool) -> Vec<Os> {
    match &family.platforms {
        Platforms::Each(list) if !runs_here => list.clone(),
        _ => Vec::new(),
    }
}
