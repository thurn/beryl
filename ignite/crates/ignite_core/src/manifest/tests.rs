use super::*;

const RUST: &str = r#"
[family.rust]
kind = "regression"
adapter = "nextest"
platforms = "any"
needs = ["rust-tests"]
inputs = ["crate-closure", "rust/.config/nextest.toml"]

[product.rust-tests]
build = "cargo nextest run --no-run {packages}"
dir = "rust"
inputs = ["crate-closure"]
outputs = ["rust/target"]
"#;

const SHOTS: &str = r#"
[family.shots]
kind = "regression"
adapter = "jsonl"
platforms = ["macos"]
needs = ["cli", "ui-deps"]
resources = ["browser-pages"]
list = "node ui/scripts/capture.mjs --list"
run = "node ui/scripts/capture.mjs {checks}"
inputs = ["ui/src/**", "ui/shots.json", "crate:game_cli"]

[product.cli]
build = "cargo build -p game_cli"
inputs = ["crate:game_cli"]
outputs = ["rust/target"]

[product.ui-deps]
build = "npm ci"
dir = "ui"
inputs = ["ui/package-lock.json"]
outputs = ["ui/node_modules"]
"#;

fn files(list: &[(&str, &str)]) -> Vec<(PathBuf, String)> {
    list.iter()
        .map(|(p, t)| (PathBuf::from(p), (*t).to_owned()))
        .collect()
}

#[test]
fn parses_the_document_examples() {
    let manifest = Manifest::from_files(&files(&[
        ("checks/rust.toml", RUST),
        ("checks/ui.toml", SHOTS),
    ]))
    .unwrap();
    let rust = &manifest.families["rust"];
    assert_eq!(rust.group, "rust");
    assert!(rust.family.platforms.is_any());
    assert_eq!(rust.family.inputs[0], InputPattern::CrateClosure);
    let shots = &manifest.families["shots"].family;
    assert_eq!(shots.platforms, Platforms::Each(vec![Os::Macos]));
    assert_eq!(shots.inputs[2], InputPattern::Crate("game_cli".to_owned()));
    assert_eq!(manifest.products["rust-tests"].outputs, ["rust/target"]);
}

#[test]
fn list_needs_names_declared_products_only() {
    let text = |list_needs: &str| {
        format!(
            "[family.f]\nkind = \"regression\"\nadapter = \"jsonl\"\nplatforms = \"any\"\n\
                 list_needs = {list_needs}\nlist = \"l\"\nrun = \"r\"\ninputs = [\"x\"]\n\
                 [product.a]\nbuild = \"b\"\ninputs = [\"x\"]\noutputs = [\"o\"]\n"
        )
    };
    let load = |t: String| Manifest::from_files(&[("f.toml".into(), t)]);
    assert!(load(text("[\"a\"]")).is_ok());
    assert_eq!(
        load(text("[\"zzz\"]")).unwrap_err().key,
        "family.f.list_needs[0]"
    );
}

#[test]
fn a_missing_product_names_the_file_and_key() {
    let error = Manifest::from_files(&files(&[(
        "checks/rust.toml",
        &RUST[..RUST.find("[product").unwrap()],
    )]))
    .unwrap_err();
    assert_eq!(error.key, "family.rust.needs[0]");
    assert_eq!(error.file, PathBuf::from("checks/rust.toml"));
}

#[test]
fn a_duplicate_family_names_both_files() {
    let error = Manifest::from_files(&files(&[("checks/a.toml", RUST), ("checks/b.toml", RUST)]))
        .unwrap_err();
    assert_eq!(error.file, PathBuf::from("checks/b.toml"));
    assert_eq!(error.key, "family.rust");
    assert!(error.message.contains("checks/a.toml"));
}

#[test]
fn field_errors_name_the_key() {
    let text = SHOTS.replace("list = \"node ui/scripts/capture.mjs --list\"\n", "");
    let error = Manifest::from_files(&files(&[("checks/ui.toml", &text)])).unwrap_err();
    assert_eq!(
        error.to_string(),
        "checks/ui.toml: family.shots.list: is required for the jsonl adapter"
    );
    let text = RUST.replace("platforms = \"any\"", "platforms = \"anywhere\"");
    let error = Manifest::from_files(&files(&[("checks/rust.toml", &text)])).unwrap_err();
    assert!(error.message.contains("platforms"), "{error}");
    let text = RUST.replace("kind = ", "knid = ");
    assert!(Manifest::from_files(&files(&[("checks/rust.toml", &text)])).is_err());
}

