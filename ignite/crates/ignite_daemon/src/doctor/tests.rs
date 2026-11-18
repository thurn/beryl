use ignite_core::fakes::FakeHub;

use super::*;

struct FakeHost {
    existing: Vec<PathBuf>,
    free_gb: u64,
    tools: Vec<&'static str>,
}

impl Host for FakeHost {
    fn hostname(&self) -> String {
        "testbox".to_owned()
    }
    fn cores(&self) -> u32 {
        8
    }
    fn memory_mb(&self) -> u64 {
        16384
    }
    fn disk_free_gb(&self, _path: &Path) -> Option<u64> {
        Some(self.free_gb)
    }
    fn tool_path(&self, name: &str) -> Option<PathBuf> {
        self.tools
            .contains(&name)
            .then(|| PathBuf::from("/bin").join(name))
    }
    fn path_exists(&self, path: &Path) -> bool {
        self.existing.iter().any(|p| p == path)
    }
}

struct FakeTrees(Vec<TreeInfo>);

impl TreeSource for FakeTrees {
    fn trees(&self) -> Result<Vec<TreeInfo>> {
        Ok(self.0.clone())
    }
}

fn config() -> Config {
    Config::parse(
        "ignite.toml",
        r#"
        [repository]
        remote = "origin"
        staging = "staging"
        master = "master"
        trees = "trees"
        checks = "checks"
        [hub]
        url = "http://hub.example:8443"
        project = "test"
        [gate]
        threshold = 0.01
        cap = 10
        [machine.testbox]
        os = "macos"
        ticket_trees = 2
        landing_trees = 1
        trees_disk_gb = 100
        "#,
    )
    .unwrap()
}

fn manifest() -> Manifest {
    Manifest::from_files(&[(
        PathBuf::from("checks/rust.toml"),
        r#"
        [product.build]
        build = "cargo build"
        inputs = ["src"]
        outputs = ["target"]
        [family.units]
        kind = "regression"
        adapter = "nextest"
        platforms = "any"
        run = "cargo nextest run"
        inputs = ["src"]
        "#
        .to_owned(),
    )])
    .unwrap()
}

fn tree(name: &str) -> TreeInfo {
    TreeInfo {
        name: name.to_owned(),
        path: PathBuf::from("/repo/trees").join(name),
        head: None,
    }
}

fn healthy_host() -> FakeHost {
    FakeHost {
        existing: [
            "/repo/trees",
            "/repo/trees/tree-01",
            "/repo/trees/tree-02",
            "/repo/trees/landing-1",
        ]
        .map(PathBuf::from)
        .to_vec(),
        free_gb: 500,
        tools: vec!["git", "cargo"],
    }
}

fn diagnose_with(host: &FakeHost, hub: &FakeHub, trees: &FakeTrees) -> Vec<Finding> {
    diagnose(&DoctorInput {
        config: &config(),
        repo: Path::new("/repo"),
        host,
        trees,
        hub,
        manifest: Ok(manifest()),
        version: Commit::new("abc1234").ok(),
    })
}

fn all_trees() -> FakeTrees {
    FakeTrees(vec![tree("tree-01"), tree("tree-02"), tree("landing-1")])
}

#[test]
fn a_healthy_machine_has_no_problems() {
    let findings = diagnose_with(&healthy_host(), &FakeHub::default(), &all_trees());
    let problems: Vec<_> = findings.iter().filter(|f| !f.ok).collect();
    assert!(problems.is_empty(), "{problems:?}");
    let reply = report(&findings);
    assert_eq!(reply.exit, Exit::Success);
    assert!(reply.text.ends_with("healthy"));
}

#[test]
fn an_unreachable_hub_and_a_missing_tree_each_get_a_line_with_a_fix() {
    let hub = FakeHub::default();
    hub.set_unreachable(true);
    let mut host = healthy_host();
    host.existing
        .retain(|p| p != Path::new("/repo/trees/tree-02"));
    let findings = diagnose_with(&host, &hub, &all_trees());
    let reply = report(&findings);

    let hub_line = reply.text.lines().find(|l| l.contains("hub")).unwrap();
    assert!(hub_line.starts_with("PROBLEM"), "{hub_line}");
    assert!(
        hub_line.contains("cannot reach the hub at http://hub.example:8443"),
        "{hub_line}"
    );
    assert!(
        hub_line.contains("To fix: check that Tailscale"),
        "{hub_line}"
    );

    let tree_line = reply.text.lines().find(|l| l.contains("tree-02")).unwrap();
    assert!(tree_line.starts_with("PROBLEM"), "{tree_line}");
    assert!(
        tree_line.contains("/repo/trees/tree-02 is missing"),
        "{tree_line}"
    );
    assert!(
        tree_line.contains("To fix: run `git worktree prune`"),
        "{tree_line}"
    );

    assert_ne!(hub_line, tree_line);
    assert_eq!(reply.exit, Exit::Unreachable);
    assert!(reply.text.ends_with("2 problems"), "{}", reply.text);
}

