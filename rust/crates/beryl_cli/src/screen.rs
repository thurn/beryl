//! `beryl_cli screen <challenger> <match-dir> [--vs NAME] [--recheck N] [--work 3M] [--games N]`
//! (`just screen`): a cheap estimate of what a challenger that changes few moves costs, from
//! the champion's last match, before the hours of its own (docs/ai-strategy.md §4.2).
//!
//! - **Replay.** Each game of the match directory replays through its log
//!   (`anchor_replay`'s [`games`], [`start`] and [`log`]). The challenger, built from the
//!   champion's own seed in that game, is asked every one of the champion's decisions and the
//!   logged answer is applied, so it plays the champion's game as the champion did. A
//!   challenger identical to the champion answers every decision as the log does.
//! - **Differences.** At each of the champion's searched decisions the two answers are
//!   compared. Where they differ, the challenger is rebuilt fresh at its next decision: its
//!   trees had grown under its own answer, not the one applied. Until the champion next plants
//!   fresh trees (a search that kept none, or a decision it did not search), the champion's
//!   searches grow trees the rebuilt challenger never had, so those decisions are asked but not
//!   compared: one difference counts once.
//! - **Rechecks.** Up to N of the differing decisions, spread over the games, are searched again
//!   from a fresh seed at the proxy budget with the whole of it split between the two answers
//!   ([`duel`], `LongSearch::duel`): the challenger's lead over the champion's choice, tree by
//!   tree, unbiased by having been chosen, as `anchor-replay --recheck` does.
//! - **Report.** Differing decisions a game, by kind, and the cost a game: differences a game
//!   times the mean lead lost, with its standard error. A cost beyond [`COST_NOTED`] a game is
//!   written beside the match's result in the rung's bead; it stops nothing.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use beryl_engine::{
    Choice, Decision, DecisionKind, GameTable, Replay, Scratch, State, Step, apply, next, observe,
};
use beryl_search::{Agent, AgentDecision, Budget, DEEP_SEARCHER, LongSearch, Settings};

use crate::anchor_replay::{GameLine, duel, games, log, mean_and_error, start};
use crate::game::{mirror, parse_work, seat_seeds};
use crate::ladder::{games_at_once, resolve};
use crate::longthink::game::Line;

/// A cost a game beyond which the screen's result is written beside the match's (§4.2).
pub const COST_NOTED: f64 = 0.03;

const USAGE: &str = "usage: beryl_cli screen <challenger> <match-dir> [--vs NAME] \
                     [--recheck N] [--work 3M] [--games N]";

/// Builds the challenger for a game from `seed`, as it stands before its own decision `from`.
pub type Build<'a> =
    dyn Fn(&GameTable, u64, AgentDecision) -> Result<Box<dyn Agent>, String> + Sync + 'a;

/// One of the champion's searched decisions the challenger answered otherwise.
pub struct Differ {
    pub game: String,
    pub state: State,
    pub decision: Decision,
    pub champion: Choice,
    pub challenger: Choice,
}

/// What the replays found.
#[derive(Default)]
pub struct Screened {
    pub games: u32,
    /// The champion's searched decisions, and those compared.
    pub searched: u32,
    pub compared: u32,
    pub differs: Vec<Differ>,
}

/// One game's part.
struct GameScreen {
    searched: u32,
    compared: u32,
    differs: Vec<Differ>,
}

