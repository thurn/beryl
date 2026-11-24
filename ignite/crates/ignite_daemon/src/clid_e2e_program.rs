//! The fake CLI runs this test binary again and emits the events recorded for each invocation.

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::test_process::ChildGate;

use super::ROLES;

#[derive(Serialize, Deserialize)]
pub(super) enum Step {
    Init,
    Say(String),
    Result,
    Emit(Value),
    Wait,
    Exit(i32),
}

pub(super) fn command(dir: &Path, original: &[String]) -> Vec<String> {
    let mut command = vec![
        original[0].clone(),
        "--exact".to_owned(),
        format!("{ROLES}program::role_cli"),
        "--ignored".to_owned(),
        "--nocapture".to_owned(),
        "--".to_owned(),
        dir.display().to_string(),
    ];
    command.extend_from_slice(&original[1..]);
    command
}

pub(super) fn invocations(dir: &Path) -> u32 {
    fs::read_to_string(dir.join("count")).map_or(0, |text| text.trim().parse().unwrap())
}

fn record_call(dir: &Path, args: &[String]) -> u32 {
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("count.lock"))
        .unwrap();
    lock.lock().unwrap();
    let n = invocations(dir) + 1;
    fs::write(dir.join(format!("args.{n}")), args.join("\0") + "\0").unwrap();
    fs::write(dir.join("count"), n.to_string()).unwrap();
    n
}

#[test]
#[ignore = "run as the scripted CLI by the end-to-end tests"]
fn role_cli() {
    let mut args = std::env::args().skip_while(|arg| arg != "--").skip(1);
    let Some(dir) = args.next().map(PathBuf::from) else {
        return;
    };
    let args: Vec<_> = args.collect();
    let n = record_call(&dir, &args);
    let scenario = fs::read_to_string(dir.join(format!("turn.{n}"))).ok();
    let steps: Vec<Step> = scenario.as_deref().map_or_else(
        || vec![Step::Init, Step::Say("done".to_owned()), Step::Result],
        |text| serde_json::from_str(text).unwrap(),
    );
    let option = |names: &[&str]| {
        args.windows(2)
            .find(|pair| names.contains(&pair[0].as_str()))
            .map(|pair| pair[1].as_str())
    };
    // Finish libtest's status line before the first JSON event.
    println!();
    for step in steps {
        match step {
            Step::Init => println!(
                "{}",
                json!({
                    "type": "system", "subtype": "init", "claude_code_version": "9.9.9",
                    "session_id": option(&["--session-id", "--resume"]).unwrap_or("probe"),
                    "model": option(&["--model"]).unwrap_or(""),
                })
            ),
            Step::Say(text) => println!(
                "{}",
                json!({"type": "assistant", "message": {"content": [{"type": "text", "text": text}]}})
            ),
            Step::Result => println!(
                "{}",
                json!({
                    "type": "result", "subtype": "success", "is_error": false, "result": "done",
                    "total_cost_usd": 0.01, "usage": {"input_tokens": 1, "output_tokens": 1},
                })
            ),
            Step::Emit(event) => println!("{event}"),
            Step::Wait => {
                ChildGate::wait(&fs::read_to_string(dir.join(format!("gate.{n}"))).unwrap());
            }
            Step::Exit(exit) => std::process::exit(exit),
        }
    }
    std::process::exit(0);
}
