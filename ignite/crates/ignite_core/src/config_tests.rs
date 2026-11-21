use super::*;

/// The design document's examples, joined into one file with the sections they leave out.
const DOCUMENT_EXAMPLES: &str = r#"
[repository]
remote = "origin"
staging = "staging"
master = "master"
trees = ".worktrees"
checks = "ignite/checks"

[hub]
url = "https://hub.example.ts.net:8443"
project = "demo"

[gate]                 # example values
threshold = 0.004      # expected failures per billion instructions
seconds = 60           # of running checks per landing, builds not counted

[machine.jarvis]
ticket_trees = 24
landing_trees = 3
trees_disk_gb = 400

[machine.jarvis.resources]
browser-pages = 9
oracle = 1
bench-vm = 1

[machine.cygnus]
os = "windows"
ticket_trees = 6
landing_trees = 2
families = ["rust", "clippy", "format", "source-rules", "python"]
floor = { batch = 8, tip = 2 }

[[rebuild]]
name = "kernel-body"
edit = "append a no-op item to a module of the kernel crate"
file = "rust/crates/kernel/src/counters.rs"
append = "\nconst _: () = ();\n"
products = ["clippy", "rust-tests"]
[[rebuild]]
name = "one-card"
edit = "append a no-op item to one card file"
file = "rust/cards/one_card.rs"
append = "\nconst _: () = ();\n"
products = ["clippy", "rust-tests"]
"#;

#[test]
fn parses_the_document_examples() {
    let config = Config::parse("ignite.toml", DOCUMENT_EXAMPLES).unwrap();
    assert!((config.gate.threshold - 0.004).abs() < 1e-12);
    assert_eq!(config.improvement, Improvement::default());
    assert_eq!(config.rules, Rules::default());
    let jarvis = config.machine("jarvis").unwrap();
    assert_eq!((jarvis.ticket_trees, jarvis.landing_trees), (24, 3));
    assert_eq!(jarvis.resources["browser-pages"], 9);
    assert_eq!(jarvis.families, None);
    let cygnus = config.machine("cygnus").unwrap();
    assert_eq!(cygnus.os, Some(Os::Windows));
    assert_eq!(cygnus.floor, Floor { batch: 8, tip: 2 });
    assert_eq!(config.rebuild[1].name, "one-card");
}

#[test]
fn machines_are_found_whatever_the_hostname_case() {
    let config = Config::parse("ignite.toml", DOCUMENT_EXAMPLES).unwrap();
    let declared = config.machine("jarvis").unwrap();
    assert!(std::ptr::eq(config.machine("JARVIS").unwrap(), declared));
    assert!(config.machine("Jarvis").is_some());
    assert!(config.machine("nobody").is_none());
}

#[test]
fn the_express_lane_covers_a_landing_only_when_every_changed_file_matches() {
    let text = format!(
        "{DOCUMENT_EXAMPLES}\n[express]\npaths = [\"ignite/**\", \"ignite.toml\"]\n\
         tree = \".worktrees/express\"\ndir = \"ignite\"\ncommands = [\"cargo fmt --check\"]\n\
         families = [\"ignite\"]\n"
    );
    let (config, unknown) = Config::parse_with_unknown("ignite.toml", &text).unwrap();
    assert!(unknown.is_empty(), "{unknown:?}");
    let express = config.express.unwrap();
    let paths = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
    assert!(express.covers(&paths(&["ignite/crates/a.rs", "ignite.toml"])));
    assert!(!express.covers(&paths(&["ignite/crates/a.rs", "rust/Cargo.toml"])));
    assert!(!express.covers(&[]));
    assert_eq!(express.families, ["ignite"]);
    assert_eq!(config.machine["jarvis"].express_cores, 0);
}

#[test]
fn unknown_keys_are_reported_but_not_refused() {
    let text = DOCUMENT_EXAMPLES
        .replace("seconds = 60", "seconds = 60\nsecondz = 1")
        .replace("[repository]", "[repository]\nfuture_key = \"x\"");
    let (config, unknown) = Config::parse_with_unknown("ignite.toml", &text).unwrap();
    assert_eq!(
        config,
        Config::parse("ignite.toml", DOCUMENT_EXAMPLES).unwrap()
    );
    assert_eq!(unknown.len(), 2, "{unknown:?}");
    assert!(
        unknown.iter().any(|k| k.ends_with("secondz")),
        "{unknown:?}"
    );
    assert!(
        unknown.contains(&"repository.future_key".to_owned()),
        "{unknown:?}"
    );
    let (_, none) = Config::parse_with_unknown("ignite.toml", DOCUMENT_EXAMPLES).unwrap();
    assert!(none.is_empty(), "{none:?}");
}