/// Replays game `g` of the match in `dir`, asking the challenger `build` makes each of
/// `champion`'s decisions.
fn screen_game(
    dir: &Path,
    t: &GameTable,
    g: &GameLine,
    champion: &str,
    build: &Build<'_>,
) -> Result<GameScreen, String> {
    let seat = g
        .seat_of(champion)
        .ok_or_else(|| format!("{}: {champion} did not play it", g.game))?;
    let seed = seat_seeds(g.seed)[usize::from(seat)];
    let mut agent = build(t, seed, AgentDecision(0))?;
    let mut asked = AgentDecision(0);
    let mut synced = true;
    let mut s = start(dir, t, g)?;
    let mut x = Scratch::new();
    let mut out = GameScreen {
        searched: 0,
        compared: 0,
        differs: Vec::new(),
    };
    for line in log::<Line>(dir, g)? {
        let Step::Decide(d) = next(&mut s, t, &mut x, &mut Replay) else {
            return Err(format!("{}: the game ended before its log", g.game));
        };
        if d.player.get() != line.seat || usize::from(line.choice) >= d.len() {
            return Err(format!("{}: the log no longer fits the game", g.game));
        }
        let logged = Choice(line.choice);
        if line.seat == seat {
            // The champion's search kept no trees here: the rebuilt challenger stands as it does.
            if line
                .search
                .as_ref()
                .is_none_or(|search| search.chain == search.decision)
            {
                synced = true;
            }
            let answer = agent.choose(observe(&s, t, &mut x, d.player), &d);
            asked = asked.next();
            if line.search.is_some() {
                out.searched += 1;
                if synced {
                    out.compared += 1;
                }
            }
            if answer != logged {
                if synced && line.search.is_some() {
                    out.differs.push(Differ {
                        game: g.game.clone(),
                        state: s,
                        decision: d,
                        champion: logged,
                        challenger: answer,
                    });
                }
                agent = build(t, seed, asked)?;
                synced = false;
            }
        }
        apply(&mut s, t, &mut x, &mut Replay, logged);
    }
    Ok(out)
}

/// Replays the first `limit` games of the match in `dir` (all without one), the challenger
/// `build` makes asked each of `champion`'s decisions, [`games_at_once`] games at a time.
pub fn screen(
    dir: &Path,
    champion: &str,
    build: &Build<'_>,
    limit: Option<usize>,
) -> Result<Screened, String> {
    let t = mirror();
    let mut all = games(dir)?;
    all.retain(|g| g.seat_of(champion).is_some());
    if let Some(n) = limit {
        all.truncate(n);
    }
    let next_game = AtomicUsize::new(0);
    let found: Mutex<Vec<(usize, Result<GameScreen, String>)>> = Mutex::new(Vec::new());
    thread::scope(|scope| {
        for _ in 0..games_at_once().min(all.len()) {
            scope.spawn(|| {
                loop {
                    let i = next_game.fetch_add(1, Ordering::Relaxed);
                    let Some(g) = all.get(i) else { break };
                    let done = screen_game(dir, &t, g, champion, build);
                    found.lock().unwrap().push((i, done));
                }
            });
        }
    });
    let mut found = found.into_inner().unwrap();
    // In game order, so the rechecks spread over the games the same way every time.
    found.sort_by_key(|f| f.0);
    let mut out = Screened {
        games: u32::try_from(all.len()).unwrap_or(u32::MAX),
        ..Screened::default()
    };
    for (_, game) in found {
        let game = game?;
        out.searched += game.searched;
        out.compared += game.compared;
        out.differs.extend(game.differs);
    }
    Ok(out)
}

/// The challenger's lead over the champion's choice at `n` of the differing decisions, spread
/// evenly over them, each dueled from a fresh seed with `settings`.
pub fn recheck(differs: &[Differ], settings: Settings, n: usize) -> Vec<(usize, f64, f64)> {
    if n == 0 || differs.is_empty() {
        return Vec::new();
    }
    let t = mirror();
    let step = (differs.len() as f64 / n as f64).max(1.0);
    (0..n.min(differs.len()))
        .map(|k| (k as f64 * step) as usize)
        .map(|i| {
            let d = &differs[i];
            let check = duel(
                &t,
                settings,
                0x5C4E_E400 ^ i as u64,
                &d.state,
                &d.decision,
                d.challenger,
                d.champion,
            );
            eprintln!(
                "rechecked {} ({}): lead {:+.4} ± {:.4}",
                i, d.game, check.lead, check.error
            );
            (i, check.lead, check.error)
        })
        .collect()
}

