//! `ignite pool init`: writes the folder a person opens the machine's dispatcher session in.
//!
//! The dispatcher runs outside the repository so that its context, which it carries through
//! every turn of its loop, holds none of the project's instruction files. The folder holds:
//!
//! - `AGENTS.md`, the dispatcher's instructions, with the main checkout's path filled in so the
//!   workers it starts run there and not in this folder; `CLAUDE.md` imports it.
//! - `.claude/settings.json` with `autoCompactWindow`, so a Claude dispatcher's context stays
//!   small, and the same window in the `CLAUDE_CODE_AUTO_COMPACT_WINDOW` environment variable.
//!   Claude Desktop sessions can carry a window of their own that outranks `autoCompactWindow`
//!   until someone runs `/autocompact`; the environment variable outranks both. The file also
//!   gets `permissions.deny` naming the tools the dispatcher never calls. Claude Code removes a
//!   denied tool from the model's context, not only from use. Without the list, about half of the
//!   dispatcher's 120,000 tokens were in use before its first message, most of them the
//!   definitions of the desktop app's browser, simulator, chart and editing tools. Other settings
//!   already in the file are kept.
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

/// The tools a Claude dispatcher never calls, as `permissions.deny` rules. A bare server prefix
/// (`mcp__playwright`) denies every tool of that MCP server.
///
/// The dispatcher's loop needs `Bash` (for `ignite dispatch`), `Skill` (for `/ignite pool`),
/// `ToolSearch`, `start_session` and the `ccd_session_mgmt` tools that read, clear, retitle,
/// configure and message a session. None of those are listed here.
const DENIED_TOOLS: &[&str] = &[
    // Built-in tools for editing, research, subagents and artifacts.
    "Agent",
    "Workflow",
    "Artifact",
    "ArtifactComments",
    "ArtifactData",
    "WebSearch",
    "WebFetch",
    "Edit",
    "Write",
    "NotebookEdit",
    "AskUserQuestion",
    "ScheduleWakeup",
    "CronCreate",
    "CronDelete",
    "CronList",
    "RemoteTrigger",
    "Monitor",
    "EnterPlanMode",
    "ExitPlanMode",
    "EnterWorktree",
    "ExitWorktree",
    "SendMessage",
    "ListAgents",
    "TaskStop",
    "ReportFindings",
    "SendUserFile",
    "PushNotification",
    "SearchPlugins",
    "SuggestPluginInstall",
    "SuggestSkills",
    "ListPlugins",
    "ListSkills",
    "SearchSkills",
    "DesignSync",
    "SendToProject",
    "SendToProjectContext",
    // MCP servers the desktop app or the user's configuration adds: browsers, the iOS simulator,
    // charts, terminal tabs, scheduled tasks, the connector registry and the app's panes,
    // sidebar, pull requests, settings and per-turn task list.
    "mcp__Claude_Browser",
    "mcp__claude-in-chrome",
    "mcp__playwright",
    "mcp__Claude_Code_iOS_Simulator",
    "mcp__visualize",
    "mcp__easel",
    "mcp__terminal",
    "mcp__scheduled-tasks",
    "mcp__mcp-registry",
    "mcp__ccd_view",
    "mcp__ccd_window",
    "mcp__ccd_sidebar",
    "mcp__ccd_pr",
    "mcp__ccd_host",
    "mcp__ccd_directory",
    "mcp__ccd_connectors",
    "mcp__ccd_settings",
    "mcp__ccd_turn",
    // The session tools other than `start_session`.
    "mcp__ccd_session__hand_off_to_session",
    "mcp__ccd_session__move_to_cloud",
    "mcp__ccd_session__mark_chapter",
    "mcp__ccd_session__dismiss_task",
    "mcp__ccd_session__read_widget_context",
    "mcp__ccd_session__list_start_targets",
];

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
    std::fs::write(&settings, with_dispatcher_settings(&existing))
}