#[test]
fn unknown_manifest_keys_are_reported_but_not_refused() {
    let text = RUST.replace("platforms = \"any\"", "platforms = \"any\"\nfuture = 1");
    let (manifest, unknown) =
        Manifest::from_files_with_unknown(&files(&[("checks/rust.toml", &text)])).unwrap();
    assert_eq!(
        manifest,
        Manifest::from_files(&files(&[("checks/rust.toml", RUST)])).unwrap()
    );
    assert_eq!(unknown, ["checks/rust.toml: family.rust.future"]);
}

#[test]
fn only_an_exploration_family_has_a_next_command() {
    let explore = SHOTS
        .replace("kind = \"regression\"", "kind = \"exploration\"")
        .replace(
            "[family.shots]\n",
            "[family.shots]\nnext = \"node seeds.mjs\"\n",
        );
    let manifest = Manifest::from_files(&files(&[("checks/ui.toml", &explore)])).unwrap();
    let shots = &manifest.families["shots"].family;
    assert_eq!(shots.next.as_deref(), Some("node seeds.mjs"));
    let text = SHOTS.replace("kind = \"regression\"", "kind = \"exploration\"");
    let error = Manifest::from_files(&files(&[("checks/ui.toml", &text)])).unwrap_err();
    assert_eq!(error.key, "family.shots.next");
    let text = SHOTS.replace(
        "[family.shots]\n",
        "[family.shots]\nnext = \"node seeds.mjs\"\n",
    );
    let error = Manifest::from_files(&files(&[("checks/ui.toml", &text)])).unwrap_err();
    assert_eq!(error.key, "family.shots.next");
}

#[test]
fn package_inputs_belong_to_their_package_alone() {
    let text = RUST.replace(
        "\n[product.rust-tests]",
        "[family.rust.package_inputs]\ncore = [\"data/cards/**\", \"crate:extra\"]\n\n\
             [product.rust-tests]",
    );
    let manifest = Manifest::from_files(&files(&[("checks/rust.toml", &text)])).unwrap();
    let rust = &manifest.families["rust"].family;
    assert_eq!(
        rust.package_patterns(Some("core")),
        [
            InputPattern::Path("data/cards/**".to_owned()),
            InputPattern::Crate("extra".to_owned())
        ]
    );
    assert!(rust.package_patterns(Some("other")).is_empty());
    assert!(rust.package_patterns(None).is_empty());
    let without = Manifest::from_files(&files(&[("checks/rust.toml", RUST)])).unwrap();
    assert!(without.families["rust"].family.package_inputs.is_empty());
}

#[test]
fn package_inputs_refuse_an_empty_list_and_crate_closure() {
    for (list, message) in [
        ("[]", "must name"),
        ("[\"crate-closure\"]", "belongs in the family's inputs"),
    ] {
        let text = RUST.replace(
            "\n[product.rust-tests]",
            &format!("[family.rust.package_inputs]\ncore = {list}\n\n[product.rust-tests]"),
        );
        let error = Manifest::from_files(&files(&[("checks/rust.toml", &text)])).unwrap_err();
        assert_eq!(error.key, "family.rust.package_inputs.core");
        assert!(error.message.contains(message), "{error}");
    }
}

#[test]
fn digest_inputs_are_read_and_refuse_crate_closure() {
    let declare = |list: &str| {
        RUST.replace(
            "\n[product.rust-tests]",
            &format!("digest_inputs = {list}\n\n[product.rust-tests]"),
        )
    };
    let text = declare("[\"data/pins.json\"]");
    let manifest = Manifest::from_files(&files(&[("checks/rust.toml", &text)])).unwrap();
    assert_eq!(
        manifest.families["rust"].family.digest_inputs,
        [InputPattern::Path("data/pins.json".to_owned())]
    );
    let text = declare("[\"crate-closure\"]");
    let error = Manifest::from_files(&files(&[("checks/rust.toml", &text)])).unwrap_err();
    assert_eq!(error.key, "family.rust.digest_inputs");
}

#[test]
fn input_patterns_round_trip() {
    for text in ["crate-closure", "crate:core", "ui/src/**"] {
        let pattern = InputPattern::from(text.to_owned());
        assert_eq!(String::from(pattern), text);
    }
}

/// `RUST` with one more family, `rust-slow`, that may name a covering family.
fn covered(covered_by: &str) -> Vec<(PathBuf, String)> {
    let slow = RUST
        .split("\n[product.rust-tests]")
        .next()
        .unwrap()
        .replace("[family.rust]", "[family.rust-slow]");
    let text = format!("{RUST}\n{slow}{covered_by}\n");
    files(&[("checks/rust.toml", &text)])
}

#[test]
fn covered_by_names_the_family_that_covers_another() {
    let (manifest, unknown) =
        Manifest::from_files_with_unknown(&covered("covered_by = \"rust\"")).unwrap();
    assert_eq!(manifest.covered_by["rust-slow"], "rust");
    assert_eq!(manifest.covered_by.len(), 1);
    assert!(unknown.is_empty(), "{unknown:?}");
}

