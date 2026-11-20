//! The digest of a CLI turn: its `output.log` boiled down to the lines a person following the
//! worker wants to read. Each app prints its own events, so [`claude`] and [`codex`] each read
//! theirs, and [`of`] picks by the app's name.
//!
//! A digest line is one of:
//!
//! - what the worker said ([`TranscriptKind::Assistant`]), one line per message;
//! - a tool call ([`TranscriptKind::Tool`]), written `<name>: <first line of its input>`;
//! - how the turn ended ([`TranscriptKind::Result`]).
//!
//! Lines that are not JSON, and events of other types, are skipped. Only whole lines count, so
//! the caller passes text cut after a newline, as `read` does. A digest of a longer text begins
//! with the digest of a shorter prefix of it, which is what lets the daemon send only the lines
//! it has not sent.

use serde_json::Value;

use crate::protocol::cli::TranscriptKind;

/// One digest line before it is given its session, turn and place in the digest.
pub type Entry = (TranscriptKind, String);

/// The digest of `output` as the app named `app` prints it; empty for an app this build does not
/// know.
pub fn of(app: &str, output: &str) -> Vec<Entry> {
    match app {
        "claude" => claude(output),
        "codex" => codex(output),
        _ => Vec::new(),
    }
}

/// The digest of a `claude -p --output-format stream-json` stream.
pub fn claude(output: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    for line in output.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match event["type"].as_str() {
            // A subagent's messages carry the id of the tool call that started it.
            Some("assistant") if event["parent_tool_use_id"].is_null() => {
                let blocks = event["message"]["content"].as_array();
                for block in blocks.into_iter().flatten() {
                    match block["type"].as_str() {
                        Some("text") => {
                            let text = block["text"].as_str().unwrap_or("");
                            if !text.trim().is_empty() {
                                entries.push((TranscriptKind::Assistant, text.to_string()));
                            }
                        }
                        Some("tool_use") => {
                            let name = block["name"].as_str().unwrap_or("tool");
                            entries.push((
                                TranscriptKind::Tool,
                                tool_line(name, &input_line(&block["input"])),
                            ));
                        }
                        _ => {}
                    }
                }
            }
            Some("result") => {
                let subtype = event["subtype"].as_str().unwrap_or("result");
                let text = event["result"].as_str().unwrap_or("").trim();
                let line = if text.is_empty() {
                    subtype.to_string()
                } else {
                    format!("{subtype}: {text}")
                };
                entries.push((TranscriptKind::Result, line));
            }
            _ => {}
        }
    }
    entries
}

/// The digest of a `codex exec --json` stream.
pub fn codex(output: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    // Items whose start already produced a tool line, so that their completion adds none.
    let mut started: Vec<String> = Vec::new();
    for line in output.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let item = &event["item"];
        let kind = item["type"].as_str().unwrap_or("");
        match event["type"].as_str() {
            Some("item.started") if is_tool(kind) => {
                started.push(item["id"].as_str().unwrap_or("").to_string());
                entries.push((TranscriptKind::Tool, tool_line(kind, &codex_input(item))));
            }
            Some("item.completed") if kind == "agent_message" => {
                let text = item["text"].as_str().unwrap_or("");
                if !text.trim().is_empty() {
                    entries.push((TranscriptKind::Assistant, text.to_string()));
                }
            }
            Some("item.completed") if is_tool(kind) => {
                let id = item["id"].as_str().unwrap_or("");
                if !started.iter().any(|seen| seen == id) {
                    entries.push((TranscriptKind::Tool, tool_line(kind, &codex_input(item))));
                }
            }
            Some("turn.completed") => entries.push((TranscriptKind::Result, "completed".into())),
            Some("turn.failed") => {
                let message = event["error"]["message"].as_str().unwrap_or("");
                entries.push((TranscriptKind::Result, format!("failed: {message}")));
            }
            _ => {}
        }
    }
    entries
}

/// Whether a Codex item type is a tool call: everything but its messages, reasoning and
/// warnings.
fn is_tool(kind: &str) -> bool {
    !matches!(kind, "" | "agent_message" | "reasoning" | "error")
}

fn codex_input(item: &Value) -> String {
    for key in ["command", "query", "tool", "path"] {
        if let Some(text) = item[key].as_str() {
            return text.to_string();
        }
    }
    String::new()
}

/// `name: input`, or just `name` for a call with no input.
fn tool_line(name: &str, input: &str) -> String {
    let first = input.lines().next().unwrap_or("").trim();
    if first.is_empty() {
        name.to_string()
    } else {
        format!("{name}: {first}")
    }
}

/// The part of a Claude tool call's input a reader wants: the command, path, pattern or the
/// like, else the input as JSON.
fn input_line(input: &Value) -> String {
    for key in [
        "command",
        "file_path",
        "path",
        "pattern",
        "url",
        "query",
        "description",
        "prompt",
    ] {
        if let Some(text) = input[key].as_str() {
            return text.to_string();
        }
    }
    if input.is_null() || input.as_object().is_some_and(serde_json::Map::is_empty) {
        String::new()
    } else {
        input.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE: &str = include_str!("fixtures/first_turn.jsonl");
    const CODEX: &str = include_str!("codex_fixtures/clean_with_tool.jsonl");

    #[test]
    fn a_recorded_claude_turn_is_its_words_and_its_result() {
        assert_eq!(
            claude(CLAUDE),
            vec![
                (TranscriptKind::Assistant, "done".to_string()),
                (TranscriptKind::Result, "success: done".to_string()),
            ]
        );
    }

    #[test]
    fn a_claude_tool_call_is_its_name_and_the_first_line_of_its_input() {
        let stream = r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"text","text":"Looking."},{"type":"tool_use","name":"Bash","input":{"command":"ignite status\nls"}},{"type":"tool_use","name":"Read","input":{"file_path":"/a/b.rs"}}]}}
not json
{"type":"assistant","parent_tool_use_id":"toolu_1","message":{"content":[{"type":"text","text":"a subagent"}]}}
"#;
        let digest = claude(stream);
        let kinds: Vec<(TranscriptKind, &str)> = digest
            .iter()
            .map(|(kind, text)| (*kind, text.as_str()))
            .collect();
        assert_eq!(
            kinds,
            vec![
                (TranscriptKind::Assistant, "Looking."),
                (TranscriptKind::Tool, "Bash: ignite status"),
                (TranscriptKind::Tool, "Read: /a/b.rs"),
            ]
        );
    }

    #[test]
    fn a_recorded_codex_turn_shows_its_command_once() {
        let entries = codex(CODEX);
        assert_eq!(
            entries,
            vec![
                (TranscriptKind::Assistant, "I’ll run `echo hi`.".to_string()),
                (
                    TranscriptKind::Tool,
                    "command_execution: /bin/zsh -lc 'echo hi'".to_string()
                ),
                (TranscriptKind::Assistant, "done".to_string()),
                (TranscriptKind::Result, "completed".to_string()),
            ]
        );
    }

    #[test]
    fn a_failed_codex_turn_names_the_failure_and_skips_warnings() {
        let entries = codex(include_str!("codex_fixtures/model_refused_0_171.jsonl"));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, TranscriptKind::Result);
        assert!(entries[0].1.starts_with("failed: The 'gpt-nonexistent-9'"));
    }

    #[test]
    fn a_digest_of_a_longer_text_starts_with_the_digest_of_a_prefix() {
        let whole = claude(CLAUDE);
        let first_two_lines = CLAUDE.lines().take(2).collect::<Vec<_>>().join("\n") + "\n";
        assert_eq!(claude(&first_two_lines), whole[..1]);
    }
}
