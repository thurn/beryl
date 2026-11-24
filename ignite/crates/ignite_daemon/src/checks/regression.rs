//! The regression checks that tip verification needs a pass for, keyed at one commit (see "Tip
//! verification" in `docs/ignite.md`).

use std::collections::HashMap;
use std::sync::Arc;

use ignite_core::config::Os;
use ignite_core::ids::{CheckName, InputHash, TreeId};
use ignite_core::manifest::{Adapter, Family, Kind, Platforms};
use ignite_core::protocol::Class;
use ignite_core::protocol::hub::{CheckState, IncompleteReason, ListingKey, ResultKey};
use ignite_core::traits::Result;

use super::listings::{Kept, Listing, Origin};
use super::prepare::on_threads;
use super::{CheckRun, Checks, LISTING_BUILD_FAILED, OpenTree, Prepared, RunOptions, UNRESOLVED};
use crate::server::Context;
use crate::tip::{CachedTipSet, ListingPhase, TipCheck, TipCheckKind, TipSet, Unlisted};

impl Checks {
    /// The regression checks for this phase, keyed in `tree`. Checks switched off as flaky are
    /// left out of the required set.
    ///
    /// A family that needs a build to list on another platform uses the listing that platform
    /// publishes, as does any nextest family that requires a pass there. Its check names and
    /// observed inputs can differ from this machine's. The round can run its own checks while
    /// it waits for the other platform's listing and results.
    ///
    /// A family that cannot be listed, one whose products cannot be built for its listing, and
    /// a check whose inputs cannot be resolved are not errors: they come back in
    /// [`TipSet::unlisted`], so the other families still verify and the tip waits for these.
    pub(crate) fn regression_checks_for(
        &self,
        ctx: &Context,
        tree: &TreeId,
        phase: ListingPhase,
    ) -> Result<TipSet> {
        let (checks, tree) = self.open(ctx, tree)?;
        Ok(checks.regression_in(ctx, &tree, phase, false))
    }

    /// Keys an ancestor's regression checks without building products or running lists.
    pub(crate) fn cached_regression_checks(
        &self,
        ctx: &Context,
        tree: &TreeId,
    ) -> Result<CachedTipSet> {
        let (checks, tree) = self.open(ctx, tree)?;
        Ok(CachedTipSet {
            listing: checks.regression_in(ctx, &tree, ListingPhase::All, true),
            covered_by: checks.covered_by().clone(),
        })
    }

    #[cfg(test)]
    pub(crate) fn regression_checks(&self, ctx: &Context, tree: &TreeId) -> Result<TipSet> {
        self.regression_checks_for(ctx, tree, ListingPhase::All)
    }

    /// Lists a phase using the candidate's manifests, including families this machine cannot run.
    fn regression_in(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        phase: ListingPhase,
        cached_only: bool,
    ) -> TipSet {
        let among: Vec<(&str, &Family)> = self
            .settings
            .manifest
            .families
            .iter()
            .filter(|(_, declared)| {
                declared.family.kind == Kind::Regression && phase.includes(declared.family.builds)
            })
            .map(|(name, declared)| (name.as_str(), &declared.family))
            .collect();
        if among.is_empty() {
            return TipSet::default();
        }
        let local: Vec<_> = among
            .iter()
            .copied()
            .filter(|(name, family)| {
                listing_platforms(family).is_none_or(|os| os.contains(&self.settings.os))
                    && (!family.machine_relative || self.families().any(|(own, _)| own == *name))
            })
            .collect();
        let mut set = TipSet {
            defer_platforms: among
                .iter()
                .filter(|(_, family)| !family.defer_platforms.is_empty())
                .map(|(name, family)| ((*name).to_owned(), family.defer_platforms.clone()))
                .collect(),
            ..TipSet::default()
        };
        if phase != ListingPhase::PeerBuild && !local.is_empty() {
            let options = RunOptions {
                verb: "tip".to_owned(),
                class: Class::Tip,
                rerun: false,
                ticket: None,
                session: None,
            };
            let mut run = CheckRun::default();
            let prepared = if cached_only {
                let kept = self.kept_listings(ctx, tree, &local);
                self.key_kept_checks(ctx, tree, &local, kept)
            } else {
                self.prepare_among(ctx, tree, &local, &options, &mut run)
            };
            self.add_regression_checks(&local, prepared, Some(&run), &mut set);
        }
        if phase == ListingPhase::Build {
            return set;
        }
        for os in [Os::Macos, Os::Windows] {
            let remote: Vec<_> = among
                .iter()
                .copied()
                .filter(|(name, family)| {
                    listing_platforms(family).is_some_and(|list| list.contains(&os))
                        && (os != self.settings.os
                            || (family.machine_relative
                                && !local.iter().any(|(own, _)| own == name)))
                })
                .collect();
            if !remote.is_empty() {
                let mut settings = self.settings.clone();
                settings.os = os;
                let reader = self.with_settings(settings);
                let prepared = reader.foreign_checks(ctx, tree, &remote);
                reader.add_regression_checks(&remote, prepared, None, &mut set);
            }
        }
        set
    }

