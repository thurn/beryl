//! `--upgrade`: adopts this binary into a run only if it plays as the run's own does, so
//! speed work reaches a running arena without mixing results. Every logged game must replay
//! through its log to the same transcript, every agent asked again for real answering as
//! logged, and a sample of every searching entrant's searches, searched again at their
//! budgets and seeds, must choose as they did.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use beryl_engine::GameTable;
use beryl_search::AgentDecision;

use super::files;
use super::game::{self, Build, Game, GameDecision, Line, Played, Resume, Stop};
use super::schedule::Config;
use super::{GameRecord, Spec, Verdict};
use crate::game::mirror;

/// Searches re-searched at most for each searching entrant: as many fresh as reusing trees.
const PER_ENTRANT: usize = 8;

/// Checks this binary against run `dir` and, if it plays identically, makes it the run's.
pub fn adopt(dir: &Path, code: Option<&BTreeMap<String, String>>) -> Result<(), String> {
    let c = super::config(dir)?;
    let checked = verify(&mirror(), dir, &c, &game::build)
        .map_err(|e| format!("not adopted: {e}; --new starts a run on this code"))?;
    super::adopt_binary(dir, code)?;
    println!("adopted: {checked}");
    if let Some(lock) = files::holder(dir)? {
        println!(
            "{} {}",
            super::run::run_name(dir),
            if super::follows(&lock) {
                "moves to it before its next decision"
            } else {
                "is played by a runner from before live upgrades, which plays on with its \
                 old binary until it is restarted"
            }
        );
    }
    Ok(())
}

/// What a check covered, or the first way the code built by `build` plays otherwise.
pub fn verify(t: &GameTable, dir: &Path, c: &Config, build: &Build<'_>) -> Result<String, String> {
    let check = Check { t, dir, c, build };
    let games: Vec<GameRecord> = files::read(&dir.join("games.jsonl"))?;
    let scored = || games.iter().filter(|g| g.outcome != Verdict::Crashed);
    for g in scored() {
        let spec = check.spec(g)?;
        let played = check
            .replay(&spec, &check.log(&g.game)?, [Resume::Log; 2], None)?
            .expect("a game replayed to its end");
        let path = dir.join("transcripts").join(format!("{}.txt", g.game));
        let logged = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if logged != played.transcript {
            let same = logged
                .lines()
                .zip(played.transcript.lines())
                .take_while(|(a, b)| a == b)
                .count();
            return Err(format!(
                "{}: the transcript differs at line {}",
                g.game,
                same + 1
            ));
        }
    }
    // The games being played, as far as they have gone.
    let unfinished = super::run::unfinished(dir, &games)?;
    for key in &unfinished {
        let next = Spec::parse(c, key).ok_or_else(|| format!("{key}: no such game"))?;
        let lines = check.log(&next.key)?;
        check.replay(
            &next,
            &lines,
            [Resume::Log; 2],
            Some(GameDecision(lines.len())),
        )?;
    }
    // Searches of every searching entrant, evenly spaced, each searched again, so that a
    // change to any family's play is caught, not just the cheapest's: half of them fresh
    // searches, half ones that reused the trees of the decisions before them, searched
    // again from the decision that planted them, so a change to how trees are kept is
    // caught too.
    let mut searches: BTreeMap<String, [Vec<Sampled<'_>>; 2]> = BTreeMap::new();
    for g in scored() {
        let spec = check.spec(g)?;
        for (i, line) in check.log(&g.game)?.iter().enumerate() {
            let seat = usize::from(line.seat);
            let e = &spec.entrants[seat];
            if let (Some(s), Some(_)) = (&line.search, e.budget()) {
                let kind = usize::from(s.chain != s.decision);
                searches.entry(e.label()).or_default()[kind].push((g, i, seat, s.chain));
            }
        }
    }
    let mut repeated = 0;
    for kinds in searches.values() {
        for found in kinds {
            for &(g, i, seat, chain) in evenly(found, PER_ENTRANT / 2) {
                let mut from = [Resume::Log; 2];
                from[seat] = Resume::At(chain);
                let until = Some(GameDecision(i + 1));
                check.replay(&check.spec(g)?, &check.log(&g.game)?, from, until)?;
                repeated += 1;
            }
        }
    }
    Ok(format!(
        "{} games replayed{}, {repeated} searches of {} entrants repeated",
        scored().count(),
        match unfinished.len() {
            0 => String::new(),
            1 => " and the one in progress".to_owned(),
            n => format!(" and the {n} in progress"),
        },
        searches.len()
    ))
}

/// A search to repeat: its game, its index there, its seat and the decision that planted
/// its trees.
type Sampled<'a> = (&'a GameRecord, usize, usize, AgentDecision);

