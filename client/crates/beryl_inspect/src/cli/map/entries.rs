//! Reads the owning registry for each kind of map entry.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use beryl_protocol::script::Verb;
use game_hud::labels::GAME_LABELS;
use game_table::profiles::Profile;
use kit_base::data::TUNING_FILES;
use kit_base::data::effect::{PrimitiveKind, SHIPPED};
use kit_base::data::profiles::{self, ProfileData, Profiles};
use kit_frame::frame::Role;
use kit_probe::agent::Rule;
use kit_scene::Keys;
use kit_space::arrange::PolicyKind;
use kit_ui::widgets::Widget;
use serde::Deserialize;

use super::super::options::size_text;
use super::Entry;
use crate::{data as tuning, pages, recordings, stories as catalog};

const CRATES: &str = "client/crates";
const LEVELS: &str = "data/client/levels.json";
const EFFECT_READER: &str = "Definition";
const PROFILE_READER: &str = "ProfileData";

pub(super) fn crate_path(path: &str) -> String {
    format!("{CRATES}/{path}")
}

fn relative(path: &Path) -> String {
    let path = path.strip_prefix(recordings::root()).unwrap_or(path);
    let text = path.to_string_lossy();
    if path.is_absolute() {
        text.into_owned()
    } else {
        text.replace('\\', "/")
    }
}

fn data_path(file: impl AsRef<Path>) -> String {
    relative(&tuning::directory().join(file))
}

fn source_path(source: &str) -> String {
    // The compiler emits workspace-relative source paths with native separators.
    let source = Path::new(source);
    if let Ok(path) = source.strip_prefix("crates") {
        crate_path(&relative(path))
    } else {
        relative(source)
    }
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

#[derive(Deserialize)]
struct Levels {
    crates: BTreeMap<String, CrateLevel>,
}

#[derive(Deserialize)]
struct CrateLevel {
    level: String,
    order: usize,
}

pub(super) fn crates() -> Result<Vec<Entry>, String> {
    let root = recordings::root();
    let levels: Levels = serde_json::from_str(&read(&root.join(LEVELS))?)
        .map_err(|error| format!("{LEVELS}: {error}"))?;
    let mut crates: Vec<_> = levels.crates.into_iter().collect();
    crates.sort_by_key(|(_, info)| info.order);
    crates
        .into_iter()
        .map(|(name, info)| {
            let src = root.join(crate_path(&name)).join("src");
            let lib = src.join("lib.rs");
            let source = read(&lib)?;
            let header = source
                .lines()
                .map_while(|line| line.strip_prefix("//!"))
                .map(str::trim)
                .collect::<Vec<_>>()
                .join(" ");
            let lines = source_lines(&src)?;
            Ok(Entry::new(
                name,
                relative(&lib),
                format!("{}  {lines} lines  {header}", info.level),
            ))
        })
        .collect()
}

fn source_lines(directory: &Path) -> Result<usize, String> {
    let error_at_directory = |error| format!("{}: {error}", directory.display());
    let mut lines = 0;
    for entry in fs::read_dir(directory).map_err(error_at_directory)? {
        let entry = entry.map_err(error_at_directory)?;
        let path = entry.path();
        let kind = entry.file_type().map_err(error_at_directory)?;
        if kind.is_dir() {
            lines += source_lines(&path)?;
        } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            lines += read(&path)?.lines().count();
        }
    }
    Ok(lines)
}

pub(super) fn features() -> Vec<Entry> {
    app_client::registered_features()
        .chain(app_client::composed_features())
        .map(|feature| {
            let shortcuts = feature
                .shortcuts
                .iter()
                .map(|shortcut| {
                    let keys = match shortcut.keys {
                        Keys::Binding(binding) => binding.to_owned(),
                        Keys::Command(id) => format!("command {id}"),
                        Keys::Commands(group) => format!("commands {group}"),
                    };
                    format!("{keys} ({})", shortcut.label)
                })
                .collect::<Vec<_>>()
                .join(", ");
            Entry::new(
                feature.id,
                source_path(feature.source),
                if shortcuts.is_empty() {
                    feature.placement
                } else {
                    format!("{}  shortcuts: {shortcuts}", feature.placement)
                },
            )
        })
        .collect()
}

struct LayoutSource {
    file: &'static str,
    layouts: &'static Profiles,
    definition: &'static ProfileData,
}

fn layouts() -> Result<Vec<LayoutSource>, String> {
    let layouts = &tuning::checkout_ref()?.profiles;
    let mut sources: Vec<_> = layouts
        .iter()
        .zip(profiles::FILES)
        .map(|(definition, file)| LayoutSource {
            file,
            layouts,
            definition,
        })
        .collect();
    for preview in &pages::INSPECTION_PROFILES {
        sources.push(LayoutSource {
            file: preview.file(),
            layouts: &preview.data()?.profiles,
            definition: preview.definition(),
        });
    }
    Ok(sources)
}

pub(super) fn profiles() -> Result<Vec<Entry>, String> {
    Ok(layouts()?
        .into_iter()
        .map(|layout| {
            let profile = layout.definition;
            let slots: Vec<_> = profile.slots.keys().map(String::as_str).collect();
            let windows = profile
                .test_windows()
                .map(|window| format!("{}@{}", size_text(window.size), window.scale))
                .collect::<Vec<_>>()
                .join(", ");
            Entry::new(
                &profile.name,
                data_path(layout.file),
                format!("slots: {}  test windows: {windows}", slots.join(", ")),
            )
        })
        .collect())
}

