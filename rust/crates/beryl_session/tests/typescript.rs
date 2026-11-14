//! The checked-in TypeScript types match the Rust protocol.

use std::path::PathBuf;

use beryl_engine::card::Purpose;
use beryl_session::prompt::{PartName, PromptView, ShapeBody, Words};
use beryl_session::typescript::{PATH, typescript};
use beryl_session::{WIRE_HASH, wire_hash};

/// The checked-in file's path. It is read at run time: a test binary from the shared target
/// directory may have been built in a worktree that no longer exists.
fn path() -> PathBuf {
    PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(PATH)
}

#[test]
fn the_generated_typescript_is_current() {
    let path = path();
    let expected = typescript();
    if std::env::var_os("BERYL_UPDATE_TS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &expected).unwrap();
        return;
    }
    let actual = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        actual == expected,
        "{PATH} is stale: run `BERYL_UPDATE_TS=1 just test-rust -p beryl_session --test session typescript`"
    );
}

#[test]
fn the_wire_hash_covers_additional_definitions() {
    assert_eq!(WIRE_HASH.len(), 16);
    assert!(WIRE_HASH.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_eq!(wire_hash(&[]), WIRE_HASH);
    assert_ne!(wire_hash(&["action"]), WIRE_HASH);
    assert_ne!(
        wire_hash(&["action", "lobby"]),
        wire_hash(&["action", "log"])
    );
    assert_ne!(wire_hash(&["act", "ion"]), wire_hash(&["action"]));
}

/// A prompt's purpose goes on the wire as one of the string literals of the checked-in
/// TypeScript type `Purpose`, and comes back as itself: a purpose renamed in Rust fails the
/// UI's type check wherever the UI compares a prompt's purpose with the old name.
#[test]
fn each_purpose_is_a_literal_of_the_typescript_purpose_type() {
    let generated = std::fs::read_to_string(path()).unwrap_or_default();
    let declared = (generated.lines())
        .find_map(|line| line.strip_prefix("export type Purpose = "))
        .expect("protocol.ts declares the type Purpose");
    for &purpose in Purpose::ALL {
        let view = PromptView {
            revision: 0,
            purpose,
            part: PartName::Pick,
            title: String::new(),
            words: Words::default(),
            count: None,
            about: Vec::new(),
            from: None,
            marks: Vec::new(),
            previews: Vec::new(),
            buttons: Vec::new(),
            body: ShapeBody::Candidates,
        };
        let json = serde_json::to_value(&view).unwrap();
        let literal = json["purpose"].to_string();
        assert!(
            declared
                .split(" | ")
                .any(|name| name.trim_end_matches(';') == literal),
            "{literal} is not a literal of the TypeScript type Purpose"
        );
        let back: PromptView = serde_json::from_value(json).unwrap();
        assert_eq!(back.purpose, purpose);
    }
}
