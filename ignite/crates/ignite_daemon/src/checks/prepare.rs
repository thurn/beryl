//! Listing and keying the checks of a set of families, and running the prepared checks, for
//! every verb and for the landing gate, which stops at the first fault it knows of (see
//! "Landing" in `docs/ignite.md`). Each family is listed, and run, on a thread of its own
//! ([`on_threads`]); the landing gate lists in two rounds, so a fault found without building
//! anything stops it before the families that need a product are listed.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::panic::resume_unwind;
use std::sync::Arc;
use std::thread;

use ignite_core::ids::CheckName;
use ignite_core::inputs;
use ignite_core::manifest::Family;
use ignite_core::protocol::Class;
use ignite_core::protocol::hub::{CachedResult, ListingKey};

use super::listings::{Kept, Listing, Origin};
use super::package_listings::Reuse;
use super::time_lines::Stopwatch;

/// The landing gate's test for a family it leaves unlisted, given its name and declaration.
pub(crate) type Cold<'a> = &'a dyn Fn(&str, &Family) -> bool;
use super::{CheckRun, Checks, Final, Keyed, ListError, OpenTree, Prepared, RunOptions};
use crate::server::Context;

/// Inputs deleted or edited since their check hashes were computed. If Git cannot read the
/// current object IDs, every input counts as changed.
pub(super) fn moved_inputs(ctx: &Context, tree: &OpenTree, finals: &[Final]) -> BTreeSet<String> {
    let mut before: BTreeMap<&str, &str> = finals
        .iter()
        .flat_map(|(keyed, ..)| &keyed.inputs)
        .map(|(path, oid)| (path.as_str(), oid.as_str()))
        .collect();
    let paths: Vec<String> = before.keys().map(|path| (*path).to_owned()).collect();
    if let Ok(now) = inputs::object_ids(ctx.git.as_ref(), &tree.path, &paths) {
        for (path, oid) in now {
            if before.get(path.as_str()) == Some(&oid.as_str()) {
                before.remove(path.as_str());
            }
        }
    }
    before.into_keys().map(str::to_owned).collect()
}

impl Checks {
    /// Lists and keys the checks of each family named that this machine runs. A family whose
    /// listing fails is recorded in `run` as incomplete; a check whose inputs cannot be
    /// resolved is kept in `broken`.
    pub(crate) fn prepare(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        families: &[&str],
        options: &RunOptions,
        run: &mut CheckRun,
    ) -> Vec<Prepared> {
        self.prepare_scoped(ctx, tree, families, &BTreeMap::new(), options, run)
    }

    /// [`Checks::prepare`], listing each family in `scopes` for the packages it names only
    /// (see [`Checks::list`]), which `ignite test <names>` asks for.
    pub(crate) fn prepare_scoped(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        families: &[&str],
        scopes: &BTreeMap<String, Vec<String>>,
        options: &RunOptions,
        run: &mut CheckRun,
    ) -> Vec<Prepared> {
        let among: Vec<(&str, &Family)> = self
            .families()
            .filter(|(n, _)| families.contains(n))
            .collect();
        self.prepare_families(ctx, tree, &among, options, run, None, scopes)
    }

    /// [`Checks::prepare`] for exactly the families given, whether or not this machine runs
    /// them, which tip verification needs to know the checks other machines run.
    pub(crate) fn prepare_among(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        among: &[(&str, &Family)],
        options: &RunOptions,
        run: &mut CheckRun,
    ) -> Vec<Prepared> {
        self.prepare_families(ctx, tree, among, options, run, None, &BTreeMap::new())
    }

    /// [`Checks::prepare_among`] for the landing gate, which stops at the first fault it knows
    /// of: a check whose manifest entry is invalid, a listing that failed, or a build that
    /// failed. The families that can be listed without building anything (a kept listing, or a
    /// `list` that needs no product) are listed first, while the products the others need build,
    /// so such a fault refuses the landing before any other family is listed or any check runs.
    /// The families not listed are named in `run.unlisted`.
    ///
    /// A family for which `cold` holds (its listing needs a product the tree has never built)
    /// and of which no listing is kept is not listed, and is named in `run.cold_families`.
    pub(crate) fn prepare_gate(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        among: &[(&str, &Family)],
        options: &RunOptions,
        run: &mut CheckRun,
        cold: Cold<'_>,
    ) -> Vec<Prepared> {
        self.prepare_families(ctx, tree, among, options, run, Some(cold), &BTreeMap::new())
    }