    /// Restores listings that other platforms need if the hub missed them or evicted them.
    pub(super) fn share_platform_listings(
        &self,
        prepared: &[Prepared],
        hashes: &HashMap<String, InputHash>,
    ) {
        let keys: Vec<_> = self
            .families()
            .filter(|(name, family)| {
                listing_platforms(family).is_some()
                    && prepared.iter().any(|part| part.family == *name)
            })
            .filter_map(|(family, _)| {
                Some(ListingKey {
                    family: family.to_owned(),
                    hash: hashes.get(family)?.clone(),
                })
            })
            .collect();
        self.cache.share_listings(&keys);
    }

    /// Keys the other platform's kept listings just as its runner does, including its observed
    /// reads. A missing or stale listing leaves the family waiting for that platform to list.
    fn foreign_checks(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        among: &[(&str, &Family)],
    ) -> Vec<Prepared> {
        let hashes = self.listing_hashes(ctx, tree, among);
        let keys: Vec<_> = hashes
            .into_iter()
            .map(|(family, hash)| ListingKey { family, hash })
            .collect();
        let Ok(answers) = ctx.hub.listings(&keys) else {
            return Vec::new();
        };
        let kept = keys
            .into_iter()
            .zip(answers)
            .filter_map(|(key, answer)| {
                let kept: Kept = serde_json::from_str(&answer?).ok()?;
                (kept.reads.is_some() || !self.observes(&key.family))
                    .then(|| (key.family.clone(), (key, Arc::new(kept))))
            })
            .collect();
        self.key_kept_checks(ctx, tree, among, kept)
    }

    /// A kept listing is usable only while its sources still match the candidate's tree.
    fn key_kept_checks(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        among: &[(&str, &Family)],
        mut kept: HashMap<String, (ListingKey, Arc<Kept>)>,
    ) -> Vec<Prepared> {
        let work = among
            .iter()
            .filter_map(|&(name, family)| {
                let (key, kept) = kept.remove(name)?;
                Some((name, family, key, kept))
            })
            .collect();
        on_threads(work, |(name, family, key, kept), _| {
            let kept = Arc::unwrap_or_clone(kept);
            let listing = Listing {
                checks: kept.checks,
                origin: Origin::Kept {
                    key,
                    sources: kept.sources.clone(),
                },
                reads: kept.reads,
            };
            let (keyed, broken, sources) = self.key_listing(ctx, tree, (name, family), &listing);
            (sources.as_ref() == Some(&kept.sources)).then_some(Prepared {
                family: name.to_owned(),
                keyed,
                broken,
            })
        })
        .into_iter()
        .filter_map(|(prepared, _)| prepared)
        .collect()
    }