#[test]
fn a_missing_tree_directory_is_one_problem() {
    let mut host = healthy_host();
    host.existing.clear();
    let findings = diagnose_with(&host, &FakeHub::default(), &FakeTrees(vec![]));
    let line = findings.iter().find(|f| f.label == "trees").unwrap().line();
    assert!(
        line.contains("tree directory /repo/trees is missing"),
        "{line}"
    );
    assert!(line.contains("To fix: restart the daemon"), "{line}");
}

#[test]
fn low_disk_and_missing_tools_are_reported() {
    let mut host = healthy_host();
    host.free_gb = 20;
    host.tools = vec!["git"];
    let findings = diagnose_with(&host, &FakeHub::default(), &all_trees());
    let disk = findings.iter().find(|f| f.label == "disk").unwrap();
    assert!(!disk.ok && disk.text.contains("20 GB free"), "{disk:?}");
    let tools = findings.iter().find(|f| f.label == "tools").unwrap();
    assert!(!tools.ok && tools.text.contains("cargo"), "{tools:?}");
}

#[test]
fn status_shows_the_sections_its_flags_ask_for() {
    let status = Arc::new(|| sched::Status {
        cores: 10,
        outside: 1,
        granted: 4,
        max_per_command: 6,
        free_memory_mb: Some(2048),
        classes: vec![],
        resources: vec![],
    });
    let verb = StatusVerb::new(vec![
        Box::new(CoresSection(status.clone())),
        Box::new(AdmissionSection(status)),
    ]);
    let plain = verb.render("m-1", None, &[]);
    assert!(plain.text.contains("4 of 10 granted"), "{}", plain.text);
    assert!(
        !plain
            .json
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("admission")
    );
    let with = verb.render("m-1", None, &["--admission".to_owned()]);
    assert!(
        with.json
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("admission")
    );
}

#[test]
fn status_counts_only_the_declared_trees_and_names_the_others() {
    let host = healthy_host();
    let mut existing = host.existing.clone();
    existing.push(PathBuf::from("/repo/trees/switch-rebase"));
    let section = TreesSection {
        source: Arc::new(FakeTrees(vec![
            tree("tree-01"),
            tree("tree-02"),
            tree("landing-1"),
            tree("switch-rebase"),
        ])),
        configured: configured_tree_names(2, 1),
        host: Arc::new(FakeHost { existing, ..host }),
    };
    assert_eq!(
        section.lines(),
        ["3 of 3 present", "not in ignite.toml: switch-rebase"]
    );
}

/// Git's trees, and directories under the trees folder that git may not know.
struct TreesAndDirectories(Vec<TreeInfo>, Vec<&'static str>);

impl TreeSource for TreesAndDirectories {
    fn trees(&self) -> Result<Vec<TreeInfo>> {
        Ok(self.0.clone())
    }
    fn directories(&self) -> Vec<String> {
        self.1.iter().map(|name| (*name).to_owned()).collect()
    }
}

#[test]
fn doctor_warns_about_directories_under_the_trees_folder_it_does_not_own() {
    let mut trees = all_trees().0;
    // A worktree git knows, and a plain directory it does not.
    trees.push(tree("switch-rebase"));
    let source = TreesAndDirectories(
        trees,
        vec![
            "tree-01",
            "tree-02",
            "landing-1",
            "switch-rebase",
            "slot-01",
        ],
    );
    let findings = diagnose(&DoctorInput {
        config: &config(),
        repo: Path::new("/repo"),
        host: &healthy_host(),
        trees: &source,
        hub: &FakeHub::default(),
        manifest: Ok(manifest()),
        version: Commit::new("abc1234").ok(),
    });
    let warning = findings.iter().find(|f| f.warning).expect("a warning");
    let line = warning.line();
    assert!(line.starts_with("WARNING  trees"), "{line}");
    assert!(line.contains("slot-01, switch-rebase."), "{line}");
    assert!(line.contains("To fix: remove them"), "{line}");
    // The declared trees are all there, so the machine is still healthy.
    assert!(
        findings
            .iter()
            .any(|f| f.line() == "ok       trees     3 trees present.")
    );
    let reply = report(&findings);
    assert_eq!(reply.exit, Exit::Success);
    assert!(reply.text.ends_with("healthy, 1 warning"), "{}", reply.text);
}

#[test]
fn git_trees_lists_the_visible_directories_under_the_trees_folder() {
    let dir = std::env::temp_dir().join(format!("ignite-doctor-dirs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for name in ["tree-01", "slot-01", ".pytest_cache"] {
        std::fs::create_dir_all(dir.join(name)).unwrap();
    }
    std::fs::write(dir.join("notes.txt"), "").unwrap();
    let source = GitTrees {
        repo: dir.clone(),
        dir: dir.clone(),
    };
    let mut names = source.directories();
    names.sort();
    assert_eq!(names, ["slot-01", "tree-01"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_version_recorded_at_startup_is_reported_after_the_link_moves() {
    // `bin/current` is repointed when an update is prepared, before the daemon restarts, so
    // the version must come from what was recorded at startup, not from the link.
    let started = Commit::new("1234567").unwrap();
    super::remember_running_version(Some(started.clone()));
    assert_eq!(super::running_version(), Some(started));
}
