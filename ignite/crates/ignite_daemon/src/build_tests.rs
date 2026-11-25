use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use ignite_core::fakes::FakeGit;
use ignite_core::ids::{Oid, TreeId};
use ignite_core::inputs::{CrateGraph, Package};
use ignite_core::manifest::{InputPattern, Manifest, Product};
use ignite_core::protocol::Usage;
use ignite_core::traits::{
    BuildResult, Builds, Clock as _, CommandSpec, Launcher, Result, RunResult,
};

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
    /// Outputs that the next build commands fail with, exit code 101, one each, before
    /// `failing` applies.
    fail_with: std::collections::VecDeque<String>,
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
        if let Some(text) = state.fail_with.pop_front() {
            let output = self.dir.join(format!("build-{n}.txt"));
            fs::write(&output, text).unwrap();
            return RunResult {
                exit: Some(101),
                ..done(output)
            };
        }
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

pub(crate) struct Fixture {
    pub(crate) manager: Arc<BuildManager>,
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
        compiles: false,
        grows: false,
        exclude: Vec::new(),
    }
}

pub(crate) fn fixture(name: &str, gated: bool) -> Fixture {
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-build-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
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
    manager_in(dir, git, gated)
}

/// A manager over the tree in `dir`, with a new launcher, as a restarted daemon has.
fn manager_in(dir: PathBuf, git: Arc<FakeGit>, gated: bool) -> Fixture {
    let mut manifest = Manifest::default();
    manifest.products.insert(
        "tests".to_owned(),
        product("cargo test --no-run {packages}", &["crate-closure"]),
    );
    manifest.products.insert(
        "lint".to_owned(),
        product("cargo clippy {packages}", &["crate-closure"]),
    );
    manifest.products.insert(
        "scripted-compile".to_owned(),
        Product {
            compiles: true,
            ..product("script build", &["docs/**"])
        },
    );
    let mut less_b = product("cargo test --no-run {packages}", &["crate-closure"]);
    less_b.exclude = names(&["b"]);
    manifest.products.insert("tests-less-b".to_owned(), less_b);
    manifest.products.insert(
        "fixed".to_owned(),
        product("cargo test --no-run -p b", &["crate-closure"]),
    );
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
    // On Windows the shims are links to this file, so it has to exist.
    fs::create_dir_all(dir.join("bin")).unwrap();
    fs::write(dir.join("bin").join("ignite"), "program").unwrap();
    let launcher = Arc::new(TestLauncher::new(dir.clone(), gated));
    let settings = BuildSettings {
        manifest: manifest.into(),
        checks: PathBuf::from("checks"),
        records: dir.join("records"),
        cores: 8,
        class: Class::Agent,
        ignite: dir.join("bin").join("ignite"),
        shims: dir.join("shims"),
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
fn matching_features_build_and_report_the_crates_recompiled() {
    let f = fixture("units", false);
    let unit = |name: &str, rebuilt: bool| {
        format!(
            r#"{{"crate":"{name}","mode":"test","instructions":1e9,"cpu_s":1.0,"rebuilt":{rebuilt}}}"#
        )
    };
    // `c` is a run of a test binary, which compiles nothing.
    let run = r#"{"crate":"c","mode":"run","instructions":1e9,"cpu_s":1.0,"rebuilt":false}"#;
    f.launcher.with(|s| {
        s.units = vec![
            unit("a", true),
            unit("a", true),
            unit("b", false),
            run.into(),
            "not json".into(),
        ];
    });
    let result = f.manager.ensure(&tree(), "tests", &names(&["a"]));
    let BuildResult::Built { usage, units } = result else {
        panic!("expected a build, got {result:?}");
    };
    assert!((usage.wall_s - 3.0).abs() < f64::EPSILON);
    assert_eq!(units.len(), 4);
    // `b` had no files before this build, and it still compiled: `a` twice counts once.
    assert_eq!(recompiled(&units), 2);
}

/// A build that ran without `RUSTC_WRAPPER` recorded no compile units, so every rebuild count
/// failed with "carry no instruction counts". Each build must run through the wrapper script,
/// which calls the daemon's own `ignite` and records into the build's units file.
#[test]
fn builds_compile_through_the_wrapper_that_records_units() {
    let f = fixture("wrapper", false);
    f.manager.ensure(&tree(), "tests", &names(&["a"]));
    let spec = f.launcher.with(|s| s.runs.last().unwrap().clone());
    let var = |name: &str| {
        let found = spec.env.iter().find(|(k, _)| k == name);
        let (_, value) =
            found.unwrap_or_else(|| panic!("the build's env has no {name}: {:?}", spec.env));
        value.clone()
    };
    let wrapper = PathBuf::from(var("RUSTC_WRAPPER"));
    assert_eq!(
        wrapper,
        f.dir.join("shims").join(crate::wrap::RUSTC_WRAP_SCRIPT)
    );
    let script = fs::read_to_string(&wrapper).unwrap();
    let ignite = f.dir.join("bin").join("ignite");
    if cfg!(windows) {
        // The shim is the executable itself, which names its verb by its own file name.
        assert_eq!(script, "program");
    } else {
        assert!(script.contains("rustc-wrap"), "{script}");
        assert!(script.contains(&*ignite.to_string_lossy()), "{script}");
    }
    let units = PathBuf::from(var(UNITS_ENV));
    assert_eq!(units.parent(), Some(f.dir.join("records").as_path()));
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

/// A tree whose manifest directory changes a product builds the product as the tree declares
/// it, not as the manifests the daemon holds do, and a product only the tree declares builds.
#[test]
fn a_tree_builds_the_products_its_own_manifests_declare() {
    let f = fixture("tree-manifest", false);
    fs::create_dir_all(f.dir.join("checks")).unwrap();
    fs::write(
        f.dir.join("checks").join("products.toml"),
        "[product.fixed]\nbuild = \"cargo test --no-run -p a\"\ninputs = [\"crate-closure\"]\n\
         outputs = [\"target\"]\n\n[product.new]\nbuild = \"cargo build -p b\"\n\
         inputs = [\"crate-closure\"]\noutputs = [\"target\"]\n",
    )
    .unwrap();
    for product in ["fixed", "new"] {
        let built = f.manager.ensure(&tree(), product, &[]);
        assert!(matches!(built, BuildResult::Built { .. }), "{built:?}");
    }
    assert_eq!(
        f.launcher.builds(),
        vec![
            names(&["cargo", "test", "--no-run", "-p", "a"]),
            names(&["cargo", "build", "-p", "b"]),
        ]
    );
    // Manifests the tree has but that do not parse build nothing, rather than the held ones.
    fs::write(f.dir.join("checks").join("broken.toml"), "[product.fixed\n").unwrap();
    let BuildResult::Failed { output, .. } = f.manager.ensure(&tree(), "tests", &[]) else {
        panic!("expected a failure");
    };
    assert!(
        fs::read_to_string(output)
            .unwrap()
            .contains("the manifests of tree")
    );
    assert_eq!(f.launcher.builds().len(), 2);
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

/// The landing gate leaves a check to tip verification when its product would be a cold build
/// in the gate's tree, so the manager must know which products a tree has built before, also
/// after a restart of the daemon.
#[test]
fn a_tree_has_built_a_product_it_built_or_whose_outputs_it_holds() {
    let f = fixture("has-built", false);
    assert!(!f.manager.has_built(&tree(), "tests"));
    f.manager.ensure(&tree(), "tests", &names(&["b"]));
    assert!(f.manager.has_built(&tree(), "tests"));
    assert!(
        !f.manager
            .has_built(&TreeId::new("tree-02").unwrap(), "tests")
    );

    // A new manager, as after a restart, knows only what the tree's directories hold: an empty
    // output directory is not a build, one with files is.
    let fresh = fixture("has-built-fresh", false);
    assert!(!fresh.manager.has_built(&tree(), "lint"));
    fs::create_dir_all(fresh.dir.join("target")).unwrap();
    assert!(!fresh.manager.has_built(&tree(), "lint"));
    fs::write(fresh.dir.join("target").join("CACHEDIR.TAG"), "x").unwrap();
    assert!(fresh.manager.has_built(&tree(), "lint"));
    assert!(!fresh.manager.has_built(&tree(), "no-such-product"));
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
    // A wrapper such as `uv run` exits with 128 plus the signal that ended its child.
    let cases = [
        ("signal", None, false),
        ("hung", Some(1), true),
        ("wrapper-term", Some(143), false),
        ("wrapper-kill", Some(137), false),
    ];
    for (name, exit, hung) in cases {
        let f = fixture(name, false);
        f.launcher.with(|s| s.failing = Some((exit, hung)));
        for _ in 0..2 {
            let result = f.manager.ensure(&tree(), "tests", &names(&["b"]));
            assert!(matches!(result, BuildResult::Failed { .. }), "{result:?}");
        }
        assert_eq!(f.launcher.builds().len(), 2, "{name}");
    }
}

/// `ignite check --rerun` asks for its builds with `retry_failure`, which builds again where the
/// same inputs' failure is remembered, so a worker is never stuck behind a failure that was not
/// the change's.
#[test]
fn a_request_that_retries_failures_builds_again_over_a_remembered_failure() {
    let f = fixture("retry-failure", false);
    f.launcher.with(|s| s.failing = Some((Some(1), false)));
    let first = f.manager.ensure(&tree(), "tests", &names(&["b"]));
    assert!(matches!(first, BuildResult::Failed { .. }), "{first:?}");
    let who = |retry_failure| BuildFor {
        class: Class::Agent,
        ticket: None,
        session: None,
        retry_failure,
    };
    f.manager
        .ensure_for(&tree(), "tests", &names(&["b"]), &who(false));
    assert_eq!(f.launcher.builds().len(), 1, "a plain request reuses it");

    f.launcher.with(|s| s.failing = None);
    let retried = f
        .manager
        .ensure_for(&tree(), "tests", &names(&["b"]), &who(true));
    assert!(matches!(retried, BuildResult::Built { .. }), "{retried:?}");
    assert_eq!(f.launcher.builds().len(), 2);
    assert_eq!(
        f.manager.ensure(&tree(), "tests", &names(&["b"])),
        BuildResult::Current
    );
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

#[test]
fn a_build_runs_in_the_class_of_the_work_that_asked_and_names_itself_as_its_step() {
    // A landing's listing needs a build. Before, every build ran in the class of the build
    // manager's settings (`agent`), so `ignite status --admission` counted a landing's builds
    // under `agent` and showed `land` with nothing running.
    let fixture = fixture("class", true);
    let steps = crate::steps::Steps::new(Arc::new(ignite_core::time::SystemClock));
    let ticket = ignite_core::ids::TicketId::new("Tqtj2").unwrap();
    let landing = steps.begin(
        crate::steps::Kind::Verb("land".to_owned()),
        crate::steps::Work {
            class: Some(Class::Land),
            ticket: Some(ticket.clone()),
            ..crate::steps::Work::default()
        },
    );
    let worker = {
        let (manager, landing) = (fixture.manager.clone(), landing.clone());
        thread::spawn(move || {
            let _entered = landing.enter();
            crate::steps::step("listing family rust");
            manager.ensure(&tree(), "tests", &names(&["a"]))
        })
    };
    fixture.launcher.wait_for(|s| s.active == 1);
    assert_eq!(
        landing.view().step.as_deref(),
        Some("building tests in tree-01")
    );
    fixture.launcher.release_one();
    assert!(matches!(worker.join().unwrap(), BuildResult::Built { .. }));
    let build = fixture
        .launcher
        .with(|s| s.runs.iter().rfind(|r| r.args[1] == "test").cloned())
        .unwrap();
    assert_eq!(build.class, Class::Land);
    assert_eq!(build.ticket, Some(ticket));
    assert_eq!(
        landing.view().step.as_deref(),
        Some("listing family rust"),
        "the listing's step comes back once the build is done"
    );
    // Work that entered no activity keeps the settings' class.
    fixture.launcher.release_one();
    fixture.manager.ensure(&tree(), "lint", &[]);
    let lint = fixture
        .launcher
        .with(|s| s.runs.iter().rfind(|r| r.args[1] == "clippy").cloned());
    assert!(lint.is_none_or(|run| run.class == Class::Agent));
}

#[test]
fn the_idle_loop_builds_its_free_tree_as_idle_work_and_status_names_the_build() {
    // On jarvis the idle loop sat 35 minutes in one round, prebuilding a free tree, while
    // `ignite status --admission` showed `idle` with nothing running: the prebuild's builds ran
    // as `agent` work, queued behind the agents' own, and nothing said what the round did.
    let fixture = fixture("idle-loop", true);
    let clock = Arc::new(ignite_core::fakes::FakeClock::new(
        ignite_core::time::Timestamp::EPOCH,
    ));
    let health = crate::loop_health::LoopHealth::new(clock.clone());
    let ticker = health
        .register("ignite-idle", std::time::Duration::from_secs(600))
        .with_class(Class::Idle);
    let round = {
        let manager = fixture.manager.clone();
        thread::spawn(move || {
            ticker.round(|| {
                crate::steps::step("keeping a free tree built at the tip");
                manager.ensure(&tree(), "tests", &[]);
                Ok::<_, String>(())
            })
        })
    };
    fixture.launcher.wait_for(|s| s.active == 1);
    clock.advance(21.0 * 60.0);
    let report = &health.reports()[0];
    assert!(
        report
            .line(clock.now())
            .contains("in a round for 21 min: building tests in tree-01 (21 min)"),
        "{}",
        report.line(clock.now())
    );
    assert!(
        report.accounted_for(),
        "a round that names its build is not called stuck"
    );
    fixture.launcher.release_one();
    round.join().unwrap();
    let build = fixture
        .launcher
        .with(|s| s.runs.iter().rfind(|r| r.args[1] == "test").cloned())
        .unwrap();
    assert_eq!(build.class, Class::Idle);
}

#[path = "build/wait_tests.rs"]
mod waits;

#[path = "build/restart_tests.rs"]
mod restart;

#[path = "build/repair_build_tests.rs"]
mod repair;

#[path = "build/room_tests.rs"]
mod room;

#[path = "build/cargo_tests.rs"]
mod cargo;

#[test]
fn a_script_that_compiles_takes_a_slot_even_with_path_only_inputs() {
    let f = fixture("script-uses-slot", false);
    let plan = f.manager.plan(&tree(), "scripted-compile", &[]).unwrap();
    assert!(f.manager.tracks_compiled_outputs(&plan));
    assert!(!plan.reads_crates);
}
