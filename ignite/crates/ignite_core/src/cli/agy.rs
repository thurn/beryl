//! The Antigravity CLI (`agy`) adapter: builds the command line of one `agy -p` turn,
//! reads its `--output-format stream-json` NDJSON events, classifies how the turn ended, and
//! parses `agy -p /usage --output-format json` quota snapshots.
//!
//! The events this adapter reads (from `agy` 1.5.x), one JSON object per line keyed by `"event"`:
//!
//! - `init` carries `conversation_id` (the transcript ID to resume with `--conversation`) and
//!   `init.model` (echoing the `--model` slug passed on the command line).
//! - `step_update` carries `step_index`, `state` (`ACTIVE` or `DONE`), `step_type`
//!   (`user_input`, `agent_response`, `tool`, `checkpoint`), `text_delta`, `tool_info` and
//!   per-step `usage` (`input_tokens` excluding cached, `cache_read_tokens`, `output_tokens`).
//! - `result` carries `conversation_id`, `status` (`SUCCESS`, `ERROR`, `CANCELED`, `INTERRUPTED`,
//!   ...), `response`, `error`, `num_turns` and cumulative `usage`.
//!
//! Because `result.usage` is cumulative across `--conversation` resumes, per-turn token counts
//! are summed from completed `step_update` events emitted during this turn. Pre-init failures
//! (such as an unknown model) emit a `result` event without an `init` event, so `classify`
//! checks `result_seen` before `transcript.is_none()`.

use serde_json::Value;

use super::{CliApp, HoldReason, TokenCounts, TurnEnd, TurnEvents, TurnSpec, UsageSample};

/// The `agy` program, run as `agy -p <prompt> ... --output-format stream-json`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Agy;

/// Which Antigravity quota group a provider draws from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuotaGroup {
    /// First-party Gemini models (`gemini-5h`, `gemini-weekly`, `gemini-argon-wtu-weekly`).
    Gemini,
    /// Partner Claude and GPT models (`3p-5h`, `3p-weekly`).
    ThirdParty,
}

/// Maps a model ID to the Antigravity quota group that bills it.
#[must_use]
pub fn quota_group_for_model(model: &str) -> QuotaGroup {
    let lower = model.to_ascii_lowercase();
    if lower.starts_with("claude") || lower.starts_with("gpt") {
        QuotaGroup::ThirdParty
    } else {
        QuotaGroup::Gemini
    }
}

/// Parses `agy -p /usage --output-format json` output for one [`QuotaGroup`].
#[must_use]
pub fn read_usage(output: &str, group: QuotaGroup) -> Vec<UsageSample> {
    let Some(root) = output
        .lines()
        .find_map(|line| serde_json::from_str::<Value>(line).ok())
    else {
        return Vec::new();
    };
    if root["status"].as_str() != Some("SUCCESS")
        || root["command"]["name"].as_str() != Some("usage")
    {
        return Vec::new();
    }
    let Some(groups) = root["command"]["data"]["groups"].as_array() else {
        return Vec::new();
    };
    let mut samples = Vec::new();
    for entry in groups {
        let group_name = entry["name"].as_str().unwrap_or("");
        let Some(buckets) = entry["buckets"].as_array() else {
            continue;
        };
        for bucket in buckets {
            let Some(window) = bucket_window(group, group_name, bucket) else {
                continue;
            };
            let Some(remaining) = bucket["remaining_fraction"].as_f64() else {
                continue;
            };
            if !remaining.is_finite() {
                continue;
            }
            let utilization = (1.0 - remaining).clamp(0.0, 1.0);
            let status = if utilization >= 1.0 {
                "rejected"
            } else {
                "allowed"
            };
            samples.push(UsageSample {
                window,
                utilization,
                resets_at: bucket["reset_time"].as_str().map(str::to_owned),
                status: Some(status.to_owned()),
            });
        }
    }
    samples
}

