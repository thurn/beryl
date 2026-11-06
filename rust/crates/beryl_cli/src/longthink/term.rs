//! The runner's terminal: a log scrolling above the panel ([`super::panel`]), redrawn in place
//! at 4 Hz by a thread of its own from a board the runner publishes to, so the search never
//! waits on the terminal. The runner only ever locks the board, briefly; the painter reads it
//! and writes outside it.
//!
//! - Drawing is plain ANSI: up with `ESC[nA`, each line cleared with `ESC[2K`, the cursor
//!   hidden while the panel is up and shown again whenever the runner exits, leaving the
//!   panel's last frame on screen.
//! - Ctrl-C once asks for a stop after this game (the header shows ⏸); twice stops at once,
//!   losing the game in progress, which the next run replays to where it was.
//! - Not a terminal, or `TERM=dumb`: no escapes at all, just the log lines and a ladder table
//!   every 10 games. `NO_COLOR` turns colors off and keeps the panel.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{self, IsTerminal, Stdout, Write};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use beryl_engine::{Decision, State, Zone};

use super::game::Line;
use super::panel::{self, Frame, Now};
use super::schedule::{Config, Matchup};
use super::{GameRecord, Spec, Verdict, matchup_label};

/// Shows the cursor.
pub const SHOW: &str = "\x1b[?25h";
const HIDE: &str = "\x1b[?25l";

/// The exit code of a runner stopped at once by a second Ctrl-C.
pub const STOPPED: i32 = 130;

/// How often the panel is redrawn.
const TICK: Duration = Duration::from_millis(250);

/// Whether a panel is up, for the panic hook.
static LIVE: AtomicBool = AtomicBool::new(false);

/// A log above a panel redrawn in place, on `W`.
pub struct Screen<W: Write> {
    out: W,
    /// The panel's lines on screen, which the cursor sits just below.
    drawn: usize,
}

impl<W: Write> Screen<W> {
    /// Takes over `out`, hiding its cursor.
    pub fn new(mut out: W) -> io::Result<Screen<W>> {
        out.write_all(HIDE.as_bytes())?;
        Ok(Screen { out, drawn: 0 })
    }

    /// Prints `logs` where the panel was, for good, and `frame` below them as the new panel.
    /// Lines must fit the terminal's width: the cursor's moves count one row a line.
    pub fn draw(&mut self, logs: &[String], frame: &[String]) -> io::Result<()> {
        let mut s = String::new();
        if self.drawn > 0 {
            let _ = write!(s, "\r\x1b[{}A", self.drawn);
        }
        for line in logs.iter().chain(frame) {
            s.push_str("\x1b[2K");
            s.push_str(line);
            s.push('\n');
        }
        // Clear what is left of a longer panel, and come back; then the cursor's line, where
        // the terminal echoes a Ctrl-C.
        let extra = self.drawn.saturating_sub(logs.len() + frame.len());
        if extra > 0 {
            s.push_str(&"\x1b[2K\n".repeat(extra));
            let _ = write!(s, "\x1b[{extra}A");
        }
        s.push_str("\x1b[2K");
        self.drawn = frame.len();
        self.out.write_all(s.as_bytes())?;
        self.out.flush()
    }

    /// Shows the cursor again, below the last frame.
    pub fn close(mut self) -> io::Result<W> {
        self.out.write_all(SHOW.as_bytes())?;
        self.out.flush()?;
        Ok(self.out)
    }
}

/// Shows the cursor if a panel hid it: for a panic, which under the search profile aborts
/// before anything else could.
pub fn restore() {
    if LIVE.load(Ordering::Relaxed) {
        let mut out = io::stdout();
        let _ = out.write_all(SHOW.as_bytes());
        let _ = out.flush();
    }
}

