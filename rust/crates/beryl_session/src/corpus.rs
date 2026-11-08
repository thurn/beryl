//! The frozen save corpus (architecture §14): saves of every age, committed under
//! `tests/saves/` and never regenerated, which every build must still play. `beryl_cli
//! freeze-saves` (`just freeze-saves`) adds new ones; none is ever changed or removed.
//!
//! Each save is gzipped JSON, `<name>.json.gz`, in the format of the build that froze it, which
//! loading migrates to today's ([`load`]). Each is held at three levels ([`check`]): it replays to the end of its log, every
//! turn-start checkpoint it records matches the board the replay reaches, and the game ends as
//! it ended, on the same turn. `endings.json` beside the saves records how each ended, as the
//! save itself does not. A save on the breaks list ([`crate::migrate::Break`]) is held to its
//! listed entry instead ([`check_listed`]).

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::time::SystemTime;

use beryl_engine::CardTable;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize};

use crate::migrate::{self, Break};
use crate::protocol::{OutcomeView, Side};
use crate::replay::Entry;
use crate::{Game, PlayerView, ReplayFile, SessionError, SessionId, Status, journal};

/// The file beside the saves that records how each ended.
pub const ENDINGS: &str = "endings.json";

/// How a recorded game stood where its log ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ending {
    pub turn: u16,
    pub outcome: Outcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    /// Player `seat` won.
    Won {
        seat: u8,
    },
    Draw,
    /// The engine gave up when a budget ran out.
    Stalled,
    /// The game goes on: a decision is open, or the agent is about to make one.
    Unfinished,
}

impl Ending {
    /// How the game `file` records stands at `view`, the view its replay ends on.
    pub fn of(file: &ReplayFile, view: &PlayerView) -> Ending {
        let outcome = match &view.status {
            Status::Over {
                outcome: OutcomeView::Win { winner, .. },
            } => Outcome::Won {
                seat: match winner {
                    Side::You => file.human,
                    Side::Opponent => 1 - file.human,
                },
            },
            Status::Over {
                outcome: OutcomeView::Draw,
            } => Outcome::Draw,
            Status::Over {
                outcome: OutcomeView::Stalled { .. },
            } => Outcome::Stalled,
            _ => Outcome::Unfinished,
        };
        Ending {
            turn: view.turn.number,
            outcome,
        }
    }
}

impl fmt::Display for Ending {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let turn = self.turn;
        match self.outcome {
            Outcome::Won { seat } => write!(f, "with player {seat} winning on turn {turn}"),
            Outcome::Draw => write!(f, "in a draw on turn {turn}"),
            Outcome::Stalled => write!(f, "stalled on turn {turn}"),
            Outcome::Unfinished => write!(f, "unfinished on turn {turn}"),
        }
    }
}

/// What a save is held to, in the order [`check`] checks them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Its log replays to the end.
    Replays,
    /// The board as each turn began matches its checkpoint.
    Checkpoints,
    /// The game ends as it ended.
    Ending,
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Level::Replays => "level 1, replaying to the end",
            Level::Checkpoints => "level 2, the board as each turn began",
            Level::Ending => "level 3, how the game ended",
        })
    }
}

/// Where a save fails [`check`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub level: Level,
    pub reason: String,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.level, self.reason)
    }
}

impl Failure {
    fn of(level: Level, e: SessionError) -> Failure {
        let reason = match e {
            SessionError::BadReplay { reason } => reason,
            e => e.to_string(),
        };
        Failure { level, reason }
    }
}

const SESSION: SessionId = SessionId(0);

/// Checks that `file` replays exactly on this build ([`Game::replay_exactly`]): to the end of
/// its log, then with every checkpoint matching, then ending as `ending` says, if given.
/// Returns how it ended, or the first level it fails at.
pub fn check(
    cards: &'static CardTable,
    file: &ReplayFile,
    ending: Option<&Ending>,
) -> Result<Ending, Failure> {
    match Game::replay_exactly(cards, file, SESSION) {
        Ok((_, view)) => {
            let reached = Ending::of(file, &view);
            match ending {
                Some(&recorded) if recorded != reached => Err(Failure {
                    level: Level::Ending,
                    reason: format!("the record ends {recorded}, the replay {reached}"),
                }),
                _ => Ok(reached),
            }
        }
        Err(exact) => {
            // Whether it fails without its checkpoints too.
            let mut bare = file.clone();
            bare.log.retain(|e| !matches!(e, Entry::Checkpoint(_)));
            Err(match Game::replay_exactly(cards, &bare, SESSION) {
                Ok(_) => Failure::of(Level::Checkpoints, exact),
                Err(e) => Failure::of(Level::Replays, e),
            })
        }
    }
}

