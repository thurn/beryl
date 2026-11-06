//! The checked-in TypeScript types match the Rust protocol.

use std::path::PathBuf;

use beryl_session::typescript::{PATH, typescript};

#[test]
fn the_generated_typescript_is_current() {
    // Read at run time: a test binary from the shared target directory may have been built in
    // a worktree that no longer exists.
    let path = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
    )
    .join("../../..")
    .join(PATH);
    let expected = typescript();
    if std::env::var_os("BERYL_UPDATE_TS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &expected).unwrap();
        return;
    }
    let actual = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        actual == expected,
        "{PATH} is stale: run `BERYL_UPDATE_TS=1 just test-rust -p beryl_session --test typescript`"
    );
}
