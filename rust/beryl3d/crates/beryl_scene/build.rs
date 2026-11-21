//! Generates the sound names, audio credits, manual table commands and sample solo deck.

use std::fmt::Write;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    write_sounds(&manifest, &out_dir);
    write_credits(&manifest, &out_dir);
    write_table_commands(&manifest, &out_dir);
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

/// Writes each command of `data/table-commands.json`, in the file's order, as a `Command`
/// constant of `src/manual/commands.rs` in the array `COMMANDS`, and the sample deck a solo
/// table starts with, `docs/sample-decks/...-wu-control.txt` read as the web's `deck_import`
/// reads a pasted list (a blank line after the main deck starts the sideboard), as the JSON of
/// a `TableSoloRequest`, `SOLO_REQUEST`. An `action` template stays JSON text, filled in when a
/// command is given; `also_applies_to: true` takes the command's own menu placement and `false`
/// none.
fn write_table_commands(manifest: &std::path::Path, out_dir: &std::path::Path) {
    let path = manifest.join("../../../../data/table-commands.json");
    let deck =
        manifest.join("../../../../docs/sample-decks/2025-11-18-17lands-cb8d5d5f-wu-control.txt");
    println!("cargo:rerun-if-changed={}", path.display());
    println!("cargo:rerun-if-changed={}", deck.display());
    let commands: Vec<serde_json::Value> =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let mut out = String::from(
        "/// The commands of `data/table-commands.json`, in the file's order.\npub const COMMANDS: &[Command] = &[\n",
    );
    for command in &commands {
        out.push_str(&command_line(command));
    }
    out.push_str("];\n");
    out.push_str(&solo_request(&deck));
    fs::write(out_dir.join("table_commands.rs"), out).unwrap();
}

/// One command of the commands file as a `Command` constant, on one line of `COMMANDS`.
fn command_line(command: &serde_json::Value) -> String {
    let text = |value: &serde_json::Value| format!("{:?}", value.as_str().unwrap());
    let optional = |value: Option<&serde_json::Value>| {
        value.map_or_else(
            || "None".to_owned(),
            |value| format!("Some({})", text(value)),
        )
    };
    let key = |value: &serde_json::Value| {
        format!(
            "CommandKey {{ mac: {}, other: {} }}",
            text(&value["mac"]),
            text(&value["other"])
        )
    };
    let placement = |value: &serde_json::Value| {
        format!(
            "Placement {{ group: {}, order: {}, submenu: {} }}",
            text(&value["group"]),
            value["order"].as_u64().unwrap(),
            optional(value.get("submenu")),
        )
    };
    let mut line = String::new();
    let menu = command.get("menu").map(&placement);
    let also: Vec<String> = command
        .get("also_applies_to")
        .and_then(serde_json::Value::as_object)
        .map(|also| {
            also.iter()
                .map(|(target, value)| {
                    let placement = match value {
                        serde_json::Value::Bool(true) => menu
                            .clone()
                            .map_or("None".to_owned(), |m| format!("Some({m})")),
                        serde_json::Value::Bool(false) => "None".to_owned(),
                        other => format!("Some({})", placement(other)),
                    };
                    format!("(AppliesTo::{}, {placement})", camel(target))
                })
                .collect()
        })
        .unwrap_or_default();
    let aliases: Vec<String> = command
        .get("key_aliases")
        .and_then(serde_json::Value::as_array)
        .map(|aliases| aliases.iter().map(&key).collect())
        .unwrap_or_default();
    writeln!(
        line,
        "    Command {{ id: {id}, label: {label}, applies_to: AppliesTo::{applies}, \
         scope: Scope::{scope}, asks: Asks::{asks}, key: {key}, key_aliases: &[{aliases}], \
         menu: {menu}, also_applies_to: &[{also}], group: {group}, assist: {assist}, \
         when: {when}, action: {action}, client: {client} }},",
        id = text(&command["id"]),
        label = text(&command["label"]),
        applies = camel(command["applies_to"].as_str().unwrap()),
        scope = camel(command["scope"].as_str().unwrap()),
        asks = camel(command["asks"].as_str().unwrap()),
        key = command
            .get("key")
            .map_or("None".to_owned(), |k| format!("Some({})", key(k))),
        aliases = aliases.join(", "),
        menu = menu.map_or("None".to_owned(), |m| format!("Some({m})")),
        also = also.join(", "),
        group = text(&command["group"]),
        assist = command.get("assist").map_or("None".to_owned(), |a| {
            format!("Some(Assist::{})", camel(a.as_str().unwrap()))
        }),
        when = optional(command.get("when")),
        action = command.get("action").map_or("None".to_owned(), |action| {
            format!("Some({:?})", action.to_string())
        }),
        client = optional(command.get("client")),
    )
    .unwrap();
    line
}

/// The sample deck at `deck` as the JSON of a `TableSoloRequest`, the constant `SOLO_REQUEST`.
fn solo_request(deck: &std::path::Path) -> String {
    let mut out = String::new();
    let (mut main, mut side) = (Vec::new(), Vec::new());
    let mut sideboard = false;
    for line in fs::read_to_string(deck).unwrap().lines() {
        let line = line.trim();
        if line.is_empty() {
            sideboard |= !main.is_empty();
            continue;
        }
        let (count, name) = line
            .split_once(' ')
            .expect("a deck line is a count and a name");
        let entry = serde_json::json!([name.trim(), count.parse::<u16>().unwrap()]);
        if sideboard {
            side.push(entry);
        } else {
            main.push(entry);
        }
    }
    let request = serde_json::json!({
        "deck": { "name": "Beryl sample deck", "main": main, "side": side }
    });
    writeln!(
        out,
        "/// The `TableSoloRequest` a solo table starts with, as JSON: the sample WU Control deck.\npub const SOLO_REQUEST: &str = {:?};",
        request.to_string()
    )
    .unwrap();
    out
}

/// `snake_case` as `CamelCase`: the name of the variant a value of the commands file names.
fn camel(snake: &str) -> String {
    snake
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_ascii_uppercase().to_string() + chars.as_str()
            })
        })
        .collect()
}