/// What the panel shows, as the runner publishes it.
pub struct Board {
    run: String,
    config: Config,
    games: Vec<GameRecord>,
    seconds: HashMap<String, f64>,
    started: Instant,
    playing: Option<Playing>,
    stopping: bool,
    /// Log lines not yet printed.
    logs: Vec<String>,
}

/// The game being played.
struct Playing {
    matchup: Matchup,
    label: String,
    pair: u32,
    /// The seat of the rung's agent.
    seat: u8,
    /// Each seat's search budget, if it searches.
    budgets: [Option<u32>; 2],
    asked: Option<Asked>,
    /// This game's searched budgets, and what the exact stop left of them.
    planned: u64,
    unspent: u64,
    prev: Option<String>,
}

/// The decision being asked.
struct Asked {
    decision: usize,
    turn: u16,
    /// The rung's agent's, then its opponent's.
    life: [i16; 2],
    hand: [u32; 2],
    budget: Option<u32>,
    /// When it was asked, and the process's rollouts then.
    at: Instant,
    rollouts: u64,
}

impl Board {
    fn frame(&self, at: Instant, rollouts: u64) -> Frame {
        let (rows, climb) = panel::ladder(
            &self.config,
            &self.games,
            &self.seconds,
            self.playing.as_ref().map(|p| p.matchup),
        );
        let now = self.playing.as_ref().and_then(|p| {
            let a = p.asked.as_ref()?;
            Some(Now {
                matchup: p.label.clone(),
                pair: p.pair,
                seat: p.seat,
                turn: a.turn,
                life: a.life,
                hand: a.hand,
                decision: a.decision,
                search: a.budget.map(|b| (rollouts.saturating_sub(a.rollouts), b)),
                seconds: at.saturating_duration_since(a.at).as_secs_f64(),
                saved: (p.planned > 0).then(|| p.unspent as f64 / p.planned as f64),
                prev: p.prev.clone(),
            })
        });
        status_frame(
            self,
            Some(at.duration_since(self.started).as_secs_f64()),
            rows,
            climb,
            now,
        )
    }
}

fn status_frame(
    b: &Board,
    up: Option<f64>,
    rows: Vec<panel::Row>,
    climb: Option<(f64, f64)>,
    now: Option<Now>,
) -> Frame {
    let tail = b.games.len().saturating_sub(10);
    Frame {
        run: b.run.clone(),
        up,
        games: b.games.len(),
        crashed: b
            .games
            .iter()
            .filter(|g| g.outcome == Verdict::Crashed)
            .count(),
        stopping: b.stopping,
        rows,
        climb,
        last: b.games[tail..].iter().map(|g| g.outcome).collect(),
        now,
    }
}

/// The terminal's columns and rows, else a default.
fn size() -> (usize, usize) {
    terminal_size::terminal_size().map_or((80, 24), |(w, h)| (usize::from(w.0), usize::from(h.0)))
}

/// Whether stdout takes the panel, and whether in color.
fn modes() -> (bool, bool) {
    let live = io::stdout().is_terminal() && std::env::var("TERM").is_ok_and(|t| t != "dumb");
    let color = std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty());
    (live, color)
}

/// `--status`: the header and the ladder, once.
pub fn status(run: &str, config: Config, games: Vec<GameRecord>, seconds: HashMap<String, f64>) {
    let b = Board {
        run: run.to_owned(),
        config,
        games,
        seconds,
        started: Instant::now(),
        playing: None,
        stopping: false,
        logs: Vec::new(),
    };
    let (rows, climb) = panel::ladder(&b.config, &b.games, &b.seconds, None);
    let frame = status_frame(&b, None, rows, climb, None);
    let (live, color) = modes();
    let (width, _) = if live { size() } else { (usize::MAX, 0) };
    for line in panel::render(&frame, width, usize::MAX, live && color) {
        println!("{line}");
    }
}

