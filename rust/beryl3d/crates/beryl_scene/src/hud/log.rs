//! The game log at the table's left, as the web interface's `GameLog` and `LogPanel` show it
//! (`ui/src/game/Narration.tsx`, styled by `.game-log` in `ui/src/screens/styles/table.css`): a
//! heading for each turn, then what happened, newest at the foot, scrolled to it whenever the
//! screen is built again (`Layer::scroll_to_end`).
//!
//! - The lines are made as each view arrives (`Logged::take`), from the difference between that
//!   view and the ones before: its events, and the names of the cards seen so far.
//! - The log is always folded to an icon button at the window's top left, which opens and closes
//!   it in its column (`GamePress::Log`), as the stage's `<GameLog folded>` does
//!   (`ui/src/stage/Chrome.tsx`).

mod lines;

pub use lines::{LogKind, LogLine, Names, counter_name, learn_names, listed, log_lines};

use crate::hud::GamePress;
use beryl_protocol::protocol::{PlayerView, Status};
use glam::Vec2;

use crate::element::{Anchor, Element, holder, text};
use crate::hud::HudIn;
use crate::widgets::{icon_button, icons, list};
use kit_base::data::space::hud::{LOG_BUTTON_TOP, LOG_LEFT, LOG_TOP};
use kit_base::tokens::{color, size, type_style};

/// The name of the log's list, which the client scrolls to its foot (`Layer::scroll_to_end`).
pub const LIST: &str = "Game log";

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

/// The game log as the client keeps it: its entries and whether the folded log is open.
#[derive(Clone, Debug, Default)]
pub struct Log {
    pub logged: Logged,
    pub open: bool,
}

fn depth(undo_depth: u32) -> usize {
    usize::try_from(undo_depth).unwrap_or(usize::MAX)
}

/// What the game screen shows for the log: nothing before a game has a view; otherwise its
/// button, with the list below it while it is open.
#[must_use]
pub fn log(input: &HudIn) -> Option<Element> {
    input.view?;
    let entries = input.log;
    let name = if input.log_open {
        "Hide log"
    } else {
        "Show log"
    };
    let mut button = icon_button(icons::LOG, name)
        .selected(input.log_open)
        .on_press(GamePress::Log);
    button.anchor = Some(Anchor::Point(
        Vec2::new(LOG_LEFT, LOG_BUTTON_TOP),
        Vec2::ZERO,
    ));
    let list = (input.log_open && !entries.is_empty()).then(|| {
        // On the desktop the oldest lines fade out under the list's top edge, as
        // `.game-log-list`'s mask fades them; the phone's sheet has no mask.
        let mut panel = (panel(entries)
            .width(size::LOG_WIDTH)
            .max_height(size::LOG_FOLDED_HEIGHT))
        .mask(kit_base::tokens::gradient::LOG_MASK);
        panel.anchor = Some(Anchor::Point(Vec2::new(LOG_LEFT, LOG_TOP), Vec2::ZERO));
        panel
    });
    Some(holder().name("log").child(button).children(list))
}

/// The log's list of `entries`, oldest first, on its panel, with no size or place of its own:
/// the column at the table's left sets them on the desktop, and the log's sheet on the phone
/// (`hud::phone`).
#[must_use]
pub fn panel(entries: &[LogEntry]) -> Element {
    let last = entries.len().saturating_sub(1);
    let rows = (entries.iter().enumerate()).map(|(at, entry)| {
        // A line said before, such as a second trigger of the same ability, is named with its
        // count, so that no two rows share a name.
        let before = (entries[..at].iter())
            .filter(|earlier| earlier.line.text == entry.line.text)
            .count();
        row(entry, before, (at == last, at == 0))
    });
    let mut panel = list(LIST, rows)
        .radius(size::LOG_RADIUS)
        .background(color::LOG);
    let (x, y) = (size::LOG_PADDING_X, size::LOG_PADDING_Y);
    panel.style.padding = [y, x, y, x];
    panel
}

/// One entry's row: a turn's heading in gold capitals, a countered spell in orange, anything
/// else in the dim text color, the newest in the text color. A row whose words `before` earlier
/// rows say too is named with its count, as "Blood Artist triggered (2)".
fn row(entry: &LogEntry, before: usize, (last, first): (bool, bool)) -> Element {
    let line = &entry.line;
    let (words, style, tint) = match line.kind {
        LogKind::Turn => (
            line.text.to_uppercase(),
            type_style::LOG_TURN,
            color::LOG_TURN,
        ),
        LogKind::Counter => (line.text.clone(), type_style::LOG, color::LOG_COUNTER),
        LogKind::Action if last => (line.text.clone(), type_style::LOG, color::LOG_LAST),
        LogKind::Action => (line.text.clone(), type_style::LOG, color::LOG_TEXT),
    };
    let name = if before == 0 {
        line.text.clone()
    } else {
        format!("{} ({})", line.text, before + 1)
    };
    let row = text(&words).style(style).color(tint).name(&name);
    if line.kind == LogKind::Turn && !first {
        row.margin([size::LOG_TURN_GAP, 0.0, 0.0, 0.0])
    } else {
        row
    }
}
