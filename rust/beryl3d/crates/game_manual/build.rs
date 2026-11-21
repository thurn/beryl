//! Generates the manual table commands and sample solo deck.

use std::fmt::Write;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    write_table_commands(&manifest, &out_dir);
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
