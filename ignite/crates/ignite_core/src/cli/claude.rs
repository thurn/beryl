//! The Claude adapter of CLI mode: builds the command line of one `claude -p` turn, reads the
//! JSON events the turn printed (`--output-format stream-json --verbose`, one object per
//! line) and says how the turn ended.
//!
//! The events this adapter uses, from claude 2.1.x:
//!
//! - `{"type":"system","subtype":"init"}` is the first line. It names `session_id` (the
//!   transcript ID), `model` and `claude_code_version` before any request is made.
//! - `{"type":"rate_limit_event"}` carries `rate_limit_info.unifiedWindows.<window>` with
//!   `utilization` (0 to 1), `resetsAt` and `status`.
//! - `{"type":"assistant"}` carries the model's message; its text blocks are the worker's
//!   words.
//! - `{"type":"result"}` is the last line of a turn that ended: `is_error`, `subtype`,
//!   `api_error_status`, `result`, `total_cost_usd` and `usage`.
//!
//! A turn killed with `SIGKILL` writes no `result` line. A turn resumed with `--resume` may
//! report a different `session_id` than the one it resumed, so the transcript ID is read
//! from each turn's `init` line and never assumed.

use serde_json::Value;

use super::{CliApp, HoldReason, TokenCounts, TurnEnd, TurnEvents, TurnSpec, UsageSample};

/// The `claude` program, run as `claude -p` with `--output-format stream-json --verbose`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Claude;

/// The `status` of a usage window that has reached its limit.
const REJECTED: &str = "rejected";

impl CliApp for Claude {
    fn name(&self) -> &'static str {
        "claude"
    }

    /// `<program> -p <args...> --model <id> [--effort <level>] --output-format stream-json
    /// --verbose`, then `--resume <transcript>` for a later turn or `--session-id <uuid>` for
    /// a first one, and the prompt last. `--fallback-model` is never passed: a turn must run
    /// on the model it asked for.
    fn command(&self, spec: &TurnSpec) -> Vec<String> {
        let mut command = vec![spec.program.clone(), "-p".to_string()];
        command.extend(spec.args.iter().cloned());
        command.push("--model".to_string());
        command.push(spec.model.clone());
        if let Some(effort) = &spec.effort {
            command.push("--effort".to_string());
            command.push(effort.clone());
        }
        command.extend(["--output-format", "stream-json", "--verbose"].map(str::to_string));
        if let Some(transcript) = &spec.transcript {
            command.push("--resume".to_string());
            command.push(transcript.clone());
        } else if let Some(new_transcript) = &spec.new_transcript {
            command.push("--session-id".to_string());
            command.push(new_transcript.clone());
        }
        command.push(spec.prompt.clone());
        command
    }

    fn read(&self, output: &str) -> TurnEvents {
        let mut events = TurnEvents::default();
        for line in output.lines() {
            events.lines += 1;
            let Ok(Value::Object(event)) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let event = Value::Object(event);
            match event["type"].as_str() {
                Some("system") if event["subtype"].as_str() == Some("init") => {
                    read_init(&event, &mut events);
                }
                Some("rate_limit_event") => read_rate_limit(&event, &mut events),
                Some("assistant") => read_assistant(&event, &mut events),
                Some("result") => read_result(&event, &mut events),
                _ => {}
            }
        }
        events
    }

    fn classify(&self, events: &TurnEvents, exit: Option<i32>) -> TurnEnd {
        let clean_result = events.result_seen && !events.result_error;
        let error = events.error.as_deref().unwrap_or("");

        // A program that is missing or fails before printing anything. `init` is printed
        // before any request, so a turn that was merely killed has one.
        let printed_nothing_useful = !events.result_seen && events.transcript.is_none();
        if printed_nothing_useful && events.lines == 0 {
            return match exit {
                Some(code @ (126 | 127)) => hold(
                    HoldReason::ProgramMissing,
                    format!("the program could not be run (exit {code})"),
                ),
                Some(code) => hold(
                    HoldReason::NoFirstLine,
                    format!("the program exited with {code} before printing its first line"),
                ),
                None => TurnEnd::Interrupted,
            };
        }
        if printed_nothing_useful && matches!(exit, Some(code) if code != 0) {
            return hold(
                HoldReason::NoFirstLine,
                format!(
                    "the program exited with {} before printing its first line",
                    exit.unwrap_or(0)
                ),
            );
        }

        if !clean_result {
            if let Some(rejected) = events
                .usage
                .iter()
                .rev()
                .find(|s| s.status.as_deref() == Some(REJECTED))
            {
                return TurnEnd::PlanLimit {
                    window: Some(rejected.window.clone()),
                };
            }
            if is_plan_limit_text(error) {
                return TurnEnd::PlanLimit { window: None };
            }
        }

        if !events.result_seen {
            return TurnEnd::Interrupted;
        }
        if clean_result {
            return TurnEnd::Clean;
        }

        let status = error_status(error);
        let message = error.lines().next().unwrap_or("").to_string();
        if status == Some(401) || mentions_any(error, AUTH_MARKERS) {
            hold(HoldReason::NotLoggedIn, message)
        } else if status == Some(404) || mentions_any(error, MODEL_MARKERS) {
            hold(HoldReason::ModelRefused, message)
        } else if matches!(status, Some(429 | 500..=599)) || mentions_any(error, API_MARKERS) {
            hold(HoldReason::ApiError, message)
        } else {
            TurnEnd::Unknown { why: message }
        }
    }
}