/// Checks a save of the corpus: as [`check`] does against its recorded `ending`, or, where it
/// is on the breaks list, its log up to the listed entry at levels 1 and 2. A listed save that
/// replays to its end fails, as it no longer belongs on the list.
pub fn check_listed(
    cards: &'static CardTable,
    file: &ReplayFile,
    ending: &Ending,
    listed: Option<&Break>,
) -> Result<(), Failure> {
    let Some(listed) = listed else {
        return check(cards, file, Some(ending)).map(|_| ());
    };
    if check(cards, file, Some(ending)).is_ok() {
        return Err(Failure {
            level: Level::Ending,
            reason: "it plays to its end again: take it off the breaks list".to_owned(),
        });
    }
    let mut held = file.clone();
    if listed.entry > held.log.len() {
        return Err(Failure {
            level: Level::Replays,
            reason: format!(
                "the breaks list holds it to entry {}, of a log of {}",
                listed.entry,
                held.log.len()
            ),
        });
    }
    held.log.truncate(listed.entry);
    check(cards, &held, None).map(|_| ())
}

/// The save at `path`, gzipped JSON, upgraded to the format this build writes
/// ([`crate::migrate`]), and the format it was frozen in.
pub fn load(cards: &'static CardTable, path: &Path) -> Result<(ReplayFile, u8), String> {
    let failed = |e: &dyn fmt::Display| format!("{}: {e}", path.display());
    let gz = std::fs::File::open(path).map_err(|e| failed(&e))?;
    let json: serde_json::Value =
        serde_json::from_reader(GzDecoder::new(std::io::BufReader::new(gz)))
            .map_err(|e| failed(&e))?;
    let frozen = json
        .get("formatVersion")
        .and_then(serde_json::Value::as_u64)
        .map_or(1, |v| u8::try_from(v).unwrap_or(u8::MAX));
    let file = migrate::to_current(json, cards).map_err(|e| failed(&e))?;
    Ok((file, frozen))
}

/// The endings of the saves in the corpus at `dir`, by file name.
pub fn endings(dir: &Path) -> Result<BTreeMap<String, Ending>, String> {
    let path = dir.join(ENDINGS);
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Adds `file`, which ended as `ending`, to the corpus at `dir` as `name`, which no save there
/// has yet.
pub fn freeze(dir: &Path, name: &str, file: &ReplayFile, ending: Ending) -> Result<(), String> {
    let mut all = endings(dir)?;
    let path = dir.join(name);
    if all.contains_key(name) || path.exists() {
        return Err(format!("{} is in the corpus already", path.display()));
    }
    let failed = |path: &Path, e: &dyn fmt::Display| format!("{}: {e}", path.display());
    std::fs::create_dir_all(dir).map_err(|e| failed(dir, &e))?;
    let mut gz = GzEncoder::new(Vec::new(), Compression::best());
    serde_json::to_writer(&mut gz, file).expect("a replay serializes");
    let bytes = gz.finish().expect("gzip writes to memory");
    std::fs::write(&path, bytes).map_err(|e| failed(&path, &e))?;
    all.insert(name.to_owned(), ending);
    // One line a save, so that adding one adds a line.
    let lines: Vec<String> = all
        .iter()
        .map(|(name, ending)| {
            let ending = serde_json::to_string(ending).expect("an ending serializes");
            format!("  {}: {ending}", serde_json::Value::from(name.as_str()))
        })
        .collect();
    let json = format!("{{\n{}\n}}\n", lines.join(",\n"));
    let endings = dir.join(ENDINGS);
    std::fs::write(&endings, json).map_err(|e| failed(&endings, &e))
}

/// Today's UTC date, `YYYY-MM-DD`, which a frozen save's name begins with.
pub fn today() -> String {
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    journal::civil(secs).0
}
