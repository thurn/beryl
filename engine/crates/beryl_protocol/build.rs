//! Fingerprint the Rust definitions of the JSON messages exchanged by table peers.

#[path = "src/wire_fingerprint.rs"]
mod wire_fingerprint;
mod wire_schema;

use std::path::{Path, PathBuf};

use beryl_card_build::catalogs::Catalogs;

// The envelope's Value payloads are TableRequest, GameUpdate and LobbyUpdate. Include
// the modules that define their fields and the custom serializers and macros they
// use. Add new sources here when a wire type starts depending on another module.
const WIRE_SOURCES: &[&str] = &[
    "../beryl_card/src/counter.rs",
    "../beryl_card/src/ids.rs",
    "../beryl_card/src/prompt.rs",
    "../beryl_link/src/wire/envelope.rs",
    "../beryl_table/src/action.rs",
    "../beryl_table/src/lib.rs",
    "../beryl_table/src/state.rs",
    "src/ipc/table.rs",
    "src/lib.rs",
    "src/prompt/view.rs",
    "src/protocol",
    "src/script/act.rs",
    "src/script/mod.rs",
    "src/script/table.rs",
    "src/script/timing.rs",
    "src/script/timing/own.rs",
    "src/script/verb.rs",
];

// Map the Rust fingerprint of the transition schema to its published wire hash.
// Keep both values fixed so future schema edits produce a different handshake.
const TRANSITION_RUST_HASH: u64 = 0x97f4_8d8d_a5ee_5e77;
const TRANSITION_WIRE_HASH: u64 = 0xb130_5f31_dfe9_4584;

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets it"));
    let mut schema_hash = 0xcbf2_9ce4_8422_2325;
    for relative in WIRE_SOURCES {
        println!("cargo:rerun-if-changed={relative}");
        schema_hash = hash_source(schema_hash, &manifest.join(relative));
    }
    let catalogs =
        Catalogs::read(&manifest.join("../../..")).expect("read the vocabulary catalogs");
    schema_hash = wire_schema::fingerprint(schema_hash, &catalogs.counter_kinds_source())
        .expect("the generated counter kinds are Rust tokens");
    let hash = schema_hash ^ TRANSITION_RUST_HASH ^ TRANSITION_WIRE_HASH;
    println!("cargo:rustc-env=BERYL_WIRE_HASH={hash:016x}");
}

fn hash_source(hash: u64, path: &Path) -> u64 {
    if path.is_dir() {
        let mut paths: Vec<_> = std::fs::read_dir(path)
            .unwrap_or_else(|error| panic!("list wire definitions in {}: {error}", path.display()))
            .map(|entry| entry.expect("read wire directory entry").path())
            .filter(|path| path.is_dir() || path.extension().is_some_and(|ext| ext == "rs"))
            .collect();
        paths.sort_unstable();
        return paths
            .iter()
            .fold(hash, |hash, path| hash_source(hash, path));
    }

    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("read wire definitions in {}: {error}", path.display()));
    wire_schema::fingerprint(hash, &source)
        .unwrap_or_else(|error| panic!("tokenize wire definitions in {}: {error}", path.display()))
}
