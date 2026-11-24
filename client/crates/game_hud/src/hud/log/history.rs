//! The session narration as its views arrive, including the history an undo rewinds.

use beryl_protocol::protocol::{PlayerView, Status};

use super::{LogLine, Names, learn_names, log_lines};

/// The log keeps this many entries.
const LOG_LENGTH: usize = 200;

/// One entry of the log: a line, numbered from 0 over the game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogEntry {
    pub key: u32,
    pub line: LogLine,
}

/// A game's log as its views arrive, and what keeps it: the web's `Logged`.
#[derive(Clone, Debug, Default)]
pub struct Logged {
    /// What has happened in the game so far, oldest first.
    log: Vec<LogEntry>,
    /// For each decision, by how many of the player's answers stand before it
    /// (`DecisionView::undo_depth`), the key of the last entry before it: an undo back to that
    /// decision goes back to there. None for a decision shown before the log saw it.
    marks: Vec<Option<u32>>,
    /// How many keys have been given: keys only grow, past entries an undo took back too.
    keys: u32,
    names: Names,
}

impl Logged {
    /// What has happened in the game so far, oldest first.
    #[must_use]
    pub fn entries(&self) -> &[LogEntry] {
        &self.log
    }

    /// Adds what `view` says happened, as the web's `logged` does for one view: an undone view
    /// first takes back what happened since the decision it asks again.
    pub fn take(&mut self, view: &PlayerView) {
        let decision = match &view.status {
            Status::Deciding { decision } => Some(decision),
            _ => None,
        };
        if let Some(decision) = decision.filter(|_| view.undone)
            && let Some(&Some(kept)) = self.marks.get(depth(decision.undo_depth))
        {
            self.log.retain(|entry| entry.key <= kept);
        }
        learn_names(&mut self.names, view);
        for line in log_lines(view, &self.names) {
            self.log.push(LogEntry {
                key: self.keys,
                line,
            });
            self.keys += 1;
        }
        if let Some(decision) = decision {
            let at = depth(decision.undo_depth);
            self.marks.truncate(at);
            self.marks.resize(at + 1, None);
            self.marks[at] = self.keys.checked_sub(1);
        }
        let excess = self.log.len().saturating_sub(LOG_LENGTH);
        self.log.drain(..excess);
    }
}

fn depth(undo_depth: u32) -> usize {
    usize::try_from(undo_depth).unwrap_or(usize::MAX)
}
