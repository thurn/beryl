//! Fakes for CLI mode: [`FakeCliApp`], a CLI app with a tiny event format of its own, and
//! [`ScriptedLauncher`], a launcher that plays a recorded event stream for each turn instead
//! of running a program.
//!
//! A test writes each turn's stream with [`FakeCliApp`]'s line builders, queues it on the
//! launcher with [`ScriptedLauncher::push`], and reads it back by the run's output path with
//! [`ScriptedLauncher::output`], as the daemon reads a real turn's `output.log`.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{Value, json};

use super::lock;
use crate::cli::{CliApp, HoldReason, TokenCounts, TurnEnd, TurnEvents, TurnSpec, UsageSample};
use crate::protocol::Usage;
use crate::traits::{CommandSpec, Launcher, RunResult};

/// A CLI app whose events are one JSON object per line with a `type` of `init`, `usage`,
/// `text`, `result` or `failed`. Its command is the program, the fixed arguments, `--model`,
/// the model, `--resume` and the transcript when there is one, and the prompt.
///
/// [`FakeCliApp::classify`] answers with the end set by [`FakeCliApp::set_end`] when there is
/// one; otherwise a result with exit 0 is clean, a stream with no lines and a failed exit is a
/// hold for no first line, a `failed` line whose error mentions `limit` is the plan limit, no
/// result is interrupted, and anything else is unknown.
#[derive(Debug, Default)]
pub struct FakeCliApp {
    commands: Mutex<Vec<TurnSpec>>,
    end: Mutex<Option<TurnEnd>>,
}

impl FakeCliApp {
    /// The first line of a turn.
    pub fn init_line(transcript: &str, model: &str, version: &str) -> String {
        json!({"type": "init", "transcript": transcript, "model": model, "version": version})
            .to_string()
    }

    /// A usage sample of one window, `utilization` from 0 to 1.
    pub fn usage_line(window: &str, utilization: f64, status: &str) -> String {
        json!({"type": "usage", "window": window, "utilization": utilization, "status": status})
            .to_string()
    }

    /// An assistant message.
    pub fn text_line(text: &str) -> String {
        json!({"type": "text", "text": text}).to_string()
    }

    /// The result line of a finished turn.
    pub fn result_line(cost_usd: f64) -> String {
        json!({"type": "result", "cost": cost_usd, "input": 10, "output": 5}).to_string()
    }

    /// The result line of a failed turn.
    pub fn failed_line(error: &str) -> String {
        json!({"type": "failed", "error": error}).to_string()
    }

    /// Makes every later [`CliApp::classify`] answer `end`.
    pub fn set_end(&self, end: TurnEnd) {
        *lock(&self.end) = Some(end);
    }

    /// Every turn [`CliApp::command`] was asked to build, in order.
    pub fn commands(&self) -> Vec<TurnSpec> {
        lock(&self.commands).clone()
    }
}

impl CliApp for FakeCliApp {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn command(&self, spec: &TurnSpec) -> Vec<String> {
        lock(&self.commands).push(spec.clone());
        let mut args = vec![spec.program.clone()];
        args.extend(spec.args.iter().cloned());
        args.extend(["--model".to_owned(), spec.model.clone()]);
        if let Some(transcript) = &spec.transcript {
            args.extend(["--resume".to_owned(), transcript.clone()]);
        }
        args.push(spec.prompt.clone());
        args
    }

    fn read(&self, output: &str) -> TurnEvents {
        let mut events = TurnEvents::default();
        for line in output.lines() {
            events.lines += 1;
            let Ok(value) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
            match value.get("type").and_then(Value::as_str) {
                Some("init") => {
                    events.transcript = text("transcript");
                    events.model = text("model");
                    events.cli_version = text("version");
                }
                Some("usage") => events.usage.push(UsageSample {
                    window: text("window").unwrap_or_default(),
                    utilization: value
                        .get("utilization")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0),
                    resets_at: text("resets_at"),
                    status: text("status"),
                }),
                Some("text") => events.last_text = text("text"),
                Some("result") => {
                    events.result_seen = true;
                    events.cost_usd = value.get("cost").and_then(Value::as_f64);
                    let count = |key: &str| value.get(key).and_then(Value::as_u64).unwrap_or(0);
                    events.tokens = Some(TokenCounts {
                        input: count("input"),
                        output: count("output"),
                        ..TokenCounts::default()
                    });
                }
                Some("failed") => {
                    events.result_seen = true;
                    events.result_error = true;
                    events.error = text("error");
                }
                _ => {}
            }
        }
        events
    }

    fn classify(&self, events: &TurnEvents, exit: Option<i32>) -> TurnEnd {
        if let Some(end) = lock(&self.end).clone() {
            return end;
        }
        let error = events.error.clone().unwrap_or_default();
        if events.lines == 0 && exit.is_some_and(|code| code != 0) {
            TurnEnd::Hold {
                reason: HoldReason::NoFirstLine,
                message: format!("exited {} before its first line", exit.unwrap_or_default()),
            }
        } else if events.result_error && error.contains("limit") {
            TurnEnd::PlanLimit { window: None }
        } else if events.result_seen && !events.result_error && exit == Some(0) {
            TurnEnd::Clean
        } else if !events.result_seen {
            TurnEnd::Interrupted
        } else {
            TurnEnd::Unknown {
                why: format!("exit {exit:?}: {error}"),
            }
        }
    }
}