/// What the painter and the Ctrl-C handler share.
struct Shared {
    board: Mutex<Board>,
    /// The panel, until the runner exits; locked before the board where both are.
    screen: Mutex<Option<Screen<Stdout>>>,
    live: bool,
    color: bool,
    done: AtomicBool,
}

impl Shared {
    fn board(&self) -> MutexGuard<'_, Board> {
        self.board
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn screen(&self) -> MutexGuard<'_, Option<Screen<Stdout>>> {
        self.screen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Prints the waiting log lines and redraws the panel.
    fn paint(&self, screen: &mut Option<Screen<Stdout>>) {
        let Some(s) = screen.as_mut() else { return };
        let (width, height) = size();
        let (logs, frame) = {
            let mut b = self.board();
            let frame = b.frame(Instant::now(), beryl_search::rollouts_run());
            (std::mem::take(&mut b.logs), frame)
        };
        let logs: Vec<String> = logs.iter().map(|l| panel::fit(l, width)).collect();
        let _ = s.draw(&logs, &panel::render(&frame, width, height, self.color));
    }

    /// The last frame, and the cursor back.
    fn close(&self) {
        let mut screen = self.screen();
        self.paint(&mut screen);
        if let Some(s) = screen.take() {
            let _ = s.close();
        }
        LIVE.store(false, Ordering::Relaxed);
    }
}

/// Ctrl-C presses so far.
static INTERRUPTS: AtomicU32 = AtomicU32::new(0);

/// The runner's terminal: the panel on a terminal, else plain log lines.
pub struct Display {
    shared: Arc<Shared>,
    painter: Option<JoinHandle<()>>,
}

impl Display {
    /// Shows run `run` with its games so far, and takes Ctrl-C.
    pub fn start(
        run: &str,
        config: &Config,
        games: &[GameRecord],
        seconds: HashMap<String, f64>,
    ) -> Result<Display, String> {
        let (live, color) = modes();
        let screen = if live {
            Some(Screen::new(io::stdout()).map_err(|e| format!("the terminal: {e}"))?)
        } else {
            None
        };
        LIVE.store(live, Ordering::Relaxed);
        let shared = Arc::new(Shared {
            board: Mutex::new(Board {
                run: run.to_owned(),
                config: config.clone(),
                games: games.to_vec(),
                seconds,
                started: Instant::now(),
                playing: None,
                stopping: false,
                logs: Vec::new(),
            }),
            screen: Mutex::new(screen),
            live,
            color: live && color,
            done: AtomicBool::new(false),
        });
        let handler = Arc::clone(&shared);
        ctrlc::set_handler(move || interrupted(&handler))
            .map_err(|e| format!("taking Ctrl-C: {e}"))?;
        let painter = live.then(|| {
            let shared = Arc::clone(&shared);
            thread::spawn(move || {
                while !shared.done.load(Ordering::Relaxed) {
                    shared.paint(&mut shared.screen());
                    thread::park_timeout(TICK);
                }
            })
        });
        Ok(Display { shared, painter })
    }

    /// Whether a stop after this game was asked for.
    pub fn stopping(&self) -> bool {
        self.shared.board().stopping
    }

    /// Prints `line` to the log.
    pub fn log(&self, line: String) {
        if self.shared.live {
            self.shared.board().logs.push(line);
        } else {
            println!("{line}");
        }
    }

    /// Game `spec` begins, with `logged` decisions already made.
    pub fn begin(&self, spec: &Spec, logged: usize) {
        let mut b = self.shared.board();
        let matchup = Matchup {
            rung: spec.rung,
            against: spec.against,
        };
        let label = matchup_label(&b.config, spec.rung, spec.against);
        let seat = if spec.seat == 0 { 'A' } else { 'B' };
        let mut lines = Vec::new();
        if !b
            .games
            .iter()
            .any(|g| (g.rung, g.against) == (spec.rung, spec.against))
            && logged == 0
        {
            lines.push(format!("▸ {label} opens"));
        }
        if logged > 0 {
            lines.push(format!(
                "↻ resumed {label}  s{}·{seat} at decision {logged}",
                spec.pair
            ));
        }
        b.playing = Some(Playing {
            matchup,
            label,
            pair: spec.pair,
            seat: spec.seat,
            budgets: [0, 1].map(|s| spec.entrants[s].budget.map(|b| b.iterations)),
            asked: None,
            planned: 0,
            unspent: 0,
            prev: None,
        });
        drop(b);
        for line in lines {
            self.log(line);
        }
    }

