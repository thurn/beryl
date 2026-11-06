//! The live panel's frames and log lines, as a pure function of what the runner publishes,
//! the terminal's size and whether to color, so they are tested without a terminal
//! ([`super::term`] draws them). Glyphs and numbers over words:
//!
//! - a header: the run, its uptime, its games and whether a stop was asked for;
//! - the ladder: a row per rung and who it plays, with a bar of its score, the Wilson 95%
//!   interval, its games of the target, its mean minutes a game and `◀` on the row being
//!   played; then Elo per tripling of the budget and the last 10 results;
//! - now: the game being played and the decision being searched.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;

use super::schedule::{Against, Config, Matchup};
use super::summary::elo;
use super::{GameRecord, Verdict, matchup_label, rung_name};
use crate::tournament::wilson;

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
    /// Elo per tripling of the budget against the opponent, and its 95% margin.
    pub climb: Option<(f64, f64)>,
    /// The last results, oldest first.
    pub last: Vec<Verdict>,
    pub now: Option<Now>,
}

/// A ladder row: a rung and who it plays.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub rung: usize,
    pub name: String,
    pub vs: String,
    /// The rung's share of the points and its Wilson 95% interval; none before a scored game.
    pub score: Option<(f64, [f64; 2])>,
    pub games: u32,
    pub target: u32,
    /// Mean wall seconds a game.
    pub seconds: Option<f64>,
    pub playing: bool,
    pub opened: bool,
}

/// The game being played, from the side of the rung's agent ("you").
#[derive(Clone, Debug, PartialEq)]
pub struct Now {
    pub matchup: String,
    pub pair: u32,
    pub seat: u8,
    pub turn: u16,
    pub life: [i16; 2],
    pub hand: [u32; 2],
    pub decision: usize,
    /// When a search is deciding: the rollouts it has run and its budget.
    pub search: Option<(u64, u32)>,
    /// Seconds spent on the decision so far.
    pub seconds: f64,
    /// The share of this game's searched budgets the exact stop left unspent.
    pub saved: Option<f64>,
    /// The last searched decision's answer, as the transcript names it.
    pub prev: Option<String>,
}

/// The ladder's rows for these games, with the matchup being played, if any: every matchup
/// begun, then the next rung up, not yet open. Also Elo per tripling.
pub fn ladder(
    c: &Config,
    games: &[GameRecord],
    seconds: &HashMap<String, f64>,
    playing: Option<Matchup>,
) -> (Vec<Row>, Option<(f64, f64)>) {
    #[derive(Default)]
    struct Tally {
        games: u32,
        scored: u32,
        score: f64,
        clocked: u32,
        seconds: f64,
    }
    let mut tallies: BTreeMap<Matchup, Tally> = BTreeMap::new();
    for g in games {
        let t = tallies
            .entry(Matchup {
                rung: g.rung,
                against: g.against,
            })
            .or_default();
        t.games += 1;
        let points = match g.outcome {
            Verdict::Win => Some(1.0),
            Verdict::Draw | Verdict::Stalled => Some(0.5),
            Verdict::Loss => Some(0.0),
            Verdict::Crashed => None,
        };
        if let Some(p) = points {
            t.scored += 1;
            t.score += p;
        }
        if let Some(s) = seconds.get(&g.game) {
            t.clocked += 1;
            t.seconds += s;
        }
    }
    if let Some(m) = playing {
        tallies.entry(m).or_default();
    }
    let next = tallies.keys().map(|m| m.rung + 1).max().unwrap_or(0);
    let next = Matchup {
        rung: next,
        against: Against::Opponent,
    };
    let target = 2 * c.pairs;
    let mut rows: Vec<Row> = tallies
        .iter()
        .map(|(&m, t)| {
            let (name, vs) = names(c, m);
            Row {
                rung: m.rung,
                name,
                vs,
                score: (t.scored > 0)
                    .then(|| (t.score / f64::from(t.scored), wilson(t.score, t.scored))),
                games: t.games,
                target,
                seconds: (t.clocked > 0).then(|| t.seconds / f64::from(t.clocked)),
                playing: playing == Some(m),
                opened: true,
            }
        })
        .collect();
    let (name, vs) = names(c, next);
    rows.push(Row {
        rung: next.rung,
        name,
        vs,
        score: None,
        games: 0,
        target,
        seconds: None,
        playing: false,
        opened: false,
    });
    let points: Vec<(f64, f64, f64)> = tallies
        .iter()
        .filter(|(m, t)| m.against == Against::Opponent && t.scored > 0)
        .map(|(m, t)| {
            let n = f64::from(t.scored);
            let p = (t.score / n).clamp(0.5 / n, 1.0 - 0.5 / n);
            // The delta method: Elo is 400 log10(p / (1 - p)), p's variance p(1 - p) / n.
            let se = 400.0 / (std::f64::consts::LN_10 * (n * p * (1.0 - p)).sqrt());
            (
                f64::from(c.iterations(m.rung)).log(3.0),
                elo(t.score, t.scored),
                se,
            )
        })
        .collect();
    (rows, climb(&points))
}

