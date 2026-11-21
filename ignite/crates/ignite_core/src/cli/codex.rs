//! The Codex adapter: builds the command line of one `codex exec --json` turn, reads its JSON
//! events and startup diagnostics, and says how the turn ended.
//!
//! The events this reads (Codex CLI 0.154 and 0.171), one JSON object per line:
//!
//! - `thread.started` carries `thread_id`, the transcript ID to resume later.
//! - `item.completed` with an item of type `agent_message` carries the assistant's `text`. Items
//!   of type `error` are warnings (for example "Model metadata ... not found") and are skipped.
//! - `turn.completed` carries `usage`: `input_tokens` (which includes the cached ones),
//!   `cached_input_tokens`, `cache_write_input_tokens` and `output_tokens`.
//! - `turn.failed` carries `error.message`; a bare `error` event carries `message`. Both hold
//!   either plain text or a JSON string `{"status":400,"error":{"code":..,"message":..}}`.
//!
//! Codex names no model and no plan utilization in its events, so `TurnEvents::model` and
//! `TurnEvents::usage` stay empty. Recorded streams are in `codex_fixtures/`.

use serde_json::Value;

use super::{CliApp, HoldReason, TokenCounts, TurnEnd, TurnEvents, TurnSpec};

/// The `codex` program, run as `codex exec --json`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Codex;

impl CliApp for Codex {
    fn name(&self) -> &'static str {
        "codex"
    }

    /// A new transcript is `<program> exec <args> --json -m <model> -c model_reasoning_effort=<e>
    /// -- <prompt>`; a resumed one is `<program> exec resume <thread> <args> ...`. The `--` keeps a
    /// prompt that starts with a dash from being read as a flag.
    fn command(&self, spec: &TurnSpec) -> Vec<String> {
        let mut command = vec![spec.program.clone(), "exec".to_string()];
        if let Some(thread) = &spec.transcript {
            command.push("resume".to_string());
            command.push(thread.clone());
        }
        command.extend(spec.args.iter().cloned());
        command.push("--json".to_string());
        command.push("-m".to_string());
        command.push(spec.model.clone());
        if let Some(effort) = &spec.effort {
            command.push("-c".to_string());
            command.push(format!("model_reasoning_effort={effort}"));
        }
        command.push("--".to_string());
        command.push(spec.prompt.clone());
        command
    }

    fn read(&self, output: &str) -> TurnEvents {
        let mut events = TurnEvents::default();
        for line in output.lines() {
            events.lines += 1;
            let Ok(event) = serde_json::from_str::<Value>(line) else {
                if events.transcript.is_none() && events.startup_error.is_none() {
                    events.startup_error = startup_error(line);
                }
                continue;
            };
            match event["type"].as_str() {
                Some("thread.started") => {
                    // Only the first thread line names the transcript.
                    if events.transcript.is_none() {
                        events.transcript = event["thread_id"].as_str().map(str::to_string);
                    }
                }
                Some("item.completed") => {
                    if event["item"]["type"].as_str() == Some("agent_message")
                        && let Some(text) = event["item"]["text"].as_str()
                    {
                        events.last_text = Some(text.to_string());
                    }
                }
                Some("turn.completed") => {
                    events.result_seen = true;
                    events.result_error = false;
                    events.tokens = Some(tokens(&event["usage"]));
                }
                Some("turn.failed") => {
                    events.result_seen = true;
                    events.result_error = true;
                    events.error = Some(error_text(&event["error"]["message"]));
                }
                Some("error") => {
                    // A bare error is not the end of the turn: Codex also prints one while it
                    // reconnects. `turn.failed` decides.
                    events.error = Some(error_text(&event["message"]));
                }
                _ => {}
            }
        }
        events
    }

    fn classify(&self, events: &TurnEvents, exit: Option<i32>) -> TurnEnd {
        if events.result_seen {
            if !events.result_error {
                return TurnEnd::Clean;
            }
            let message = events.error.clone().unwrap_or_default();
            return classify_failure(&message);
        }
        match exit {
            // The shell's "command not found" and "not executable" codes.
            Some(126 | 127) if events.lines == 0 => TurnEnd::Hold {
                reason: HoldReason::ProgramMissing,
                message: "the codex program could not be started".to_string(),
            },
            // The program ended by itself before printing its first event.
            Some(code) if code != 0 && events.transcript.is_none() => TurnEnd::Hold {
                reason: HoldReason::NoFirstLine,
                message: events
                    .error
                    .clone()
                    .or_else(|| events.startup_error.clone())
                    .unwrap_or_else(|| format!("codex exited with {code} before its first event")),
            },
            // No `turn.completed` or `turn.failed`: a crash, a kill, a reboot or a hung turn.
            _ => TurnEnd::Interrupted,
        }
    }
}

