//! Writes `data/beryl3d/space.json` and the `audit` object of `data/beryl3d/gates.json` to
//! `OUT_DIR/data.rs` as string constants, which `src/data.rs` includes.

use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let data = manifest.join("../../../../data/beryl3d");
    let space = data.join("space.json");
    let gates = data.join("gates.json");
    println!("cargo:rerun-if-changed={}", space.display());
    println!("cargo:rerun-if-changed={}", gates.display());
    let space_text = fs::read_to_string(&space).unwrap();
    let gates_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&gates).unwrap()).unwrap();
    // Checked to parse, so a malformed file fails the build, not a run.
    serde_json::from_str::<serde_json::Value>(&space_text).unwrap();
    let audit = gates_json["audit"].to_string();
    let out = format!(
        "/// The contents of `data/beryl3d/space.json`.\npub const SPACE: &str = {space_text:?};\n\
         /// The `audit` object of `data/beryl3d/gates.json`.\npub const AUDIT: &str = {audit:?};\n"
    );
    let dest = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("data.rs");
    fs::write(dest, out).unwrap();
}