/// A matchup's row names: the rung, the omniscient search marked, and who it plays.
fn names(c: &Config, m: Matchup) -> (String, String) {
    let r = rung_name(c.iterations(m.rung));
    match m.against {
        Against::Opponent => (r, c.opponent.clone()),
        Against::Omniscient => (format!("{r} omni"), c.opponent.clone()),
        Against::Below => (r, rung_name(c.iterations(m.rung - 1))),
    }
}

/// The weighted least-squares slope of Elo against triplings of the budget, and its 95%
/// margin, from (triplings, Elo, standard error) points; none from fewer than two budgets.
fn climb(points: &[(f64, f64, f64)]) -> Option<(f64, f64)> {
    let w: Vec<f64> = points.iter().map(|&(_, _, se)| 1.0 / (se * se)).collect();
    let total: f64 = w.iter().sum();
    let mean = |f: fn(&(f64, f64, f64)) -> f64| {
        points.iter().zip(&w).map(|(p, w)| w * f(p)).sum::<f64>() / total
    };
    let (x, y) = (mean(|p| p.0), mean(|p| p.1));
    let sxx: f64 = points
        .iter()
        .zip(&w)
        .map(|(p, w)| w * (p.0 - x).powi(2))
        .sum();
    if points.len() < 2 || sxx <= 0.0 {
        return None;
    }
    let sxy: f64 = points
        .iter()
        .zip(&w)
        .map(|(p, w)| w * (p.0 - x) * (p.1 - y))
        .sum();
    Some((sxy / sxx, 1.96 / sxx.sqrt()))
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
/// cursor's line stays on screen: a terminal too short drops the ladder rows below the one
/// being played first, then those above it.
pub fn render(f: &Frame, width: usize, height: usize, color: bool) -> Vec<String> {
    let rule = "─".repeat(width.min(RULE));
    let mut head = vec![rule.clone(), header(f, color)];
    let (rows, columns) = rows(f, color);
    let mut tail = Vec::new();
    let mut trend = Vec::new();
    if let Some((slope, margin)) = f.climb {
        // Rounded first, so that a slope a hair below zero reads +0, not -0; adding zero
        // turns the -0 rounding leaves into 0.
        let slope = slope.round() + 0.0;
        trend.push(format!("Elo/×3  {slope:+.0} ±{margin:.0}"));
    }
    if !f.last.is_empty() {
        let dots: String = f.last.iter().map(|&v| dot(v, color)).collect();
        trend.push(format!("last {}  {dots}", f.last.len()));
    }
    if !trend.is_empty() {
        tail.push(trend.join("      "));
    }
    if let Some(now) = &f.now {
        tail.push(rule);
        tail.extend(now_lines(now, color));
    }
    let max = height.saturating_sub(1);
    // Rows in the order a short terminal drops them: those below the row played, cheapest
    // first, then those above it, the highest first.
    let at = f
        .rows
        .iter()
        .position(|r| r.playing)
        .or_else(|| f.rows.iter().rposition(|r| r.opened))
        .unwrap_or(0);
    let mut order: Vec<usize> = (0..at).chain((at + 1..rows.len()).rev()).collect();
    order.reverse();
    let mut keep = vec![true; rows.len()];
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

/// The ladder's rows, and the line naming its columns.
fn rows(f: &Frame, color: bool) -> (Vec<String>, String) {
    let nw = f
        .rows
        .iter()
        .map(|r| r.name.chars().count())
        .max()
        .unwrap_or(0);
    let vw = f
        .rows
        .iter()
        .map(|r| r.vs.chars().count())
        .max()
        .unwrap_or(0)
        .max(2);
    let columns = format!(
        "{:nw$}  {:vw$}  {:<11} {:<7}  {:>7}  {:>5}",
        "", "vs", "win", "95%", "games", "min"
    );
    let rows = f
        .rows
        .iter()
        .map(|r| {
            let games = format!("{}/{}", r.games, r.target);
            let line = if r.opened {
                let (bar, pct, ci) = match r.score {
                    Some((share, [lo, hi])) => (
                        bar(share, 6),
                        format!("{:.0}%", 100.0 * share),
                        format!("{:.0}–{:.0}", 100.0 * lo, 100.0 * hi),
                    ),
                    None => (String::new(), String::new(), String::new()),
                };
                format!(
                    "{:nw$}  {:vw$}  {bar:<6} {pct:>4} {ci:<7}  {games:>7}  {:>5}{}",
                    r.name,
                    r.vs,
                    r.seconds.map_or(String::new(), clock),
                    if r.playing { "  ◀" } else { "" },
                )
            } else {
                format!("{:nw$}  {:vw$}  {:21}{games:>7}", r.name, r.vs, "")
            };
            let line = line.trim_end().to_owned();
            if r.opened {
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
        "▶ {}  s{}·{seat}  T{}  you {} · them {}  hand {} · {}",
        n.matchup, n.pair, n.turn, n.life[0], n.life[1], n.hand[0], n.hand[1]
    )];
    let mut search = format!("  decision {}", n.decision);
    if let Some((spent, budget)) = n.search {
        let share = spent as f64 / f64::from(budget.max(1));
        let _ = write!(
            search,
            "  {}  {}/{}  {:.1}s",
            paint(color, GREEN, &format!("{:<12}", bar(share, 12))),
            thousands(spent),
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

/// A line for the log: a finished game, from the side of its rung's agent.
pub fn game_line(c: &Config, g: &GameRecord, seconds: Option<f64>, color: bool) -> String {
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
        matchup_label(c, g.rung, g.against),
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
            rungs: vec![3_000, 10_000, 30_000, 100_000],
            cheap: 3,
            pairs: 30,
            trees: 18,
            opponent: "expert".to_owned(),
            seed: 0,
        }
    }

    /// `n` games of rung `rung` against the opponent, `wins` of them won, `draws` drawn, each
    /// `seconds` long.
    fn games(
        out: &mut Vec<GameRecord>,
        clock: &mut HashMap<String, f64>,
        rung: usize,
        n: u32,
        wins: u32,
        draws: u32,
        seconds: f64,
    ) {
        for g in 0..n {
            let key = format!("{rung}-{g}");
            clock.insert(key.clone(), seconds);
            out.push(GameRecord {
                game: key,
                rung,
                iterations: config().iterations(rung),
                against: Against::Opponent,
                pair: g / 2,
                seed: 0,
                seat: (g % 2) as u8,
                outcome: if g < wins {
                    Win
                } else if g < wins + draws {
                    Draw
                } else {
                    Loss
                },
                turns: 20,
                decisions: 300,
                searched: 130,
                rollouts: 1,
                crashed_at: None,
            });
        }
    }

    fn mid_run() -> Frame {
        let c = config();
        let (mut g, mut s) = (Vec::new(), HashMap::new());
        games(&mut g, &mut s, 0, 60, 17, 0, 21.0);
        games(&mut g, &mut s, 1, 60, 24, 1, 65.0);
        games(&mut g, &mut s, 2, 60, 34, 0, 192.0);
        games(&mut g, &mut s, 3, 33, 24, 0, 424.0);
        let playing = Matchup {
            rung: 3,
            against: Against::Opponent,
        };
        let (rows, climb) = ladder(&c, &g, &s, Some(playing));
        Frame {
            run: "run-001".to_owned(),
            up: Some(6.0 * 3600.0 + 12.0 * 60.0 + 5.0),
            games: g.len(),
            crashed: 0,
            stopping: true,
            rows,
            climb,
            last: vec![Win, Win, Loss, Win, Win, Win, Loss, Win, Win, Draw],
            now: Some(Now {
                matchup: "100k v expert".to_owned(),
                pair: 16,
                seat: 1,
                turn: 9,
                life: [14, 9],
                hand: [4, 3],
                decision: 64,
                search: Some((58_120, 100_000)),
                seconds: 3.1,
                saved: Some(0.31),
                prev: Some("cast Counterspell".to_owned()),
            }),
        }
    }

    fn text(lines: &[String]) -> String {
        lines.join("\n") + "\n"
    }

    const MID_RUN: &str = "\
────────────────────────────────────────────────────────────────
longthink run-001   up 6h12m   213 games   ⏸ after this game
      vs      win         95%        games    min
3k    expert  █▊      28% 19–41      60/60   0:21
10k   expert  ██▌     41% 29–53      60/60   1:05
30k   expert  ███▍    57% 44–68      60/60   3:12
100k  expert  ████▍   73% 56–85      33/60   7:04  ◀
300k  expert                          0/60
Elo/×3  +103 ±47      last 10  ●●○●●●○●●◐
────────────────────────────────────────────────────────────────
▶ 100k v expert  s16·B  T9  you 14 · them 9  hand 4 · 3
  decision 64  ███████       58,120/100k  3.1s  stop −31%
  prev  cast Counterspell
";

    #[test]
    fn a_frame_mid_run() {
        assert_eq!(text(&render(&mid_run(), 100, 40, false)), MID_RUN);
    }

    /// Rungs all alike leave a slope of rounding noise, which reads +0.
    #[test]
    fn a_flat_climb_reads_zero() {
        let mut f = mid_run();
        f.climb = Some((-1e-14, 478.0));
        let lines = render(&f, 100, 40, false);
        assert!(
            lines.iter().any(|l| l.starts_with("Elo/×3  +0 ±478")),
            "{lines:?}"
        );
    }

    /// Colors mark the results and the search's bar, and nothing else changes.
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
        assert_eq!(plain, MID_RUN);
    }

    #[test]
    fn a_fresh_run() {
        let c = config();
        let playing = Matchup {
            rung: 0,
            against: Against::Opponent,
        };
        let (rows, climb) = ladder(&c, &[], &HashMap::new(), Some(playing));
        let f = Frame {
            run: "run-002".to_owned(),
            up: Some(3.0),
            games: 0,
            crashed: 0,
            stopping: false,
            rows,
            climb,
            last: Vec::new(),
            now: Some(Now {
                matchup: "3k v expert".to_owned(),
                pair: 0,
                seat: 0,
                turn: 1,
                life: [20, 20],
                hand: [7, 7],
                decision: 0,
                search: Some((0, 3_000)),
                seconds: 0.0,
                saved: None,
                prev: None,
            }),
        };
        let expected = "\
────────────────────────────────────────────────────────────────
longthink run-002   up 3s   0 games
     vs      win         95%        games    min
3k   expert                          0/60         ◀
10k  expert                          0/60
────────────────────────────────────────────────────────────────
▶ 3k v expert  s0·A  T1  you 20 · them 20  hand 7 · 7
  decision 0                0/3k  0.0s
";
        assert_eq!(text(&render(&f, 100, 40, false)), expected);
    }

    /// Every line is cut to the width; colored lines end their colors.
    #[test]
    fn a_narrow_terminal() {
        let lines = render(&mid_run(), 60, 40, false);
        assert_eq!(lines.len(), MID_RUN.lines().count());
        for (line, full) in lines.iter().zip(MID_RUN.lines()) {
            assert_eq!(*line, full.chars().take(60).collect::<String>());
        }
        assert_eq!(lines[0].chars().count(), 60);
        for line in render(&mid_run(), 60, 40, true) {
            if line.contains('\x1b') && fit(&line, 60) != line {
                assert!(line.ends_with("\x1b[0m"));
            }
        }
        assert_eq!(fit("\x1b[32mabcdef\x1b[0m", 3), "\x1b[32mabc\x1b[0m");
    }

    /// A short terminal drops the cheap rungs' rows first, then those above the one played;
    /// the panel never takes the cursor's line.
    #[test]
    fn a_short_terminal() {
        let short = text(&render(&mid_run(), 100, 12, false));
        let expected = "\
────────────────────────────────────────────────────────────────
longthink run-001   up 6h12m   213 games   ⏸ after this game
      vs      win         95%        games    min
30k   expert  ███▍    57% 44–68      60/60   3:12
100k  expert  ████▍   73% 56–85      33/60   7:04  ◀
300k  expert                          0/60
Elo/×3  +103 ±47      last 10  ●●○●●●○●●◐
────────────────────────────────────────────────────────────────
▶ 100k v expert  s16·B  T9  you 14 · them 9  hand 4 · 3
  decision 64  ███████       58,120/100k  3.1s  stop −31%
  prev  cast Counterspell
";
        assert_eq!(short, expected);
        let shorter = render(&mid_run(), 100, 10, false);
        assert_eq!(shorter.len(), 9);
        assert!(shorter.iter().any(|l| l.ends_with('◀')));
        assert!(!shorter.iter().any(|l| l.starts_with("300k")));
        assert_eq!(render(&mid_run(), 100, 4, false).len(), 3);
    }

    #[test]
    fn log_lines() {
        let c = config();
        let (mut g, mut s) = (Vec::new(), HashMap::new());
        games(&mut g, &mut s, 3, 2, 1, 0, 411.0);
        let lines: Vec<String> = g
            .iter()
            .map(|g| game_line(&c, g, Some(411.0), false))
            .collect();
        assert_eq!(
            lines,
            [
                "✓ 100k v expert  s0·A  won  T20  6:51",
                "✗ 100k v expert  s0·B  lost T20  6:51"
            ]
        );
        g[0].outcome = Crashed;
        g[0].crashed_at = Some(9);
        assert_eq!(
            game_line(&c, &g[0], None, true),
            "\x1b[31m!\x1b[0m 100k v expert  s0·A  crashed at decision 9"
        );
    }

    /// Hand-computed: two budgets a tripling apart, 100 Elo between them, each known to a
    /// standard error of 10, climb 100 per tripling, ±1.96 √200; a third point on the line
    /// changes nothing but the margin.
    #[test]
    fn elo_per_tripling_is_a_weighted_slope() {
        let (slope, margin) = climb(&[(0.0, 0.0, 10.0), (1.0, 100.0, 10.0)]).unwrap();
        assert!((slope - 100.0).abs() < 1e-9 && (margin - 27.718).abs() < 1e-3);
        let (slope, margin) =
            climb(&[(0.0, 0.0, 10.0), (1.0, 100.0, 10.0), (2.0, 200.0, 10.0)]).unwrap();
        assert!((slope - 100.0).abs() < 1e-9 && margin < 27.0);
        assert_eq!(climb(&[(1.0, 50.0, 10.0)]), None);
        assert_eq!(climb(&[(1.0, 0.0, 10.0), (1.0, 50.0, 5.0)]), None);
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
