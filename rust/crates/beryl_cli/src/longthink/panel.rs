//! The live panel's frames and log lines, as a pure function of what the runner publishes,
//! the terminal's size and whether to color, so they are tested without a terminal
//! ([`super::term`] draws them). Glyphs and numbers over words:
//!
//! - a header: the run, its uptime, its games and whether a stop was asked for;
//! - the board: a row per entrant, best first, with its Elo and 95% margin, games, its chance
//!   of being the best, its mean minutes a game and `◀` on the sides of the game being
//!   played; then the last 10 results;
//! - now: the game being played and the decision being searched.

use std::collections::HashMap;
use std::fmt::Write;

use super::game::GameDecision;
use super::ratings::Ratings;
use super::schedule::{Config, Matchup};
use super::summary::{best, ranked};
use super::{GameRecord, Verdict, rung_name};

/// What a frame shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub run: String,
    /// Seconds the runner has been up; none when nothing plays (`--status`).
    pub up: Option<f64>,
    pub games: usize,
    pub crashed: usize,
    /// Whether a stop after this game was asked for.
    pub stopping: bool,
    pub rows: Vec<Row>,
    /// The last results, of the first sides, oldest first.
    pub last: Vec<Verdict>,
    pub now: Option<Now>,
}

/// A row of the board: an entrant.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub name: String,
    /// Its rating and 95% margin; none before a scored game.
    pub elo: Option<(f64, f64)>,
    pub games: u32,
    /// Its chance of being the best of all.
    pub best: f64,
    /// Mean wall seconds a game.
    pub seconds: Option<f64>,
    pub playing: bool,
}

/// The game being played.
#[derive(Clone, Debug, PartialEq)]
pub struct Now {
    pub matchup: String,
    pub pair: u32,
    /// The seat of the matchup's first side.
    pub seat: u8,
    pub turn: u16,
    /// The first side's, then the second's.
    pub life: [i16; 2],
    pub hand: [u32; 2],
    pub decision: GameDecision,
    /// When a search is deciding: the iterations it has run and its budget in steps.
    pub search: Option<(u64, u64)>,
    /// Seconds spent on the decision so far.
    pub seconds: f64,
    /// The share of this game's searched budgets the stop left unspent.
    pub saved: Option<f64>,
    /// The last searched decision's answer, as the transcript names it.
    pub prev: Option<String>,
}

/// The board's rows for these games, with the matchup being played, if any: every entrant
/// played, best first, then those open but not yet played.
pub fn board(
    c: &Config,
    games: &[GameRecord],
    seconds: &HashMap<String, f64>,
    playing: Option<&Matchup>,
) -> Vec<Row> {
    let r = Ratings::new(c, games, seconds, &[]);
    let all = ranked(c, &r);
    let best = best(&r, &all, games.len());
    all.iter()
        .zip(best)
        .map(|(e, overall)| {
            let t = r.tally(e);
            let (m, s) = r.get(e);
            Row {
                name: e.clone(),
                elo: (t.scored() > 0 || *e == c.anchor).then_some((m, 1.96 * s)),
                games: t.games,
                best: overall,
                seconds: t.mean_seconds(),
                playing: playing.is_some_and(|p| p.a == *e || p.b == *e),
            }
        })
        .collect()
}

/// SGR codes.
const GREEN: &str = "32";
const RED: &str = "31";
const DIM: &str = "2";
const BOLD: &str = "1";

fn paint(color: bool, code: &str, text: &str) -> String {
    if color {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_owned()
    }
}

/// The widest a separator gets.
const RULE: usize = 64;