/// Codex prints a fatal `thread/start` error before emitting events when a required MCP
/// handshake fails. Only the server identifier belongs in a hold report, since the transport
/// error can contain endpoint URLs and local paths.
fn startup_error(line: &str) -> Option<String> {
    let failure = line.strip_prefix(concat!(
        "Error: thread/start: thread/start failed: error creating thread: Fatal error: ",
        "Failed to initialize session: required MCP servers failed to initialize: "
    ))?;
    let (server, detail) = failure.split_once(": ")?;
    if !(1..=64).contains(&server.len())
        || !server
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        || !detail.starts_with("handshaking with MCP server failed:")
    {
        return None;
    }
    Some(format!(
        "required MCP server '{server}' failed its startup handshake"
    ))
}

/// Maps the message of a `turn.failed` to the way the turn ended.
fn classify_failure(message: &str) -> TurnEnd {
    let lower = message.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|needle| lower.contains(needle));
    let hold = |reason| TurnEnd::Hold {
        reason,
        message: message.to_string(),
    };
    if has(&[
        "model_not_found",
        "model is not supported",
        "does not exist",
    ]) {
        hold(HoldReason::ModelRefused)
    } else if has(&[
        "usage limit",
        "usage_limit_reached",
        "hit your limit",
        "rate_limit_exceeded",
    ]) {
        TurnEnd::PlanLimit { window: None }
    } else if has(&[
        "status 401",
        "unauthorized",
        "not logged in",
        "log in again",
        "sign in again",
        "token_expired",
        "refresh token",
        "authentication",
    ]) {
        hold(HoldReason::NotLoggedIn)
    } else if has(&[
        "status 500",
        "status 502",
        "status 503",
        "status 504",
        "status 529",
        "overloaded",
        "service unavailable",
        "stream disconnected",
        "connection",
        "timed out",
        "network",
    ]) {
        hold(HoldReason::ApiError)
    } else {
        TurnEnd::Unknown {
            why: message.to_string(),
        }
    }
}

/// The counts of a `turn.completed` usage object. Codex's `input_tokens` includes the cached
/// ones, so the fresh input is the difference.
fn tokens(usage: &Value) -> TokenCounts {
    let count = |key: &str| usage[key].as_u64().unwrap_or(0);
    let cached = count("cached_input_tokens");
    TokenCounts {
        input: count("input_tokens").saturating_sub(cached),
        output: count("output_tokens"),
        cache_read: cached,
        cache_write: count("cache_write_input_tokens"),
    }
}

