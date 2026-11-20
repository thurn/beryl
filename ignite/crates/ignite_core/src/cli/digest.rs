//! The digest of a CLI turn: its `output.log` boiled down to the lines a person following the
//! worker wants to read. Each app prints its own events, so [`claude`] and [`codex`] each read
//! theirs, and [`of`] picks by the app's name.
//!
//! A digest line is one of:
//!
//! - what the worker said ([`TranscriptKind::Assistant`]), one line per message;
//! - a tool call ([`TranscriptKind::Tool`]), written `<name>: <first line of its input>`;
//! - the start of what a tool call returned ([`TranscriptKind::ToolResult`]), cut to
//!   [`TOOL_RESULT_CHARS`] characters, with [`Entry::call`] naming the call's line;
//! - how the turn ended ([`TranscriptKind::Result`]).
//!
//! Lines that are not JSON, and events of other types, are skipped. Only whole lines count, so
//! the caller passes text cut after a newline, as `read` does. A digest of a longer text begins
//! with the digest of a shorter prefix of it, which is what lets the daemon send only the lines
//! it has not sent: each event adds lines at the end and never changes one already made, and a
//! result names its call by the call's place, which an earlier event fixed.
//!
//! Text the model streams before its message is whole (`--include-partial-messages`) is not
//! read: a line made from part of a message would have to change once the rest arrived, and
//! lines never change once made.

use serde_json::Value;

use crate::protocol::cli::{TOOL_RESULT_CHARS, TranscriptKind};

/// One digest line before it is given its session, turn and place in the digest. Its place is
/// its position in the list [`of`] returns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub kind: TranscriptKind,
    pub text: String,
    /// For a tool result, the position of its tool call's entry.
    pub call: Option<u32>,
}

impl Entry {
    fn new(kind: TranscriptKind, text: String) -> Entry {
        Entry {
            kind,
            text,
            call: None,
        }
    }
}

/// The digest of `output` as the app named `app` prints it; empty for an app this build does not
/// know.
pub fn of(app: &str, output: &str) -> Vec<Entry> {
    match app {
        "claude" => claude(output),
        "codex" => codex(output),
        _ => Vec::new(),
    }
}

/// The position the next entry pushed onto `entries` will have.
fn next_place(entries: &[Entry]) -> u32 {
    u32::try_from(entries.len()).unwrap_or(u32::MAX)
}

/// Pushes the result of the call at `call` when it has any text.
fn push_result(entries: &mut Vec<Entry>, call: u32, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    entries.push(Entry {
        kind: TranscriptKind::ToolResult,
        text: cut_result(text),
        call: Some(call),
    });
}

/// `text` cut to [`TOOL_RESULT_CHARS`] characters, with an ellipsis where it was cut.
fn cut_result(text: &str) -> String {
    if text.chars().count() <= TOOL_RESULT_CHARS {
        return text.to_string();
    }
    let mut kept: String = text.chars().take(TOOL_RESULT_CHARS - 1).collect();
    kept.push('…');
    kept
}

