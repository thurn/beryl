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
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    for name in ["beryl_engine", "beryl_cards"] {
        sources(&crates.join(name).join("src"), &mut files);
    }
    assert!(files.len() > 10);
    let mut found = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap();
        for banned in BANNED {
            if text.contains(banned) {
                found.push(format!("{}: {banned}", file.display()));
            }
        }
    }
    assert!(found.is_empty(), "banned in the engine crates: {found:#?}");
}