#[test]
fn a_tip_claim_defaults_to_a_minute_and_must_be_above_zero() {
    let config = Config::parse("ignite.toml", DOCUMENT_EXAMPLES).unwrap();
    assert!((config.tip.claim_seconds - 60.0).abs() < f64::EPSILON);
    let text = format!("{DOCUMENT_EXAMPLES}\n[tip]\nclaim_seconds = 30\n");
    let config = Config::parse("ignite.toml", &text).unwrap();
    assert!((config.tip.claim_seconds - 30.0).abs() < f64::EPSILON);
    let text = format!("{DOCUMENT_EXAMPLES}\n[tip]\nclaim_seconds = 0\n");
    let error = Config::parse("ignite.toml", &text).unwrap_err();
    assert_eq!(error.key, "tip.claim_seconds");
}

/// A file written before the two budgets has `cap` and no `seconds`: it still parses, with
/// the default budgets, and `cap` is reported as a key this build does not know.
#[test]
fn a_file_from_before_the_budgets_parses_with_the_default_seconds() {
    let text = DOCUMENT_EXAMPLES.replace("seconds = 60", "cap = 8890");
    let (config, unknown) = Config::parse_with_unknown("ignite.toml", &text).unwrap();
    assert!((config.gate.seconds - 60.0).abs() < f64::EPSILON);
    assert!((config.tip.seconds - 300.0).abs() < f64::EPSILON);
    assert_eq!(unknown, ["gate.cap"]);
    let text = format!("{DOCUMENT_EXAMPLES}\n[tip]\nclaim_seconds = 60\nseconds = 0\n");
    assert_eq!(
        Config::parse("ignite.toml", &text).unwrap_err().key,
        "tip.seconds"
    );
}

#[test]
fn syntax_errors_still_name_the_file() {
    let text = DOCUMENT_EXAMPLES.replace("seconds = 60", "seconds = = 60");
    let error = Config::parse("ignite.toml", &text).unwrap_err();
    assert_eq!(error.file, "ignite.toml");
}

#[test]
fn semantic_errors_name_the_key() {
    let text = DOCUMENT_EXAMPLES.replace("landing_trees = 2", "landing_trees = 0");
    let error = Config::parse("ignite.toml", &text).unwrap_err();
    assert_eq!(error.key, "machine.cygnus.landing_trees");
    assert_eq!(
        error.to_string(),
        "ignite.toml: machine.cygnus.landing_trees: must be above zero"
    );
    let text = DOCUMENT_EXAMPLES.replace("name = \"one-card\"", "name = \"kernel-body\"");
    assert_eq!(
        Config::parse("ignite.toml", &text).unwrap_err().key,
        "rebuild[1].name"
    );
}

#[test]
fn the_repository_configuration_parses() {
    let text = crate::test_files::read_to_string("ignite.toml").unwrap();
    let (config, unknown) = Config::parse_with_unknown("ignite.toml", &text).unwrap();
    assert!(unknown.is_empty(), "{unknown:?}");
    assert!(config.machine.len() >= 3);
    assert_eq!(config.rules, Rules::default());
}

#[test]
fn a_model_id_comes_from_the_apps_table_or_is_the_tier_when_there_is_none() {
    let text = format!("{DOCUMENT_EXAMPLES}\n[pool.models.claude]\nopus = \"the-big-one\"\n");
    let config = Config::parse("ignite.toml", &text).unwrap();
    assert_eq!(
        config.pool.model_id("claude", "opus"),
        Some("the-big-one".to_owned())
    );
    assert_eq!(config.pool.model_id("claude", "sonnet"), None);
    assert_eq!(
        config.pool.model_id("codex", "sonnet"),
        Some("sonnet".to_owned())
    );
    let config = Config::parse("ignite.toml", DOCUMENT_EXAMPLES).unwrap();
    assert_eq!(config.pool, PoolConfig::default());
    let text = format!("{DOCUMENT_EXAMPLES}\n[pool.models.claude]\nopus = \" \"\n");
    assert_eq!(
        Config::parse("ignite.toml", &text).unwrap_err().key,
        "pool.models.claude.opus"
    );
}