/// The report: differences a game by kind and the cost a game.
pub fn report(
    challenger: &str,
    champion: &str,
    s: &Screened,
    fresh: &[(usize, f64, f64)],
) -> String {
    let games = f64::from(s.games.max(1));
    let per_game = s.differs.len() as f64 / games;
    let mut out = format!(
        "{challenger} over {champion}'s games: {} games, {} searched decisions, {} compared, \
         {} differ ({per_game:.2} a game)\n",
        s.games,
        s.searched,
        s.compared,
        s.differs.len()
    );
    let mut kinds: BTreeMap<String, (usize, Vec<f64>)> = BTreeMap::new();
    for (i, d) in s.differs.iter().enumerate() {
        let k: DecisionKind = d.decision.kind;
        let entry = kinds.entry(format!("{k:?}")).or_default();
        entry.0 += 1;
        entry
            .1
            .extend(fresh.iter().filter(|f| f.0 == i).map(|f| f.1));
    }
    let mut kinds: Vec<_> = kinds.into_iter().collect();
    kinds.sort_by_key(|(_, (n, _))| std::cmp::Reverse(*n));
    for (kind, (n, leads)) in kinds {
        let fresh = if leads.is_empty() {
            String::new()
        } else {
            let (mean, error) = mean_and_error(&leads);
            format!("  lead {mean:+.4} ± {error:.4} over {}", leads.len())
        };
        let _ = writeln!(out, "  {kind}: {n} ({:.2} a game){fresh}", n as f64 / games);
    }
    if fresh.is_empty() {
        let _ = writeln!(out, "cost a game: not rechecked");
    } else {
        let leads: Vec<f64> = fresh.iter().map(|f| f.1).collect();
        let (mean, error) = mean_and_error(&leads);
        let cost = -per_game * mean;
        let _ = writeln!(
            out,
            "cost a game: {cost:+.4} ± {:.4} ({} rechecked){}",
            per_game * error,
            fresh.len(),
            if cost > COST_NOTED {
                format!("; beyond {COST_NOTED}: note it beside the match's result")
            } else {
                String::new()
            }
        );
    }
    out
}

/// The challenger `name` names, for the screen: a long-think entrant, rebuilt as it stands
/// before a decision ([`LongSearch::resumed`]) on one thread, or a registered agent.
fn builder(name: &str) -> Result<Box<Build<'static>>, String> {
    if let Some(info) = beryl_search::agent(name) {
        let budget = Budget {
            parallel: false,
            ..Budget::DEFAULT
        };
        return Ok(Box::new(move |t, seed, _| Ok((info.new)(t, seed, budget))));
    }
    let settings = entrant_settings(name)?;
    Ok(Box::new(move |t, seed, from| {
        Ok(Box::new(LongSearch::resumed(t, settings, seed, from)) as Box<dyn Agent>)
    }))
}

/// The settings of the long-think entrant `name` (`family@budget`), on one thread.
fn entrant_settings(name: &str) -> Result<Settings, String> {
    let (family, work) = name
        .rsplit_once('@')
        .ok_or_else(|| format!("{name:?} is no long-think entrant (family@budget)"))?;
    Ok(Settings {
        family: family.parse()?,
        work: parse_work(work).ok_or_else(|| format!("{name:?}: no budget {work:?}"))?,
        threads: 1,
    })
}