    /// `gate` is the landing gate's test for a family it leaves unlisted; `None` for every
    /// other caller, which lists every family and does not stop at the first fault. `scopes`
    /// names the packages to list some families for.
    ///
    /// Outside the landing gate each family is listed on a thread of its own, so a family whose
    /// `list` needs no product, or a product already current, is listed while another family's
    /// product builds, and the products of different families build at once where the
    /// scheduler grants the cores. What the threads find is added to `run` in the families'
    /// order, as if they had been listed one after another.
    ///
    /// The landing gate lists in two rounds, because it stops at the first fault. First it
    /// lists the families that build nothing to be listed, each on a thread of its own, while
    /// every product the other families need to be listed builds. When that round finds no
    /// fault, it lists the other families at once, whose products are by then current. A
    /// fault in the first round leaves the other families unlisted, but the gate still waits
    /// for the builds already started; their products are current for the next attempt.
    #[allow(clippy::too_many_arguments)]
    fn prepare_families(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        among: &[(&str, &Family)],
        options: &RunOptions,
        run: &mut CheckRun,
        gate: Option<Cold<'_>>,
        scopes: &BTreeMap<String, Vec<String>>,
    ) -> Vec<Prepared> {
        let fail_fast = gate.is_some();
        // Every family's listing hash from one reading of the tree, and one request for every
        // kept listing, so a hub that cannot be reached costs its timeout once, not once per
        // family.
        let hashing = Stopwatch::start(ctx);
        let hashes = self.listing_hashes(ctx, tree, among);
        run.slow(ctx, "hashing the families' inputs", hashing);
        let reading = Stopwatch::start(ctx);
        // A published measurement listing can tell other verifiers what this OS must pass.
        // A runner must list locally to observe its own current calibration.
        let mut reusable = hashes.clone();
        for &(name, family) in among {
            if family.machine_relative {
                reusable.remove(name);
            }
        }
        let mut kept = self.kept_for(reusable);
        run.slow(ctx, "reading kept listings", reading);
        // A `nextest` family that misses its kept listing may reuse its newest listing for the
        // packages whose test names cannot have changed (see `package_listings`).
        let reusing = Stopwatch::start(ctx);
        let mut reuse: BTreeMap<String, Reuse> = among
            .iter()
            .filter(|(name, _)| !kept.contains_key(*name) && !scopes.contains_key(*name))
            .filter_map(|&(name, family)| {
                self.reuse(ctx, tree, name, family)
                    .map(|r| (name.to_owned(), r))
            })
            .collect();
        run.slow(ctx, "comparing newest listings by package", reusing);
        let mut among = among.to_vec();
        if let Some(cold) = gate {
            among.retain(|&(name, family)| {
                let whole = matches!(reuse.get(name), Some(Reuse::Whole(_)));
                let unlisted = !kept.contains_key(name) && !whole && cold(name, family);
                if unlisted {
                    run.cold_families.push(name.to_owned());
                }
                !unlisted
            });
        }
        let work: Vec<FamilyWork> = among
            .iter()
            .map(|&(name, family)| FamilyWork {
                name,
                family,
                key: hashes.get(name).map(|hash| ListingKey {
                    family: name.to_owned(),
                    hash: hash.clone(),
                }),
                kept: kept.remove(name),
                scope: scopes.get(name).cloned(),
                reuse: reuse.remove(name),
            })
            .collect();
        if !fail_fast {
            let prepared = self.prepare_at_once(ctx, tree, work, options, run);
            if options.class == Class::Tip {
                self.share_platform_listings(&prepared, &hashes);
            }
            return prepared;
        }
        // The families that build nothing to be listed are listed first, at once, while the
        // products the others need to be listed build.
        let (free, building): (Vec<FamilyWork>, Vec<FamilyWork>) =
            work.into_iter().partition(|w| {
                let needs = w.family.list_needs.as_ref().unwrap_or(&w.family.needs);
                w.kept.is_some() || needs.is_empty() || matches!(w.reuse, Some(Reuse::Whole(_)))
            });
        let mut products: Vec<String> = Vec::new();
        // A family listed for its changed packages alone builds only theirs when it is listed,
        // so its whole product is not built here.
        for w in building.iter().filter(|w| w.reuse.is_none()) {
            for product in w.family.list_needs.as_ref().unwrap_or(&w.family.needs) {
                if !products.contains(product) {
                    products.push(product.clone());
                }
            }
        }
        let mut prepared = self.list_while_building(ctx, tree, free, &products, options, run);
        if known_fault(&prepared, run) {
            run.unlisted
                .extend(building.iter().map(|w| w.name.to_owned()));
            return prepared;
        }
        prepared.extend(self.prepare_at_once(ctx, tree, building, options, run));
        prepared
    }

