//! The app's append-only record of completed human games.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read as _, Seek as _, SeekFrom, Write as _};
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::protocol::{LossReason, OutcomeView, PlayerView, Side, Status};
use crate::{ReplayFile, journal};

pub(crate) struct Ledger {
    path: PathBuf,
    lock: Mutex<()>,
}

#[derive(Serialize, Deserialize)]
struct FinishedGame {
    date: String,
    commit: String,
    agent: String,
    result: String,
    turns: u16,
}

pub(crate) struct Counts {
    pub(crate) against_agent: usize,
    pub(crate) since_report: usize,
}

impl Ledger {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    pub(crate) fn record_end(&self, replay: &ReplayFile, view: &PlayerView) {
        let Status::Over { outcome } = &view.status else {
            return;
        };
        let result = match outcome {
            OutcomeView::Win {
                winner: Side::You, ..
            } => "win",
            OutcomeView::Win {
                reason: LossReason::Concede,
                ..
            } => "concession",
            OutcomeView::Win { .. } => "loss",
            OutcomeView::Draw => "draw",
            OutcomeView::Stalled { .. } => return,
        };
        let time = journal::utc(SystemTime::now());
        let game = FinishedGame {
            date: time[..10].to_owned(),
            commit: replay.commit.as_deref().unwrap_or("unknown").to_owned(),
            agent: replay.opponent.clone(),
            result: result.to_owned(),
            turns: view.turn.number,
        };
        let mut line = serde_json::to_vec(&game).expect("a finished game serializes");
        line.push(b'\n');
        let _guard = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        if let Err(error) = (|| {
            fs::create_dir_all(self.directory())?;
            let mut file = OpenOptions::new()
                .create(true)
                .read(true)
                .append(true)
                .open(&self.path)?;
            if file.metadata()?.len() != 0 {
                file.seek(SeekFrom::End(-1))?;
                let mut tail = [0];
                file.read_exact(&mut tail)?;
                // Keep an interrupted write from swallowing the next complete line.
                if tail[0] != b'\n' {
                    line.insert(0, b'\n');
                }
            }
            file.write_all(&line)?;
            file.sync_data()
        })() {
            self.warn(&error);
        }
    }

    /// Publish the report and advance the local checkpoint under the same lock used to append
    /// games. A failed report leaves the previous checkpoint in place.
    pub(crate) fn publish<T, E>(
        &self,
        agent: Option<&str>,
        publish: impl FnOnce(Option<Counts>) -> Result<T, E>,
    ) -> Result<T, E> {
        let _guard = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let checkpoint = self.path.with_extension("reported");
        let counts = self.totals(agent).and_then(|(total, against_agent)| {
            let previous = match fs::read_to_string(&checkpoint) {
                Ok(text) => text.trim().parse::<usize>().map_err(io::Error::other)?,
                Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
                Err(error) => return Err(error),
            };
            Ok((
                total,
                Counts {
                    against_agent,
                    since_report: total.saturating_sub(previous),
                },
            ))
        });
        let (total, counts) = match counts {
            Ok(counts) => counts,
            Err(error) => {
                self.warn(&error);
                return publish(None);
            }
        };
        let saved = publish(Some(counts))?;
        if let Err(error) = (|| -> io::Result<()> {
            fs::create_dir_all(self.directory())?;
            let mut temporary = tempfile::NamedTempFile::new_in(self.directory())?;
            writeln!(temporary, "{total}")?;
            temporary.as_file().sync_data()?;
            temporary.persist(checkpoint)?;
            Ok(())
        })() {
            self.warn(&error);
        }
        Ok(saved)
    }

    fn totals(&self, agent: Option<&str>) -> io::Result<(usize, usize)> {
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((0, 0)),
            Err(error) => return Err(error),
        };
        let mut total = 0;
        let mut against_agent = 0;
        for line in BufReader::new(file).split(b'\n') {
            let line = line?;
            if let Ok(game) = serde_json::from_slice::<FinishedGame>(&line) {
                total += 1;
                against_agent += usize::from(agent == Some(game.agent.as_str()));
            }
        }
        Ok((total, against_agent))
    }

    fn directory(&self) -> &std::path::Path {
        self.path
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."))
    }

    fn warn(&self, error: &io::Error) {
        journal::warn("ledger", error.to_string(), json!({ "path": self.path }));
    }
}