/// The frame's lines, each cut to `width` columns, at most `height - 1` of them so the
/// cursor's line stays on screen: a terminal too short drops the board's lowest rows first,
/// never those being played.
pub fn render(f: &Frame, width: usize, height: usize, color: bool) -> Vec<String> {
    let rule = "─".repeat(width.min(RULE));
    let mut head = vec![rule.clone(), header(f, color)];
    let (rows, columns) = rows(f, color);
    let mut tail = Vec::new();
    if !f.last.is_empty() {
        let dots: String = f.last.iter().map(|&v| dot(v, color)).collect();
        tail.push(format!("last {}  {dots}", f.last.len()));
    }
    if let Some(now) = &f.now {
        tail.push(rule);
        tail.extend(now_lines(now, color));
    }
    let max = height.saturating_sub(1);
    // A short terminal drops the lowest rows first, never those being played.
    let mut keep = vec![true; rows.len()];
    let mut order: Vec<usize> = (0..rows.len()).filter(|&i| !f.rows[i].playing).collect();
    while head.len() + 1 + keep.iter().filter(|&&k| k).count() + tail.len() > max {
        match order.pop() {
            Some(i) => keep[i] = false,
            None => break,
        }
    }
    head.push(columns);
    head.extend(
        rows.into_iter()
            .zip(keep)
            .filter_map(|(r, k)| k.then_some(r)),
    );
    head.extend(tail);
    let cut = head.len().saturating_sub(max);
    head.split_off(cut).iter().map(|l| fit(l, width)).collect()
}

fn header(f: &Frame, color: bool) -> String {
    let mut parts = vec![format!("longthink {}", f.run)];
    if let Some(up) = f.up {
        parts.push(format!("up {}", uptime(up)));
    }
    parts.push(match f.games {
        1 => "1 game".to_owned(),
        n => format!("{n} games"),
    });
    if f.crashed > 0 {
        parts.push(paint(color, RED, &format!("{} crashed", f.crashed)));
    }
    if f.stopping {
        parts.push(paint(color, BOLD, "⏸ after this game"));
    }
    parts.join("   ")
}

/// The board's rows, and the line naming its columns.
fn rows(f: &Frame, color: bool) -> (Vec<String>, String) {
    let nw = f
        .rows
        .iter()
        .map(|r| r.name.chars().count())
        .max()
        .unwrap_or(0);
    let columns = format!(
        "{:nw$}  {:>5} {:>4}  {:>5}  {:<6} {:>5}",
        "", "elo", "95%", "games", "best", "min"
    );
    let rows = f
        .rows
        .iter()
        .map(|r| {
            let (elo, margin) = r.elo.map_or((String::new(), String::new()), |(m, s)| {
                (format!("{m:+.0}"), format!("±{s:.0}"))
            });
            let line = format!(
                "{:nw$}  {elo:>5} {margin:>4}  {:>5}  {:<6} {:>5}{}",
                r.name,
                r.games,
                bar(r.best, 6),
                r.seconds.map_or(String::new(), clock),
                if r.playing { "  ◀" } else { "" },
            );
            let line = line.trim_end().to_owned();
            if r.games > 0 {
                line
            } else {
                paint(color, DIM, &line)
            }
        })
        .collect();
    (rows, columns.trim_end().to_owned())
}

fn now_lines(n: &Now, color: bool) -> Vec<String> {
    let seat = if n.seat == 0 { 'A' } else { 'B' };
    let mut lines = vec![format!(
        "▶ {}  s{}·{seat}  T{}  life {} · {}  hand {} · {}",
        n.matchup, n.pair, n.turn, n.life[0], n.life[1], n.hand[0], n.hand[1]
    )];
    let mut search = format!("  decision {}", n.decision);
    if let Some((iterations, budget)) = n.search {
        let _ = write!(
            search,
            "  {} it / {} steps  {:.1}s",
            paint(color, GREEN, &thousands(iterations)),
            rung_name(budget),
            n.seconds
        );
    }
    if let Some(saved) = n.saved {
        let _ = write!(search, "  stop −{:.0}%", 100.0 * saved);
    }
    lines.push(search);
    if let Some(prev) = &n.prev {
        lines.push(format!("  prev  {prev}"));
    }
    lines
}

/// A line for the log: a finished game, from its first side's view.
pub fn game_line(g: &GameRecord, seconds: Option<f64>, color: bool) -> String {
    let (glyph, word, code) = match g.outcome {
        Verdict::Win => ("✓", "won", GREEN),
        Verdict::Loss => ("✗", "lost", RED),
        Verdict::Draw => ("½", "drew", DIM),
        Verdict::Stalled => ("½", "stalled", DIM),
        Verdict::Crashed => ("!", "crashed", RED),
    };
    let seat = if g.seat == 0 { 'A' } else { 'B' };
    let mut line = format!(
        "{} {}  s{}·{seat}  {word:<4}",
        paint(color, code, glyph),
        g.matchup().label(),
        g.pair,
    );
    match g.crashed_at {
        Some(at) => {
            let _ = write!(line, " at decision {at}");
        }
        None => {
            let _ = write!(line, " T{:<2}", g.turns);
        }
    }
    if let Some(s) = seconds {
        let _ = write!(line, "  {}", clock(s));
    }
    line
}

