//! Fingerprint the checked-in wire types so builds can compare them during a handshake.

#[path = "src/wire_fingerprint.rs"]
mod wire_fingerprint;

use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets it"));
    let mut hash = 0xcbf2_9ce4_8422_2325;
    for relative in [
        "../../../ui/src/generated/protocol.ts",
        "../beryl_link/src/wire/envelope.rs",
    ] {
        println!("cargo:rerun-if-changed={relative}");
        let bytes = std::fs::read(manifest.join(relative))
            .unwrap_or_else(|error| panic!("read wire definitions in {relative}: {error}"));
        hash = wire_fingerprint::extend(hash, &(bytes.len() as u64).to_le_bytes());
        hash = wire_fingerprint::extend(hash, &bytes);
    }
    println!("cargo:rustc-env=BERYL_WIRE_HASH={hash:016x}");
}
