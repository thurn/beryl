use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use ignite_core::fakes::FakeGit;
use ignite_core::ids::{Oid, TreeId};
use ignite_core::inputs::{CrateGraph, Package};
use ignite_core::manifest::{InputPattern, Manifest, Product};
use ignite_core::protocol::Usage;
use ignite_core::traits::{BuildResult, Builds, CommandSpec, Launcher, Result, RunResult};

use super::*;

/// A launcher that can hold builds open until the test releases them, writes scripted compile
/// units to the file `IGNITE_UNITS` names, and answers `cargo tree` from scripted text.
struct TestLauncher {
    dir: PathBuf,
    gated: bool,
    state: Mutex<Launched>,
    changed: Condvar,
}

#[derive(Default)]
struct Launched {
    runs: Vec<CommandSpec>,
    builds: usize,
    active: usize,
    max_active: usize,
    released: usize,
    units: Vec<String>,
    whole_tree: String,
    subset_tree: String,
    /// Makes the next build command panic.
    panic_next: bool,
    /// While set, every build command fails with this exit code (`None` for a signal) and
    /// whether its supervisor ended it as hung, writing its output to a file.
    failing: Option<(Option<i32>, bool)>,
}

impl TestLauncher {
    fn new(dir: PathBuf, gated: bool) -> Self {
        let tree = "a v0.1.0 (/t/a)|\nserde feature \"std\"\nserde v1.0.0|default,std\n";
        Self {
            dir,
            gated,
            state: Mutex::new(Launched {
                whole_tree: tree.to_owned(),
                subset_tree: tree.to_owned(),
                ..Launched::default()
            }),
            changed: Condvar::new(),
        }
    }

    fn with<T>(&self, f: impl FnOnce(&mut Launched) -> T) -> T {
        let mut state = lock(&self.state);
        let value = f(&mut state);
        self.changed.notify_all();
        value
    }

    /// Blocks until `done` holds for the launcher's state.
    fn wait_for(&self, done: impl Fn(&Launched) -> bool) {
        let mut state = lock(&self.state);
        while !done(&state) {
            state = self.changed.wait(state).unwrap();
        }
    }

    fn release_one(&self) {
        self.with(|s| s.released += 1);
    }

    fn builds(&self) -> Vec<Vec<String>> {
        self.with(|s| {
            s.runs
                .iter()
                .filter(|r| r.args.get(1).map(String::as_str) != Some("tree"))
                .map(|r| r.args.clone())
                .collect()
        })
    }

    fn tree_runs(&self) -> usize {
        self.with(|s| s.runs.len()) - self.builds().len()
    }
}

impl Launcher for TestLauncher {
    fn run(&self, spec: &CommandSpec) -> RunResult {
        let done = |output: PathBuf| RunResult {
            exit: Some(0),
            output,
            usage: Usage {
                wall_s: 3.0,
                ..Usage::default()
            },
            hung: false,
        };
        let mut state = lock(&self.state);
        state.runs.push(spec.clone());
        let n = state.runs.len();
        if spec.args.get(1).map(String::as_str) == Some("tree") {
            let text = if spec.args.contains(&"--workspace".to_owned()) {
                state.whole_tree.clone()
            } else {
                state.subset_tree.clone()
            };
            let output = self.dir.join(format!("tree-{n}.txt"));
            fs::write(&output, text).unwrap();
            return done(output);
        }
        if std::mem::take(&mut state.panic_next) {
            drop(state);
            panic!("the launcher failed");
        }
        state.builds += 1;
        let mine = state.builds;
        state.active += 1;
        state.max_active = state.max_active.max(state.active);
        self.changed.notify_all();
        while self.gated && state.released < mine {
            state = self.changed.wait(state).unwrap();
        }
        let (_, units) = spec.env.iter().find(|(k, _)| k == UNITS_ENV).unwrap();
        fs::write(units, state.units.join("\n")).unwrap();
        state.active -= 1;
        self.changed.notify_all();
        if let Some((exit, hung)) = state.failing {
            let output = self.dir.join(format!("build-{n}.txt"));
            fs::write(&output, "error: the build failed\n").unwrap();
            return RunResult {
                exit,
                hung,
                ..done(output)
            };
        }
        done(PathBuf::new())
    }
}

struct OneTree {
    path: PathBuf,
    graph: CrateGraph,
}