fn hold(reason: HoldReason, message: String) -> TurnEnd {
    TurnEnd::Hold { reason, message }
}

/// Phrases of an authentication failure, lower case.
const AUTH_MARKERS: &[&str] = &[
    "authentication_error",
    "invalid api key",
    "please run /login",
    "not logged in",
    "oauth token has expired",
    "invalid bearer token",
];

/// Phrases of a refused or unknown model, lower case.
const MODEL_MARKERS: &[&str] = &[
    "issue with the selected model",
    "model_not_found",
    "not_found_error",
    "may not have access to it",
];

/// Phrases of an API or network failure that says nothing about the session, lower case.
const API_MARKERS: &[&str] = &[
    "overloaded",
    "api_error",
    "internal server error",
    "connection error",
    "econnrefused",
    "econnreset",
    "etimedout",
    "enotfound",
    "fetch failed",
    "socket hang up",
    "service unavailable",
    "bad gateway",
    "gateway timeout",
];

fn mentions_any(text: &str, markers: &[&str]) -> bool {
    let lower = text.to_lowercase();
    markers.iter().any(|marker| lower.contains(marker))
}

/// Whether an error says the plan's usage limit is reached, for a stream that printed no
/// usage window with a `rejected` status.
fn is_plan_limit_text(error: &str) -> bool {
    mentions_any(
        error,
        &[
            "usage limit reached",
            "hit your limit",
            "limit reached",
            "you've reached your",
        ],
    )
}

/// The marker [`read_result`] puts in front of the error text when the result named an HTTP
/// status.
const STATUS_PREFIX: &str = "API error ";