/// A result as a dot: won, lost, or half.
fn dot(v: Verdict, color: bool) -> String {
    match v {
        Verdict::Win => paint(color, GREEN, "●"),
        Verdict::Loss => paint(color, RED, "○"),
        Verdict::Draw | Verdict::Stalled => paint(color, DIM, "◐"),
        Verdict::Crashed => paint(color, RED, "!"),
    }
}

/// A bar `cells` wide filled to `share`, in eighths of a cell.
fn bar(share: f64, cells: usize) -> String {
    const PARTS: [char; 8] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉'];
    let eighths = (share.clamp(0.0, 1.0) * (cells * 8) as f64).round() as usize;
    let mut s = "█".repeat(eighths / 8);
    if !eighths.is_multiple_of(8) {
        s.push(PARTS[eighths % 8]);
    }
    s
}

/// Seconds as minutes and seconds: 6:51.
fn clock(seconds: f64) -> String {
    let s = seconds.round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// An uptime, short: 45s, 12m, 6h12m, 2d06h.
fn uptime(seconds: f64) -> String {
    let s = seconds as u64;
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m", s / 60),
        3600..86_400 => format!("{}h{:02}m", s / 3600, s % 3600 / 60),
        _ => format!("{}d{:02}h", s / 86_400, s % 86_400 / 3600),
    }
}

/// A count with thousands separators: 58,120.
fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, d) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(d);
    }
    out
}

