//! Every transaction that reads and then writes starts `IMMEDIATE`. In WAL mode a deferred
//! transaction that reads first and then upgrades to a write fails at once with
//! `SQLITE_BUSY_SNAPSHOT` when another connection wrote in between, instead of waiting for
//! the lock. So code under `ignite_store` and `ignite_hub` must not call
//! `Connection::transaction()`; the schema migrations (which run before any other connection
//! exists) are the one exception.

use std::path::Path;

fn sources(dir: &Path, found: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push((
                path.display().to_string(),
                std::fs::read_to_string(&path).unwrap(),
            ));
        }
    }
}

#[test]
fn no_code_starts_a_deferred_transaction() {
    let crates = ignite_core::test_files::root()
        .unwrap()
        .join("ignite/crates");
    let mut files = Vec::new();
    for krate in ["ignite_store", "ignite_hub"] {
        // Opening the folder through the helper makes its whole tree an input of this test.
        ignite_core::test_files::read_dir(format!("ignite/crates/{krate}/src")).unwrap();
        sources(&crates.join(krate).join("src"), &mut files);
    }
    let call = [".transaction", "()"].concat();
    let offenders: Vec<&str> = files
        .iter()
        .filter(|(path, text)| !path.ends_with("migrate.rs") && text.contains(&call))
        .map(|(path, _)| path.as_str())
        .collect();
    assert!(
        offenders.is_empty(),
        "deferred transactions in {offenders:?}"
    );
}
