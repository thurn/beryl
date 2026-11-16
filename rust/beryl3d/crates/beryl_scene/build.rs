//! Writes `data/beryl3d/space.json` as a string constant and its numbers as constants of the
//! module `space`, and each number of the `audit` object of `data/beryl3d/gates.json` as an
//! `f32` constant named for its key in upper case, to `OUT_DIR/data.rs`, which `src/data.rs`
//! includes. `write_space` describes the one rule that turns `space.json` into modules and
//! constants. It also writes the design tokens of `data/design-tokens.json` as the constants of
//! `src/tokens.rs`'s modules, one file each under `OUT_DIR/tokens` (`write_tokens`).

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
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    fs::write(out_dir.join("data.rs"), out).unwrap();
    write_sounds(&manifest, &out_dir);
    write_credits(&manifest, &out_dir);
    let tokens = manifest.join("../../../../data/design-tokens.json");
    println!("cargo:rerun-if-changed={}", tokens.display());
    let tokens_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&tokens).unwrap()).unwrap();
    let tokens_dir = out_dir.join("tokens");
    fs::create_dir_all(&tokens_dir).unwrap();
    for (module, text) in write_tokens(&tokens_json) {
        fs::write(tokens_dir.join(format!("{module}.rs")), text).unwrap();
    }
}

/// Writes the names of the keys of `sounds` in `ui/src/audio/manifest.json`, in the order the
/// file lists them, as `sound::NAMES` for `src/sound.rs` to include. `serde_json` keeps an
/// object's keys sorted, so the order is found from where each key stands in the file's text.
/// Nothing is written under `ui/`.
fn write_sounds(manifest: &std::path::Path, out_dir: &std::path::Path) {
    let path = manifest.join("../../../../ui/src/audio/manifest.json");
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
        "/// The sounds of `ui/src/audio/manifest.json`, in the order the file lists them.\npub const NAMES: [&str; {}] = [\n",
        names.len()
    );
    for (_, name) in names {
        writeln!(out, "    {name:?},").unwrap();
    }
    out.push_str("];\n");
    fs::write(out_dir.join("sounds.rs"), out).unwrap();
}

/// Writes `CREDITS` for `src/screens/settings.rs`: each author whose work the music or sounds of
/// `ui/src/audio/manifest.json` use, as `ui/src/audio/SoundControls.tsx`'s `credits` finds them,
/// with the kinds of work ("Music", "Sound" or "Music & Sound"), the license and the titles of
/// their packs. The authors are in alphabetical order. Nothing is written under `ui/`.
fn write_credits(manifest: &std::path::Path, out_dir: &std::path::Path) {
    use std::collections::{BTreeMap, BTreeSet};
    let path = manifest.join("../../../../ui/src/audio/manifest.json");
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

/// The modules of `src/tokens.rs` from `design-tokens.json`, each as its name and its
/// constants. `color` holds a constant for every token of kind `color`, named for the token in
/// upper case with `_` for `-`, then the colors of `flat.color`. `space`, `type_style` and
/// `size` hold the entries of `flat` under the same names, and `mana` the colors of
/// `flat.mana`. A number in `flat` is a `value`, or the `token` of kind `length` whose pixels it
/// takes; a `note` becomes the doc comment.
fn write_tokens(json: &serde_json::Value) -> Vec<(&'static str, String)> {
    let tokens = Tokens {
        list: json["tokens"].as_array().expect("`tokens` is an array"),
        flat: &json["flat"],
    };
    let mut space = String::new();
    let mut size = String::new();
    for (module, out) in [("space", &mut space), ("size", &mut size)] {
        for (key, entry) in tokens.entries(module) {
            let (value, doc) = tokens.number(entry, &format!("flat.{module}.{key}"));
            writeln!(out, "/// {doc}\npub const {key}: f32 = {value:?};").unwrap();
        }
    }
    vec![
        ("space", space),
        ("type_style", write_type_styles(&tokens)),
        ("size", size),
        ("color", write_colors(&tokens)),
        ("mana", write_mana(&tokens)),
    ]
}

/// The module `mana`: each entry of `flat.mana`, a mana symbol's disc or ink as the hex color
/// in its `value`.
fn write_mana(tokens: &Tokens) -> String {
    let mut out = String::new();
    for (key, entry) in tokens.entries("mana") {
        let value = entry["value"]
            .as_str()
            .unwrap_or_else(|| panic!("`flat.mana.{key}` has a color `value`"));
        let rgba = parse_color(value, &|name| tokens.find(name));
        let doc = entry["note"].as_str().unwrap_or("A mana symbol's color.");
        writeln!(out, "/// {doc}\npub const {key}: [u8; 4] = {rgba:?};").unwrap();
    }
    out
}

/// The tokens of `design-tokens.json` and its `flat` object.
struct Tokens<'a> {
    list: &'a [serde_json::Value],
    flat: &'a serde_json::Value,
}