pub(super) fn containers() -> Result<Vec<Entry>, String> {
    let mut entries = Vec::new();
    for source in layouts()? {
        let profile = source.definition;
        let layout = Profile::for_window(source.layouts, profile.usual_window()).layout();
        entries.extend(game_probe::explain::containers(&layout).map(|info| {
            Entry::new(
                format!("{} {}", profile.name, info.name),
                crate_path(&format!("game_table/src/table/{}", info.source)),
                format!("{}  {}", info.container, info.policy),
            )
        }));
    }
    Ok(entries)
}

pub(super) fn verbs() -> Vec<Entry> {
    Verb::ALL
        .iter()
        .map(|&verb| {
            Entry::new(
                verb.name(),
                crate_path(&format!(
                    "game_play/src/verbs/{}.rs",
                    verb.name().replace('-', "_")
                )),
                if game_play::verbs::built(verb) {
                    "motion"
                } else {
                    "instant change"
                },
            )
        })
        .collect()
}

fn effect_file(name: &str) -> String {
    data_path(format!("effects/{name}.json"))
}

pub(super) fn effects() -> Result<Vec<Entry>, String> {
    Ok(tuning::checkout_ref()?
        .effects
        .iter()
        .map(|effect| {
            let names = effect
                .layers
                .iter()
                .map(|layer| layer.primitive.kind().word())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join(", ");
            Entry::new(effect.name.as_ref(), effect_file(&effect.name), names)
        })
        .collect())
}

pub(super) fn primitives() -> Vec<Entry> {
    named(
        PrimitiveKind::ALL.iter().map(|kind| kind.word()),
        "kit_base/src/data/effect.rs",
    )
}

pub(super) fn policies() -> Vec<Entry> {
    named(
        PolicyKind::ALL.iter().map(|kind| kind.word()),
        "kit_space/src/arrange.rs",
    )
}

pub(super) fn roles() -> Vec<Entry> {
    let mut entries = named(Role::ALL.map(Role::word), "kit_frame/src/frame.rs");
    entries.extend(named(
        GAME_LABELS.roles.iter().copied().map(Role::word),
        "game_hud/src/labels.rs",
    ));
    entries
}

pub(super) fn widgets() -> Vec<Entry> {
    Widget::ALL
        .iter()
        .map(|widget| {
            Entry::new(
                widget.word(),
                source_path(widget.source()),
                widget.role().word(),
            )
        })
        .collect()
}

pub(super) fn data() -> Result<Vec<Entry>, String> {
    let mut entries = TUNING_FILES
        .iter()
        .map(|file| Entry::new(file.name, data_path(file.name), file.reader))
        .collect::<Vec<_>>();
    entries.push(Entry::new(
        "ui-conventions.json",
        "data/ui-conventions.json",
        format!(
            "Shared verb and effect timing read by beryl_protocol::script::timing ({} shipped effects)",
            SHIPPED.len()
        ),
    ));
    let profile_files = profiles::FILES.into_iter().chain(
        pages::INSPECTION_PROFILES
            .iter()
            .map(pages::InspectionProfile::file),
    );
    entries.extend(profile_files.map(|file| Entry::new(file, data_path(file), PROFILE_READER)));
    entries.extend(tuning::checkout_ref()?.effects.iter().map(|effect| {
        Entry::new(
            format!("effects/{}.json", effect.name),
            effect_file(&effect.name),
            EFFECT_READER,
        )
    }));
    Ok(entries)
}

pub(super) fn checks() -> Vec<Entry> {
    named(Rule::ALL.map(Rule::word), "kit_probe/src/agent/fault.rs")
}

pub(super) fn stories() -> Result<Vec<Entry>, String> {
    Ok(catalog::entries()?
        .map(|(name, path)| Entry::new(name, relative(path), ""))
        .collect())
}

pub(super) fn commands() -> Result<Vec<Entry>, String> {
    // `just client` also has launcher and renderer commands. The launcher supplies its own entries
    // when it runs this binary; a standalone invocation still knows the text commands.
    let launcher = std::env::var("BERYL_CLIENT_LAUNCHER_COMMANDS").ok();
    let mut commands: BTreeMap<_, _> = if let Some(json) = launcher {
        let entries: Vec<Entry> = serde_json::from_str(&json)
            .map_err(|error| format!("BERYL_CLIENT_LAUNCHER_COMMANDS: {error}"))?;
        entries
            .into_iter()
            .map(|entry| (entry.name.clone(), entry))
            .collect()
    } else {
        BTreeMap::new()
    };
    for command in super::super::COMMANDS {
        commands.insert(
            command.name.to_owned(),
            Entry::new(
                command.name,
                crate_path("beryl_inspect/src/cli/mod.rs"),
                command.summary,
            ),
        );
    }
    Ok(commands.into_values().collect())
}

fn named(names: impl IntoIterator<Item = &'static str>, path: &str) -> Vec<Entry> {
    names
        .into_iter()
        .map(|name| Entry::new(name, crate_path(path), ""))
        .collect()
}