/// The HTTP status [`read_result`] recorded at the start of an error, if any.
fn error_status(error: &str) -> Option<u16> {
    let rest = error.strip_prefix(STATUS_PREFIX)?;
    rest.split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

fn read_init(event: &Value, events: &mut TurnEvents) {
    // A later `init` replaces an earlier one: the latest names the transcript in use.
    events.transcript = event["session_id"].as_str().map(str::to_string);
    events.model = event["model"].as_str().map(str::to_string);
    events.cli_version = event["claude_code_version"].as_str().map(str::to_string);
}

fn read_rate_limit(event: &Value, events: &mut TurnEvents) {
    let info = &event["rate_limit_info"];
    if let Some(windows) = info["unifiedWindows"].as_object() {
        for (window, value) in windows {
            if let Some(utilization) = value["utilization"].as_f64() {
                events.usage.push(UsageSample {
                    window: window.clone(),
                    utilization,
                    resets_at: text_of(&value["resetsAt"]),
                    status: value["status"].as_str().map(str::to_string),
                });
            }
        }
        return;
    }
    // An event that names its one window as `rateLimitType`, with no `unifiedWindows`.
    if let Some(window) = info["rateLimitType"].as_str() {
        events.usage.push(UsageSample {
            window: window.to_string(),
            utilization: info["utilization"].as_f64().unwrap_or(0.0),
            resets_at: text_of(&info["resetsAt"]),
            status: info["status"].as_str().map(str::to_string),
        });
    }
}

/// A JSON string or number as the text the CLI printed.
fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn read_assistant(event: &Value, events: &mut TurnEvents) {
    // A subagent's messages carry the id of the tool call that started it; they are not the
    // worker's own words.
    if !event["parent_tool_use_id"].is_null() {
        return;
    }
    let Some(blocks) = event["message"]["content"].as_array() else {
        return;
    };
    let text: String = blocks
        .iter()
        .filter(|block| block["type"].as_str() == Some("text"))
        .filter_map(|block| block["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if !text.trim().is_empty() {
        events.last_text = Some(text);
    }
}

fn read_result(event: &Value, events: &mut TurnEvents) {
    events.result_seen = true;
    let subtype = event["subtype"].as_str().unwrap_or("");
    let failed = event["is_error"].as_bool().unwrap_or(false) || subtype != "success";
    events.result_error = failed;
    events.cost_usd = event["total_cost_usd"].as_f64();
    let usage = &event["usage"];
    if usage.is_object() {
        let count = |key: &str| usage[key].as_u64().unwrap_or(0);
        events.tokens = Some(TokenCounts {
            input: count("input_tokens"),
            output: count("output_tokens"),
            cache_read: count("cache_read_input_tokens"),
            cache_write: count("cache_creation_input_tokens"),
        });
    }
    events.error = failed.then(|| {
        let text = event["result"].as_str().unwrap_or("");
        let detail = if text.is_empty() {
            subtype.to_string()
        } else {
            format!("{subtype}: {text}")
        };
        match event["api_error_status"].as_u64() {
            Some(status) => format!("{STATUS_PREFIX}{status}: {detail}"),
            None => detail,
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST_TURN: &str = include_str!("fixtures/first_turn.jsonl");
    const RESUMED_TURN: &str = include_str!("fixtures/resumed_turn.jsonl");
    const KILLED_TURN: &str = include_str!("fixtures/killed_turn.jsonl");

    fn spec() -> TurnSpec {
        TurnSpec {
            app: "claude".to_string(),
            program: "claude".to_string(),
            args: vec!["--dangerously-skip-permissions".to_string()],
            model: "claude-opus-5-5".to_string(),
            effort: Some("medium".to_string()),
            transcript: None,
            new_transcript: Some("11111111-2222-3333-4444-555555555555".to_string()),
            prompt: "Run `ignite start T-1`".to_string(),
        }
    }

    fn classify(output: &str, exit: Option<i32>) -> TurnEnd {
        let events = Claude.read(output);
        Claude.classify(&events, exit)
    }

    /// A one-line `result` event.
    fn result(subtype: &str, is_error: bool, status: Option<u16>, text: &str) -> String {
        serde_json::json!({
            "type": "result", "subtype": subtype, "is_error": is_error,
            "api_error_status": status, "result": text, "total_cost_usd": 0.01,
            "usage": {"input_tokens": 1, "output_tokens": 2},
        })
        .to_string()
    }

    fn init(session: &str) -> String {
        serde_json::json!({
            "type": "system", "subtype": "init", "session_id": session,
            "model": "claude-opus-5-5", "claude_code_version": "2.1.323",
        })
        .to_string()
    }

    fn failed_turn(status: Option<u16>, text: &str) -> TurnEnd {
        classify(
            &format!("{}\n{}\n", init("s"), result("success", true, status, text)),
            Some(1),
        )
    }

    #[test]
    fn a_first_turn_asks_for_a_new_transcript() {
        let command = Claude.command(&spec());
        assert_eq!(
            command,
            [
                "claude",
                "-p",
                "--dangerously-skip-permissions",
                "--model",
                "claude-opus-5-5",
                "--effort",
                "medium",
                "--output-format",
                "stream-json",
                "--verbose",
                "--session-id",
                "11111111-2222-3333-4444-555555555555",
                "Run `ignite start T-1`",
            ]
        );
    }

    #[test]
    fn a_later_turn_resumes_and_never_names_a_fallback_model() {
        let mut later = spec();
        later.transcript = Some("abc".to_string());
        later.effort = None;
        let command = Claude.command(&later);
        let resume = command.iter().position(|a| a == "--resume").unwrap();
        assert_eq!(command[resume + 1], "abc");
        assert!(!command.contains(&"--session-id".to_string()));
        assert!(!command.contains(&"--effort".to_string()));
        assert!(!command.iter().any(|a| a.contains("fallback")));
        assert_eq!(command.last().unwrap(), "Run `ignite start T-1`");
    }

    #[test]
    fn a_recorded_first_turn_is_read_and_ends_clean() {
        let events = Claude.read(FIRST_TURN);
        assert_eq!(
            events.transcript.as_deref(),
            Some("dd691057-0915-48ba-8bce-06b6bdf0d3da")
        );
        assert_eq!(events.model.as_deref(), Some("claude-haiku-5-5"));
        assert_eq!(events.cli_version.as_deref(), Some("2.1.323"));
        assert_eq!(events.last_text.as_deref(), Some("done"));
        assert_eq!(events.usage.len(), 1);
        assert_eq!(events.usage[0].window, "seven_day");
        assert!((events.usage[0].utilization - 0.83).abs() < 1e-9);
        assert_eq!(events.usage[0].resets_at.as_deref(), Some("1795554000"));
        assert_eq!(events.usage[0].status.as_deref(), Some("allowed_warning"));
        assert!(events.result_seen && !events.result_error);
        assert!((events.cost_usd.unwrap() - 0.003_202_12).abs() < 1e-9);
        assert_eq!(
            events.tokens,
            Some(TokenCounts {
                input: 2,
                output: 3,
                cache_read: 24222,
                cache_write: 14791
            })
        );
        assert_eq!(events.lines, 4);
        assert_eq!(Claude.classify(&events, Some(0)), TurnEnd::Clean);
    }

    #[test]
    fn a_recorded_turn_killed_before_its_result_is_interrupted() {
        let events = Claude.read(KILLED_TURN);
        assert!(events.transcript.is_some());
        assert!(!events.result_seen);
        assert!(events.last_text.is_some());
        assert_eq!(Claude.classify(&events, None), TurnEnd::Interrupted);
        // Whatever exit a supervisor records for a killed process, there is no result.
        assert_eq!(Claude.classify(&events, Some(137)), TurnEnd::Interrupted);
    }

    #[test]
    fn a_resumed_turns_new_transcript_id_is_the_one_read() {
        let resumed = RESUMED_TURN.replace(
            "dd691057-0915-48ba-8bce-06b6bdf0d3da",
            "99999999-aaaa-bbbb-cccc-dddddddddddd",
        );
        let first = Claude.read(FIRST_TURN);
        let events = Claude.read(&resumed);
        assert_eq!(
            events.transcript.as_deref(),
            Some("99999999-aaaa-bbbb-cccc-dddddddddddd")
        );
        assert_ne!(events.transcript, first.transcript);
        assert_eq!(Claude.classify(&events, Some(0)), TurnEnd::Clean);
    }

    #[test]
    fn a_rejected_window_is_the_plan_limit() {
        let output = format!(
            "{}\n{}\n{}\n",
            init("s"),
            r#"{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","unifiedWindows":{"five_hour":{"utilization":1.0,"resetsAt":1795554000,"status":"rejected"},"seven_day":{"utilization":0.4,"status":"allowed"}}}}"#,
            result("success", true, Some(429), "You've hit your limit"),
        );
        assert_eq!(
            classify(&output, Some(1)),
            TurnEnd::PlanLimit {
                window: Some("five_hour".to_string())
            }
        );
    }

    #[test]
    fn a_plan_limit_message_without_a_window_is_still_the_plan_limit() {
        let end = failed_turn(Some(429), "Claude AI usage limit reached|1795554000");
        assert_eq!(end, TurnEnd::PlanLimit { window: None });
    }

    #[test]
    fn an_authentication_error_holds_as_not_logged_in() {
        let end = failed_turn(
            Some(401),
            "Invalid API key · Please run /login\nsecond line",
        );
        assert_eq!(
            end,
            TurnEnd::Hold {
                reason: HoldReason::NotLoggedIn,
                message: "API error 401: success: Invalid API key · Please run /login".to_string(),
            }
        );
    }

    #[test]
    fn a_refused_model_holds_as_model_refused() {
        let end = failed_turn(
            None,
            "There's an issue with the selected model (claude-nope). It may not exist or you may not have access to it.",
        );
        assert!(matches!(
            end,
            TurnEnd::Hold {
                reason: HoldReason::ModelRefused,
                ..
            }
        ));
        let end = failed_turn(Some(404), "model: claude-nope");
        assert!(matches!(
            end,
            TurnEnd::Hold {
                reason: HoldReason::ModelRefused,
                ..
            }
        ));
    }

    #[test]
    fn api_errors_hold_as_api_error() {
        for (status, text) in [
            (Some(500), "Internal server error"),
            (Some(529), "Overloaded"),
            (None, "API Error: Connection error. (ECONNRESET)"),
        ] {
            let end = failed_turn(status, text);
            assert!(
                matches!(
                    end,
                    TurnEnd::Hold {
                        reason: HoldReason::ApiError,
                        ..
                    }
                ),
                "{text}: {end:?}"
            );
        }
    }

    #[test]
    fn a_turn_that_printed_nothing_holds_as_no_first_line() {
        assert!(matches!(
            classify("", Some(1)),
            TurnEnd::Hold {
                reason: HoldReason::NoFirstLine,
                ..
            }
        ));
        assert!(matches!(
            classify("claude: something went wrong\n", Some(2)),
            TurnEnd::Hold {
                reason: HoldReason::NoFirstLine,
                ..
            }
        ));
        assert!(matches!(
            classify("", Some(127)),
            TurnEnd::Hold {
                reason: HoldReason::ProgramMissing,
                ..
            }
        ));
        // Killed before it printed anything: no evidence against the CLI.
        assert_eq!(classify("", None), TurnEnd::Interrupted);
    }

    #[test]
    fn an_error_nothing_recognises_is_unknown() {
        let end = failed_turn(None, "something odd");
        assert_eq!(
            end,
            TurnEnd::Unknown {
                why: "success: something odd".to_string()
            }
        );
        let output = format!(
            "{}\n{}\n",
            init("s"),
            result("error_max_turns", false, None, "")
        );
        assert_eq!(
            classify(&output, Some(1)),
            TurnEnd::Unknown {
                why: "error_max_turns".to_string()
            }
        );
    }

    #[test]
    fn lines_that_are_not_json_and_events_of_other_types_are_skipped() {
        let output = format!(
            "warning: not json\n{}\n[1,2]\n{{\"type\":\"future_event\"}}\n{{\"type\":\"user\"}}\n{}\n",
            init("s"),
            result("success", false, None, "done")
        );
        let events = Claude.read(&output);
        assert_eq!(events.lines, 6);
        assert_eq!(events.transcript.as_deref(), Some("s"));
        assert_eq!(Claude.classify(&events, Some(0)), TurnEnd::Clean);
    }

    #[test]
    fn a_subagents_message_is_not_the_workers_last_text() {
        let output = format!(
            "{}\n{}\n{}\n",
            init("s"),
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"mine"}]},"parent_tool_use_id":null}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"sub"}]},"parent_tool_use_id":"toolu_1"}"#,
        );
        assert_eq!(Claude.read(&output).last_text.as_deref(), Some("mine"));
    }
}
