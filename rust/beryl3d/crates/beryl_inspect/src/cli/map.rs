//! A map of the client assembled from the same registries used by the client and its tools.

mod entries;

use std::fmt::Write;

use serde::{Deserialize, Serialize};

use super::options::nearest;
use super::{Options, Printed, Usage};

const MOST_ENTRIES: usize = 95;

struct Kind {
    name: &'static str,
    path: &'static str,
    entries: fn() -> Result<Vec<Entry>, String>,
}

const KINDS: &[Kind] = &[
    Kind::new("crates", "data/beryl3d/levels.json", entries::crates),
    Kind::new("features", "app_client/src/client/features.rs", || {
        Ok(entries::features())
    }),
    Kind::new(
        "containers",
        "game_table/src/table/place.rs",
        entries::containers,
    ),
    Kind::new("profiles", "data/beryl3d/profiles", entries::profiles),
    Kind::new("verbs", "game_play/src/verbs", || Ok(entries::verbs())),
    Kind::new("effects", "data/beryl3d/effects", entries::effects),
    Kind::new("primitives", "kit_base/src/data/effect.rs", || {
        Ok(entries::primitives())
    }),
    Kind::new("policies", "kit_space/src/arrange.rs", || {
        Ok(entries::policies())
    }),
    Kind::new("roles", "kit_frame/src/frame.rs", || Ok(entries::roles())),
    Kind::new("widgets", "kit_ui/src/widgets", || Ok(entries::widgets())),
    Kind::new("data", "kit_base/src/data/tuning.rs", entries::data),
    Kind::new("checks", "kit_probe/src/agent/fault.rs", || {
        Ok(entries::checks())
    }),
    Kind::new("stories", "beryl_inspect/src/stories.rs", entries::stories),
    Kind::new(
        "commands",
        "beryl_inspect/src/cli/mod.rs",
        entries::commands,
    ),
];

impl Kind {
    const fn new(
        name: &'static str,
        path: &'static str,
        entries: fn() -> Result<Vec<Entry>, String>,
    ) -> Self {
        Self {
            name,
            path,
            entries,
        }
    }

    fn path(&self) -> String {
        if self.path.starts_with("data/") {
            self.path.to_owned()
        } else {
            entries::crate_path(self.path)
        }
    }
}

#[derive(Deserialize, Serialize)]
struct Entry {
    name: String,
    path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    detail: String,
}

impl Entry {
    fn new(name: impl Into<String>, path: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            detail: detail.into(),
        }
    }
}

#[derive(Serialize)]
struct Summary {
    name: &'static str,
    count: usize,
    path: String,
}

#[derive(Serialize)]
struct Inventory<'a> {
    command: &'a str,
    kind: &'static str,
    count: usize,
    omitted: usize,
    entries: &'a [Entry],
}

pub(super) fn run(options: &Options) -> Result<Printed, Usage> {
    let command = options.command_line();
    let output = match options.rest.as_slice() {
        [] => overview(&command, options.json)?,
        [name] => {
            let kind = KINDS.iter().find(|kind| kind.name == name).ok_or_else(|| {
                let names: Vec<_> = KINDS.iter().map(|kind| kind.name).collect();
                Usage(format!(
                    "unknown map kind {name:?}. Nearest: {}. Kinds: {}.",
                    nearest(name, &names).join(", "),
                    names.join(", ")
                ))
            })?;
            inventory(kind, &command, options)?
        }
        _ => {
            return Err(Usage(
                "map takes at most one kind; `just b3d map` lists them".to_owned(),
            ));
        }
    };
    Ok(Printed::text(output))
}

fn overview(command: &str, as_json: bool) -> Result<String, Usage> {
    let kinds: Vec<_> = KINDS
        .iter()
        .map(|kind| {
            Ok(Summary {
                name: kind.name,
                count: (kind.entries)()?.len(),
                path: kind.path(),
            })
        })
        .collect::<Result<_, String>>()
        .map_err(Usage)?;

    if as_json {
        return json(&serde_json::json!({ "command": command, "kinds": kinds }));
    }
    let mut output = format!("{command}\n");
    for kind in kinds {
        let _ = writeln!(output, "{}  {}\t{}", kind.name, kind.count, kind.path);
    }
    Ok(output)
}

fn inventory(kind: &Kind, command: &str, options: &Options) -> Result<String, Usage> {
    let entries = (kind.entries)().map_err(Usage)?;
    let count = entries.len();
    let shown = if options.switch("--full") {
        count
    } else {
        count.min(MOST_ENTRIES)
    };
    let omitted = count - shown;
    if options.json {
        return json(&Inventory {
            command,
            kind: kind.name,
            count,
            omitted,
            entries: &entries[..shown],
        });
    }
    let mut output = format!("{command}\n");
    for entry in &entries[..shown] {
        let _ = write!(output, "{}", entry.name);
        if !entry.detail.is_empty() {
            let _ = write!(output, "  {}", entry.detail);
        }
        let _ = writeln!(output, "\t{}", entry.path);
    }
    if omitted > 0 {
        let _ = writeln!(output, "{omitted} more entries; use --full to show them");
    }
    Ok(output)
}

fn json(value: &impl Serialize) -> Result<String, Usage> {
    serde_json::to_string_pretty(value)
        .map(|text| text + "\n")
        .map_err(|error| Usage(error.to_string()))
}