impl TreeFacts for OneTree {
    fn path(&self, tree: &TreeId) -> Option<PathBuf> {
        (tree.as_str() == "tree-01").then(|| self.path.clone())
    }
    fn tracked(&self, _path: &Path) -> Result<Vec<String>> {
        Ok(TRACKED.iter().map(|p| (*p).to_owned()).collect())
    }
    fn crate_graph(&self, _path: &Path, _tracked: &[String]) -> Result<CrateGraph> {
        Ok(self.graph.clone())
    }
}

const TRACKED: &[&str] = &[
    "Cargo.lock",
    "Cargo.toml",
    "a/Cargo.toml",
    "a/src/lib.rs",
    "b/Cargo.toml",
    "b/src/lib.rs",
    "docs/notes.txt",
];

struct Fixture {
    manager: Arc<BuildManager>,
    launcher: Arc<TestLauncher>,
    git: Arc<FakeGit>,
    dir: PathBuf,
}

fn oid(c: char) -> Oid {
    Oid::new(c.to_string().repeat(40)).unwrap()
}

fn product(build: &str, inputs: &[&str]) -> Product {
    Product {
        build: build.to_owned(),
        dir: None,
        inputs: inputs
            .iter()
            .map(|i| InputPattern::from((*i).to_owned()))
            .collect(),
        outputs: vec!["target".to_owned()],
        grows: false,
    }
}

