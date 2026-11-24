//! Determinism (architecture §14): no randomly seeded hashing, no clocks, no hidden global
//! state in the engine crates.

use std::path::{Path, PathBuf};

const BANNED: &[&str] = &[
    "HashMap",
    "HashSet",
    "RandomState",
    "std::time",
    "Instant::",
    "SystemTime",
    "thread_local!",
    "static mut",
];

/// Files allowed one banned name, and why.
const EXEMPT: &[(&str, &str)] = &[(
    // The `coverage` feature's record of the abilities a game exercised, which only tests
    // enable and the engine only writes, so no game depends on it.
    "coverage.rs",
    "thread_local!",
)];

/// The engine crates' sources, without this file, which has to name what it bans.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") && !path.ends_with(file!()) {
            out.push(path);
        }
    }
}

#[test]
fn the_engine_crates_use_no_banned_types() {
    let crates = beryl_test_files::root().unwrap().join("engine/crates");
    let mut files = Vec::new();
    for name in ["beryl_rules", "beryl_cards"] {
        // Opening the folder through the helper makes its whole tree an input of this test;
        // `sources` then walks it.
        beryl_test_files::read_dir(format!("engine/crates/{name}/src")).unwrap();
        sources(&crates.join(name).join("src"), &mut files);
    }
    assert!(files.len() > 10);
    let mut found = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap();
        for banned in BANNED {
            let exempt = EXEMPT
                .iter()
                .any(|&(name, allowed)| file.ends_with(name) && allowed == *banned);
            if text.contains(banned) && !exempt {
                found.push(format!("{}: {banned}", file.display()));
            }
        }
    }
    assert!(found.is_empty(), "banned in the engine crates: {found:#?}");
}