    /// Decision `decision`, `d`, is about to be asked in position `s`.
    pub fn asking(&self, decision: usize, s: &State, d: &Decision) {
        let rollouts = beryl_search::rollouts_run();
        let mut b = self.shared.board();
        let Some(p) = b.playing.as_mut() else { return };
        let you = beryl_engine::Seat::new(p.seat);
        let seats = [you, you.opponent()];
        p.asked = Some(Asked {
            decision,
            turn: s.turn().number,
            life: seats.map(|q| s.player(q).life),
            hand: seats.map(|q| s.zone(q, Zone::Hand).count()),
            budget: p.budgets[d.player.index()],
            at: Instant::now(),
            rollouts,
        });
    }

    /// A decision was answered with `line`, which the transcript calls `label`.
    pub fn answered(&self, line: &Line, label: &str) {
        let mut b = self.shared.board();
        let Some(p) = b.playing.as_mut() else { return };
        if let (Some(s), Some(budget)) = (&line.search, p.budgets[usize::from(line.seat)]) {
            p.planned += u64::from(budget);
            p.unspent += u64::from(budget.saturating_sub(s.rollouts + s.inherited));
            p.prev = Some(label.to_owned());
        }
    }

    /// Game `g` is committed, after `seconds` of play if it was played.
    pub fn finished(&self, g: &GameRecord, seconds: Option<f64>) {
        let mut b = self.shared.board();
        let line = panel::game_line(&b.config, g, seconds, self.shared.color);
        if let Some(s) = seconds {
            b.seconds.insert(g.game.clone(), s);
        }
        b.games.push(g.clone());
        b.playing = None;
        let target = 2 * b.config.pairs;
        let done = b
            .games
            .iter()
            .filter(|x| (x.rung, x.against) == (g.rung, g.against))
            .count()
            == target as usize;
        let label = matchup_label(&b.config, g.rung, g.against);
        // Plain output has no panel: a table every 10 games stands for it.
        let table = (!self.shared.live && b.games.len().is_multiple_of(10)).then(|| {
            let (rows, climb) = panel::ladder(&b.config, &b.games, &b.seconds, None);
            panel::render(
                &status_frame(&b, None, rows, climb, None),
                usize::MAX,
                usize::MAX,
                false,
            )
        });
        drop(b);
        self.log(line);
        if done {
            self.log(format!("■ {label} {target}/{target}"));
        }
        for line in table.into_iter().flatten() {
            self.log(line);
        }
    }

    /// Leaves the panel's last frame on screen and the cursor shown.
    pub fn close(mut self) {
        self.shut();
    }

    fn shut(&mut self) {
        self.shared.done.store(true, Ordering::Relaxed);
        if let Some(painter) = self.painter.take() {
            painter.thread().unpark();
            let _ = painter.join();
        }
        self.shared.close();
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        self.shut();
    }
}

/// Ctrl-C: once, a stop after this game; twice, a stop now.
fn interrupted(shared: &Shared) {
    if INTERRUPTS.fetch_add(1, Ordering::Relaxed) == 0 {
        shared.board().stopping = true;
        if shared.live {
            shared.paint(&mut shared.screen());
        } else {
            println!("⏸ stopping after this game");
        }
    } else {
        shared.close();
        std::process::exit(STOPPED);
    }
}

#[cfg(test)]
#[path = "../../tests/ansi/mod.rs"]
mod ansi;

