//! Writes `data/beryl3d/space.json` as a string constant, and each number of the `audit` object
//! of `data/beryl3d/gates.json` as an `f32` constant named for its key in upper case, to
//! `OUT_DIR/data.rs`, which `src/data.rs` includes.

use std::fmt::Write;
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
    let mut out = format!(
        "/// The contents of `data/beryl3d/space.json`.\npub const SPACE: &str = {space_text:?};\n"
    );
    let audit = gates_json["audit"]
        .as_object()
        .expect("gates.json's `audit` is an object");
    out.push_str(
        "/// The interface checks' thresholds: the `audit` object of `data/beryl3d/gates.json`.\n",
    );
    out.push_str("pub mod audit {\n");
    for (key, value) in audit {
        let number = value
            .as_f64()
            .unwrap_or_else(|| panic!("gates.json's `audit.{key}` is a number"));
        let name = key.to_uppercase();
        writeln!(
            out,
            "    /// `audit.{key}` in `data/beryl3d/gates.json`.\n    pub const {name}: f32 = {number:?}_f32;"
        )
        .unwrap();
    }
    out.push_str("}\n");
    let dest = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("data.rs");
    fs::write(dest, out).unwrap();
}