/// Plain text for an error message. Codex often prints the API's answer as a JSON string; that
/// becomes `status <n> <code>: <message>`, so that the status and code stay visible to
/// `classify` and to reports.
fn error_text(message: &Value) -> String {
    let Some(text) = message.as_str() else {
        return message.to_string();
    };
    let Ok(Value::Object(body)) = serde_json::from_str::<Value>(text) else {
        return text.to_string();
    };
    let inner = body.get("error").unwrap_or(&Value::Null);
    let Some(detail) = inner["message"].as_str() else {
        return text.to_string();
    };
    let mut parts = Vec::new();
    if let Some(status) = body.get("status").and_then(Value::as_u64) {
        parts.push(format!("status {status}"));
    }
    match inner["code"].as_str() {
        Some(code) => parts.push(format!("{code}: {detail}")),
        None => parts.push(detail.to_string()),
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLEAN: &str = include_str!("codex_fixtures/clean.jsonl");
    const CLEAN_WITH_TOOL: &str = include_str!("codex_fixtures/clean_with_tool.jsonl");
    const RESUMED: &str = include_str!("codex_fixtures/resumed.jsonl");
    const REFUSED_OLD: &str = include_str!("codex_fixtures/model_refused_0_154.jsonl");
    const REFUSED_NEW: &str = include_str!("codex_fixtures/model_refused_0_171.jsonl");
    const MCP_HANDSHAKE: &str = include_str!("codex_fixtures/required_mcp_handshake.txt");

    fn spec(transcript: Option<&str>) -> TurnSpec {
        TurnSpec {
            app: "codex".to_string(),
            program: "codex".to_string(),
            args: vec!["--dangerously-bypass-approvals-and-sandbox".to_string()],
            model: "gpt-6.2-sol".to_string(),
            effort: Some("high".to_string()),
            transcript: transcript.map(str::to_string),
            new_transcript: Some("ignored".to_string()),
            prompt: "- do the work".to_string(),
        }
    }

    fn end(output: &str, exit: Option<i32>) -> TurnEnd {
        let events = Codex.read(output);
        Codex.classify(&events, exit)
    }

    #[test]
    fn first_turn_command_starts_a_new_thread() {
        assert_eq!(
            Codex.command(&spec(None)),
            [
                "codex",
                "exec",
                "--dangerously-bypass-approvals-and-sandbox",
                "--json",
                "-m",
                "gpt-6.2-sol",
                "-c",
                "model_reasoning_effort=high",
                "--",
                "- do the work",
            ]
        );
    }

    #[test]
    fn later_turn_command_resumes_the_thread_and_omits_missing_effort() {
        let mut later = spec(Some("01a1fa13-784d-7cf2-86f0-60c51eda12b8"));
        later.effort = None;
        assert_eq!(
            Codex.command(&later),
            [
                "codex",
                "exec",
                "resume",
                "01a1fa13-784d-7cf2-86f0-60c51eda12b8",
                "--dangerously-bypass-approvals-and-sandbox",
                "--json",
                "-m",
                "gpt-6.2-sol",
                "--",
                "- do the work",
            ]
        );
    }

    #[test]
    fn clean_turn_reads_thread_text_and_tokens() {
        let events = Codex.read(CLEAN);
        assert_eq!(
            events.transcript.as_deref(),
            Some("01a1fa13-784d-7cf2-86f0-60c51eda12b8")
        );
        assert_eq!(events.last_text.as_deref(), Some("ok"));
        assert_eq!(events.model, None);
        assert!(events.usage.is_empty());
        assert_eq!(
            events.tokens,
            Some(TokenCounts {
                input: 24249 - 14976,
                output: 5,
                cache_read: 14976,
                cache_write: 0,
            })
        );
        assert!(events.result_seen && !events.result_error);
        assert_eq!(events.lines, 4);
        assert_eq!(Codex.classify(&events, Some(0)), TurnEnd::Clean);
    }

    #[test]
    fn turn_with_a_tool_call_keeps_the_last_message() {
        let events = Codex.read(CLEAN_WITH_TOOL);
        assert_eq!(events.last_text.as_deref(), Some("done"));
        assert_eq!(Codex.classify(&events, Some(0)), TurnEnd::Clean);
    }

    #[test]
    fn resumed_turn_names_the_same_thread() {
        let events = Codex.read(RESUMED);
        assert_eq!(
            events.transcript.as_deref(),
            Some("01a1fa13-784d-7cf2-86f0-60c51eda12b8")
        );
        assert_eq!(events.last_text.as_deref(), Some("again"));
    }

    #[test]
    fn recorded_model_refusal_holds_the_app() {
        for output in [REFUSED_OLD, REFUSED_NEW] {
            let events = Codex.read(output);
            assert!(events.result_seen && events.result_error);
            assert_eq!(events.last_text, None);
            assert_eq!(events.tokens, None);
            match Codex.classify(&events, Some(1)) {
                TurnEnd::Hold { reason, message } => {
                    assert_eq!(reason, HoldReason::ModelRefused);
                    assert!(
                        message.contains("not supported when using Codex"),
                        "{message}"
                    );
                }
                other => panic!("expected a hold, got {other:?}"),
            }
        }
        // The old stream wraps the API answer in JSON; the reader unwraps it.
        let error = Codex.read(REFUSED_OLD).error.unwrap();
        assert!(error.starts_with("status 400 model_not_found: The 'gpt-6.2-sol' model"));
    }

    #[test]
    fn authentication_failure_holds_the_app() {
        let output = concat!(
            r#"{"type":"thread.started","thread_id":"t"}"#,
            "\n",
            r#"{"type":"turn.failed","error":{"message":"{\"status\":401,\"error\":{\"message\":\"Your access token could not be refreshed\"}}"}}"#,
            "\n",
        );
        assert!(matches!(
            end(output, Some(1)),
            TurnEnd::Hold {
                reason: HoldReason::NotLoggedIn,
                ..
            }
        ));
    }

    #[test]
    fn usage_limit_is_the_plan_limit() {
        let output = concat!(
            r#"{"type":"thread.started","thread_id":"t"}"#,
            "\n",
            r#"{"type":"turn.failed","error":{"message":"You've hit your usage limit. Try again later."}}"#,
            "\n",
        );
        assert_eq!(end(output, Some(1)), TurnEnd::PlanLimit { window: None });
    }

    #[test]
    fn server_error_holds_the_app_as_an_api_error() {
        let output = concat!(
            r#"{"type":"thread.started","thread_id":"t"}"#,
            "\n",
            r#"{"type":"turn.failed","error":{"message":"stream disconnected before completion"}}"#,
            "\n",
        );
        assert!(matches!(
            end(output, Some(1)),
            TurnEnd::Hold {
                reason: HoldReason::ApiError,
                ..
            }
        ));
    }

    #[test]
    fn unrecognised_failure_is_unknown() {
        let output = r#"{"type":"turn.failed","error":{"message":"something new"}}"#;
        assert_eq!(
            end(output, Some(1)),
            TurnEnd::Unknown {
                why: "something new".to_string()
            }
        );
    }

    #[test]
    fn stream_without_a_result_is_interrupted() {
        // Cut off after the tool call started, and killed by a signal.
        let cut: String = CLEAN_WITH_TOOL
            .lines()
            .take(4)
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(end(&cut, None), TurnEnd::Interrupted);
        assert_eq!(end(&cut, Some(1)), TurnEnd::Interrupted);
        // A reconnect error that never turned into a failure is still an interruption.
        let reconnecting = concat!(
            r#"{"type":"thread.started","thread_id":"t"}"#,
            "\n",
            r#"{"type":"error","message":"Reconnecting... 1/5"}"#,
            "\n",
        );
        assert_eq!(end(reconnecting, None), TurnEnd::Interrupted);
        assert_eq!(end("", None), TurnEnd::Interrupted);
    }

    #[test]
    fn a_transient_error_before_a_completed_turn_is_clean() {
        let output = concat!(
            r#"{"type":"thread.started","thread_id":"t"}"#,
            "\n",
            r#"{"type":"error","message":"Reconnecting... 1/5"}"#,
            "\n",
            r#"{"type":"turn.completed","usage":{"input_tokens":3,"output_tokens":1}}"#,
            "\n",
        );
        assert_eq!(end(output, Some(0)), TurnEnd::Clean);
    }

    #[test]
    fn missing_program_and_silent_exit_hold_the_app() {
        assert!(matches!(
            end("", Some(127)),
            TurnEnd::Hold {
                reason: HoldReason::ProgramMissing,
                ..
            }
        ));
        let generic = TurnEnd::Hold {
            reason: HoldReason::NoFirstLine,
            message: "codex exited with 1 before its first event".to_string(),
        };
        assert_eq!(end("", Some(1)), generic);
        assert_eq!(end("Error: stdin is not a terminal\n", Some(1)), generic);
    }

    #[test]
    fn required_mcp_handshake_failure_names_only_the_server_in_the_hold() {
        let events = Codex.read(MCP_HANDSHAKE);
        assert_eq!(events.transcript, None);
        assert_eq!(events.error, None);
        assert!(!events.result_seen);
        assert_eq!(
            Codex.classify(&events, Some(1)),
            TurnEnd::Hold {
                reason: HoldReason::NoFirstLine,
                message: "required MCP server 'playwright' failed its startup handshake"
                    .to_string(),
            }
        );
    }

    #[test]
    fn startup_diagnostic_ignores_unrecognized_output_and_unsafe_names() {
        let warning = MCP_HANDSHAKE.lines().nth(1).unwrap();
        let unsafe_name = MCP_HANDSHAKE.replace("playwright:", "https://example.test:");
        let generic = TurnEnd::Hold {
            reason: HoldReason::NoFirstLine,
            message: "codex exited with 1 before its first event".to_string(),
        };
        assert_eq!(end(warning, Some(1)), generic);
        assert_eq!(end(&unsafe_name, Some(1)), generic);
    }

    #[test]
    fn startup_diagnostic_does_not_replace_json_result() {
        let output = format!("{MCP_HANDSHAKE}{REFUSED_NEW}");
        assert_eq!(end(&output, Some(1)), end(REFUSED_NEW, Some(1)));
    }

    #[test]
    fn lines_that_are_not_json_are_skipped_but_counted() {
        let output = format!("warning: something\n{CLEAN}");
        let events = Codex.read(&output);
        assert_eq!(events.lines, 5);
        assert_eq!(events.last_text.as_deref(), Some("ok"));
    }
}