fn bucket_window(group: QuotaGroup, group_name: &str, bucket: &Value) -> Option<String> {
    let id = bucket["id"].as_str().unwrap_or("");
    let window = bucket["window"].as_str().unwrap_or("");
    match group {
        QuotaGroup::Gemini => match id {
            "gemini-5h" => Some("five_hour".to_owned()),
            "gemini-weekly" => Some("seven_day".to_owned()),
            "gemini-argon-wtu-weekly" => Some("seven_day_argon".to_owned()),
            _ if group_name.eq_ignore_ascii_case("Gemini Models") => fallback_window(window),
            _ => None,
        },
        QuotaGroup::ThirdParty => match id {
            "3p-5h" => Some("five_hour".to_owned()),
            "3p-weekly" => Some("seven_day".to_owned()),
            _ if group_name.to_ascii_lowercase().contains("claude") => fallback_window(window),
            _ => None,
        },
    }
}

fn fallback_window(window: &str) -> Option<String> {
    match window {
        "5h" => Some("five_hour".to_owned()),
        "weekly" => Some("seven_day".to_owned()),
        _ => None,
    }
}

impl CliApp for Agy {
    fn name(&self) -> &'static str {
        "agy"
    }

    /// `<program> -p <prompt> <args...> --model <id> [--effort <level>] --output-format
    /// stream-json [--conversation <transcript>]`. Passing the prompt as `-p`'s option value
    /// handles prompts starting with `-` cleanly. `new_transcript` is ignored because `agy`
    /// assigns its own `conversation_id`.
    fn command(&self, spec: &TurnSpec) -> Vec<String> {
        let mut command = vec![spec.program.clone(), "-p".to_owned(), spec.prompt.clone()];
        command.extend(spec.args.iter().cloned());
        command.push("--model".to_owned());
        command.push(spec.model.clone());
        if let Some(effort) = &spec.effort {
            command.push("--effort".to_owned());
            command.push(effort.clone());
        }
        command.extend(["--output-format", "stream-json"].map(str::to_owned));
        if let Some(transcript) = &spec.transcript {
            command.push("--conversation".to_owned());
            command.push(transcript.clone());
        }
        command
    }

    fn digest(&self, output: &str) -> Vec<super::digest::Entry> {
        super::digest::agy(output)
    }

    fn read(&self, output: &str) -> TurnEvents {
        let mut events = TurnEvents::default();
        let mut step_tokens = TokenCounts::default();
        let mut has_step_tokens = false;
        let mut pending_text: Vec<(u64, String)> = Vec::new();
        let mut fallback_result_tokens: Option<TokenCounts> = None;

        for line in output.lines() {
            events.lines += 1;
            let Ok(Value::Object(map)) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let event = Value::Object(map);
            match event["event"].as_str() {
                Some("init") => read_init(&event, &mut events),
                Some("step_update") => read_step_update(
                    &event["step_update"],
                    &mut events,
                    &mut step_tokens,
                    &mut has_step_tokens,
                    &mut pending_text,
                ),
                Some("result") => {
                    fallback_result_tokens = read_result(&event["result"], &mut events);
                }
                _ => {}
            }
        }
        if has_step_tokens {
            events.tokens = Some(step_tokens);
        } else if let Some(tokens) = fallback_result_tokens {
            events.tokens = Some(tokens);
        }
        events
    }

    fn classify(&self, events: &TurnEvents, exit: Option<i32>) -> TurnEnd {
        if events.result_seen {
            if !events.result_error {
                return TurnEnd::Clean;
            }
            let error = events.error.as_deref().unwrap_or("");
            return classify_failure(error);
        }
        match exit {
            Some(126 | 127) if events.lines == 0 => TurnEnd::Hold {
                reason: HoldReason::ProgramMissing,
                message: "the agy program could not be started".to_owned(),
            },
            Some(code) if code != 0 && events.transcript.is_none() => TurnEnd::Hold {
                reason: HoldReason::NoFirstLine,
                message: events
                    .error
                    .clone()
                    .or_else(|| events.startup_error.clone())
                    .unwrap_or_else(|| format!("agy exited with {code} before its first event")),
            },
            _ => TurnEnd::Interrupted,
        }
    }
}