/// `line` cut to `width` visible columns, its color escapes not counted; a cut colored line
/// ends its colors. Every glyph here takes one column.
pub fn fit(line: &str, width: usize) -> String {
    let mut out = String::new();
    let mut seen = 0;
    let mut chars = line.chars().peekable();
    let mut escaped = false;
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            escaped = true;
            out.push(ch);
            for e in chars.by_ref() {
                out.push(e);
                if e == 'm' {
                    break;
                }
            }
            continue;
        }
        if seen == width {
            if escaped {
                out.push_str("\x1b[0m");
            }
            return out;
        }
        seen += 1;
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::longthink::Verdict::{Crashed, Draw, Loss, Win};

    fn config() -> Config {
        Config {
            families: vec!["ismcts2/hidden/rollout/tuned".to_owned()],
            fixed: vec!["expert".to_owned()],
            anchor: "expert".to_owned(),
            budgets: vec![1_000, 3_000],
            threads: 1,
            seed: 0,
        }
    }

    const LOW: &str = "ismcts2/hidden/rollout/tuned@1k";
    const HIGH: &str = "ismcts2/hidden/rollout/tuned@3k";

    /// `n` games of the expert against `e`, the expert winning `wins`, each `seconds` long.
    fn games(
        out: &mut Vec<GameRecord>,
        clock: &mut HashMap<String, f64>,
        e: &str,
        n: u32,
        wins: u32,
        seconds: f64,
    ) {
        let m = Matchup::new("expert", e);
        for g in 0..n {
            let r = GameRecord::example(&m, g, if g < wins { Win } else { Loss });
            clock.insert(r.game.clone(), seconds);
            out.push(r);
        }
    }

    fn mid_run() -> Frame {
        let c = config();
        let (mut g, mut s) = (Vec::new(), HashMap::new());
        games(&mut g, &mut s, LOW, 40, 30, 21.0);
        games(&mut g, &mut s, HIGH, 20, 6, 65.0);
        let playing = Matchup::new("expert", HIGH);
        let rows = board(&c, &g, &s, Some(&playing));
        Frame {
            run: "run-001".to_owned(),
            up: Some(6.0 * 3600.0 + 12.0 * 60.0 + 5.0),
            games: g.len(),
            crashed: 0,
            stopping: true,
            rows,
            last: vec![Win, Win, Loss, Win, Win, Win, Loss, Win, Win, Draw],
            now: Some(Now {
                matchup: playing.label(),
                pair: 10,
                seat: 1,
                turn: 9,
                life: [14, 9],
                hand: [4, 3],
                decision: GameDecision(64),
                search: Some((58_120, 3_000)),
                seconds: 3.1,
                saved: Some(0.31),
                prev: Some("cast Counterspell".to_owned()),
            }),
        }
    }

    fn text(lines: &[String]) -> String {
        lines.join("\n") + "\n"
    }

    #[test]
    fn a_frame_mid_run() {
        let frame = text(&render(&mid_run(), 100, 40, false));
        let expected = "\
────────────────────────────────────────────────────────────────
longthink run-001   up 6h12m   60 games   ⏸ after this game
                                   elo  95%  games  best     min
ismcts2/hidden/rollout/tuned@3k   +141 ±161     20  █████▊  1:05  ◀
expert                              +0   ±0     60  ▎       0:36  ◀
ismcts2/hidden/rollout/tuned@1k   -186 ±122     40          0:21
ismcts2/hidden/rollout/tuned@9k                  0
last 10  ●●○●●●○●●◐
────────────────────────────────────────────────────────────────
▶ expert v ismcts2/hidden/rollout/tuned@3k  s10·B  T9  life 14 · 9  hand 4 · 3
  decision 64  58,120 it / 3k steps  3.1s  stop −31%
  prev  cast Counterspell
";
        assert_eq!(frame, expected);
    }

    /// Colors mark the results, and nothing else changes.
    #[test]
    fn colors_change_nothing_but_colors() {
        let colored = text(&render(&mid_run(), 100, 40, true));
        assert!(colored.contains("\x1b[32m●\x1b[0m") && colored.contains("\x1b[31m○\x1b[0m"));
        let mut plain = String::new();
        let mut chars = colored.chars();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                chars.by_ref().find(|&e| e == 'm');
            } else {
                plain.push(ch);
            }
        }
        assert_eq!(plain, text(&render(&mid_run(), 100, 40, false)));
    }

    /// A short terminal drops the lowest rows first, never one being played; the panel
    /// never takes the cursor's line.
    #[test]
    fn a_short_terminal() {
        let short = render(&mid_run(), 100, 11, false);
        assert_eq!(short.len(), 10);
        assert!(!short.iter().any(|l| l.starts_with(LOW)), "{short:#?}");
        assert!(short.iter().any(|l| l.starts_with(HIGH)), "{short:#?}");
        assert_eq!(render(&mid_run(), 100, 4, false).len(), 3);
    }

    /// Every line is cut to the width; colored lines end their colors.
    #[test]
    fn a_narrow_terminal() {
        for line in render(&mid_run(), 30, 40, true) {
            if line.contains('\x1b') && fit(&line, 30) != line {
                assert!(line.ends_with("\x1b[0m"));
            }
        }
        assert_eq!(fit("\x1b[32mabcdef\x1b[0m", 3), "\x1b[32mabc\x1b[0m");
    }

    #[test]
    fn log_lines() {
        let m = Matchup::new("expert", HIGH);
        let mut g = GameRecord::example(&m, 1, Loss);
        assert_eq!(
            game_line(&g, Some(411.0), false),
            format!("✗ expert v {HIGH}  s0·B  lost T20  6:51")
        );
        g.outcome = Crashed;
        g.crashed_at = Some(GameDecision(9));
        assert_eq!(
            game_line(&g, None, true),
            format!("\x1b[31m!\x1b[0m expert v {HIGH}  s0·B  crashed at decision 9")
        );
    }

    #[test]
    fn numbers_read_short() {
        assert_eq!(
            [0, 999, 1_000, 58_120, 1_000_000].map(thousands),
            ["0", "999", "1,000", "58,120", "1,000,000"]
        );
        assert_eq!(
            [5.0, 754.0, 22_325.0, 200_000.0].map(uptime),
            ["5s", "12m", "6h12m", "2d07h"]
        );
        assert_eq!(
            [0.0, 0.5, 1.0 / 48.0, 1.0].map(|s| bar(s, 6)),
            ["", "███", "▏", "██████"]
        );
        assert_eq!(clock(411.4), "6:51");
    }
}