    /// Prepares each family of `work` on a thread of its own, and adds what each found to
    /// `run` in order.
    fn prepare_at_once(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        work: Vec<FamilyWork>,
        options: &RunOptions,
        run: &mut CheckRun,
    ) -> Vec<Prepared> {
        let found = on_threads(work, |one, own| {
            self.prepare_family(ctx, tree, one, options, own)
        });
        let mut prepared = Vec::new();
        for (one, own) in found {
            run.absorb(own);
            prepared.extend(one);
        }
        prepared
    }

    /// Prepares each family of `free` on a thread of its own while each of `products` builds
    /// on another, and adds what each found to `run`: the families' findings in order, then
    /// the builds'. On 2026-11-19 `ignite check` of a one-card comment change listed 26
    /// families one after another and built the two products it needed only when the
    /// listing reached the families that needed them, so 137 s went to listing.
    fn list_while_building(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        free: Vec<FamilyWork>,
        products: &[String],
        options: &RunOptions,
        run: &mut CheckRun,
    ) -> Vec<Prepared> {
        let who = super::execute::build_for(options);
        let jobs = products
            .iter()
            .map(Job::Build)
            .chain(free.into_iter().map(|w| Job::List(Box::new(w))))
            .collect();
        let found = on_threads(jobs, |job, own| match job {
            Job::Build(product) => {
                // A failure is recorded in `own`, which the gate reads as a fault.
                let _ =
                    self.build_products(ctx, tree, std::slice::from_ref(product), &[], &who, own);
                None
            }
            Job::List(one) => self.prepare_family(ctx, tree, *one, options, own),
        });
        let (built, listed): (Vec<_>, Vec<_>) = found
            .into_iter()
            .enumerate()
            .partition(|(i, _)| *i < products.len());
        let mut prepared = Vec::new();
        for (_, (one, own)) in listed.into_iter().chain(built) {
            run.absorb(own);
            prepared.extend(one);
        }
        prepared
    }