/// The settings file's text with `autoCompactWindow` and the `CLAUDE_CODE_AUTO_COMPACT_WINDOW`
/// environment variable set and every rule in [`DENIED_TOOLS`] added to `permissions.deny`,
/// keeping every other setting, variable and deny rule. Text that is not a JSON object is
/// replaced.
fn with_dispatcher_settings(existing: &str) -> String {
    let mut settings = match serde_json::from_str::<serde_json::Value>(existing) {
        Ok(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    settings.insert(
        "autoCompactWindow".to_owned(),
        serde_json::Value::from(AUTO_COMPACT_WINDOW),
    );
    let env = settings
        .entry("env")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if !env.is_object() {
        *env = serde_json::Value::Object(serde_json::Map::new());
    }
    if let serde_json::Value::Object(env) = env {
        env.insert(
            "CLAUDE_CODE_AUTO_COMPACT_WINDOW".to_owned(),
            serde_json::Value::from(AUTO_COMPACT_WINDOW.to_string()),
        );
    }
    let permissions = settings
        .entry("permissions")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if !permissions.is_object() {
        *permissions = serde_json::Value::Object(serde_json::Map::new());
    }
    if let serde_json::Value::Object(permissions) = permissions {
        let deny = permissions
            .entry("deny")
            .or_insert_with(|| serde_json::Value::Array(Vec::new()));
        if !deny.is_array() {
            *deny = serde_json::Value::Array(Vec::new());
        }
        if let serde_json::Value::Array(deny) = deny {
            for rule in DENIED_TOOLS {
                if !deny.iter().any(|existing| existing.as_str() == Some(rule)) {
                    deny.push(serde_json::Value::from(*rule));
                }
            }
        }
    }
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
    fn keeps_other_settings_and_sets_the_window_and_deny_list() {
        let folder = scratch("settings");
        std::fs::create_dir_all(folder.join(".claude")).unwrap();
        std::fs::write(
            folder.join(".claude/settings.json"),
            r#"{"model": "sonnet", "autoCompactWindow": 5, "env": {"OTHER": "1"},
                "permissions": {"defaultMode": "bypassPermissions", "deny": ["Read", "Agent"]}}"#,
        )
        .unwrap();
        run(&folder, Path::new("/work/project"));
        let text = std::fs::read_to_string(folder.join(".claude/settings.json")).unwrap();
        let settings: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(settings["model"], "sonnet");
        assert_eq!(settings["autoCompactWindow"], AUTO_COMPACT_WINDOW);
        assert_eq!(settings["env"]["OTHER"], "1");
        assert_eq!(settings["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "120000");
        assert_eq!(settings["permissions"]["defaultMode"], "bypassPermissions");
        let deny: Vec<&str> = settings["permissions"]["deny"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect();
        assert_eq!(deny[..2], ["Read", "Agent"]);
        assert_eq!(deny.len(), 1 + DENIED_TOOLS.len(), "no rule is added twice");
        assert!(deny.contains(&"mcp__playwright"));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn replaces_settings_that_are_not_an_object() {
        let settings: serde_json::Value =
            serde_json::from_str(&with_dispatcher_settings("not json")).unwrap();
        assert_eq!(settings["autoCompactWindow"], AUTO_COMPACT_WINDOW);
        assert_eq!(settings["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "120000");
        assert_eq!(
            settings["permissions"]["deny"].as_array().unwrap().len(),
            DENIED_TOOLS.len()
        );
    }

    /// A rule that denied a tool the loop calls would stop every hand-off on the machine.
    #[test]
    fn never_denies_a_tool_the_loop_calls() {
        for needed in [
            "Bash",
            "Skill",
            "ToolSearch",
            "Read",
            "mcp__ccd_session__start_session",
            "mcp__ccd_session_mgmt__get_session",
            "mcp__ccd_session_mgmt__clear_session",
            "mcp__ccd_session_mgmt__set_session_model",
            "mcp__ccd_session_mgmt__set_session_effort",
            "mcp__ccd_session_mgmt__set_session_title",
            "mcp__ccd_session_mgmt__send_message",
        ] {
            for rule in DENIED_TOOLS {
                let server_prefix = rule.starts_with("mcp__")
                    && !rule["mcp__".len()..].contains("__")
                    && needed.starts_with(&format!("{rule}__"));
                assert!(*rule != needed && !server_prefix, "{rule} denies {needed}");
            }
        }
    }
}
