use std::collections::BTreeMap;

use ignite_core::fakes::FakeHub;
use ignite_core::traits::Result;

use super::listener::WriteProbe as _;
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
    fn disk_space(&self, _path: &Path) -> Option<ignite_os::system::DiskSpace> {
        let gb = ignite_core::capacity::BYTES_PER_GB;
        Some(ignite_os::system::DiskSpace {
            total: 1000 * gb,
            available: self.free_gb * gb,
        })
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
            "/repo/trees/landing-2",
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
        probe: &Writable,
        manifest: Ok(manifest()),
        version: Commit::new("abc1234").ok(),
        fleet: None,
        build_slots: None,
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
        fleet: None,
        build_slots: None,
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
        fleet: None,
        build_slots: None,
    });
    let hub_line = findings.iter().find(|f| f.label == "hub").unwrap().line();
    assert!(hub_line.contains("cannot reach the hub"), "{hub_line}");
    assert!(probe.1.borrow().is_empty());
}

fn all_trees() -> FakeTrees {
    FakeTrees(vec![
        tree("tree-01"),
        tree("tree-02"),
        tree("landing-1"),
        tree("landing-2"),
    ])
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
fn doctor_warns_when_a_configured_provider_has_no_model_tiers() {
    let mut config = config();
    for provider in ["fake", "limited"] {
        config.pool.cli.insert(
            provider.to_owned(),
            ignite_core::config::CliProgram {
                program: "fake-cli".to_owned(),
                dialect: Some("codex".to_owned()),
                ..Default::default()
            },
        );
    }
    config.pool.models.insert(
        "limited".to_owned(),
        [("opus".to_owned(), "an-id".to_owned())].into(),
    );
    let findings = diagnose(&DoctorInput {
        config: &config,
        repo: Path::new("/repo"),
        host: &healthy_host(),
        trees: &all_trees(),
        hub: &FakeHub::default(),
        probe: &Writable,
        manifest: Ok(manifest()),
        version: Commit::new("abc1234").ok(),
        fleet: None,
        build_slots: None,
    });
    let lines: Vec<String> = findings
        .iter()
        .filter(|f| f.label == "models")
        .map(Finding::line)
        .collect();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].starts_with("WARNING"), "{lines:?}");
    assert!(lines[0].contains("[pool.models.fake]"), "{lines:?}");
}

#[test]
fn doctor_warns_when_a_configured_cli_program_is_missing_from_the_path() {
    let mut config = config();
    config.pool.cli.insert(
        "fake".to_owned(),
        ignite_core::config::CliProgram {
            program: "fake-not-installed".to_owned(),
            dialect: Some("codex".to_owned()),
            ..Default::default()
        },
    );
    let findings = diagnose(&DoctorInput {
        config: &config,
        repo: Path::new("/repo"),
        host: &healthy_host(),
        trees: &all_trees(),
        hub: &FakeHub::default(),
        probe: &Writable,
        manifest: Ok(manifest()),
        version: Commit::new("abc1234").ok(),
        fleet: None,
        build_slots: None,
    });
    let lines: Vec<String> = findings
        .iter()
        .filter(|f| f.label == "cli")
        .map(Finding::line)
        .collect();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].starts_with("WARNING"), "{lines:?}");
    assert!(lines[0].contains("fake-not-installed"), "{lines:?}");
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
        tree_line
            .replace('\\', "/")
            .contains("/repo/trees/tree-02 is missing"),
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
        line.replace('\\', "/")
            .contains("tree directory /repo/trees is missing"),
        "{line}"
    );
    assert!(line.contains("To fix: restart the daemon"), "{line}");
}