/// The digest of a `claude -p --output-format stream-json` stream.
pub fn claude(output: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    // The ID of each tool call the digest shows, with its entry's position.
    let mut calls: Vec<(String, u32)> = Vec::new();
    for line in output.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        // A subagent's messages, and the results of its tools, carry the id of the tool call
        // that started it.
        if !event["parent_tool_use_id"].is_null() {
            continue;
        }
        match event["type"].as_str() {
            Some("assistant") => {
                let blocks = event["message"]["content"].as_array();
                for block in blocks.into_iter().flatten() {
                    match block["type"].as_str() {
                        Some("text") => {
                            let text = block["text"].as_str().unwrap_or("");
                            if !text.trim().is_empty() {
                                entries.push(Entry::new(TranscriptKind::Assistant, text.into()));
                            }
                        }
                        Some("tool_use") => {
                            let name = block["name"].as_str().unwrap_or("tool");
                            if let Some(id) = block["id"].as_str() {
                                calls.push((id.to_string(), next_place(&entries)));
                            }
                            entries.push(Entry::new(
                                TranscriptKind::Tool,
                                tool_line(name, &input_line(&block["input"])),
                            ));
                        }
                        _ => {}
                    }
                }
            }
            Some("user") => {
                let blocks = event["message"]["content"].as_array();
                for block in blocks.into_iter().flatten() {
                    if block["type"].as_str() != Some("tool_result") {
                        continue;
                    }
                    let id = block["tool_use_id"].as_str().unwrap_or("");
                    if let Some((_, call)) = calls.iter().find(|(seen, _)| seen == id) {
                        push_result(&mut entries, *call, &claude_result(&block["content"]));
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
                entries.push(Entry::new(TranscriptKind::Result, line));
            }
            _ => {}
        }
    }
    entries
}

/// The text of a Claude `tool_result` block's content: a string, or a list of blocks of which
/// the text ones count.
fn claude_result(content: &Value) -> String {
    if let Some(text) = content.as_str() {
        return text.to_string();
    }
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|block| block["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The digest of a `codex exec --json` stream.
pub fn codex(output: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    // Items whose start already produced a tool line, with that line's position, so that their
    // completion adds only the result.
    let mut started: Vec<(String, u32)> = Vec::new();
    for line in output.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let item = &event["item"];
        let kind = item["type"].as_str().unwrap_or("");
        let id = item["id"].as_str().unwrap_or("");
        match event["type"].as_str() {
            Some("item.started") if is_tool(kind) => {
                started.push((id.to_string(), next_place(&entries)));
                let line = tool_line(kind, &codex_input(item));
                entries.push(Entry::new(TranscriptKind::Tool, line));
            }
            Some("item.completed") if kind == "agent_message" => {
                let text = item["text"].as_str().unwrap_or("");
                if !text.trim().is_empty() {
                    entries.push(Entry::new(TranscriptKind::Assistant, text.into()));
                }
            }
            Some("item.completed") if is_tool(kind) => {
                let call = if let Some((_, call)) = started.iter().find(|(seen, _)| seen == id) {
                    *call
                } else {
                    let call = next_place(&entries);
                    let line = tool_line(kind, &codex_input(item));
                    entries.push(Entry::new(TranscriptKind::Tool, line));
                    call
                };
                push_result(&mut entries, call, &codex_result(item));
            }
            Some("turn.completed") => {
                entries.push(Entry::new(TranscriptKind::Result, "completed".into()));
            }
            Some("turn.failed") => {
                let message = event["error"]["message"].as_str().unwrap_or("");
                entries.push(Entry::new(
                    TranscriptKind::Result,
                    format!("failed: {message}"),
                ));
            }
            _ => {}
        }
    }
    entries
}

/// What a finished Codex tool item returned: a command's output, an MCP call's result or error,
/// or the files a change touched.
fn codex_result(item: &Value) -> String {
    if let Some(text) = item["aggregated_output"].as_str() {
        return text.to_string();
    }
    if let Some(message) = item["error"]["message"].as_str() {
        return message.to_string();
    }
    let result = &item["result"];
    if let Some(text) = result.as_str() {
        return text.to_string();
    }
    let texts: Vec<&str> = result["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|block| block["text"].as_str())
        .collect();
    if !texts.is_empty() {
        return texts.join("\n");
    }
    item["changes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|change| {
            let path = change["path"].as_str()?;
            Some(match change["kind"].as_str() {
                Some(kind) => format!("{kind} {path}"),
                None => path.to_string(),
            })
        })
        .collect::<Vec<_>>()
        .join("\n")
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

    /// Each entry as its kind, text and call, for comparing.
    fn rows(entries: &[Entry]) -> Vec<(TranscriptKind, &str, Option<u32>)> {
        entries
            .iter()
            .map(|entry| (entry.kind, entry.text.as_str(), entry.call))
            .collect()
    }

    #[test]
    fn a_recorded_claude_turn_is_its_words_and_its_result() {
        assert_eq!(
            rows(&claude(CLAUDE)),
            vec![
                (TranscriptKind::Assistant, "done", None),
                (TranscriptKind::Result, "success: done", None),
            ]
        );
    }

    #[test]
    fn a_claude_tool_call_is_its_name_and_the_first_line_of_its_input() {
        let stream = r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"text","text":"Looking."},{"type":"tool_use","id":"toolu_a","name":"Bash","input":{"command":"ignite status\nls"}},{"type":"tool_use","id":"toolu_b","name":"Read","input":{"file_path":"/a/b.rs"}}]}}