#[cfg(test)]
mod tests {
    use super::ansi::Term;
    use super::*;
    use crate::longthink::schedule::Against;

    fn game(rung: usize, n: u32, outcome: Verdict) -> GameRecord {
        GameRecord {
            game: format!("{rung}-{n}"),
            rung,
            iterations: 0,
            against: Against::Opponent,
            pair: n / 2,
            seed: 0,
            seat: (n % 2) as u8,
            outcome,
            turns: 12,
            decisions: 200,
            searched: 90,
            rollouts: 0,
            budget: 0,
            crashed_at: None,
        }
    }

    /// Frames that grow and shrink, with log lines between some of them, leave on screen
    /// every log line in order and the last frame below them, nothing else, the cursor on
    /// the line after it and shown again: at two widths, one cutting the lines.
    #[test]
    fn redraws_leave_the_log_and_the_last_frame() {
        let c = Config {
            rungs: vec![3_000, 10_000],
            opponent: "expert".to_owned(),
            ..Config::default()
        };
        for width in [40, 100] {
            let mut b = Board {
                run: "run-001".to_owned(),
                config: c.clone(),
                games: Vec::new(),
                seconds: HashMap::new(),
                started: Instant::now(),
                playing: None,
                stopping: false,
                logs: Vec::new(),
            };
            let mut screen = Screen::new(Vec::new()).unwrap();
            let mut expected = Vec::new();
            for k in 0..13u32 {
                if k % 3 == 0 {
                    b.playing = Some(Playing {
                        matchup: Matchup {
                            rung: (k / 6) as usize,
                            against: Against::Opponent,
                        },
                        label: "3k v expert".to_owned(),
                        pair: k / 2,
                        seat: 0,
                        budgets: [Some(3_000), None],
                        asked: Some(Asked {
                            decision: k as usize,
                            turn: 3,
                            life: [20, 18],
                            hand: [5, 6],
                            budget: Some(3_000),
                            at: b.started,
                            rollouts: 0,
                        }),
                        planned: 0,
                        unspent: 0,
                        prev: (k % 2 == 0).then(|| "cast Counterspell".to_owned()),
                    });
                } else if k % 3 == 2 {
                    b.playing = None;
                }
                let mut logs = Vec::new();
                if k % 4 == 1 {
                    let g = game((k / 6) as usize, k, Verdict::Win);
                    logs.push(panel::fit(
                        &panel::game_line(&c, &g, Some(61.0), true),
                        width,
                    ));
                    b.games.push(g);
                }
                if k == 7 {
                    b.stopping = true;
                    logs.push(panel::fit(
                        "a log line long enough to be cut at forty columns, at least",
                        width,
                    ));
                }
                let frame = panel::render(&b.frame(b.started, 1_234), width, 30, true);
                screen.draw(&logs, &frame).unwrap();
                expected.extend(logs);
            }
            // The run's end: its last game logged, and the panel without a game in it.
            let g = game(2, 13, Verdict::Loss);
            let logs = vec![panel::fit(&panel::game_line(&c, &g, None, true), width)];
            b.games.push(g);
            b.playing = None;
            let frame = panel::render(&b.frame(b.started, 1_234), width, 30, true);
            screen.draw(&logs, &frame).unwrap();
            expected.extend(logs);
            let bytes = screen.close().unwrap();
            let t = Term::replay(&bytes, width);
            let plain = |l: &String| Term::replay(l.as_bytes(), width).lines().concat();
            expected.extend(frame);
            let expected: Vec<String> = expected.iter().map(plain).collect();
            assert_eq!(t.lines(), expected);
            assert_eq!(t.row, expected.len());
            assert!(t.cursor_shown);
            assert!(
                expected.last().unwrap().starts_with("Elo/×3"),
                "{expected:?}"
            );
            assert!(expected.iter().any(|l| l.contains('⏸')));
        }
    }
}