#[test]
fn covered_by_refuses_an_unknown_family_and_a_cycle() {
    let error = Manifest::from_files(&covered("covered_by = \"nothing\"")).unwrap_err();
    assert_eq!(error.key, "family.rust-slow.covered_by");
    assert!(error.message.contains("no family named"), "{error}");

    let error = Manifest::from_files(&covered("covered_by = \"rust-slow\"")).unwrap_err();
    assert!(error.message.contains("covers itself"), "{error}");

    // A chain back to the start is refused too: rust covered by rust-slow, covered by rust.
    let mut cycle = covered("covered_by = \"rust\"");
    cycle[0].1 = cycle[0].1.replacen(
        "[family.rust]\n",
        "[family.rust]\ncovered_by = \"rust-slow\"\n",
        1,
    );
    let error = Manifest::from_files(&cycle).unwrap_err();
    assert!(error.message.contains("covers itself"), "{error}");
}

#[test]
fn always_when_narrows_the_always_set_to_changes_it_names() {
    let lint = RUST.replacen(
        "needs = [\"rust-tests\"]\n",
        "needs = [\"rust-tests\"]\nalways = true\nalways_when = [\"rust/viewer/**\"]\n",
        1,
    );
    let (manifest, unknown) =
        Manifest::from_files_with_unknown(&files(&[("checks/rust.toml", &lint)])).unwrap();
    assert!(unknown.is_empty(), "{unknown:?}");
    let family = &manifest.families["rust"].family;
    let changed = |paths: &[&str]| paths.iter().map(|p| (*p).to_owned()).collect::<Vec<_>>();
    assert!(family.always_for(&changed(&["rust/viewer/src/main.rs"])));
    assert!(family.always_for(&changed(&["rust/game/a.rs", "rust/viewer/Cargo.toml"])));
    assert!(!family.always_for(&changed(&["rust/game/a.rs"])));

    // Without `always_when`, `always` holds for every change, and without `always`, for none.
    let plain = Manifest::from_files(&files(&[("checks/rust.toml", RUST)])).unwrap();
    let plain = &plain.families["rust"].family;
    assert!(!plain.always_for(&changed(&["rust/viewer/src/main.rs"])));
    let mut always = plain.clone();
    always.always = true;
    assert!(always.always_for(&changed(&["rust/game/a.rs"])));
}

#[test]
fn builds_marks_a_family_whose_checks_fail_when_the_code_does_not_compile() {
    let plain = Manifest::from_files(&files(&[("checks/rust.toml", RUST)])).unwrap();
    assert!(!plain.families["rust"].family.builds);
    let builds = RUST.replacen(
        "needs = [\"rust-tests\"]\n",
        "needs = [\"rust-tests\"]\nbuilds = true\n",
        1,
    );
    let (manifest, unknown) =
        Manifest::from_files_with_unknown(&files(&[("checks/rust.toml", &builds)])).unwrap();
    assert!(unknown.is_empty(), "{unknown:?}");
    assert!(manifest.families["rust"].family.builds);
}

#[test]
fn deferral_names_only_the_platforms_that_run_a_family() {
    let named = SHOTS.replace(
        "platforms = [\"macos\"]",
        "platforms = [\"macos\", \"windows\"]\ndefer_platforms = [\"windows\"]",
    );
    let load = |text: &str| Manifest::from_files(&files(&[("checks/test.toml", text)]));
    let manifest = load(&named).unwrap();
    assert_eq!(
        manifest.families["shots"].family.defer_platforms,
        [Os::Windows]
    );
    assert!(
        load(SHOTS).unwrap().families["shots"]
            .family
            .defer_platforms
            .is_empty()
    );

    for platforms in ["[\"macos\"]", "\"any\""] {
        let undeclared = named.replace("[\"macos\", \"windows\"]", platforms);
        assert_eq!(
            load(&undeclared).unwrap_err().key,
            "family.shots.defer_platforms"
        );
    }
}

#[test]
fn single_batch_marks_a_family_that_runs_in_one_tip_batch() {
    let plain = Manifest::from_files(&files(&[("checks/rust.toml", RUST)])).unwrap();
    assert!(!plain.families["rust"].family.single_batch);
    let batched = RUST.replacen(
        "needs = [\"rust-tests\"]\n",
        "needs = [\"rust-tests\"]\nsingle_batch = true\n",
        1,
    );
    let (manifest, unknown) =
        Manifest::from_files_with_unknown(&files(&[("checks/rust.toml", &batched)])).unwrap();
    assert!(unknown.is_empty(), "{unknown:?}");
    assert!(manifest.families["rust"].family.single_batch);
}
