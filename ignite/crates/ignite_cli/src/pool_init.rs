//! `ignite pool init`: writes the folder a person opens the machine's dispatcher session in.
//!
//! The dispatcher runs outside the repository so that its context, which it carries through
//! every turn of its loop, holds none of the project's instruction files. The folder holds:
//!
//! - `AGENTS.md`, the dispatcher's instructions, with the main checkout's path filled in so the
//!   workers it starts run there and not in this folder; `CLAUDE.md` imports it.
//! - `.claude/settings.json` with `autoCompactWindow`, so a Claude dispatcher's context stays
//!   small. Other settings already in the file are kept.
//! - A skill named `ignite` for Claude and Codex, so that `/ignite pool <n>` works there.
//!
//! Running it again rewrites the instructions, which is how a dispatcher folder picks up a newer
//! version of them.

use std::path::{Path, PathBuf};

use ignite_core::protocol::client::{Exit, Reply};

/// The dispatcher's instructions; `{checkout}` is replaced by the main checkout's path.
const INSTRUCTIONS: &str = include_str!("pool_init/dispatcher.md");

/// The context size, in tokens, at which a Claude dispatcher compacts.
const AUTO_COMPACT_WINDOW: u64 = 120_000;

const SKILL: &str = "---
name: ignite
description: Start or add to this machine's Ignite pool with `/ignite pool <n>`, then run the dispatcher loop. Use when the person sends /ignite pool.
---

Follow `AGENTS.md` in this folder: the section on `/ignite pool <n>`, with `n` the number given,
then the loop.
";

/// `~/ignite`: the folder `ignite pool init` writes when it is given none.
pub fn default_folder() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .unwrap_or_default();
    PathBuf::from(home).join("ignite")
}

/// Writes the dispatcher folder at `folder` for the main checkout at `checkout`.
pub fn run(folder: &Path, checkout: &Path) -> Reply {
    match write(folder, checkout) {
        Ok(()) => Reply::text(
            Exit::Success,
            format!(
                "wrote    {}\nnext     open a session in that folder on Sonnet, in a permission \
                 mode that never prompts, and send `/ignite pool <n>`",
                folder.display()
            ),
        ),
        Err(error) => Reply::text(
            Exit::Usage,
            format!("could not write {}: {error}", folder.display()),
        ),
    }
}

fn write(folder: &Path, checkout: &Path) -> std::io::Result<()> {
    let instructions = INSTRUCTIONS.replace("{checkout}", &checkout.to_string_lossy());
    std::fs::create_dir_all(folder)?;
    std::fs::write(folder.join("AGENTS.md"), instructions)?;
    std::fs::write(folder.join("CLAUDE.md"), "@AGENTS.md\n")?;
    for skills in [".claude", ".agents"] {
        let skill = folder.join(skills).join("skills").join("ignite");
        std::fs::create_dir_all(&skill)?;
        std::fs::write(skill.join("SKILL.md"), SKILL)?;
    }
    let settings = folder.join(".claude").join("settings.json");
    let existing = std::fs::read_to_string(&settings).unwrap_or_default();
    std::fs::write(&settings, with_compact_window(&existing))
}

/// The settings file's text with `autoCompactWindow` set, keeping every other setting. Text that
/// is not a JSON object is replaced.
fn with_compact_window(existing: &str) -> String {
    let mut settings = match serde_json::from_str::<serde_json::Value>(existing) {
        Ok(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    settings.insert(
        "autoCompactWindow".to_owned(),
        serde_json::Value::from(AUTO_COMPACT_WINDOW),
    );
    let mut text = serde_json::to_string_pretty(&serde_json::Value::Object(settings))
        .unwrap_or_else(|_| "{}".to_owned());
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ignite-pool-init-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn writes_instructions_with_the_checkout_and_both_skills() {
        let folder = scratch("fresh");
        let reply = run(&folder, Path::new("/work/project"));
        assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
        let agents = std::fs::read_to_string(folder.join("AGENTS.md")).unwrap();
        assert!(agents.contains("`cwd: \"/work/project\"`"), "{agents}");
        assert!(!agents.contains("{checkout}"), "{agents}");
        assert_eq!(
            std::fs::read_to_string(folder.join("CLAUDE.md")).unwrap(),
            "@AGENTS.md\n"
        );
        for skills in [".claude", ".agents"] {
            let skill = folder.join(skills).join("skills/ignite/SKILL.md");
            assert!(
                std::fs::read_to_string(skill)
                    .unwrap()
                    .contains("name: ignite")
            );
        }
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn keeps_other_settings_and_sets_the_window() {
        let folder = scratch("settings");
        std::fs::create_dir_all(folder.join(".claude")).unwrap();
        std::fs::write(
            folder.join(".claude/settings.json"),
            r#"{"model": "sonnet", "autoCompactWindow": 5}"#,
        )
        .unwrap();
        run(&folder, Path::new("/work/project"));
        let text = std::fs::read_to_string(folder.join(".claude/settings.json")).unwrap();
        let settings: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(settings["model"], "sonnet");
        assert_eq!(settings["autoCompactWindow"], AUTO_COMPACT_WINDOW);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn replaces_settings_that_are_not_an_object() {
        assert_eq!(
            with_compact_window("not json"),
            "{\n  \"autoCompactWindow\": 120000\n}\n"
        );
    }
}
