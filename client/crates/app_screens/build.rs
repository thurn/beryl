//! Generates the audio credits.

use std::fmt::Write;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    write_credits(&manifest, &out_dir);
}

/// Writes `CREDITS` for the settings screen. For each author whose work is used by a cue in
/// `data/audio/manifest.json`, it lists the kinds of work ("Music", "Sound" or "Music & Sound"),
/// the license and the titles of their packs. The authors are in alphabetical order.
fn write_credits(manifest: &std::path::Path, out_dir: &std::path::Path) {
    use std::collections::{BTreeMap, BTreeSet};
    let path = manifest.join("../../../data/audio/manifest.json");
    println!("cargo:rerun-if-changed={}", path.display());
    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    // Author -> (license, kinds, pack titles).
    let mut authors: BTreeMap<String, (String, BTreeSet<&str>, BTreeSet<String>)> = BTreeMap::new();
    for (kind, key) in [("Music", "music"), ("Sound", "sounds")] {
        let cues = json[key]
            .as_object()
            .unwrap_or_else(|| panic!("the manifest's `{key}` is an object"));
        for cue in cues.values() {
            for variant in cue["variants"].as_array().expect("a cue has `variants`") {
                let source = variant["source"]
                    .as_str()
                    .expect("a variant has a `source`");
                let source = &json["sources"][source];
                let text = |field: &str| {
                    source[field]
                        .as_str()
                        .unwrap_or_else(|| panic!("a source has `{field}`"))
                        .to_owned()
                };
                let entry = authors
                    .entry(text("author"))
                    .or_insert_with(|| (text("license"), BTreeSet::new(), BTreeSet::new()));
                entry.1.insert(kind);
                entry.2.insert(text("title"));
            }
        }
    }
    let mut out = String::from(
        "/// Who made the music and sounds: author, license, kinds of work and pack titles.\npub const CREDITS: &[(&str, &str, &str, &[&str])] = &[\n",
    );
    for (author, (license, kinds, packs)) in authors {
        let kinds = kinds.into_iter().collect::<Vec<_>>().join(" & ");
        writeln!(
            out,
            "    ({author:?}, {license:?}, {kinds:?}, &{:?}),",
            packs.into_iter().collect::<Vec<_>>()
        )
        .unwrap();
    }
    out.push_str("];\n");
    fs::write(out_dir.join("credits.rs"), out).unwrap();
}