    /// Lists one family, or takes its kept listing, and keys its checks. `None` when it has
    /// no listing, after recording why in `run`.
    fn prepare_family(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        work: FamilyWork,
        options: &RunOptions,
        run: &mut CheckRun,
    ) -> Option<Prepared> {
        let (name, family, key) = (work.name, work.family, work.key.clone());
        let listed = self.list_family(ctx, tree, work, options, run);
        let mut listing = Self::listed_or_noted(name, listed, run)?;
        let keying = Stopwatch::start(ctx);
        let (mut keyed, mut broken, mut sources) =
            self.key_listing(ctx, tree, (name, family), &listing);
        run.slow(ctx, format!("keying {name}"), keying);
        if let Origin::Kept {
            key: old,
            sources: kept,
        } = &listing.origin
            && sources.as_ref() != Some(kept)
        {
            // The listing was made from other files than the tree's: list the family again.
            self.forget(old);
            self.forget_foreign(name);
            let listed = self.list(ctx, tree, name, family, options, run, key, None, None);
            listing = Self::listed_or_noted(name, listed, run)?;
            let keying = Stopwatch::start(ctx);
            (keyed, broken, sources) = self.key_listing(ctx, tree, (name, family), &listing);
            run.slow(ctx, format!("keying {name}"), keying);
        }
        match (&listing.origin, sources) {
            (Origin::Made { key: Some(key) }, Some(sources)) => {
                let keeping = Stopwatch::start(ctx);
                let mut kept = Kept {
                    sources,
                    checks: listing.checks,
                    reads: listing.reads,
                    packages: None,
                };
                kept.packages = self.packages_of(ctx, tree, family, &kept);
                self.keep(key.clone(), kept);
                run.slow(ctx, format!("keeping the listing of {name}"), keeping);
            }
            // A listing that must not be kept still names the family's checks, which is all
            // `ignite test <word>` asks of a family's newest listing.
            (Origin::Made { .. }, sources) if !listing.checks.is_empty() => {
                let mut kept = Kept {
                    sources: sources.unwrap_or_else(|| Self::sources_hash(&[])),
                    checks: listing.checks,
                    reads: listing.reads,
                    packages: None,
                };
                kept.packages = self.packages_of(ctx, tree, family, &kept);
                self.keep_names(name, &kept);
            }
            _ => {}
        }
        Some(Prepared {
            family: name.to_owned(),
            keyed,
            broken,
        })
    }

    /// Lists a family's checks, reusing package listings whose inputs have not changed.
    fn list_family(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        work: FamilyWork,
        options: &RunOptions,
        run: &mut CheckRun,
    ) -> std::result::Result<Listing, ListError> {
        let FamilyWork {
            name,
            family,
            key,
            kept,
            scope,
            reuse,
        } = work;
        match reuse {
            Some(Reuse::Whole(base)) => {
                run.listings_kept += 1;
                self.note_listed(name, &base.checks);
                Ok(Listing {
                    checks: base.checks,
                    origin: Origin::Made { key },
                    reads: base.reads,
                })
            }
            Some(Reuse::Partial { base, changed }) => {
                let mut list = |scope| {
                    self.list(
                        ctx,
                        tree,
                        name,
                        family,
                        options,
                        run,
                        key.clone(),
                        None,
                        scope,
                    )
                };
                list(Some(changed.as_slice())).and_then(|fresh| {
                    let merged = Self::merged(base, &changed, fresh, key.clone());
                    if merged.checks.is_empty() {
                        // Confirm that the whole family is empty before refusing it.
                        list(None)
                    } else {
                        Ok(merged)
                    }
                })
            }
            None => self.list_remembering_foreign(
                ctx,
                tree,
                (name, family),
                options,
                run,
                key,
                kept,
                scope.as_deref(),
            ),
        }
    }

    /// Looks up every prepared check in one request, then runs each family's misses, each
    /// family on a thread of its own so that the products they need build at once where the
    /// scheduler grants the cores. What each found is added to `run` in the families' order.
    #[allow(clippy::type_complexity)]
    pub(crate) fn run_prepared(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        prepared: Vec<Prepared>,
        options: &RunOptions,
        run: &mut CheckRun,
    ) {
        self.run_prepared_with_stop(ctx, tree, prepared, options, run, None);
    }