not json
{"type":"assistant","parent_tool_use_id":"toolu_1","message":{"content":[{"type":"text","text":"a subagent"}]}}
"#;
        assert_eq!(
            rows(&claude(stream)),
            vec![
                (TranscriptKind::Assistant, "Looking.", None),
                (TranscriptKind::Tool, "Bash: ignite status", None),
                (TranscriptKind::Tool, "Read: /a/b.rs", None),
            ]
        );
    }

    #[test]
    fn a_claude_tool_result_names_its_call_and_is_cut_short() {
        let long = "y".repeat(TOOL_RESULT_CHARS + 50);
        let stream = format!(
            r#"{{"type":"assistant","parent_tool_use_id":null,"message":{{"content":[{{"type":"tool_use","id":"toolu_a","name":"Bash","input":{{"command":"ls"}}}},{{"type":"tool_use","id":"toolu_b","name":"Read","input":{{"file_path":"/a.rs"}}}}]}}}}
{{"type":"user","parent_tool_use_id":null,"message":{{"content":[{{"type":"tool_result","tool_use_id":"toolu_b","content":[{{"type":"text","text":"{long}"}}]}},{{"type":"tool_result","tool_use_id":"toolu_a","content":"a.rs\nb.rs"}}]}}}}
{{"type":"user","parent_tool_use_id":"toolu_a","message":{{"content":[{{"type":"tool_result","tool_use_id":"toolu_z","content":"a subagent's"}}]}}}}
{{"type":"user","parent_tool_use_id":null,"message":{{"content":[{{"type":"tool_result","tool_use_id":"toolu_unknown","content":"no call"}},{{"type":"tool_result","tool_use_id":"toolu_a","content":"  "}}]}}}}
"#
        );
        let entries = claude(&stream);
        assert_eq!(entries.len(), 4, "{entries:?}");
        assert_eq!(
            (entries[2].kind, entries[2].call),
            (TranscriptKind::ToolResult, Some(1))
        );
        assert_eq!(entries[2].text.chars().count(), TOOL_RESULT_CHARS);
        assert!(entries[2].text.ends_with('…'));
        assert_eq!(
            rows(&entries[3..]),
            vec![(TranscriptKind::ToolResult, "a.rs\nb.rs", Some(0))]
        );
    }

    #[test]
    fn a_recorded_codex_turn_shows_its_command_once_with_its_output() {
        assert_eq!(
            rows(&codex(CODEX)),
            vec![
                (TranscriptKind::Assistant, "I’ll run `echo hi`.", None),
                (
                    TranscriptKind::Tool,
                    "command_execution: /bin/zsh -lc 'echo hi'",
                    None
                ),
                (TranscriptKind::ToolResult, "hi", Some(1)),
                (TranscriptKind::Assistant, "done", None),
                (TranscriptKind::Result, "completed", None),
            ]
        );
    }

    #[test]
    fn a_codex_tool_seen_only_when_it_completed_gets_its_line_and_result() {
        let stream = r#"{"type":"item.completed","item":{"id":"item_3","type":"mcp_tool_call","tool":"search","result":{"content":[{"type":"text","text":"found it"}]}}}
{"type":"item.completed","item":{"id":"item_4","type":"file_change","changes":[{"path":"/a.rs","kind":"update"}]}}
"#;
        assert_eq!(
            rows(&codex(stream)),
            vec![
                (TranscriptKind::Tool, "mcp_tool_call: search", None),
                (TranscriptKind::ToolResult, "found it", Some(0)),
                (TranscriptKind::Tool, "file_change", None),
                (TranscriptKind::ToolResult, "update /a.rs", Some(2)),
            ]
        );
    }

    #[test]
    fn a_failed_codex_turn_names_the_failure_and_skips_warnings() {
        let entries = codex(include_str!("codex_fixtures/model_refused_0_171.jsonl"));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, TranscriptKind::Result);
        assert!(
            entries[0]
                .text
                .starts_with("failed: The 'gpt-nonexistent-9'")
        );
    }

    #[test]
    fn a_digest_of_a_longer_text_starts_with_the_digest_of_a_prefix() {
        for (app, text) in [("claude", CLAUDE), ("codex", CODEX)] {
            let whole = of(app, text);
            let lines: Vec<&str> = text.lines().collect();
            for count in 0..=lines.len() {
                let prefix = lines[..count].join("\n") + "\n";
                let part = of(app, &prefix);
                assert_eq!(part, whole[..part.len()], "{app}, {count} lines");
            }
        }
    }
}
