//! The client's request to its daemon and the daemon's reply: one JSON line each way.
//!
//! The daemon renders all text, so the client only prints `text` (or `json` under `--json`)
//! and exits with `exit`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ids::{Handle, SessionId};

/// The exit codes every verb shares.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum Exit {
    Success = 0,
    /// A usage or configuration error.
    Usage = 1,
    /// A check failed; the agent can fix it.
    CheckFailed = 2,
    /// A rebase conflict or a lost claim.
    Conflict = 3,
    /// The hub or the git remote cannot be reached.
    Unreachable = 4,
    /// Still running; the reply names the handle to wait on.
    Running = 5,
}

impl Exit {
    pub fn code(self) -> u8 {
        self as u8
    }
}

impl TryFrom<u8> for Exit {
    type Error = String;
    fn try_from(code: u8) -> Result<Self, String> {
        Ok(match code {
            0 => Exit::Success,
            1 => Exit::Usage,
            2 => Exit::CheckFailed,
            3 => Exit::Conflict,
            4 => Exit::Unreachable,
            5 => Exit::Running,
            _ => return Err(format!("unknown exit code {code}")),
        })
    }
}

impl From<Exit> for u8 {
    fn from(exit: Exit) -> u8 {
        exit.code()
    }
}

/// One command from the client.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    /// The verb, such as `land`.
    pub verb: String,
    /// The arguments after the verb, as typed.
    pub args: Vec<String>,
    /// The client's working directory, which names the build tree.
    pub cwd: PathBuf,
    /// The desktop-app session asking, when the environment names one.
    pub session: Option<SessionId>,
    /// How long the daemon may hold the reply before answering with a handle.
    pub wait_s: u32,
}

/// The daemon's answer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    pub exit: Exit,
    /// The rendered output, at most 40 lines.
    pub text: String,
    /// The same data as one JSON object, for `--json`.
    pub json: Option<serde_json::Value>,
    /// The operation still running, when `exit` is [`Exit::Running`].
    pub handle: Option<Handle>,
}

impl Reply {
    /// A reply with text only.
    pub fn text(exit: Exit, text: impl Into<String>) -> Reply {
        Reply {
            exit,
            text: text.into(),
            json: None,
            handle: None,
        }
    }
}

/// What a dispatcher is told to do next, printed by `ignite dispatch --wait`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchInstruction {
    pub id: u64,
    #[serde(rename = "do")]
    pub action: DispatchAction,
    pub session: Option<SessionId>,
    /// Clear the session before prompting it.
    #[serde(default)]
    pub clear: bool,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// The prompt or message text.
    pub prompt: Option<String>,
}

/// The kinds of instruction a dispatcher carries out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DispatchAction {
    /// Clear a worker, set its model and send it a ticket's prompt.
    Handoff,
    /// Send text to a live session.
    Message,
    /// The wait ended with nothing to do.
    Idle,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_and_reply_match_the_documented_lines() {
        let request: Request = serde_json::from_str(
            r#"{"verb": "land", "args": ["-m", "note"], "cwd": "/trees/tree-07",
                "session": "local_c8470108", "wait_s": 540}"#,
        )
        .unwrap();
        assert_eq!(request.verb, "land");
        assert_eq!(request.session.unwrap().as_str(), "local_c8470108");
        let reply: Reply = serde_json::from_str(
            r#"{"exit": 5, "text": "waiting", "json": null, "handle": "L-5521"}"#,
        )
        .unwrap();
        assert_eq!(reply.exit, Exit::Running);
        assert_eq!(serde_json::to_value(Exit::Conflict).unwrap(), 3);
        assert!(serde_json::from_str::<Exit>("9").is_err());
    }

    #[test]
    fn dispatch_instruction_matches_the_documented_line() {
        let instruction: DispatchInstruction = serde_json::from_str(
            r#"{"id": 4812, "do": "handoff", "session": "local_c8470108",
                "clear": true, "model": "opus", "effort": "high",
                "prompt": "Run `ignite start Tq44s.9.48` and follow what it prints."}"#,
        )
        .unwrap();
        assert_eq!(instruction.action, DispatchAction::Handoff);
        assert!(instruction.clear);
    }
}