#[test]
fn default_effort_is_configured_per_app() {
    let text = format!("{DOCUMENT_EXAMPLES}\n[pool.default_effort]\ncodex = \"high\"\n");
    let config = Config::parse("ignite.toml", &text).unwrap();
    assert_eq!(config.pool.default_effort("codex"), "high");
    assert_eq!(config.pool.default_effort("claude"), "medium");

    let text = format!("{DOCUMENT_EXAMPLES}\n[pool.default_effort]\ncodex = \" \"\n");
    assert_eq!(
        Config::parse("ignite.toml", &text).unwrap_err().key,
        "pool.default_effort.codex"
    );
}

#[test]
fn a_check_is_protected_by_its_own_name_or_its_family() {
    let rules = Rules {
        report_only: Vec::new(),
        protected: vec!["lint".to_owned(), "tests:boots".to_owned()],
    };
    assert!(rules.is_protected("lint:workspace"));
    assert!(rules.is_protected("tests:boots"));
    assert!(!rules.is_protected("tests:other"));
    assert!(!rules.is_protected("linter:workspace"));
}

#[test]
fn the_cli_pool_tables_parse_and_default_to_desktop_mode() {
    let config = Config::parse("ignite.toml", DOCUMENT_EXAMPLES).unwrap();
    assert_eq!(
        config.machine_pool("jarvis", "claude"),
        MachinePool {
            mode: PoolMode::Desktop,
            slots: 0
        }
    );
    assert_eq!(
        config.machine_pool("nowhere", "claude").mode,
        PoolMode::Desktop
    );
    assert_eq!(config.cli_program("claude"), None);

    let cli = "\n[machine.jarvis.pool.claude]\nmode = \"cli\"\nslots = 8\n\
               [machine.jarvis.pool.codex]\nslots = 2\n\
               [pool.cli.claude]\nprogram = \"claude\"\n\
               args = [\"--dangerously-skip-permissions\"]\n\
               [pool.cli.codex]\nprogram = \"codex\"\n";
    let (config, unknown) =
        Config::parse_with_unknown("ignite.toml", &format!("{DOCUMENT_EXAMPLES}{cli}")).unwrap();
    assert!(unknown.is_empty(), "{unknown:?}");
    assert_eq!(
        config.machine_pool("JARVIS", "claude"),
        MachinePool {
            mode: PoolMode::Cli,
            slots: 8
        }
    );
    assert_eq!(
        config.machine_pool("jarvis", "codex"),
        MachinePool {
            mode: PoolMode::Desktop,
            slots: 2
        },
        "a table without a mode is desktop mode"
    );
    assert_eq!(
        config.machine_pool("cygnus", "claude").mode,
        PoolMode::Desktop
    );
    let claude = config.cli_program("claude").unwrap();
    assert_eq!(
        (claude.program.as_str(), claude.args.as_slice()),
        (
            "claude",
            ["--dangerously-skip-permissions".to_owned()].as_slice()
        )
    );
    assert!(config.cli_program("codex").unwrap().args.is_empty());

    let no_slots = "\n[machine.jarvis.pool.claude]\nmode = \"cli\"\n\
                    [pool.cli.claude]\nprogram = \"claude\"\n";
    assert_eq!(
        Config::parse("ignite.toml", &format!("{DOCUMENT_EXAMPLES}{no_slots}"))
            .unwrap_err()
            .key,
        "machine.jarvis.pool.claude.slots"
    );
    let no_program = "\n[machine.jarvis.pool.claude]\nmode = \"cli\"\nslots = 1\n";
    assert_eq!(
        Config::parse("ignite.toml", &format!("{DOCUMENT_EXAMPLES}{no_program}"))
            .unwrap_err()
            .key,
        "machine.jarvis.pool.claude.mode"
    );
    let bad_mode = "\n[machine.jarvis.pool.claude]\nmode = \"tui\"\n";
    assert!(Config::parse("ignite.toml", &format!("{DOCUMENT_EXAMPLES}{bad_mode}")).is_err());
}