#[test]
fn doctor_counts_present_trees() {
    let config = config();
    let host = healthy_host();
    let trees = FakeTrees(vec![tree("landing-1")]);
    let findings = diagnose(&DoctorInput {
        config: &config,
        repo: Path::new("/repo"),
        host: &host,
        trees: &trees,
        hub: &FakeHub::default(),
        probe: &Writable,
        manifest: Ok(manifest()),
        version: Commit::new("abc1234").ok(),
        fleet: None,
        build_slots: None,
    });
    assert!(
        findings
            .iter()
            .any(|f| f.label == "trees" && f.text == "1 tree present.")
    );
    assert!(findings.iter().all(|f| f.ok));
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
fn a_product_only_an_unrunnable_family_needs_asks_for_no_tool() {
    let manifest = Manifest::from_files(&[(
        PathBuf::from("checks/all.toml"),
        r#"
        [product.tests]
        build = "make tests"
        inputs = ["src"]
        outputs = ["target"]
        [product.deps]
        build = "npm ci"
        inputs = ["package.json"]
        outputs = ["node_modules"]
        [family.units]
        kind = "regression"
        adapter = "nextest"
        platforms = "any"
        run = "cargo nextest run"
        needs = ["tests"]
        inputs = ["src"]
        [family.browser]
        kind = "regression"
        adapter = "nextest"
        platforms = ["macos"]
        run = "cargo nextest run"
        needs = ["deps"]
        inputs = ["src"]
        "#
        .to_owned(),
    )])
    .unwrap();
    let runnable = BTreeSet::from(["units".to_owned()]);
    let tools = needed_tools(&manifest, &runnable);
    assert!(tools.contains("make"), "{tools:?}");
    assert!(!tools.contains("npm"), "{tools:?}");
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
        tip_build: 0,
        tip_build_used: 0,
        agent_borrowed: 0,
        priority_grants: BTreeMap::new(),
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
fn status_lists_declared_exploration_families_only_with_the_flag() {
    let verb = StatusVerb::new(vec![Box::new(ExplorationSection(Arc::new(|| {
        vec!["new-seeds".to_owned(), "timeline-drift".to_owned()]
    })))]);

    let plain = verb.render("m-1", None, &[]);
    assert!(!plain.text.contains("exploration"), "{}", plain.text);
    assert!(plain.json.unwrap().get("exploration").is_none());

    let requested = verb.render("m-1", None, &["--exploration".to_owned()]);
    assert!(requested.text.contains("exploration  new-seeds"));
    assert!(
        requested
            .text
            .lines()
            .any(|line| line.trim() == "timeline-drift")
    );
    assert_eq!(
        requested.json.unwrap()["exploration"],
        json!(["new-seeds", "timeline-drift"])
    );
}

#[test]
fn status_shows_the_tip_reserve_and_grants_with_ticket_priority() {
    let status = Arc::new(|| sched::Status {
        cores: 10,
        outside: 0,
        granted: 0,
        committed: 0,
        used: 0,
        swap_used_mb: None,
        memory_pressed: false,
        max_per_command: 6,
        free_memory_mb: None,
        tip_build: 4,
        tip_build_used: 3,
        agent_borrowed: 2,
        priority_grants: BTreeMap::from([(ignite_core::ids::TicketId::new("FIX").unwrap(), 1)]),
        classes: vec![],
        resources: vec![],
    });
    let verb = StatusVerb::new(vec![Box::new(CoresSection(status))]);
    let plain = verb.render("m-1", None, &[]);
    assert!(
        plain.text.contains("4 kept for tip builds (3 in use)"),
        "{}",
        plain.text
    );
    assert!(plain.text.contains("agents borrowed 2 from the reserves"));
    assert!(plain.text.contains(
        "1 command for FIX granted cores with priority because the ticket holds master back"
    ));
}

#[test]
fn status_counts_ticket_trees_and_names_foreign_directories() {
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
        landing_trees: 1,
        host: Arc::new(FakeHost { existing, ..host }),
    };
    assert_eq!(
        section.lines(),
        [
            "3 trees present",
            "outside Ignite's tree names: switch-rebase"
        ]
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
            "landing-2",
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
        fleet: None,
        build_slots: None,
    });
    let warning = findings.iter().find(|f| f.warning).expect("a warning");
    let line = warning.line();
    assert!(line.starts_with("WARNING  trees"), "{line}");
    assert!(line.contains("slot-01, switch-rebase."), "{line}");
    assert!(line.contains("To fix: remove them"), "{line}");
    // All registered Ignite trees are present.
    assert!(
        findings
            .iter()
            .any(|f| f.line() == "ok       trees     4 trees present.")
    );
    let reply = report(&findings);
    assert_eq!(reply.exit, Exit::Success);
    assert!(reply.text.ends_with("healthy, 1 warning"), "{}", reply.text);
}

#[test]
fn git_trees_lists_the_visible_directories_under_the_trees_folder() {
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-doctor-dirs-{}", std::process::id()));
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

#[test]
fn doctor_warns_when_a_new_program_waits_for_a_security_scan() {
    use std::time::Duration;
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-doctor-launcher-{}", std::process::id()));
    let scanned = scanning::judge(
        scanning::Starts {
            first: Duration::from_millis(180),
            second: Duration::from_millis(5),
        },
        &dir.join("launcher"),
    );
    assert!(scanned.warning, "{}", scanned.line());
    assert!(scanned.line().contains("180 ms"), "{}", scanned.line());
    let without = scanned.fix.unwrap();
    assert!(without.contains("ignite install"), "{without}");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("launcher"), b"").unwrap();
    let with = scanning::judge(
        scanning::Starts {
            first: Duration::from_millis(180),
            second: Duration::from_millis(5),
        },
        &dir.join("launcher"),
    );
    std::fs::remove_dir_all(&dir).unwrap();
    let fix = with.fix.unwrap();
    assert!(fix.contains("Developer Tools"), "{fix}");
    assert!(
        fix.contains(&dir.join("launcher").display().to_string()),
        "{fix}"
    );
    let exempt = scanning::judge(
        scanning::Starts {
            first: Duration::from_millis(9),
            second: Duration::from_millis(5),
        },
        &dir.join("launcher"),
    );
    assert!(exempt.ok && !exempt.warning, "{}", exempt.line());
}

/// The probe itself, on the machine running the test: it builds and runs a program and reports a
/// finding either way.
#[cfg(target_os = "macos")]
#[test]
fn the_scanning_probe_reports_a_finding_on_macos() {
    let finding = scanning::finding().expect("cc builds the probe");
    assert_eq!(finding.label, "scanning");
}