fn read_init(event: &Value, events: &mut TurnEvents) {
    if let Some(id) = event["conversation_id"]
        .as_str()
        .filter(|id| !id.is_empty())
    {
        events.transcript = Some(id.to_owned());
    }
    if let Some(model) = event["init"]["model"]
        .as_str()
        .filter(|model| !model.is_empty())
    {
        events.model = Some(model.to_owned());
    }
}

fn read_step_update(
    update: &Value,
    events: &mut TurnEvents,
    step_tokens: &mut TokenCounts,
    has_step_tokens: &mut bool,
    pending_text: &mut Vec<(u64, String)>,
) {
    let step_type = update["step_type"].as_str().unwrap_or("");
    let state = update["state"].as_str().unwrap_or("");
    if step_type == "agent_response" {
        let index = update["step_index"].as_u64().unwrap_or(0);
        if let Some(delta) = update["text_delta"].as_str() {
            if let Some((_, buf)) = pending_text.iter_mut().find(|(i, _)| *i == index) {
                buf.push_str(delta);
            } else {
                pending_text.push((index, delta.to_owned()));
            }
        }
        if state == "DONE"
            && let Some(pos) = pending_text.iter().position(|(i, _)| *i == index)
        {
            let (_, text) = pending_text.remove(pos);
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                events.last_text = Some(trimmed.to_owned());
            }
        }
    }
    if state == "DONE" && step_type != "user_input" && update["usage"].is_object() {
        let added = read_tokens(&update["usage"]);
        step_tokens.input = step_tokens.input.saturating_add(added.input);
        step_tokens.output = step_tokens.output.saturating_add(added.output);
        step_tokens.cache_read = step_tokens.cache_read.saturating_add(added.cache_read);
        step_tokens.cache_write = step_tokens.cache_write.saturating_add(added.cache_write);
        *has_step_tokens = true;
    }
}

fn read_result(result: &Value, events: &mut TurnEvents) -> Option<TokenCounts> {
    events.result_seen = true;
    if events.transcript.is_none()
        && let Some(id) = result["conversation_id"]
            .as_str()
            .filter(|id| !id.is_empty())
    {
        events.transcript = Some(id.to_owned());
    }
    let status = result["status"].as_str().unwrap_or("");
    let failed = status != "SUCCESS";
    events.result_error = failed;
    if failed {
        let detail = result["error"].as_str().unwrap_or("").trim();
        events.error = Some(if detail.is_empty() {
            status.to_owned()
        } else {
            detail.to_owned()
        });
    } else if let Some(text) = result["response"].as_str() {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            events.last_text = Some(trimmed.to_owned());
        }
    }
    if !failed && result["num_turns"].as_u64() == Some(1) && result["usage"].is_object() {
        Some(read_tokens(&result["usage"]))
    } else {
        None
    }
}

fn read_tokens(usage: &Value) -> TokenCounts {
    let count = |key: &str| usage[key].as_u64().unwrap_or(0);
    TokenCounts {
        input: count("input_tokens"),
        output: count("output_tokens"),
        cache_read: count("cache_read_tokens"),
        cache_write: count("cache_write_tokens"),
    }
}