    /// [`Checks::run_prepared`], stopping before new builds or attempts once `stop` is raised.
    #[allow(clippy::type_complexity)]
    pub(crate) fn run_prepared_with_stop(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        prepared: Vec<Prepared>,
        options: &RunOptions,
        run: &mut CheckRun,
        stop: Option<&std::sync::atomic::AtomicBool>,
    ) {
        let head = options
            .session
            .as_ref()
            .and_then(|_| self.trees.head(&tree.path).ok());
        let looking = Stopwatch::start(ctx);
        let hits = self.lookup(&prepared, options);
        run.slow(ctx, "looking up cached results", looking);
        let mut hits = hits.into_iter();
        let families: Vec<(String, Vec<(Keyed, Option<CachedResult>)>)> = prepared
            .into_iter()
            .map(|family| {
                let items = family.keyed.into_iter().map(|k| (k, hits.next().flatten()));
                (family.family, items.collect())
            })
            .collect();
        let hashes: HashMap<CheckName, String> = families
            .iter()
            .flat_map(|(_, items)| items)
            .map(|(keyed, _)| (keyed.listed.check.clone(), keyed.key.hash.to_string()))
            .collect();
        let ran = on_threads(families, |(name, items), own| {
            self.execute(ctx, tree, &name, items, options, own, stop);
        });
        for ((), own) in ran {
            // The daemon compares session outcomes to detect shared failures.
            self.cache.note_sightings(
                options.session.as_ref(),
                &options.verb,
                &own,
                &hashes,
                head.as_ref(),
            );
            run.absorb(own);
        }
    }

    /// The package hashes to keep with a listing of `family`: those of a `nextest` family whose
    /// listing names a package for every test, `None` for any other.
    fn packages_of(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        family: &Family,
        kept: &Kept,
    ) -> Option<BTreeMap<String, ignite_core::ids::InputHash>> {
        let named = kept.checks.iter().all(|c| c.package.is_some());
        (family.adapter == ignite_core::manifest::Adapter::Nextest && named)
            .then(|| self.package_hashes(ctx, tree, family, kept))
            .flatten()
    }

    /// The listing, or `None` after recording in `run` why there is none.
    fn listed_or_noted(
        name: &str,
        listed: std::result::Result<Listing, ListError>,
        run: &mut CheckRun,
    ) -> Option<Listing> {
        match listed {
            Ok(listing) => Some(listing),
            Err(ListError::Build(product)) => {
                run.listing_build_failures.push((name.to_owned(), product));
                None
            }
            Err(ListError::Output(output)) => {
                run.incomplete_families.push((name.to_owned(), output));
                None
            }
        }
    }
}

/// Calls `work` on each item on a thread of its own, each with a fresh [`CheckRun`] to record
/// in, and returns what each call returned with its run, in the items' order. Each thread works
/// for the calling thread's activity, so its builds are charged to the same command and
/// `ignite status` shows what it does. A panic on a thread is raised again here.
pub(super) fn on_threads<I: Send, T: Send>(
    items: Vec<I>,
    work: impl Fn(I, &mut CheckRun) -> T + Sync,
) -> Vec<(T, CheckRun)> {
    let activity = crate::steps::current();
    let work = &work;
    thread::scope(|scope| {
        let threads: Vec<_> = items
            .into_iter()
            .map(|item| {
                let activity = activity.clone();
                scope.spawn(move || {
                    let _entered = activity.as_ref().map(crate::steps::Activity::enter);
                    let mut own = CheckRun::default();
                    let found = work(item, &mut own);
                    (found, own)
                })
            })
            .collect();
        threads
            .into_iter()
            .map(|thread| thread.join().unwrap_or_else(|panic| resume_unwind(panic)))
            .collect()
    })
}

/// One family to prepare: its name and declaration, the key its listing is kept under, the
/// kept listing found for it, if any, and the packages to list it for, if only some.
struct FamilyWork<'a> {
    name: &'a str,
    family: &'a Family,
    key: Option<ListingKey>,
    kept: Option<(ListingKey, Arc<Kept>)>,
    scope: Option<Vec<String>>,
    /// How much of the family's newest listing serves, when it has no kept listing.
    reuse: Option<Reuse>,
}

/// One piece of the landing gate's first round of listing: a product to build, or a family to
/// list that builds nothing to be listed.
enum Job<'a, 'p> {
    Build(&'p String),
    List(Box<FamilyWork<'a>>),
}

/// Whether listing has already found a fault that refuses a landing: a failed build or
/// listing, or a listed check that cannot be keyed, such as one whose manifest entry is invalid.
fn known_fault(prepared: &[Prepared], run: &CheckRun) -> bool {
    !run.build_failures.is_empty()
        || !run.incomplete_families.is_empty()
        || prepared.iter().any(|p| !p.broken.is_empty())
}
