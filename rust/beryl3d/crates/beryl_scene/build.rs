//! Writes `data/beryl3d/space.json` as a string constant and its numbers as constants of the
//! module `space`, and each number of the `audit` object of `data/beryl3d/gates.json` as an
//! `f32` constant named for its key in upper case, to `OUT_DIR/data.rs`, which `src/data.rs`
//! includes. `write_space` describes the one rule that turns `space.json` into modules and
//! constants.

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

/// Writes the module `space` from `space.json` by one rule, whatever the keys are. An object
/// becomes a `pub mod` named for its key. A number becomes a `pub const` named for its key in
/// upper case: a `usize` when its dotted path is listed in the top-level `counts` array, and
/// an `f32` otherwise. An array of two or three numbers becomes a `glam::Vec2` or `glam::Vec3`,
/// and an array of such arrays a fixed-size array of them. `counts` itself is not written.
fn write_space(json: &serde_json::Value, out: &mut String) {
    let counts: Vec<&str> = json["counts"]
        .as_array()
        .map(|paths| paths.iter().filter_map(serde_json::Value::as_str).collect())
        .unwrap_or_default();
    out.push_str("/// The numbers of `data/beryl3d/space.json`.\npub mod space {\n");
    let object = json.as_object().expect("space.json is an object");
    write_members(object, "", 1, &counts, out);
    out.push_str("}\n");
}

/// Writes the members of `object`, whose own dotted path is `path`, at `depth` levels of
/// indentation.
fn write_members(
    object: &serde_json::Map<String, serde_json::Value>,
    path: &str,
    depth: usize,
    counts: &[&str],
    out: &mut String,
) {
    let indent = "    ".repeat(depth);
    for (key, value) in object {
        if path.is_empty() && key == "counts" {
            continue;
        }
        let dotted = if path.is_empty() {
            key.clone()
        } else {
            format!("{path}.{key}")
        };
        writeln!(out, "{indent}/// `{dotted}` in `data/beryl3d/space.json`.").unwrap();
        if let Some(members) = value.as_object() {
            let module: String = key
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect();
            writeln!(out, "{indent}pub mod {module} {{").unwrap();
            write_members(members, &dotted, depth + 1, counts, out);
            writeln!(out, "{indent}}}").unwrap();
        } else {
            let name = key.to_uppercase();
            let (kind, expression) = if counts.contains(&dotted.as_str()) {
                let count = value
                    .as_u64()
                    .unwrap_or_else(|| panic!("space.json's `{dotted}` is a count"));
                ("usize".to_string(), count.to_string())
            } else {
                value_expression(value)
                    .unwrap_or_else(|| panic!("space.json's `{dotted}` is not a number or vector"))
            };
            writeln!(out, "{indent}pub const {name}: {kind} = {expression};").unwrap();
        }
    }
}

/// The type and the constant expression of a number, a vector or an array of vectors.
fn value_expression(value: &serde_json::Value) -> Option<(String, String)> {
    if let Some(number) = value.as_f64() {
        return Some(("f32".to_string(), format!("{number:?}_f32")));
    }
    let items = value.as_array()?;
    if items.iter().all(serde_json::Value::is_number) && matches!(items.len(), 2 | 3) {
        let numbers: Vec<String> = items
            .iter()
            .map(|item| format!("{:?}_f32", item.as_f64().unwrap()))
            .collect();
        let kind = format!("glam::Vec{}", items.len());
        return Some((kind.clone(), format!("{kind}::new({})", numbers.join(", "))));
    }
    let elements: Vec<(String, String)> =
        items.iter().map(value_expression).collect::<Option<_>>()?;
    let kind = &elements.first()?.0;
    if kind == "f32" || elements.iter().any(|(other, _)| other != kind) {
        return None;
    }
    let expressions: Vec<&str> = elements.iter().map(|(_, e)| e.as_str()).collect();
    Some((
        format!("[{kind}; {}]", elements.len()),
        format!("[{}]", expressions.join(", ")),
    ))
}
