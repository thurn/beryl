//! Fingerprint the checked-in wire types so builds can compare them during a handshake.

#[path = "src/wire_fingerprint.rs"]
mod wire_fingerprint;

use std::path::PathBuf;

fn main() {
    let relative = "../../../ui/src/generated/protocol.ts";
    println!("cargo:rerun-if-changed={relative}");
    let protocol = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets it"))
        .join(relative);
    let bytes = std::fs::read(protocol).expect("read the generated TypeScript protocol");
    let hash = wire_fingerprint::extend(0xcbf2_9ce4_8422_2325, &bytes);
    println!("cargo:rustc-env=BERYL_WIRE_HASH={hash:016x}");
}