fn classify_failure(error: &str) -> TurnEnd {
    let first = error.lines().next().unwrap_or("").to_owned();
    let lower = error.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|needle| lower.contains(needle));
    let hold = |reason| TurnEnd::Hold {
        reason,
        message: first.clone(),
    };
    if matches!(error, "INTERRUPTED" | "CANCELED") {
        TurnEnd::Interrupted
    } else if has(&[
        "invalid model selection",
        "not recognized as a known model",
        "model_not_found",
        "model is not supported",
        "conflicts with --effort",
        "may not have access",
    ]) {
        hold(HoldReason::ModelRefused)
    } else if has(&[
        "quota",
        "usage limit",
        "weekly limit",
        "five hour limit",
        "5-hour limit",
        "hit your limit",
        "limit reached",
        "resource_exhausted",
    ]) {
        TurnEnd::PlanLimit { window: None }
    } else if has(&[
        "authentication required",
        "not authenticated",
        "unauthenticated",
        "not logged in",
        "please run /login",
        "invalid api key",
        "status 401",
        "unauthorized",
        "token expired",
    ]) {
        hold(HoldReason::NotLoggedIn)
    } else if has(&[
        "status 429",
        "status 500",
        "status 502",
        "status 503",
        "status 504",
        "status 529",
        "overloaded",
        "service unavailable",
        "unavailable",
        "internal server error",
        "internal error",
        "deadline exceeded",
        "connection",
        "timed out",
        "network",
        "econnreset",
        "econnrefused",
        "socket hang up",
    ]) {
        hold(HoldReason::ApiError)
    } else {
        TurnEnd::Unknown { why: first }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLEAN_WITH_TOOL: &str = include_str!("agy_fixtures/clean_with_tool.jsonl");
    const RESUMED: &str = include_str!("agy_fixtures/resumed.jsonl");
    const MODEL_REFUSED: &str = include_str!("agy_fixtures/model_refused.jsonl");
    const USAGE: &str = include_str!("agy_fixtures/usage.json");

    fn spec(transcript: Option<&str>) -> TurnSpec {
        TurnSpec {
            app: "agy-gemini".to_owned(),
            program: "agy".to_owned(),
            args: vec!["--dangerously-skip-permissions".to_owned()],
            model: "gemini-4-neon".to_owned(),
            effort: Some("high".to_owned()),
            transcript: transcript.map(str::to_owned),
            new_transcript: Some("ignored".to_owned()),
            prompt: "- do the work".to_owned(),
        }
    }

    fn end(output: &str, exit: Option<i32>) -> TurnEnd {
        let events = Agy.read(output);
        Agy.classify(&events, exit)
    }

    #[test]
    fn first_turn_command_starts_a_new_conversation() {
        assert_eq!(
            Agy.command(&spec(None)),
            [
                "agy",
                "-p",
                "- do the work",
                "--dangerously-skip-permissions",
                "--model",
                "gemini-4-neon",
                "--effort",
                "high",
                "--output-format",
                "stream-json",
            ]
        );
    }

    #[test]
    fn resumed_turn_passes_conversation_and_omits_missing_effort() {
        let mut later = spec(Some("f25c9f7a-0266-4483-8d18-d0900a514594"));
        later.effort = None;
        assert_eq!(
            Agy.command(&later),
            [
                "agy",
                "-p",
                "- do the work",
                "--dangerously-skip-permissions",
                "--model",
                "gemini-4-neon",
                "--output-format",
                "stream-json",
                "--conversation",
                "f25c9f7a-0266-4483-8d18-d0900a514594",
            ]
        );
    }

    #[test]
    fn clean_turn_reads_conversation_model_text_and_step_tokens() {
        let events = Agy.read(CLEAN_WITH_TOOL);
        assert_eq!(
            events.transcript.as_deref(),
            Some("f25c9f7a-0266-4483-8d18-d0900a514594")
        );
        assert_eq!(events.model.as_deref(), Some("gemini-4-neon"));
        assert_eq!(events.last_text.as_deref(), Some("done"));
        assert_eq!(
            events.tokens,
            Some(TokenCounts {
                input: 16120 + 2184,
                output: 41 + 1,
                cache_read: 14068,
                cache_write: 0,
            })
        );
        assert!(events.result_seen && !events.result_error);
        assert_eq!(Agy.classify(&events, Some(0)), TurnEnd::Clean);
    }

    #[test]
    fn resumed_turn_counts_only_its_own_step_tokens_not_cumulative_result_usage() {
        let events = Agy.read(RESUMED);
        assert_eq!(
            events.transcript.as_deref(),
            Some("f25c9f7a-0266-4483-8d18-d0900a514594")
        );
        assert_eq!(events.last_text.as_deref(), Some("resumed"));
        assert_eq!(
            events.tokens,
            Some(TokenCounts {
                input: 2255,
                output: 2,
                cache_read: 14206,
                cache_write: 0,
            })
        );
        assert_eq!(Agy.classify(&events, Some(0)), TurnEnd::Clean);
    }

    #[test]
    fn pre_init_model_refusal_holds_as_model_refused() {
        let events = Agy.read(MODEL_REFUSED);
        assert_eq!(events.transcript, None);
        assert!(events.result_seen && events.result_error);
        assert!(matches!(
            Agy.classify(&events, Some(1)),
            TurnEnd::Hold {
                reason: HoldReason::ModelRefused,
                ..
            }
        ));
    }

    #[test]
    fn failure_classification_covers_auth_quota_api_and_interruption() {
        let err = |msg: &str| {
            format!(
                r#"{{"event":"result","result":{{"conversation_id":"c","status":"ERROR","error":"{msg}"}}}}"#
            )
        };
        assert!(matches!(
            end(&err("authentication required: run agy to sign in"), Some(1)),
            TurnEnd::Hold {
                reason: HoldReason::NotLoggedIn,
                ..
            }
        ));
        assert_eq!(
            end(&err("Weekly limit reached for Gemini Models"), Some(1)),
            TurnEnd::PlanLimit { window: None }
        );
        assert!(matches!(
            end(&err("service unavailable (status 503)"), Some(1)),
            TurnEnd::Hold {
                reason: HoldReason::ApiError,
                ..
            }
        ));
        assert_eq!(
            end(
                r#"{"event":"result","result":{"conversation_id":"c","status":"INTERRUPTED"}}"#,
                Some(130)
            ),
            TurnEnd::Interrupted
        );
        assert!(matches!(
            end("", Some(127)),
            TurnEnd::Hold {
                reason: HoldReason::ProgramMissing,
                ..
            }
        ));
        assert!(matches!(
            end("boom\n", Some(1)),
            TurnEnd::Hold {
                reason: HoldReason::NoFirstLine,
                ..
            }
        ));
        let cut: String = CLEAN_WITH_TOOL
            .lines()
            .take(5)
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(end(&cut, None), TurnEnd::Interrupted);
    }

    #[test]
    fn usage_parser_splits_gemini_and_third_party_quota_groups() {
        assert_eq!(quota_group_for_model("gemini-4-neon"), QuotaGroup::Gemini);
        assert_eq!(
            quota_group_for_model("claude-opus-5-5"),
            QuotaGroup::ThirdParty
        );
        let gemini = read_usage(USAGE, QuotaGroup::Gemini);
        assert_eq!(gemini.len(), 3);
        assert_eq!(gemini[0].window, "seven_day");
        assert!((gemini[0].utilization - (1.0 - 0.833_070_456_981_658_9)).abs() < 1e-9);
        assert_eq!(gemini[0].resets_at.as_deref(), Some("2026-11-30T01:15:28Z"));
        assert_eq!(gemini[1].window, "five_hour");
        assert_eq!(gemini[2].window, "seven_day_argon");

        let third_party = read_usage(USAGE, QuotaGroup::ThirdParty);
        assert_eq!(third_party.len(), 2);
        assert_eq!(third_party[0].window, "seven_day");
        assert!((third_party[0].utilization - 0.05).abs() < 1e-9);
        assert_eq!(third_party[1].window, "five_hour");
        assert!((third_party[1].utilization - 0.20).abs() < 1e-9);
    }
}
