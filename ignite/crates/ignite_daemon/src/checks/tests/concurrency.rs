//! Families are listed and run on threads of their own, so the products they need build at
//! once, in the landing gate as outside it.

use std::sync::Condvar;
use std::time::Duration;

use ignite_core::traits::{BuildFor, BuildResult, Builds};

use super::*;

/// How long a build waits for the others before it gives up. It only ends a test that would
/// otherwise hang: a runner that asks for products one at a time never lets them meet.
const HANG: Duration = Duration::from_secs(30);

/// Builds that finish only once `expected` requests have been made, each with the same
/// result: a build of 7 s. A runner that waits for one build before asking for the next gets
/// every build failed once [`HANG`] has passed.
struct MeetingBuilds {
    expected: usize,
    asked: Mutex<Vec<String>>,
    met: Condvar,
}

impl MeetingBuilds {
    fn new(expected: usize) -> Arc<Self> {
        Arc::new(Self {
            expected,
            asked: Mutex::new(Vec::new()),
            met: Condvar::new(),
        })
    }
}

impl Builds for MeetingBuilds {
    fn ensure(&self, _tree: &TreeId, product: &str, _packages: &[String]) -> BuildResult {
        let mut asked = self.asked.lock().unwrap();
        // A product asked for again after the meeting is current.
        if asked.len() >= self.expected {
            return BuildResult::Current;
        }
        asked.push(product.to_owned());
        self.met.notify_all();
        let (asked, timeout) = self
            .met
            .wait_timeout_while(asked, HANG, |asked| asked.len() < self.expected)
            .unwrap();
        drop(asked);
        if timeout.timed_out() {
            return BuildResult::Failed {
                output: PathBuf::from("never met"),
                usage: Usage::default(),
            };
        }
        BuildResult::Built {
            usage: Usage {
                wall_s: 7.0,
                cpu_s: 20.0,
                ..Usage::default()
            },
            units: Vec::new(),
        }
    }

    fn ensure_for(
        &self,
        tree: &TreeId,
        product: &str,
        packages: &[String],
        _who: &BuildFor,
    ) -> BuildResult {
        self.ensure(tree, product, packages)
    }

    fn has_built(&self, _tree: &TreeId, _product: &str) -> bool {
        true
    }

    fn forget(&self, _tree: &TreeId) {}
}

fn product(name: &str) -> String {
    format!(
        "[product.{name}]\nbuild = \"make {name}\"\ninputs = [\"src\"]\noutputs = [\"{name}\"]\n"
    )
}

fn family(name: &str, needs: &str) -> String {
    format!(
        "[family.{name}]\nkind = \"regression\"\nadapter = \"jsonl\"\nplatforms = \"any\"\n\
         needs = [\"{needs}\"]\nlist = \"lister\"\nrun = \"runner {{checks}}\"\n\
         inputs = [\"src\"]\n"
    )
}

/// `ignite test` of two families, each needing its own product, in a world whose builds are
/// `builds`.
fn test_both(manifest: &str, builds: &Arc<MeetingBuilds>) -> (World, CheckRun) {
    let world = world(manifest, &[], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() =
        "{\"check\": \"alpha:a\"}\n{\"check\": \"beta:b\"}\n".to_owned();
    let ctx = Context {
        builds: builds.clone(),
        ..world.ctx.clone()
    };
    let tree = TreeId::new("tree-01").unwrap();
    let names = [name("alpha:a"), name("beta:b")];
    let run = world.checks.run(&ctx, &tree, &names, &options(false));
    (world, run)
}

/// On 2026-11-19 `ignite test` of a one-line card change built the products of its families
/// one after another, listing each family only after the product before it was built. Two
/// families that need different products now have both built at once.
#[test]
fn the_products_of_two_families_build_at_once() {
    let manifest = [
        product("one"),
        product("two"),
        family("alpha", "one"),
        family("beta", "two"),
    ]
    .concat();
    let builds = MeetingBuilds::new(2);
    let (_, run) = test_both(&manifest, &builds);
    assert!(run.build_failures.is_empty(), "{:?}", run.build_failures);
    assert!(run.passed(), "{run:?}");
    let mut built: Vec<&str> = run.builds.iter().map(|b| b.product.as_str()).collect();
    built.sort_unstable();
    assert_eq!(built, ["one", "two"]);
    // What each family found is added in the families' order, as when they ran in turn.
    let checks: Vec<&str> = run.outcomes.iter().map(|(c, _)| c.as_str()).collect();
    assert_eq!(checks, ["alpha:a", "beta:b"]);
}

/// Two families that ask for one product at once are answered with the same build, which is
/// reported and recorded once.
#[test]
fn a_build_two_families_share_is_reported_once() {
    let manifest = [
        product("one"),
        family("alpha", "one"),
        family("beta", "one"),
    ]
    .concat();
    let builds = MeetingBuilds::new(2);
    let (world, run) = test_both(&manifest, &builds);
    assert!(run.passed(), "{run:?}");
    let built: Vec<&str> = run.builds.iter().map(|b| b.product.as_str()).collect();
    assert_eq!(built, ["one"]);
    let recorded = world
        .hub
        .stored_records()
        .into_iter()
        .filter(|r| r.verb == "build")
        .count();
    assert_eq!(recorded, 1);
}

/// On 2026-11-19 `ignite check` of a one-card comment change listed its families one after
/// another and built each product only when the listing reached the family that needed it. The
/// landing gate now builds every product its families need to be listed at once.
#[test]
fn the_gate_builds_the_products_its_listings_need_at_once() {
    let manifest = [
        product("one"),
        product("two"),
        family("alpha", "one"),
        family("beta", "two"),
    ]
    .concat();
    let builds = MeetingBuilds::new(2);
    let world = world(&manifest, &["src/a.py"], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() =
        "{\"check\": \"alpha:a\"}\n{\"check\": \"beta:b\"}\n".to_owned();
    let ctx = Context {
        builds: builds.clone(),
        ..world.ctx.clone()
    };
    let tree = TreeId::new("tree-01").unwrap();
    let options = RunOptions {
        verb: "check".to_owned(),
        ..options(false)
    };

    let gated = world.checks.gate(&ctx, &tree, &options);

    assert!(gated.run.build_failures.is_empty(), "{gated:?}");
    let mut built: Vec<&str> = gated
        .run
        .builds
        .iter()
        .map(|b| b.product.as_str())
        .collect();
    built.sort_unstable();
    assert_eq!(built, ["one", "two"]);
    assert_eq!(world.launcher.commands("lister").len(), 2);
}