#[test]
fn cli_dialects_default_to_the_provider_name_and_must_be_registered() {
    let tables = r#"
[pool.cli.codex]
program = "codex"
[pool.cli.fake]
program = "fake-cli"
dialect = "codex"
"#;
    let text = format!("{DOCUMENT_EXAMPLES}{tables}");
    let config = Config::parse("ignite.toml", &text).unwrap();
    let codex = config.cli_program("codex").unwrap();
    let fake = config.cli_program("fake").unwrap();
    assert_eq!(codex.dialect("codex"), "codex");
    assert_eq!(fake.dialect("fake"), "codex");

    let error = Config::parse(
        "ignite.toml",
        &text.replace("dialect = \"codex\"", "dialect = \"unknown\""),
    )
    .unwrap_err();
    assert_eq!(error.key, "pool.cli.fake.dialect");

    let error = Config::parse(
        "ignite.toml",
        &text.replace("pool.cli.fake", "pool.cli.fake-slot"),
    )
    .unwrap_err();
    assert_eq!(error.key, "pool.cli.fake-slot");
}

/// One family that must pass on macOS and Windows, beside one that may pass anywhere.
const PLATFORM_FAMILIES: &str = r#"
[family.platform]
kind = "regression"
adapter = "nextest"
platforms = ["macos", "windows"]
list = "cargo nextest list"
run = "cargo nextest run"
inputs = ["crate-closure"]

[family.rust]
kind = "regression"
adapter = "nextest"
platforms = "any"
list = "cargo nextest list"
run = "cargo nextest run"
inputs = ["crate-closure"]
"#;

fn platform_manifest() -> crate::manifest::Manifest {
    crate::manifest::Manifest::from_files(&[(
        PathBuf::from("checks/rest.toml"),
        PLATFORM_FAMILIES.to_owned(),
    )])
    .unwrap()
}

#[test]
fn a_family_no_machine_of_its_platform_runs_is_unrunnable() {
    // The only Windows machine lists its families and leaves out `platform`, which must pass
    // on Windows, so tip verification would wait for it forever.
    let text = DOCUMENT_EXAMPLES.replace("jarvis]\n", "jarvis]\nos = \"macos\"\n");
    let config = Config::parse("ignite.toml", &text).unwrap();
    assert_eq!(
        config.unrunnable_families(&platform_manifest()),
        vec![("platform".to_owned(), Some(Os::Windows))]
    );

    let listed = text.replace("\"python\"]", "\"python\", \"platform\"]");
    let config = Config::parse("ignite.toml", &listed).unwrap();
    assert!(config.unrunnable_families(&platform_manifest()).is_empty());
}

#[test]
fn a_machine_without_a_declared_os_runs_only_families_that_pass_anywhere() {
    let config = Config::parse("ignite.toml", DOCUMENT_EXAMPLES).unwrap();
    assert!(config.runs_family(None, "rust"));
    assert!(!config.runs_family(Some(Os::Macos), "rust"));
    assert_eq!(
        config.unrunnable_families(&platform_manifest()),
        vec![
            ("platform".to_owned(), Some(Os::Macos)),
            ("platform".to_owned(), Some(Os::Windows)),
        ]
    );
}

#[test]
fn every_family_of_the_repository_has_a_machine_on_each_of_its_platforms() {
    let text = crate::test_files::read_to_string("ignite.toml").unwrap();
    let config = Config::parse("ignite.toml", &text).unwrap();
    let dir = &config.repository.checks;
    let mut paths: Vec<PathBuf> = crate::test_files::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    let files: Vec<(PathBuf, String)> = paths
        .into_iter()
        .map(|path| {
            let name = path.file_name().unwrap().to_owned();
            let text = crate::test_files::read_to_string(dir.join(&name)).unwrap();
            (path, text)
        })
        .collect();
    let manifest = crate::manifest::Manifest::from_files(&files).unwrap();
    let missing = config.unrunnable_families(&manifest);
    assert!(
        missing.is_empty(),
        "no machine in ignite.toml can run these families on these platforms; add each family \
         to the `families` of a `[machine.*]` with that `os`: {missing:?}"
    );
}