/// At most `n` of `xs`, evenly spaced from the first.
fn evenly<T>(xs: &[T], n: usize) -> impl Iterator<Item = &T> {
    xs.iter().step_by(xs.len().div_ceil(n).max(1))
}

/// A run and the code checked against it.
struct Check<'a> {
    t: &'a GameTable,
    dir: &'a Path,
    c: &'a Config,
    build: &'a Build<'a>,
}

impl Check<'_> {
    fn log(&self, key: &str) -> Result<Vec<Line>, String> {
        files::read(&self.dir.join("logs").join(format!("{key}.jsonl")))
    }

    /// The finished game `g`, as this code names it.
    fn spec(&self, g: &GameRecord) -> Result<Spec, String> {
        let spec = Spec::new(self.c, &g.matchup(), 2 * g.pair + u32::from(g.seat))?;
        if spec.key == g.game {
            Ok(spec)
        } else {
            Err(format!("{}: this code names the game {}", g.game, spec.key))
        }
    }

    /// Replays a game through its log, each seat's search asked from where `from` says, to
    /// its end, or with `until` to that decision, and then returns nothing.
    fn replay(
        &self,
        spec: &Spec,
        lines: &[Line],
        from: [Resume; 2],
        until: Option<GameDecision>,
    ) -> Result<Option<Played>, String> {
        let g = Game {
            seed: spec.seed,
            entrants: [&spec.entrants[0], &spec.entrants[1]],
            from,
            until,
        };
        let key = &spec.key;
        match game::play(
            self.t,
            &g,
            self.build,
            lines,
            |_, _, _| Ok(()),
            |_, _, _| Ok(()),
        ) {
            Ok(played) if until.is_none() => Ok(Some(played)),
            Ok(_) => Err(format!(
                "{key}: the game ends before decision {}",
                until.unwrap_or_default()
            )),
            Err(Stop::Reached(_)) if until.is_some() => Ok(None),
            Err(Stop::Reached(i)) => Err(format!("{key}: replay stopped at decision {i}")),
            Err(Stop::Differs {
                decision,
                logged,
                now,
            }) => Err(format!(
                "{key} decision {decision}: logged candidate {logged}, now {now}"
            )),
            Err(Stop::Failed(e)) => Err(format!("{key}: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use beryl_engine::{Choice, Decision, Observation};
    use beryl_search::{Agent, AgentDecision, Search};

    use super::super::game::Entrant;
    use super::*;

    /// An agent that answers as another but for its third real choice.
    struct Off {
        inner: Box<dyn Agent>,
        choices: u32,
    }

    impl Agent for Off {
        fn choose(&mut self, obs: Observation<'_>, d: &Decision) -> Choice {
            let c = self.inner.choose(obs, d);
            if d.len() < 2 {
                return c;
            }
            self.choices += 1;
            if self.choices == 3 {
                Choice((c.0 + 1) % d.len() as u16)
            } else {
                c
            }
        }
    }

    /// A search that answers otherwise only where it reused the trees of the decision
    /// before: what a change to how trees are kept could do.
    struct Reuses {
        inner: Box<dyn Agent>,
    }

    impl Agent for Reuses {
        fn choose(&mut self, obs: Observation<'_>, d: &Decision) -> Choice {
            let c = self.inner.choose(obs, d);
            match self.inner.last_search() {
                Some(s) if s.chain != s.decision => Choice((c.0 + 1) % d.len() as u16),
                _ => c,
            }
        }

        fn last_search(&self) -> Option<&Search> {
            self.inner.last_search()
        }

        fn resume_point(&self) -> Option<AgentDecision> {
            self.inner.resume_point()
        }
    }

    /// `games` games of a tiny arena of `families`, played in-process.
    fn tiny_run(name: &str, families: &[&str], games: u32) -> (std::path::PathBuf, Config) {
        let dir = std::env::temp_dir().join(format!("beryl-upgrade-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let c = Config {
            families: families.iter().map(|&f| f.to_owned()).collect(),
            fixed: vec!["expert".to_owned()],
            anchor: "expert".to_owned(),
            budgets: vec![1_500],
            threads: 1,
            seed: 0,
        };
        super::super::start(&dir, &c, None).unwrap();
        super::super::run::run(&dir, Some(games)).unwrap();
        (dir, c)
    }

    /// The code a run was played with passes; an opponent that answers one decision
    /// otherwise is caught replaying, and a search that would choose otherwise is caught
    /// searching again, whichever entrant it is, each named by its decision. (One run: a
    /// process takes Ctrl-C for one runner.)
    #[test]
    fn only_code_that_plays_identically_is_adopted() {
        let families = ["ismcts1/hidden/rollout/tuned", "pimc1/hidden/rollout/tuned"];
        let (dir, c) = tiny_run("verify", &families, 4);
        let t = mirror();
        let ok = verify(&t, &dir, &c, &game::build).unwrap();
        assert!(
            ok.starts_with("4 games replayed") && ok.contains("of 2 entrants"),
            "{ok}"
        );
        // A change to the entrant that sorts after the cheapest alone.
        let pimc = |t: &GameTable, e: &Entrant, seed: u64, from| {
            let seed = match e {
                Entrant::Search(s) if s.family.to_string() == families[1] => !seed,
                _ => seed,
            };
            game::build(t, e, seed, from)
        };
        let e = verify(&t, &dir, &c, &pimc).unwrap_err();
        assert!(e.contains("pimc1") && e.contains("logged candidate"), "{e}");
        let opponent = |t: &GameTable, e: &Entrant, seed, from| {
            let inner = game::build(t, e, seed, from)?;
            Ok(match e {
                Entrant::Fixed(_) => Box::new(Off { inner, choices: 0 }) as Box<dyn Agent>,
                Entrant::Search(_) => inner,
            })
        };
        let e = verify(&t, &dir, &c, &opponent).unwrap_err();
        assert!(
            e.contains("decision ") && e.contains("logged candidate"),
            "{e}"
        );
        let search = |t: &GameTable, e: &Entrant, seed: u64, from| {
            let seed = match e {
                Entrant::Search(_) => !seed,
                Entrant::Fixed(_) => seed,
            };
            game::build(t, e, seed, from)
        };
        let e = verify(&t, &dir, &c, &search).unwrap_err();
        assert!(
            e.contains("decision") && e.contains("logged candidate"),
            "{e}"
        );
        let logs: Vec<Line> = fs::read_dir(dir.join("logs"))
            .unwrap()
            .flat_map(|f| files::read::<Line>(&f.unwrap().path()).unwrap())
            .collect();
        assert!(
            logs.iter()
                .filter_map(|l| l.search.as_ref())
                .any(|s| s.chain != s.decision),
            "the run reuses trees somewhere"
        );
        let reuse = |t: &GameTable, e: &Entrant, seed, from| {
            let inner = game::build(t, e, seed, from)?;
            Ok(match e {
                Entrant::Search(_) => Box::new(Reuses { inner }) as Box<dyn Agent>,
                Entrant::Fixed(_) => inner,
            })
        };
        let e = verify(&t, &dir, &c, &reuse).unwrap_err();
        assert!(e.contains("logged candidate"), "{e}");
    }
}
