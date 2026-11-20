//! The messages CLI mode adds (see "CLI mode" in the design document): the arguments and
//! replies of `ignite pool mode`, `ignite pool watch` and `ignite tell --now`, and the hub's
//! routes that keep a digest of each CLI worker's transcript.
//!
//! - `POST /transcripts/append` takes a [`TranscriptAppend`] from a registered machine and
//!   answers a [`TranscriptAppendReply`]. Appending a line twice stores it once: a line is keyed
//!   by session, turn and index.
//! - `GET /transcripts/<session>?after=<seq>` answers a [`TranscriptPage`] of the lines stored
//!   after `seq`, oldest first.
//!
//! Transcripts can hold secrets, so the hub serves both routes on its private listener only.

use serde::{Deserialize, Serialize};

use crate::config::PoolMode;
use crate::ids::{MachineId, SessionId, TicketId};
use crate::time::Timestamp;

/// The flag of `ignite tell` that ends the worker's running turn and resumes it with the
/// message, instead of waiting for the turn's end. It works only for CLI workers.
pub const TELL_NOW_FLAG: &str = "--now";

/// The ticket metadata key a `tell --now` sets to its comment's number, beside `tell`, so the
/// daemon holding the ticket delivers that comment at once.
pub const TELL_NOW_KEY: &str = "tell_now";

/// The flag of `ignite pool watch` that prints the digest so far and exits instead of
/// following it.
pub const WATCH_NO_FOLLOW_FLAG: &str = "--no-follow";

/// The most characters of one digest line; a longer one is cut.
pub const DIGEST_LINE_CHARS: usize = 2000;

/// Days the hub keeps transcript lines.
pub const TRANSCRIPT_KEEP_DAYS: f64 = 7.0;

/// The last word of `ignite pool mode <app> cli|desktop|config`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PoolModeSetting {
    /// Run the app in CLI mode on this machine whatever `ignite.toml` says.
    Cli,
    /// Run the app with its desktop dispatcher whatever `ignite.toml` says.
    Desktop,
    /// Remove the override, so `ignite.toml` decides again.
    Config,
}

impl PoolModeSetting {
    pub fn parse(word: &str) -> Option<PoolModeSetting> {
        match word {
            "cli" => Some(PoolModeSetting::Cli),
            "desktop" => Some(PoolModeSetting::Desktop),
            "config" => Some(PoolModeSetting::Config),
            _ => None,
        }
    }

    /// The mode this setting forces; `None` for [`PoolModeSetting::Config`].
    pub fn mode(self) -> Option<PoolMode> {
        match self {
            PoolModeSetting::Cli => Some(PoolMode::Cli),
            PoolModeSetting::Desktop => Some(PoolMode::Desktop),
            PoolModeSetting::Config => None,
        }
    }
}

/// The JSON of `ignite pool mode`: the mode an app now runs in on this machine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolModeReply {
    pub app: String,
    pub mode: PoolMode,
    /// Whether the mode comes from an override stored in the daemon's database rather than
    /// from `ignite.toml`.
    pub overridden: bool,
}

/// What one digest line of a transcript shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptKind {
    /// Text the worker wrote.
    Assistant,
    /// A tool call: the tool's name and the first line of its input.
    Tool,
    /// How the turn ended.
    Result,
    /// A message the daemon delivered as a turn's prompt.
    Prompt,
}

/// One line of a CLI worker's transcript digest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptLine {
    pub session: SessionId,
    #[serde(default)]
    pub ticket: Option<TicketId>,
    pub turn: u32,
    /// The line's place in its turn's digest, from 0.
    pub index: u32,
    pub kind: TranscriptKind,
    /// At most [`DIGEST_LINE_CHARS`] characters.
    pub text: String,
    #[serde(default)]
    pub at: Option<Timestamp>,
}

/// `POST /transcripts/append`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptAppend {
    pub machine: MachineId,
    pub lines: Vec<TranscriptLine>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptAppendReply {
    /// Lines stored now; a line the hub already had is not counted.
    pub stored: u32,
}

/// A transcript line as the hub stores it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredTranscriptLine {
    /// The hub's sequence number, which a reader passes as `after` to get later lines.
    pub seq: i64,
    pub machine: MachineId,
    #[serde(flatten)]
    pub line: TranscriptLine,
}

/// `GET /transcripts/<session>?after=<seq>`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptPage {
    pub session: SessionId,
    pub lines: Vec<StoredTranscriptLine>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transcript_append_matches_the_documented_line() {
        let append: TranscriptAppend = serde_json::from_str(
            r#"{"machine": "m-1", "lines": [{"session": "cli-claude-0a1b2c3d",
                "ticket": "T.1", "turn": 2, "index": 0, "kind": "tool",
                "text": "Bash: ignite status"}]}"#,
        )
        .unwrap();
        assert_eq!(append.lines[0].kind, TranscriptKind::Tool);
        assert_eq!(append.lines[0].at, None);
        let stored = StoredTranscriptLine {
            seq: 7,
            machine: append.machine.clone(),
            line: append.lines[0].clone(),
        };
        let value = serde_json::to_value(&stored).unwrap();
        assert_eq!(value["seq"], 7);
        assert_eq!(value["turn"], 2);
        assert_eq!(
            serde_json::from_value::<StoredTranscriptLine>(value).unwrap(),
            stored
        );
    }

    #[test]
    fn pool_mode_words_parse() {
        assert_eq!(PoolModeSetting::parse("cli").unwrap().mode(), Some(PoolMode::Cli));
        assert_eq!(PoolModeSetting::parse("config").unwrap().mode(), None);
        assert_eq!(PoolModeSetting::parse("tui"), None);
    }
}
