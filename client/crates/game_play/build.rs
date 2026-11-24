//! Generates the sound names from the audio manifest.

use std::fmt::Write;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    write_sounds(&manifest, &out_dir);
}

/// Writes the names of the keys of `sounds` in `data/audio/manifest.json`, in the order the
/// file lists them, as `sound::NAMES` for `src/sound.rs` to include. `serde_json` keeps an
/// object's keys sorted, so the order is found from where each key stands in the file's text.
fn write_sounds(manifest: &std::path::Path, out_dir: &std::path::Path) {
    let path = manifest.join("../../../data/audio/manifest.json");
    println!("cargo:rerun-if-changed={}", path.display());
    let text = fs::read_to_string(&path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let sounds = json["sounds"]
        .as_object()
        .expect("the manifest's `sounds` is an object");
    let start = text
        .find("\"sounds\":")
        .expect("the manifest's text has `sounds`");
    let mut names: Vec<(usize, &String)> = sounds
        .keys()
        .map(|name| {
            let at = text[start..]
                .find(&format!("\"{name}\":"))
                .unwrap_or_else(|| panic!("the manifest's text has the sound {name}"));
            (at, name)
        })
        .collect();
    names.sort();
    let mut out = format!(
        "/// The sounds of `data/audio/manifest.json`, in the order the file lists them.\npub const NAMES: [&str; {}] = [\n",
        names.len()
    );
    for (_, name) in names {
        writeln!(out, "    {name:?},").unwrap();
    }
    out.push_str("];\n");
    fs::write(out_dir.join("sounds.rs"), out).unwrap();
}