impl<'a> Tokens<'a> {
    /// The token named `name`.
    fn find(&self, name: &str) -> &'a serde_json::Value {
        self.list
            .iter()
            .find(|token| token["name"] == name)
            .unwrap_or_else(|| panic!("design-tokens.json has no token `{name}`"))
    }

    /// The entries of `flat.<module>`.
    fn entries(&self, module: &str) -> &'a serde_json::Map<String, serde_json::Value> {
        self.flat[module]
            .as_object()
            .unwrap_or_else(|| panic!("design-tokens.json's `flat.{module}` is an object"))
    }

    /// A number of `flat` at `path` and its doc comment: a plain number, or an object with its
    /// `value` or the `token` whose length in pixels it takes, and an optional `note`.
    fn number(&self, entry: &serde_json::Value, path: &str) -> (f32, String) {
        let doc = entry["note"].as_str().map(str::to_string);
        if let Some(name) = entry["token"].as_str() {
            let value = self.find(name)["value"].as_str().unwrap_or_default();
            let pixels = value
                .strip_suffix("px")
                .and_then(|n| n.parse::<f32>().ok())
                .unwrap_or_else(|| panic!("`{path}` names `{name}`, which is not a length"));
            let doc = doc.map_or(format!("`--{name}`."), |d| format!("`--{name}`: {d}"));
            return (pixels, doc);
        }
        #[allow(clippy::cast_possible_truncation)]
        let value = entry
            .as_f64()
            .or_else(|| entry["value"].as_f64())
            .unwrap_or_else(|| panic!("`{path}` is a number, or has a `value` or a `token`"))
            as f32;
        (value, doc.unwrap_or_else(|| format!("`{path}`.")))
    }
}

/// The module `type_style`: a `TypeStyle` for each entry of `flat.type_style`, and `ALL`.
fn write_type_styles(tokens: &Tokens) -> String {
    let mut out = String::new();
    let styles = tokens.entries("type_style");
    for (key, entry) in styles {
        let path = format!("flat.type_style.{key}");
        let font = entry["font"]
            .as_str()
            .expect("a type style names its `font`");
        let weight = entry["weight"]
            .as_u64()
            .expect("a type style has a `weight`");
        let (size, _) = tokens.number(&entry["size"], &path);
        let (line, _) = tokens.number(&entry["line_height"], &path);
        let doc = entry["note"]
            .as_str()
            .map_or(format!("`{path}`."), str::to_string);
        writeln!(
            out,
            "/// {doc}\npub const {key}: TypeStyle = TypeStyle {{\n    font: Font::{font},\n    \
             weight: {weight},\n    size: {size:?},\n    line_height: {line:?},\n}};"
        )
        .unwrap();
    }
    let names: Vec<&str> = styles.keys().map(String::as_str).collect();
    writeln!(
        out,
        "/// Every type style, for the glyph rasterizer to prepare before the first frame.\n\
         pub const ALL: [TypeStyle; {}] = [{}];",
        names.len(),
        names.join(", ")
    )
    .unwrap();
    out
}

/// The module `color`: each token of kind `color`, then each entry of `flat.color`, the
/// `color` token it names with its own `alpha`.
fn write_colors(tokens: &Tokens) -> String {
    let find = |name: &str| tokens.find(name);
    let mut out = String::new();
    for token in tokens.list.iter().filter(|token| token["kind"] == "color") {
        let name = token["name"].as_str().unwrap();
        let rgba = parse_color(token["value"].as_str().unwrap(), &find);
        let constant = name.to_uppercase().replace('-', "_");
        let doc = token["note"]
            .as_str()
            .map_or(format!("`--{name}`."), |note| format!("`--{name}`: {note}"));
        writeln!(out, "/// {doc}\npub const {constant}: [u8; 4] = {rgba:?};").unwrap();
    }
    for (key, entry) in tokens.entries("color") {
        // A flat color is a token's, named by `color`, or a CSS color of its own in `value`.
        let (mut rgba, base) = match (entry["color"].as_str(), entry["value"].as_str()) {
            (Some(base), _) => (
                parse_color(find(base)["value"].as_str().unwrap(), &find),
                format!("`--{base}`."),
            ),
            (None, Some(value)) => (parse_color(value, &find), format!("`{value}`.")),
            (None, None) => panic!("`flat.color.{key}` names its `color` or gives its `value`"),
        };
        if let Some(alpha) = entry["alpha"].as_u64() {
            rgba[3] = u8::try_from(alpha).expect("`alpha` is from 0 to 255");
        }
        let doc = entry["note"].as_str().map_or(base, str::to_string);
        writeln!(out, "/// {doc}\npub const {key}: [u8; 4] = {rgba:?};").unwrap();
    }
    out
}

/// A CSS color as RGBA: `#rrggbb`, `rgb(r g b)`, `rgb(r g b / a%)`, or `rgb(var(--name))`
/// where the token `name` holds `r g b`. `find` looks a token up by name.
fn parse_color<'a>(value: &str, find: &dyn Fn(&str) -> &'a serde_json::Value) -> [u8; 4] {
    if let Some(hex) = value.strip_prefix('#') {
        let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap();
        assert_eq!(hex.len(), 6, "`{value}` is a six-digit hex color");
        return [channel(0), channel(2), channel(4), 0xff];
    }
    let inner = value
        .strip_prefix("rgb(")
        .and_then(|v| v.strip_suffix(')'))
        .unwrap_or_else(|| panic!("`{value}` is not a color the build script reads"));
    let inner = match inner
        .strip_prefix("var(--")
        .and_then(|v| v.strip_suffix(')'))
    {
        Some(name) => find(name)["value"].as_str().unwrap(),
        None => inner,
    };
    let (channels, alpha) = inner.split_once('/').unwrap_or((inner, "100%"));
    let mut rgba = [0xff; 4];
    for (slot, channel) in rgba.iter_mut().zip(channels.split_whitespace()) {
        *slot = channel
            .parse()
            .unwrap_or_else(|_| panic!("`{value}` has a bad channel"));
    }
    let percent: f64 = alpha
        .trim()
        .strip_suffix('%')
        .and_then(|a| a.parse().ok())
        .unwrap_or_else(|| panic!("`{value}` has a bad alpha"));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = (percent * 255.0 / 100.0).round() as u8;
    rgba[3] = byte;
    rgba
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