/// One turn's recorded event stream and how its process ended.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptedTurn {
    /// The lines the turn printed, in order.
    pub lines: Vec<String>,
    /// The exit code; `None` when a signal ended the process.
    pub exit: Option<i32>,
    /// The supervisor ended the turn for running past its hang limit.
    pub hung: bool,
}

impl ScriptedTurn {
    /// A turn that printed `lines` and exited 0.
    pub fn clean(lines: Vec<String>) -> ScriptedTurn {
        ScriptedTurn {
            lines,
            exit: Some(0),
            hung: false,
        }
    }

    /// A turn that printed `lines` and was killed.
    pub fn killed(lines: Vec<String>) -> ScriptedTurn {
        ScriptedTurn {
            lines,
            exit: None,
            hung: false,
        }
    }
}

/// A launcher that runs nothing. Each run takes the next queued [`ScriptedTurn`] (a turn that
/// printed nothing and exited 0 when none is queued), keeps its lines under the run's output
/// path, `scripted/<n>/output.log`, and returns its exit.
#[derive(Debug, Default)]
pub struct ScriptedLauncher {
    queue: Mutex<VecDeque<ScriptedTurn>>,
    runs: Mutex<Vec<CommandSpec>>,
    outputs: Mutex<BTreeMap<PathBuf, ScriptedTurn>>,
}

impl ScriptedLauncher {
    /// Queues the stream the next run plays.
    pub fn push(&self, turn: ScriptedTurn) {
        lock(&self.queue).push_back(turn);
    }

    /// Every command run so far, in order.
    pub fn runs(&self) -> Vec<CommandSpec> {
        lock(&self.runs).clone()
    }

    /// The text a run wrote to `output`, one line per event, as a real `output.log` holds it.
    pub fn output(&self, output: &Path) -> Option<String> {
        lock(&self.outputs).get(output).map(|turn| {
            turn.lines.iter().fold(String::new(), |mut text, line| {
                text.push_str(line);
                text.push('\n');
                text
            })
        })
    }
}

impl Launcher for ScriptedLauncher {
    fn run(&self, spec: &CommandSpec) -> RunResult {
        let n = {
            let mut runs = lock(&self.runs);
            runs.push(spec.clone());
            runs.len()
        };
        let turn = lock(&self.queue)
            .pop_front()
            .unwrap_or_else(|| ScriptedTurn {
                exit: Some(0),
                ..ScriptedTurn::default()
            });
        let output = PathBuf::from(format!("scripted/{n}/output.log"));
        let result = RunResult {
            exit: turn.exit,
            output: output.clone(),
            usage: Usage::default(),
            hung: turn.hung,
        };
        lock(&self.outputs).insert(output, turn);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Class;

    fn spec(transcript: Option<&str>) -> TurnSpec {
        TurnSpec {
            app: "fake".to_owned(),
            program: "fake-cli".to_owned(),
            args: vec!["--yes".to_owned()],
            model: "m-1".to_owned(),
            effort: None,
            transcript: transcript.map(str::to_owned),
            new_transcript: None,
            prompt: "go".to_owned(),
        }
    }

    fn command(args: Vec<String>) -> CommandSpec {
        CommandSpec {
            args,
            cwd: PathBuf::from("/checkout"),
            env: Vec::new(),
            class: Class::Agent,
            cores: 0,
            resources: Vec::new(),
            grows: false,
            tree: None,
            ticket: None,
            hang_limit_s: None,
            reserve: None,
        }
    }

    #[test]
    fn a_played_stream_reads_back_as_the_turn_that_printed_it() {
        let app = FakeCliApp::default();
        let launcher = ScriptedLauncher::default();
        launcher.push(ScriptedTurn::clean(vec![
            FakeCliApp::init_line("t-1", "m-1", "1.0"),
            "not json".to_owned(),
            FakeCliApp::usage_line("five_hour", 0.25, "allowed"),
            FakeCliApp::text_line("done"),
            FakeCliApp::result_line(0.5),
        ]));
        launcher.push(ScriptedTurn::killed(vec![FakeCliApp::init_line(
            "t-2", "m-1", "1.0",
        )]));

        let first = launcher.run(&command(app.command(&spec(None))));
        let events = app.read(&launcher.output(&first.output).unwrap());
        assert_eq!(events.transcript.as_deref(), Some("t-1"));
        assert_eq!(events.usage.len(), 1);
        assert_eq!(events.last_text.as_deref(), Some("done"));
        assert_eq!((events.lines, events.cost_usd), (5, Some(0.5)));
        assert_eq!(app.classify(&events, first.exit), TurnEnd::Clean);

        let second = launcher.run(&command(app.command(&spec(Some("t-1")))));
        let events = app.read(&launcher.output(&second.output).unwrap());
        assert_eq!(app.classify(&events, second.exit), TurnEnd::Interrupted);
        assert!(launcher.runs()[1].args.ends_with(&[
            "--resume".to_owned(),
            "t-1".to_owned(),
            "go".to_owned()
        ]));

        let third = launcher.run(&command(vec!["fake-cli".to_owned()]));
        assert_eq!(launcher.output(&third.output).as_deref(), Some(""));
        app.set_end(TurnEnd::PlanLimit { window: None });
        assert_eq!(
            app.classify(&TurnEvents::default(), Some(0)),
            TurnEnd::PlanLimit { window: None }
        );
    }
}
