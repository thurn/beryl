//! The regression checks that tip verification needs a pass for, keyed at one commit (see "Tip
//! verification" in `docs/ignite.md`).

use std::collections::HashMap;

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
use crate::tip::{TipCheck, TipSet, Unlisted};

impl Checks {
    /// Every regression check of every regression family, keyed in `tree`, as a tip needs a
    /// pass for it. A check the hub switched off as flaky is left out.
    ///
    /// A family that needs a build to list on another platform uses the listing that platform
    /// publishes, as does any nextest family that requires a pass there. Its check names and
    /// observed inputs can differ from this machine's. The round can run its own checks while
    /// it waits for the other platform's listing and results.
    ///
    /// A family that cannot be listed, one whose products cannot be built for its listing, and
    /// a check whose inputs cannot be resolved are not errors: they come back in
    /// [`TipSet::unlisted`], so the other families still verify and the tip waits for these.
    pub(crate) fn regression_checks(&self, ctx: &Context, tree: &TreeId) -> Result<TipSet> {
        let (checks, tree) = self.open(ctx, tree)?;
        Ok(checks.regression_in(ctx, &tree))
    }

    /// [`Checks::regression_checks`] in a tree opened with its own manifests.
    fn regression_in(&self, ctx: &Context, tree: &OpenTree) -> TipSet {
        let among: Vec<(&str, &Family)> = self
            .settings
            .manifest
            .families
            .iter()
            .filter(|(_, declared)| declared.family.kind == Kind::Regression)
            .map(|(name, declared)| (name.as_str(), &declared.family))
            .collect();
        let local: Vec<_> = among
            .iter()
            .copied()
            .filter(|(_, family)| {
                listing_platforms(family).is_none_or(|os| os.contains(&self.settings.os))
            })
            .collect();
        let options = RunOptions {
            verb: "tip".to_owned(),
            class: Class::Tip,
            rerun: false,
            ticket: None,
            session: None,
            reserve: None,
        };
        let mut run = CheckRun::default();
        let mut set = TipSet::default();
        let prepared = self.prepare_among(ctx, tree, &local, &options, &mut run);
        self.add_regression_checks(&local, prepared, Some(&run), &mut set);
        for os in [Os::Macos, Os::Windows] {
            if os == self.settings.os {
                continue;
            }
            let remote: Vec<_> = among
                .iter()
                .copied()
                .filter(|(_, family)| {
                    listing_platforms(family).is_some_and(|list| list.contains(&os))
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
        let work = keys
            .into_iter()
            .zip(answers)
            .filter_map(|(key, answer)| {
                let kept: Kept = serde_json::from_str(&answer?).ok()?;
                let (name, family) = among
                    .iter()
                    .copied()
                    .find(|(name, _)| *name == key.family)?;
                (kept.reads.is_some() || !self.observes(name)).then_some((name, family, key, kept))
            })
            .collect();
        on_threads(work, |(name, family, key, kept), _| {
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
            // Without the hub's record every check counts as on, which can only hold `master`
            // longer, never move it past a check.
            let family_stats = self.history.stats(name).ok();
            let keys_for: Vec<Option<Os>> = if family.platforms.is_any() {
                vec![None]
            } else {
                platforms.into_iter().map(Some).collect()
            };
            for keyed in prepared.keyed {
                let state = family_stats
                    .as_ref()
                    .and_then(|s| s.checks.iter().find(|c| c.check == keyed.listed.check))
                    .map_or(CheckState::On, |c| c.state);
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

/// The platforms that must supply their own listing. A buildless JSONL list can still
/// enumerate its checks on any host, including listings whose digests cannot be cached.
fn listing_platforms(family: &Family) -> Option<&[Os]> {
    let Platforms::Each(platforms) = &family.platforms else {
        return None;
    };
    let needs = family.list_needs.as_ref().unwrap_or(&family.needs);
    (family.adapter == Adapter::Nextest || !needs.is_empty()).then_some(platforms)
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
