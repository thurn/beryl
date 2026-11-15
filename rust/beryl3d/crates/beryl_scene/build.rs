//! Writes `data/beryl3d/space.json` as a string constant and its numbers as constants of the
//! module `space`, and each number of the `audit` object of `data/beryl3d/gates.json` as an
//! `f32` constant named for its key in upper case, to `OUT_DIR/data.rs`, which `src/data.rs`
//! includes. In `space`, each verb or case under `verbs` is a module of `f32` constants, one per
//! value, and each count under `catch_up` a `usize` constant. Under `frame_rate`, a whole
//! number becomes a `usize` constant and any other number an `f32` one.

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
    let space_json: serde_json::Value = serde_json::from_str(&space_text).unwrap();
    let mut out = format!(
        "/// The contents of `data/beryl3d/space.json`.\npub const SPACE: &str = {space_text:?};\n"
    );
    write_space(&space_json, &mut out);
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

/// Writes the module `space`: a module of `f32` constants for each entry under `verbs`, and a
/// `usize` constant for each count under `catch_up`.
fn write_space(json: &serde_json::Value, out: &mut String) {
    out.push_str("/// The numbers of `data/beryl3d/space.json`.\npub mod space {\n");
    out.push_str(
        "    /// Each verb's or case's spatial values, in millimetres.\n    pub mod verbs {\n",
    );
    for (verb, values) in json["verbs"]
        .as_object()
        .expect("space.json's `verbs` is an object")
    {
        let module: String = verb
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        writeln!(
            out,
            "        /// `verbs.{verb}` in `data/beryl3d/space.json`.\n        pub mod {module} {{"
        )
        .unwrap();
        for (key, value) in values.as_object().expect("a verb's values are an object") {
            let number = value
                .as_f64()
                .unwrap_or_else(|| panic!("space.json's `verbs.{verb}.{key}` is a number"));
            let name = key.to_uppercase();
            writeln!(out, "            pub const {name}: f32 = {number:?}_f32;").unwrap();
        }
        out.push_str("        }\n");
    }
    out.push_str("    }\n");
    out.push_str("    /// How many updates must wait before the script clock runs faster.\n    pub mod catch_up {\n");
    for (key, value) in json["catch_up"]
        .as_object()
        .expect("space.json's `catch_up` is an object")
    {
        let count = value
            .as_u64()
            .unwrap_or_else(|| panic!("space.json's `catch_up.{key}` is a count"));
        let name = key.to_uppercase();
        writeln!(out, "        /// `catch_up.{key}` in `data/beryl3d/space.json`.\n        pub const {name}: usize = {count};").unwrap();
    }
    out.push_str("    }\n");
    out.push_str("    /// When the host steps its frame rate down or up (`beryl_host`'s `rate`).\n    pub mod frame_rate {\n");
    for (key, value) in json["frame_rate"]
        .as_object()
        .expect("space.json's `frame_rate` is an object")
    {
        let name = key.to_uppercase();
        let constant = match (value.as_u64(), value.as_f64()) {
            (Some(count), _) => format!("pub const {name}: usize = {count};"),
            (None, Some(number)) => format!("pub const {name}: f32 = {number:?}_f32;"),
            _ => panic!("space.json's `frame_rate.{key}` is a number"),
        };
        writeln!(
            out,
            "        /// `frame_rate.{key}` in `data/beryl3d/space.json`.\n        {constant}"
        )
        .unwrap();
    }
    out.push_str("    }\n}\n");
}
