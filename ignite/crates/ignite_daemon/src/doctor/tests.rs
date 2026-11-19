use ignite_core::fakes::FakeHub;

use super::listener::WriteProbe as _;
use super::*;

struct FakeHost {
    existing: Vec<PathBuf>,
    free_gb: u64,
    /// What the trees use already.
    used_gb: u64,
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
    fn trees_used_gb(&self, _dir: &Path) -> Option<u64> {
        Some(self.used_gb)
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
        used_gb: 10,
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
        probe: &Writable,
        manifest: Ok(manifest()),
        version: Commit::new("abc1234").ok(),
    })
}

/// A hub whose listener accepts writes: it refuses a tokenless `POST` with 401.
struct Writable;

impl listener::WriteProbe for Writable {
    fn post_without_token(&self, _url: &str, _path: &str) -> Option<(u16, String)> {
        Some((401, r#"{"error":"missing token"}"#.to_owned()))
    }
}

/// A probe that answers with a fixed status and body, and records the path it was sent to.
struct Answering(Option<(u16, &'static str)>, std::cell::RefCell<Vec<String>>);

impl listener::WriteProbe for Answering {
    fn post_without_token(&self, url: &str, path: &str) -> Option<(u16, String)> {
        self.1.borrow_mut().push(format!("{url}{path}"));
        self.0.map(|(status, body)| (status, body.to_owned()))
    }
}

fn diagnose_with_probe(probe: &dyn listener::WriteProbe) -> Vec<Finding> {
    diagnose(&DoctorInput {
        config: &config(),
        repo: Path::new("/repo"),
        host: &healthy_host(),
        trees: &all_trees(),
        hub: &FakeHub::default(),
        probe,
        manifest: Ok(manifest()),
        version: Commit::new("abc1234").ok(),
    })
}

#[test]
fn doctor_reports_a_hub_url_that_reaches_the_read_only_listener() {
    let probe = Answering(
        Some((405, r#"{"error":"this listener is read-only"}"#)),
        std::cell::RefCell::default(),
    );
    let findings = diagnose_with_probe(&probe);
    let problems: Vec<_> = findings.iter().filter(|f| !f.ok).collect();
    assert_eq!(problems.len(), 1, "{problems:?}");
    let line = problems[0].line();
    assert!(line.starts_with("PROBLEM  listener"), "{line}");
    assert!(line.contains("http://hub.example:8443"), "{line}");
    assert!(
        line.contains("To fix: point the hub URL at the writable listener"),
        "{line}"
    );
    assert_eq!(
        *probe.1.borrow(),
        ["http://hub.example:8443/test/api/machines/heartbeat"]
    );
    assert_eq!(report(&findings).exit, Exit::CheckFailed);
}

#[test]
fn doctor_accepts_a_writable_hub_and_ignores_other_405s() {
    for answer in [
        Some((401, r#"{"error":"missing token"}"#)),
        // A proxy's own 405 is not the read-only listener.
        Some((405, "Method Not Allowed")),
        None,
    ] {
        let findings = diagnose_with_probe(&Answering(answer, std::cell::RefCell::default()));
        assert!(
            findings
                .iter()
                .any(|f| f.line() == "ok       hub       reachable at http://hub.example:8443."),
            "{answer:?}: {findings:?}"
        );
    }
}

#[test]
fn doctor_does_not_probe_a_hub_it_cannot_reach() {
    let hub = FakeHub::default();
    hub.set_unreachable(true);
    let probe = Answering(
        Some((405, r#"{"error":"this listener is read-only"}"#)),
        std::cell::RefCell::default(),
    );
    let findings = diagnose(&DoctorInput {
        config: &config(),
        repo: Path::new("/repo"),
        host: &healthy_host(),
        trees: &all_trees(),
        hub: &hub,
        probe: &probe,
        manifest: Ok(manifest()),
        version: Commit::new("abc1234").ok(),
    });
    let hub_line = findings.iter().find(|f| f.label == "hub").unwrap().line();
    assert!(hub_line.contains("cannot reach the hub"), "{hub_line}");
    assert!(probe.1.borrow().is_empty());
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
        committed: 3,
        used: 1,
        swap_used_mb: Some(512),
        memory_pressed: false,
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
    assert!(
        plain
            .text
            .contains("4 of 10 granted to Ignite (using 1, 3 counted)"),
        "{}",
        plain.text
    );
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
        probe: &Writable,
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

#[test]
fn the_http_probe_sends_a_tokenless_post_and_reads_the_refusal() {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", server.server_addr().to_ip().unwrap());
    let handle = std::thread::spawn(move || {
        let request = server.recv().unwrap();
        let seen = (
            request.method().to_string(),
            request.url().to_owned(),
            request
                .headers()
                .iter()
                .any(|h| h.field.equiv("Authorization")),
        );
        request
            .respond(
                tiny_http::Response::from_string(r#"{"error":"this listener is read-only"}"#)
                    .with_status_code(405),
            )
            .unwrap();
        seen
    });
    let answer = listener::HttpProbe.post_without_token(&url, "/test/api/machines/heartbeat");
    assert_eq!(
        answer,
        Some((405, r#"{"error":"this listener is read-only"}"#.to_owned()))
    );
    assert_eq!(
        handle.join().unwrap(),
        (
            "POST".to_owned(),
            "/test/api/machines/heartbeat".to_owned(),
            false
        )
    );
}