fn fixture(name: &str, gated: bool) -> Fixture {
    let dir = std::env::temp_dir().join(format!("ignite-build-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let mut manifest = Manifest::default();
    manifest.products.insert(
        "tests".to_owned(),
        product("cargo test --no-run {packages}", &["crate-closure"]),
    );
    manifest.products.insert(
        "lint".to_owned(),
        product("cargo clippy {packages}", &["crate-closure"]),
    );
    let git = Arc::new(FakeGit::default());
    for path in [
        "Cargo.lock",
        "Cargo.toml",
        "a/Cargo.toml",
        "b/Cargo.toml",
        "a",
        "b",
        "docs",
    ] {
        git.set_object(path, oid('1'));
    }
    let package = |name: &str, deps: &[&str]| Package {
        name: name.to_owned(),
        dir: name.to_owned(),
        deps: deps.iter().map(|d| (*d).to_owned()).collect(),
        workspace: String::new(),
    };
    let trees = Arc::new(OneTree {
        path: dir.clone(),
        graph: CrateGraph {
            packages: vec![package("a", &[]), package("b", &["a"])],
        },
    });
    let launcher = Arc::new(TestLauncher::new(dir.clone(), gated));
    let settings = BuildSettings {
        manifest,
        records: dir.join("records"),
        cores: 8,
        class: Class::Agent,
    };
    let manager = Arc::new(BuildManager::new(
        settings,
        Arc::clone(&launcher) as Arc<dyn Launcher>,
        Arc::clone(&git) as Arc<dyn Git>,
        trees,
    ));
    Fixture {
        manager,
        launcher,
        git,
        dir,
    }
}

fn tree() -> TreeId {
    TreeId::new("tree-01").unwrap()
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

/// Waits, without a timeout, until the manager reports the given waiting requests.
fn until_waiting(manager: &BuildManager, want: (usize, usize)) {
    while manager.waiting() != want {
        thread::yield_now();
    }
}

#[test]
fn an_unchanged_tree_starts_no_build() {
    let f = fixture("unchanged", false);
    let first = f.manager.ensure(&tree(), "tests", &names(&["b"]));
    assert!(matches!(first, BuildResult::Built { .. }), "{first:?}");
    // An edit outside the product's inputs leaves it current.
    f.git.set_object("docs", oid('2'));
    assert_eq!(
        f.manager.ensure(&tree(), "tests", &names(&["b"])),
        BuildResult::Current
    );
    assert_eq!(f.launcher.builds().len(), 1);
}

#[test]
fn a_changed_input_starts_one_build() {
    let f = fixture("changed", false);
    f.manager.ensure(&tree(), "tests", &names(&["b"]));
    // `b` depends on `a`, so an edit to `a` changes the inputs of `b`'s tests.
    f.git.set_object("a", oid('2'));
    let again = f.manager.ensure(&tree(), "tests", &names(&["b"]));
    assert!(matches!(again, BuildResult::Built { .. }), "{again:?}");
    assert_eq!(
        f.manager.ensure(&tree(), "tests", &names(&["b"])),
        BuildResult::Current
    );
    assert_eq!(f.launcher.builds().len(), 2);
    // Only the packages named are built, in the tree's directory.
    assert_eq!(
        f.launcher.builds()[0],
        names(&["cargo", "test", "--no-run", "-p", "b"])
    );
    let spec = &f.launcher.with(|s| s.runs.last().cloned()).unwrap();
    assert_eq!(spec.cwd, f.dir);
    assert_eq!(spec.tree, Some(tree()));
}

#[test]
fn two_requests_at_once_start_one_build() {
    let f = fixture("join", true);
    let first = {
        let manager = Arc::clone(&f.manager);
        thread::spawn(move || manager.ensure(&tree(), "tests", &names(&["b"])))
    };
    f.launcher.wait_for(|s| s.active == 1);
    let second = {
        let manager = Arc::clone(&f.manager);
        thread::spawn(move || manager.ensure(&tree(), "tests", &names(&["b"])))
    };
    until_waiting(&f.manager, (1, 0));
    f.launcher.release_one();
    let (first, second) = (first.join().unwrap(), second.join().unwrap());
    assert!(matches!(first, BuildResult::Built { .. }), "{first:?}");
    assert_eq!(first, second);
    assert_eq!(f.launcher.builds().len(), 1);
}

#[test]
fn products_sharing_a_target_directory_never_overlap() {
    let f = fixture("overlap", true);
    let tests = {
        let manager = Arc::clone(&f.manager);
        thread::spawn(move || manager.ensure(&tree(), "tests", &[]))
    };
    f.launcher.wait_for(|s| s.active == 1);
    let lint = {
        let manager = Arc::clone(&f.manager);
        thread::spawn(move || manager.ensure(&tree(), "lint", &[]))
    };
    until_waiting(&f.manager, (0, 1));
    assert_eq!(f.launcher.builds().len(), 1);
    f.launcher.release_one();
    f.launcher.wait_for(|s| s.builds == 2);
    f.launcher.release_one();
    assert!(matches!(tests.join().unwrap(), BuildResult::Built { .. }));
    assert!(matches!(lint.join().unwrap(), BuildResult::Built { .. }));
    assert_eq!(f.launcher.with(|s| s.max_active), 1);
    assert_eq!(
        f.launcher.builds()[1],
        names(&["cargo", "clippy", "--workspace"])
    );
}

#[test]
fn a_subset_that_changes_shared_features_fails_naming_the_crate() {
    let f = fixture("features", false);
    f.launcher.with(|s| {
        s.subset_tree = "a v0.1.0 (/t/a)|\nserde v1.0.0|default,std (*)\nlibc v0.2.0|\n".into();
        s.whole_tree = "a v0.1.0 (/t/a)|\nserde v1.0.0|default,derive,std\nlibc v0.2.0|\n".into();
    });
    let result = f.manager.ensure(&tree(), "tests", &names(&["a"]));
    let BuildResult::Failed { output, .. } = result else {
        panic!("expected a failure, got {result:?}");
    };
    let text = fs::read_to_string(output).unwrap();
    assert!(text.contains("on: serde\n"), "{text}");
    assert!(f.launcher.builds().is_empty());
    // The comparison is remembered until a Cargo file changes.
    f.manager.ensure(&tree(), "tests", &names(&["a"]));
    assert_eq!(f.launcher.tree_runs(), 2);
    f.git.set_object("Cargo.lock", oid('2'));
    f.manager.ensure(&tree(), "tests", &names(&["a"]));
    assert_eq!(f.launcher.tree_runs(), 4);
}

#[test]
fn matching_features_build_and_report_the_crates_recompiled() {
    let f = fixture("units", false);
    let unit = |name: &str, rebuilt: bool| {
        format!(
            r#"{{"crate":"{name}","mode":"test","instructions":1e9,"cpu_s":1.0,"rebuilt":{rebuilt}}}"#
        )
    };
    f.launcher.with(|s| {
        s.units = vec![
            unit("a", true),
            unit("a", true),
            unit("b", false),
            "not json".into(),
        ];
    });
    let result = f.manager.ensure(&tree(), "tests", &names(&["a"]));
    let BuildResult::Built { usage, units } = result else {
        panic!("expected a build, got {result:?}");
    };
    assert!((usage.wall_s - 3.0).abs() < f64::EPSILON);
    assert_eq!(units.len(), 3);
    assert_eq!(recompiled(&units), 1);
}

#[test]
fn unknown_products_and_trees_fail_with_a_reason() {
    let f = fixture("unknown", false);
    for (tree, product) in [
        (tree(), "nothing"),
        (TreeId::new("tree-99").unwrap(), "tests"),
    ] {
        let BuildResult::Failed { output, .. } = f.manager.ensure(&tree, product, &[]) else {
            panic!("expected a failure");
        };
        assert!(
            fs::read_to_string(output)
                .unwrap()
                .starts_with("cannot build")
        );
    }
}

#[test]
fn forgetting_a_tree_builds_again() {
    let f = fixture("forget", false);
    f.manager.ensure(&tree(), "tests", &[]);
    f.manager.forget(&tree());
    assert!(matches!(
        f.manager.ensure(&tree(), "tests", &[]),
        BuildResult::Built { .. }
    ));
}

#[test]
fn feature_differences_ignore_feature_nodes_and_repeats() {
    let whole = "x v1|a,b\nx feature \"a\"\nx v1|a,b (*)\ny v2|\n";
    assert!(feature_differences(whole, "x v1|a,b\ny v2|\n").is_empty());
    assert_eq!(feature_differences(whole, "x v1|a\n"), ["x"]);
}

#[test]
fn a_build_that_panics_frees_its_directories_and_the_next_request_builds() {
    let f = fixture("panics", false);
    f.launcher.with(|s| s.panic_next = true);
    let manager = Arc::clone(&f.manager);
    let panicked = thread::spawn(move || manager.ensure(&tree(), "tests", &names(&["b"]))).join();
    assert!(panicked.is_err());
    assert_eq!(f.manager.waiting(), (0, 0));
    let next = f.manager.ensure(&tree(), "tests", &names(&["b"]));
    assert!(matches!(next, BuildResult::Built { .. }), "{next:?}");
}

#[test]
fn builds_and_feature_checks_run_with_a_hang_limit() {
    let f = fixture("hang-limit", false);
    f.manager.ensure(&tree(), "tests", &names(&["b"]));
    let limits: Vec<Option<u64>> = f
        .launcher
        .with(|s| s.runs.iter().map(|r| r.hang_limit_s).collect());
    assert!(!limits.is_empty());
    assert!(
        limits.iter().all(|l| *l == Some(BUILD_HANG_LIMIT_S)),
        "{limits:?}"
    );
}

#[test]
fn a_build_that_exits_failing_is_not_run_again_until_an_input_changes() {
    let f = fixture("failed", false);
    f.launcher.with(|s| s.failing = Some((Some(1), false)));
    let first = f.manager.ensure(&tree(), "tests", &names(&["b"]));
    let BuildResult::Failed { output, .. } = &first else {
        panic!("expected a failure, got {first:?}");
    };
    // The same inputs get the same failure, and its output, without a second launch.
    let again = f.manager.ensure(&tree(), "tests", &names(&["b"]));
    assert_eq!(
        again,
        BuildResult::Failed {
            output: output.clone(),
            usage: Usage::default()
        }
    );
    assert_eq!(f.launcher.builds().len(), 1);
    // An edit to an input builds again.
    f.git.set_object("a", oid('2'));
    let edited = f.manager.ensure(&tree(), "tests", &names(&["b"]));
    assert!(matches!(edited, BuildResult::Failed { .. }), "{edited:?}");
    assert_eq!(f.launcher.builds().len(), 2);
    // Re-leasing the tree builds again too, and a success replaces the remembered failure.
    f.launcher.with(|s| s.failing = None);
    f.manager.forget(&tree());
    let fixed = f.manager.ensure(&tree(), "tests", &names(&["b"]));
    assert!(matches!(fixed, BuildResult::Built { .. }), "{fixed:?}");
    assert_eq!(f.launcher.builds().len(), 3);
}

#[test]
fn a_build_ended_by_a_signal_or_a_hang_is_tried_again() {
    for (name, exit, hung) in [("signal", None, false), ("hung", Some(1), true)] {
        let f = fixture(name, false);
        f.launcher.with(|s| s.failing = Some((exit, hung)));
        for _ in 0..2 {
            let result = f.manager.ensure(&tree(), "tests", &names(&["b"]));
            assert!(matches!(result, BuildResult::Failed { .. }), "{result:?}");
        }
        assert_eq!(f.launcher.builds().len(), 2, "{name}");
    }
}

#[test]
fn a_failed_build_leaves_the_tree_holding_no_built_state() {
    let f = fixture("failed-held", false);
    f.manager.ensure(&tree(), "tests", &names(&["b"]));
    f.git.set_object("a", oid('2'));
    f.launcher.with(|s| s.failing = Some((Some(1), false)));
    f.manager.ensure(&tree(), "tests", &names(&["b"]));
    // Back at the state built first: the failed build may have overwritten it, so it builds.
    f.launcher.with(|s| s.failing = None);
    f.git.set_object("a", oid('1'));
    let back = f.manager.ensure(&tree(), "tests", &names(&["b"]));
    assert!(matches!(back, BuildResult::Built { .. }), "{back:?}");
    assert_eq!(f.launcher.builds().len(), 3);
}
