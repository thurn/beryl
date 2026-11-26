//! The match's side of games played as child processes, `beryl_cli ladder-game`
//! ([`super::game`]): at most [`games_at_once`] at a time, each started again when its process
//! dies, and recorded as crashed when it dies twice at the same decision.
//!
//! - **A death.** A game whose process exits other than [`game::FINISHED`] is started again,
//!   and continues from its log. If it dies again with its log no longer than at its last
//!   death, it is recorded with [`crate::match_log::Verdict::Crashed`] at the decision its log ends before.
//!   A death at a later decision starts the count again.
//! - **A hang.** A game whose log gains no line for [`HANG`] is killed, and that is a death.
//! - **Orphans.** When the match's own process is killed, its games play on. Run again, the
//!   match starts a game only when it can take the game's lock, which the orphan holds until
//!   it exits, so no game is played by two processes at once.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::{Match, Progress, Shared, Sprt, game, games_at_once};
use crate::match_log::GameRecord;
use crate::match_log::files;
use crate::match_log::game::{GameDecision, Line};

/// How long a game may go without adding a line to its log before it is killed as hung. No
/// decision at any budget in use takes a tenth of this.
const HANG: Duration = Duration::from_hours(1);

/// How often the match looks at its games' processes. Standard Rust has no wait for any one
/// of several children, and a game takes seconds at the least.
const POLL: Duration = Duration::from_millis(20);

/// A game being played by a child process.
struct Playing {
    g: u32,
    child: Child,
    /// Its log's length in bytes when last looked at, and when that length last changed.
    len: u64,
    since: Instant,
}

/// What the match has seen of its games' processes.
struct Games<'a> {
    exe: &'a Path,
    m: &'a Match,
    sprt: Option<Sprt>,
    /// The games still to start, in order.
    waiting: BTreeSet<u32>,
    playing: Vec<Playing>,
    /// Each game whose process has died, with the lines its log held at the last death.
    died: BTreeMap<u32, usize>,
}

/// Plays games `todo` of match `m`, each in a child process of `exe`, until they are played or
/// the test decides. Games already started when it decides finish and are recorded, but not
/// counted.
pub(super) fn run(
    exe: &Path,
    m: &Match,
    sprt: Option<Sprt>,
    todo: &[u32],
    mut sh: Shared,
    progress: &mut Progress<'_>,
) -> Result<Shared, String> {
    let mut games = Games {
        exe,
        m,
        sprt,
        waiting: todo.iter().copied().collect(),
        playing: Vec::new(),
        died: BTreeMap::new(),
    };
    let played = games.play(&mut sh, progress);
    // On a failure the games still playing are stopped: no match is left to record them.
    for p in &mut games.playing {
        let _ = p.child.kill();
        let _ = p.child.wait();
    }
    played.map(|()| sh)
}

impl Games<'_> {
    fn play(&mut self, sh: &mut Shared, progress: &mut Progress<'_>) -> Result<(), String> {
        let limit = games_at_once();
        loop {
            if sh.verdict.is_none() {
                self.start(limit)?;
            }
            if self.playing.is_empty() && (sh.verdict.is_some() || self.waiting.is_empty()) {
                return Ok(());
            }
            thread::sleep(POLL);
            let mut i = 0;
            while i < self.playing.len() {
                match self.look(i)? {
                    Some(status) => {
                        let p = self.playing.swap_remove(i);
                        if let Some(record) = self.ended(p.g, status)? {
                            let log = files::read::<Line>(&game::log_path(&self.m.out, p.g))?;
                            if sh.finish(self.m, self.sprt, &record, &log)? {
                                progress(sh.pairs, self.sprt);
                            }
                        }
                    }
                    None => i += 1,
                }
            }
        }
    }

    /// Starts waiting games, in order, up to `limit` playing, passing over each whose lock
    /// another process holds.
    fn start(&mut self, limit: usize) -> Result<(), String> {
        let mut started = Vec::new();
        for &g in &self.waiting {
            if self.playing.len() >= limit {
                break;
            }
            // The lock is let go before the child takes it. No process of this match can take
            // it in between, since this one starts every game.
            match files::lock_file(&game::lock_path(&self.m.out, g))? {
                Ok(lock) => drop(lock),
                Err(_) => continue,
            }
            let child = Command::new(self.exe)
                .arg("ladder-game")
                .arg("--match")
                .arg(&self.m.out)
                .arg("--game")
                .arg(g.to_string())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .spawn()
                .map_err(|e| format!("starting {}: {e}", self.exe.display()))?;
            self.playing.push(Playing {
                g,
                child,
                len: log_len(&self.m.out, g),
                since: Instant::now(),
            });
            started.push(g);
        }
        for g in started {
            self.waiting.remove(&g);
        }
        Ok(())
    }

    /// How the process of playing game `i` exited, killing it first if it has hung; none
    /// while it plays on.
    fn look(&mut self, i: usize) -> Result<Option<ExitStatus>, String> {
        let out = &self.m.out;
        let p = &mut self.playing[i];
        let at = |e: std::io::Error| format!("{}: {e}", game::key(p.g));
        if let Some(status) = p.child.try_wait().map_err(at)? {
            return Ok(Some(status));
        }
        let len = log_len(out, p.g);
        if len != p.len {
            (p.len, p.since) = (len, Instant::now());
            return Ok(None);
        }
        if p.since.elapsed() < HANG {
            return Ok(None);
        }
        eprintln!(
            "ladder: {}: no decision in {} minutes, so it is stopped",
            game::key(p.g),
            HANG.as_secs() / 60
        );
        p.child.kill().map_err(at)?;
        Ok(Some(p.child.wait().map_err(at)?))
    }

    /// Takes the end of game `g`'s process with `status`: its record if the game finished or
    /// crashed for good, none if it is to be started again.
    fn ended(&mut self, g: u32, status: ExitStatus) -> Result<Option<GameRecord>, String> {
        let key = game::key(g);
        let code = |c: u8| status.code() == Some(i32::from(c));
        if code(game::FINISHED) {
            let path = game::result_path(&self.m.out, g);
            let at = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
            let text = fs::read_to_string(&path).map_err(|e| at(&e))?;
            return serde_json::from_str(&text).map(Some).map_err(|e| at(&e));
        }
        // An orphan of an earlier run took the lock after this one looked.
        if code(game::BUSY) {
            self.waiting.insert(g);
            return Ok(None);
        }
        if code(game::UNPLAYABLE) {
            return Err(format!("{key}: the game could not be started ({status})"));
        }
        let lines = files::read::<Line>(&game::log_path(&self.m.out, g))?.len();
        match self.died.insert(g, lines) {
            Some(before) if lines <= before => {
                eprintln!("ladder: {key} died twice at decision {lines}, so it is skipped");
                Ok(Some(game::crashed(self.m, g, GameDecision(lines))))
            }
            _ => {
                eprintln!("ladder: {key} died at decision {lines} ({status}): playing it again");
                self.waiting.insert(g);
                Ok(None)
            }
        }
    }
}

/// Game `g`'s log's length in bytes, 0 before it is written.
fn log_len(dir: &Path, g: u32) -> u64 {
    fs::metadata(game::log_path(dir, g)).map_or(0, |m| m.len())
}