    /// Checks without a record stay on. Index the hub's states once for the whole listing.
    fn check_states(&self, family: &str) -> HashMap<CheckName, CheckState> {
        self.history
            .stats(family)
            .map(|stats| {
                stats
                    .checks
                    .into_iter()
                    .map(|entry| (entry.check, entry.state))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Adds one platform's prepared checks, or the missing listing, to the round.
    fn add_regression_checks(
        &self,
        among: &[(&str, &Family)],
        prepared: Vec<Prepared>,
        run: Option<&CheckRun>,
        set: &mut TipSet,
    ) {
        let mut prepared: HashMap<_, _> = prepared
            .into_iter()
            .map(|part| (part.family.clone(), part))
            .collect();
        for &(name, family) in among {
            let runs_here = run.is_some() && self.families().any(|(own, _)| own == name);
            let platforms = match &family.platforms {
                Platforms::Each(_) if listing_platforms(family).is_some() => vec![self.settings.os],
                Platforms::Each(list) => list.clone(),
                Platforms::Any(_) => Vec::new(),
            };
            let Some(prepared) = prepared.remove(name) else {
                set.unlisted.push(Unlisted {
                    name: CheckName::new(format!("{name}:listing")).unwrap_or_else(|_| {
                        unreachable!("a family name and a word make a check name")
                    }),
                    family: name.to_owned(),
                    message: missing_listing(name, self.settings.os, run),
                    reportable: runs_here,
                    platforms,
                });
                continue;
            };
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
                    platforms: platforms.clone(),
                });
            }
            let states = self.check_states(name);
            let keys_for: Vec<Option<Os>> = if family.platforms.is_any() {
                vec![None]
            } else {
                platforms.into_iter().map(Some).collect()
            };
            for keyed in prepared.keyed {
                let state = states
                    .get(&keyed.listed.check)
                    .copied()
                    .unwrap_or(CheckState::On);
                // A check switched off is not needed, but the one this machine would run is
                // kept apart: tip verification cuts its batches over it too.
                let off = state == CheckState::Off;
                let inputs: Vec<String> = keyed.inputs.iter().map(|(p, _)| p.clone()).collect();
                for &platform in &keys_for {
                    let runnable = runs_here && platform.is_none_or(|os| os == self.settings.os);
                    let key = if platform == keyed.key.platform {
                        keyed.key.clone()
                    } else {
                        ResultKey {
                            check: keyed.listed.check.clone(),
                            hash: self.hash_for(
                                family,
                                &keyed.inputs,
                                keyed.listed.digest.as_deref(),
                                platform,
                            ),
                            platform,
                        }
                    };
                    let check = TipCheck {
                        check: keyed.listed.check.clone(),
                        family: prepared.family.clone(),
                        key,
                        inputs: inputs.clone(),
                        runnable,
                        reuse: state == CheckState::On,
                        machine_relative: family.machine_relative,
                        kind: if family.builds {
                            TipCheckKind::Build
                        } else {
                            TipCheckKind::Other
                        },
                    };
                    match (off, runnable) {
                        (false, _) => set.checks.push(check),
                        (true, true) => set.off.push(check),
                        (true, false) => {}
                    }
                }
            }
        }
    }
}

/// The platforms that must supply their own listing. A buildless JSONL list can enumerate
/// its checks on any host when it doesn't use a machine's calibration.
fn listing_platforms(family: &Family) -> Option<&[Os]> {
    let Platforms::Each(platforms) = &family.platforms else {
        return None;
    };
    let needs = family.list_needs.as_ref().unwrap_or(&family.needs);
    (family.machine_relative || family.adapter == Adapter::Nextest || !needs.is_empty())
        .then_some(platforms)
}

/// A local listing has a command or build failure to report. For a foreign listing, the
/// platform that owns it reports any failure it encountered.
fn missing_listing(name: &str, os: Os, run: Option<&CheckRun>) -> String {
    let Some(run) = run else {
        return format!("waiting for the {os} listing of {name}");
    };
    if let Some((_, output)) = run
        .incomplete_families
        .iter()
        .find(|(family, _)| family == name)
    {
        return format!(
            "the family {name} could not be listed; see {}",
            output.display()
        );
    }
    if let Some((_, product)) = run
        .listing_build_failures
        .iter()
        .find(|(family, _)| family == name)
    {
        return format!("a product the listing of {name}{LISTING_BUILD_FAILED}{product}");
    }
    format!("the family {name} could not be listed")
}