/// The champion the match in `dir` played, as its `match.json` names it.
fn match_champion(dir: &Path) -> Result<String, String> {
    let path = dir.join("match.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    json.get("champion")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("{}: no champion", path.display()))
}

pub fn main(args: &[String]) -> Result<(), String> {
    let mut positional: Vec<String> = Vec::new();
    let mut champion = None;
    let mut recheck_n = 20;
    let mut work = 3_000_000;
    let mut limit = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .cloned()
                .ok_or_else(|| format!("{arg} needs a value\n{USAGE}"))
        };
        match arg.as_str() {
            "--vs" => champion = Some(value()?),
            "--recheck" => recheck_n = crate::number(arg, &value()?)?,
            "--games" => limit = Some(crate::number(arg, &value()?)?),
            "--work" => {
                let v = value()?;
                work = parse_work(&v).ok_or_else(|| format!("--work: no budget {v:?}"))?;
            }
            flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}\n{USAGE}")),
            other => positional.push(other.to_owned()),
        }
    }
    let [challenger, dir] = positional.as_slice() else {
        return Err(USAGE.to_owned());
    };
    let dir = PathBuf::from(dir);
    let challenger = resolve(challenger, work)?;
    let champion = match champion {
        Some(name) => resolve(&name, work)?,
        None => match_champion(&dir)?,
    };
    let build = builder(&challenger)?;
    let screened = screen(&dir, &champion, build.as_ref(), limit)?;
    // The duels judge with the champion's search, or the Deep Searcher's for a champion that
    // does not search as a long-think entrant, at the proxy budget on every core.
    let judge = entrant_settings(&champion).map_or(DEEP_SEARCHER, |s| s.family);
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| u32::try_from(n.get()).unwrap_or(u32::MAX));
    let settings = Settings {
        family: judge,
        work,
        threads,
    };
    let fresh = recheck(&screened.differs, settings, recheck_n);
    print!("{}", report(&challenger, &champion, &screened, &fresh));
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::*;
    use crate::ladder::{Bar, Match, run};

    const CHAMPION: &str = "pimc2/hidden/rollout/tuned@2k";

    fn match_dir(name: &str) -> PathBuf {
        let out = std::env::temp_dir().join(format!("beryl-screen-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        let m = Match {
            challenger: "greedy".to_owned(),
            champion: CHAMPION.to_owned(),
            work: 2_000,
            bar: Bar::Stronger,
            cap: 2,
            seed: 6,
            out: out.clone(),
            positions: Vec::new(),
        };
        run(&m, |_, _, _| {}).unwrap();
        out
    }

    /// A challenger identical to the champion differs nowhere and costs nothing.
    #[test]
    fn the_champion_against_itself_differs_nowhere() {
        let dir = match_dir("same");
        assert_eq!(match_champion(&dir).unwrap(), CHAMPION);
        let build = builder(CHAMPION).unwrap();
        let s = screen(&dir, CHAMPION, build.as_ref(), None).unwrap();
        assert_eq!(s.games, 2);
        assert!(s.searched > 10, "{}", s.searched);
        assert_eq!(s.compared, s.searched);
        assert!(s.differs.is_empty());
        let text = report("same", CHAMPION, &s, &[]);
        assert!(text.contains("0 differ"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The champion, but once answering otherwise at one decision: that difference is counted
    /// once and dueled once.
    #[test]
    fn one_crafted_difference_counts_and_duels_once() {
        struct Crafted {
            inner: LongSearch,
            seen: usize,
            at: usize,
        }
        impl Agent for Crafted {
            fn choose(&mut self, obs: beryl_engine::Observation<'_>, d: &Decision) -> Choice {
                let c = self.inner.choose(obs, d);
                if Agent::last_search(&self.inner).is_some() {
                    self.seen += 1;
                    if self.seen == self.at {
                        return Choice((c.0 + 1) % d.len() as u16);
                    }
                }
                c
            }
        }
        let dir = match_dir("crafted");
        let settings = entrant_settings(CHAMPION).unwrap();
        let crafted = AtomicBool::new(false);
        let build = |t: &GameTable, seed, from| -> Result<Box<dyn Agent>, String> {
            let inner = LongSearch::resumed(t, settings, seed, from);
            // The first game's champion seat alone, once.
            let at = if crafted.swap(true, Ordering::Relaxed) {
                0
            } else {
                3
            };
            Ok(Box::new(Crafted { inner, seen: 0, at }))
        };
        let s = screen(&dir, CHAMPION, &build, Some(1)).unwrap();
        assert_eq!(s.differs.len(), 1);
        let fresh = recheck(&s.differs, settings, 5);
        assert_eq!(fresh.len(), 1);
        assert!(report("crafted", CHAMPION, &s, &fresh).contains("1 rechecked"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
