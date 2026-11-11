//! Time-boxed discovery, not a blunder verdict or a ladder match. Fresh champion games
//! supply positions; policy disagreements and random audit positions supply review leads.
//! Durable packets contain semantic replay prefixes, labels and independently seeded duels.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use beryl_engine::rng::SplitMix64;
use beryl_engine::{
    Choice, Decision, GameTable, Profile, Replay, Scratch, State, Step, apply, next, observe, view,
};
use beryl_search::{Agent, Budget, DEEP_SEARCHER, LongSearch, RolloutPolicy, Settings, agent};
use beryl_session::save::Chosen;
use serde::{Deserialize, Serialize};

use crate::anchor_replay::duel;
use crate::game::{mirror, parse_work, seat_seeds};
use crate::longthink::{cache, files};
use crate::transcript::{Transcript, labels};

const USAGE: &str = "usage: just blunder-hunt [--hours 6] [--seconds N] [--seed N] [--work 30M] [--threads N] [--out DIR]\n       just blunder-hunt --inspect CASE.json";
const REVIEW: &str = "These are candidate blunders, NOT confirmed mistakes. Start with shortlist.json, then inspect candidates/ and games/. The shortlist balances decision kinds and includes random audit positions. All packets remain available in index.json; no candidate is deleted for a near-zero search gap.\n\nFor each case, explain the board, actual choice and a plausible alternative in plain English. Classify as likely blunder / debatable / reasonable / insufficient evidence. Group duplicates by underlying mistake; aim for 10-20 distinct useful examples, but do not invent mistakes to fill a quota. Look at the surrounding game, especially follow-up choices.\n\nWARNING: transcripts and replay prefixes are omniscient evidence. Do not judge a move using the opponent's actual hidden hand or future draws. Search comparisons use the acting player's observation (Hidden). Greedy and the frozen expert are proposal generators, not authorities. Scores are discounted rollout rewards, NOT calibrated win probabilities. Fresh duels reduce selection noise but share the champion's rollout blind spots; their error bars are not proof or multiple-testing-corrected significance. A near-zero gap does not excuse a visibly pointless play. Random audit cases are not accusations.\n\nEach packet stores game seed, semantic history before the decision, candidate labels, actual choice, search statistics, proposed alternatives, and fresh duel seeds/results. Reconstruct with `just blunder-hunt --inspect PATH`. manifest.json identifies revision, budgets and deck. status.json records the final outcome. A copy of the exact executable is saved as beryl-hunt; use that with --inspect if the checkout has changed. Non-default work is exploratory, not evidence that the shipped 30M champion makes that play. Do not add suite entries or call a blunder fixed without human confirmation.\n";

#[derive(Clone, Debug, Serialize)]
struct Config {
    seconds: u64,
    seed: u64,
    work: u64,
    threads: u32,
    out: PathBuf,
}

impl Config {
    fn parse(args: &[String]) -> Result<Self, String> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs();
        let mut c = Self {
            seconds: 6 * 3600,
            seed: stamp,
            work: 30_000_000,
            threads: std::thread::available_parallelism().map_or(1, |n| n.get() as u32),
            out: cache()
                .join("blunder-hunts")
                .join(format!("{stamp}-{}", std::process::id())),
        };
        for pair in args.chunks(2) {
            let [flag, value] = pair else {
                return Err(USAGE.to_owned());
            };
            match flag.as_str() {
                "--hours" => {
                    let hours = value.parse::<f64>().map_err(|e| e.to_string())?;
                    if !hours.is_finite() || hours <= 0.0 || hours > 168.0 {
                        return Err("hours must be in (0, 168]".into());
                    }
                    c.seconds = (hours * 3600.0).ceil() as u64;
                }
                "--seconds" => c.seconds = crate::number(flag, value)?,
                "--seed" => c.seed = crate::number(flag, value)?,
                "--work" => c.work = parse_work(value).ok_or("invalid work budget")?,
                "--threads" => c.threads = crate::number(flag, value)?,
                "--out" => c.out = PathBuf::from(value),
                _ => return Err(USAGE.to_owned()),
            }
        }
        if c.seconds == 0 || c.seconds > 604_800 || c.work == 0 || c.threads == 0 || c.threads > 256
        {
            return Err(
                "duration, work and threads must be positive; max 7 days and 256 threads".into(),
            );
        }
        Ok(c)
    }

    fn settings(&self) -> Settings {
        Settings {
            family: DEEP_SEARCHER,
            work: self.work,
            threads: self.threads,
        }
    }
}

struct Clock {
    start: Instant,
    duration: Duration,
    stopped: Arc<AtomicBool>,
}
impl Clock {
    fn done(&self) -> bool {
        self.stopped.load(Ordering::Relaxed) || self.start.elapsed() >= self.duration
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Comparison {
    alternative: u16,
    seed: u64,
    alternative_lead: f64,
    standard_error: f64,
    trees: u16,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Packet {
    id: String,
    seed: u64,
    decision: usize,
    turn: u16,
    seat: u8,
    kind: String,
    reasons: Vec<String>,
    history: Vec<Chosen>,
    labels: Vec<String>,
    chosen: u16,
    alternatives: Vec<u16>,
    visits: Vec<u32>,
    means: Vec<Option<f64>>,
    context: String,
    game_transcript: String,
    comparisons: Vec<Comparison>,
    checks_complete: bool,
}

struct Pending {
    packet: Packet,
    state: State,
    decision: Decision,
}

/// Keep a bounded random reservoir, rather than only early-turn disagreements. Audit
/// positions have their own reservoir, so broad policy agreement cannot hide every mistake.
fn reservoir<T>(items: &mut Vec<T>, seen: u64, cap: usize, item: T, rng: &mut SplitMix64) {
    if items.len() < cap {
        items.push(item);
    } else {
        let at = rng.next_u64() % seen;
        if at < cap as u64 {
            items[at as usize] = item;
        }
    }
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let text = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    files::write_whole(path, &text)
}

#[derive(Clone, Serialize)]
struct Entry {
    file: String,
    kind: String,
    chosen: String,
    audit: bool,
    // A ranking heuristic only: the weakest independent recheck's lead minus twice
    // its tree standard error. Retain near-ties and unchecked cases for plausibility review.
    score: f64,
    checks: usize,
}

fn shortlist(entries: &[Entry]) -> Vec<Entry> {
    let mut sorted = entries.to_vec();
    sorted.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.file.cmp(&b.file)));
    let mut groups = BTreeMap::new();
    let mut out = Vec::new();
    // Reserve up to four places for audits; then fill with diverse policy disagreements.
    for audit in [true, false] {
        let mut taken = 0;
        for e in sorted.iter().filter(|e| e.audit == audit) {
            let n = groups
                .entry((e.kind.clone(), e.chosen.clone()))
                .or_insert(0);
            if *n < 2 && out.len() < 20 && (!audit || taken < 4) {
                out.push(e.clone());
                *n += 1;
                taken += 1;
            }
        }
    }
    out
}

fn entry(p: &Packet) -> Entry {
    let score = p
        .alternatives
        .iter()
        .filter_map(|a| {
            let checks: Vec<_> = p
                .comparisons
                .iter()
                .filter(|c| c.alternative == *a && c.trees > 1)
                .collect();
            (checks.len() == 3).then(|| {
                checks
                    .iter()
                    .map(|c| c.alternative_lead - 2.0 * c.standard_error)
                    .fold(f64::INFINITY, f64::min)
            })
        })
        .max_by(f64::total_cmp)
        .unwrap_or(0.0);
    Entry {
        file: format!("candidates/{}.json", p.id),
        kind: p.kind.clone(),
        chosen: p.labels[usize::from(p.chosen)].clone(),
        audit: p.reasons.iter().any(|s| s == "random audit"),
        score,
        checks: p.comparisons.len(),
    }
}

/// Reconstruct from semantic choices and fail on drift, including the candidate labels.
fn reconstruct(t: &GameTable, p: &Packet) -> Result<(State, Decision), String> {
    let mut s = State::new(t, p.seed, Profile::SMART).map_err(|e| e.to_string())?;
    let mut x = Scratch::new();
    if p.history.len() != p.decision {
        return Err("history length differs from decision index".into());
    }
    for (i, choice) in p.history.iter().enumerate() {
        let Step::Decide(d) = next(&mut s, t, &mut x, &mut Replay) else {
            return Err(format!("replay ended at {i}"));
        };
        let c = choice
            .find(t, view(&s, t, &mut x), &d)
            .ok_or_else(|| format!("replay drift at {i}"))?;
        apply(&mut s, t, &mut x, &mut Replay, c);
    }
    let Step::Decide(d) = next(&mut s, t, &mut x, &mut Replay) else {
        return Err("replay ended before candidate".into());
    };
    let displayed = view(&s, t, &mut x);
    if labels(t, displayed, &d) != p.labels
        || d.player.get() != p.seat
        || s.turn().number() != p.turn
        || format!("{:?}", d.kind) != p.kind
    {
        return Err("candidate position drifted".into());
    }
    Ok((s, d))
}

fn inspect(path: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let p: Packet = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let t = mirror();
    let (s, d) = reconstruct(&t, &p)?;
    if usize::from(p.chosen) >= d.len() {
        return Err("invalid chosen candidate".into());
    }
    let mut tr = Transcript::new(["champion", "champion"]);
    let mut x = Scratch::new();
    tr.decision(&t, view(&s, &t, &mut x), &d, Choice(p.chosen), &[]);
    println!(
        "Replay verified: {}\nOMNISCIENT context; judge only information available to P{}.\n{}",
        p.id,
        p.seat,
        tr.text()
    );
    Ok(())
}

pub fn main(args: &[String]) -> Result<(), String> {
    if let [flag, file] = args
        && flag == "--inspect"
    {
        return inspect(Path::new(file));
    }
    if args == ["--help"] {
        println!(
            "{USAGE}\nDeadline and Ctrl-C stop after the current search; completed packets survive interruption."
        );
        return Ok(());
    }
    let c = Config::parse(args)?;
    // create_dir, not create_dir_all: never append into an earlier run accidentally.
    if let Some(parent) = c.out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir(&c.out).map_err(|e| format!("{}: {e}", c.out.display()))?;
    for sub in ["candidates", "games"] {
        std::fs::create_dir(c.out.join(sub)).map_err(|e| e.to_string())?;
    }
    let stopped = Arc::new(AtomicBool::new(false));
    let signal = stopped.clone();
    ctrlc::set_handler(move || {
        signal.store(true, Ordering::Relaxed);
    })
    .map_err(|e| e.to_string())?;
    let clock = Clock {
        start: Instant::now(),
        duration: Duration::from_secs(c.seconds),
        stopped,
    };
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    std::fs::copy(&executable, c.out.join("beryl-hunt")).map_err(|e| e.to_string())?;
    let repo = executable
        .ancestors()
        .nth(4)
        .ok_or("cannot locate checkout")?;
    let revision = std::process::Command::new("git")
        .current_dir(repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
    let dirty = std::process::Command::new("git")
        .current_dir(repo)
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    write_json(
        &c.out.join("manifest.json"),
        &serde_json::json!({"schema":1, "config":c, "revision":revision, "working_tree_changes":dirty, "executable":executable, "champion":DEEP_SEARCHER.to_string(), "deck":"WU_CONTROL mirror", "duel_repeats":3, "review":REVIEW, "deadline":"wall time after startup; finishes current search", "progress_file":"progress.json"}),
    )?;
    files::write_whole(&c.out.join("REVIEW.txt"), REVIEW.as_bytes())?;
    println!(
        "Blunder discovery: {} seconds, {} steps/decision.\nResults: {}\nStop: Ctrl-C (finishes current search).",
        c.seconds,
        c.work,
        c.out.display()
    );
    let result = run(&c, &clock);
    write_json(
        &c.out.join("status.json"),
        &serde_json::json!({
            "status": if result.is_err() { "failed" } else if clock.stopped.load(Ordering::Relaxed) { "interrupted" } else { "finished" },
            "elapsed_seconds": clock.start.elapsed().as_secs(), "error": result.as_ref().err()
        }),
    )?;
    result
}

fn run(c: &Config, clock: &Clock) -> Result<(), String> {
    let t = mirror();
    let mut rng = SplitMix64::new(c.seed);
    let mut index = Vec::new();
    let mut games = 0;
    write_json(&c.out.join("index.json"), &index)?;
    write_json(&c.out.join("shortlist.json"), &index)?;
    while !clock.done() {
        let seed = rng.next_u64();
        let (mut pending, transcript, ended) = collect(&t, c, clock, seed, games, 4000)?;
        files::write_whole(
            &c.out.join(format!("games/{games:05}.txt")),
            transcript.as_bytes(),
        )?;
        // The reservoirs already randomize across the game. At most two per decision kind
        // in the disagreement stream, plus audits, keeps the recheck budget diverse.
        let mut kinds = BTreeMap::new();
        pending.retain(|p| {
            let audit = p.packet.reasons.iter().any(|r| r == "random audit");
            let count = kinds.entry(p.packet.kind.clone()).or_insert(0);
            let keep = audit || *count < 2;
            if keep {
                *count += 1;
            }
            keep
        });
        // Publish the entire batch before any expensive recheck. Even an engine/search
        // abort leaves all of this game's selected positions available to the reviewer.
        let first = index.len();
        for p in &pending {
            write_json(
                &c.out.join(format!("candidates/{}.json", p.packet.id)),
                &p.packet,
            )?;
            index.push(entry(&p.packet));
        }
        write_json(&c.out.join("index.json"), &index)?;
        write_json(&c.out.join("shortlist.json"), &shortlist(&index))?;
        for (offset, mut p) in pending.into_iter().enumerate() {
            let path = c.out.join(format!("candidates/{}.json", p.packet.id));
            // Save before checking: reaching the deadline never discards collected evidence.
            write_json(&path, &p.packet)?;
            for &alternative in &p.packet.alternatives.clone() {
                for _ in 0..3 {
                    if clock.done() {
                        break;
                    }
                    let seed = rng.next_u64();
                    let check = duel(
                        &t,
                        c.settings(),
                        seed,
                        &p.state,
                        &p.decision,
                        Choice(alternative),
                        Choice(p.packet.chosen),
                    );
                    p.packet.comparisons.push(Comparison {
                        alternative,
                        seed,
                        alternative_lead: check.lead,
                        standard_error: check.error,
                        trees: check.trees,
                    });
                    write_json(&path, &p.packet)?;
                }
            }
            p.packet.checks_complete =
                p.packet.comparisons.len() == p.packet.alternatives.len() * 3;
            write_json(&path, &p.packet)?;
            index[first + offset] = entry(&p.packet);
            write_json(&c.out.join("index.json"), &index)?;
            write_json(&c.out.join("shortlist.json"), &shortlist(&index))?;
        }
        games += 1;
        write_json(
            &c.out.join("progress.json"),
            &serde_json::json!({"games":games,"last_game_completed":ended,"candidates":index.len(),"elapsed_seconds":clock.start.elapsed().as_secs(),"status":if clock.done() {"stopped"} else {"running"}}),
        )?;
        eprintln!(
            "{games} games sampled; {} candidates saved; {}s elapsed",
            index.len(),
            clock.start.elapsed().as_secs()
        );
    }
    println!(
        "Review bundle: {}\n{} candidates; {} shortlisted. Start with REVIEW.txt.",
        c.out.display(),
        index.len(),
        shortlist(&index).len()
    );
    Ok(())
}

fn proposals(
    search: Option<&beryl_search::Search>,
    greedy: Choice,
    expert: Choice,
    chosen: Choice,
) -> (Vec<u16>, Vec<String>) {
    let mut alternatives = Vec::new();
    let mut reasons = Vec::new();
    for (name, proposal) in [
        ("greedy disagreement", greedy),
        ("frozen expert disagreement", expert),
    ] {
        if proposal != chosen {
            reasons.push(name.to_owned());
            if !alternatives.contains(&proposal.0) {
                alternatives.push(proposal.0);
            }
        }
    }
    if let Some(search) = search {
        let best = search
            .means
            .iter()
            .enumerate()
            .filter_map(|(i, m)| m.map(|v| (i as u16, v)))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((best, value)) = best
            && best != chosen.0
            && search
                .means
                .get(usize::from(chosen.0))
                .copied()
                .flatten()
                .is_some_and(|v| value > v + 0.01)
        {
            reasons.push("search mean prefers another move".into());
            if !alternatives.contains(&best) {
                alternatives.push(best);
            }
        }
    }
    (alternatives, reasons)
}

fn collect(
    t: &GameTable,
    c: &Config,
    clock: &Clock,
    seed: u64,
    game: u32,
    max_decisions: usize,
) -> Result<(Vec<Pending>, String, bool), String> {
    let mut s = State::new(t, seed, Profile::SMART).map_err(|e| e.to_string())?;
    let mut x = Scratch::recording();
    let mut players = seat_seeds(seed).map(|seed| LongSearch::new(t, c.settings(), seed));
    let mut experts = seat_seeds(seed).map(|seed| {
        (agent("expert-frozen").expect("registered expert").new)(t, seed, Budget::DEFAULT)
    });
    let policy = RolloutPolicy::with(t, DEEP_SEARCHER.knowledge);
    let mut tr = Transcript::new(["champion", "champion"]);
    let mut history = Vec::new();
    let mut suspects = Vec::new();
    let mut audits = Vec::new();
    let mut counts = [0, 0];
    let mut rng = SplitMix64::new(seed ^ 0xCA11_DA7A);
    let mut ended = false;
    while !clock.done() && history.len() < max_decisions {
        let d = match next(&mut s, t, &mut x, &mut Replay) {
            Step::Over(outcome) => {
                tr.end(t, &s, outcome, &x.events.drain().collect::<Vec<_>>());
                ended = true;
                break;
            }
            Step::Decide(d) => d,
        };
        if history.len() % 50 == 0 {
            eprintln!(
                "game {game}, turn {}, decision {} ({}s elapsed)",
                s.turn().number(),
                history.len(),
                clock.start.elapsed().as_secs()
            );
        }
        let seat = d.player.index();
        let chosen = players[seat].choose(observe(&s, t, &mut x, d.player), &d);
        let greedy = policy.choose(t, observe(&s, t, &mut x, d.player), &d);
        let expert = experts[seat].choose(observe(&s, t, &mut x, d.player), &d);
        let state = *view(&s, t, &mut x);
        let events = x.events.drain().collect::<Vec<_>>();
        tr.note(&format!("decision {}", history.len()));
        tr.decision(t, &state, &d, chosen, &events);
        if d.len() > 1 {
            let search = players[seat].last_search();
            let (mut alternatives, mut reasons) = proposals(search, greedy, expert, chosen);
            let audit = alternatives.is_empty();
            if audit {
                reasons.push("random audit".into());
                // Uniform alternative excluding the chosen move, explicitly not a recommendation.
                let a = (rng.next_u64() % (d.len() - 1) as u64) as u16;
                alternatives.push(if a >= chosen.0 { a + 1 } else { a });
            }
            let mut context = Transcript::new(["champion", "champion"]);
            context.decision(t, &state, &d, chosen, &[]);
            let p = Pending {
                // Keep the committed root, not `view`: during resolution the displayed
                // position is a scratch preview and cannot be advanced by apply/duel.
                state: s,
                decision: d,
                packet: Packet {
                    id: format!("g{game:05}-d{:05}", history.len()),
                    seed,
                    decision: history.len(),
                    turn: state.turn().number(),
                    seat: d.player.get(),
                    kind: format!("{:?}", d.kind),
                    reasons,
                    history: history.clone(),
                    labels: labels(t, &state, &d),
                    chosen: chosen.0,
                    alternatives,
                    visits: search.map_or_else(Vec::new, |s| s.visits.clone()),
                    means: search.map_or_else(Vec::new, |s| s.means.clone()),
                    context: context.text().to_owned(),
                    game_transcript: format!("games/{game:05}.txt"),
                    comparisons: Vec::new(),
                    checks_complete: false,
                },
            };
            let stream = usize::from(audit);
            counts[stream] += 1;
            if audit {
                reservoir(&mut audits, counts[stream], 2, p, &mut rng);
            } else {
                reservoir(&mut suspects, counts[stream], 12, p, &mut rng);
            }
        }
        history.push(Chosen::new(t, &state, &d, chosen));
        apply(&mut s, t, &mut x, &mut Replay, chosen);
    }
    s.check_invariants(t)
        .map_err(|e| format!("game {game}: {e:?}"))?;
    if !ended {
        tr.note("PARTIAL GAME: deadline, interrupt, or 4000-decision cap.");
    }
    suspects.extend(audits);
    Ok((suspects, tr.text().to_owned(), ended))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_limits() {
        for args in [
            ["--hours", "NaN"],
            ["--hours", "-1"],
            ["--seconds", "0"],
            ["--threads", "0"],
            ["--work", "0"],
        ] {
            assert!(Config::parse(&args.map(str::to_owned)).is_err());
        }
        assert_eq!(Config::parse(&[]).unwrap().seconds, 21600);
    }

    #[test]
    fn reservoir_is_bounded_and_not_just_the_opening() {
        let mut rng = SplitMix64::new(42);
        let mut items = Vec::new();
        for i in 1..=1000 {
            reservoir(&mut items, i, 12, i, &mut rng);
        }
        assert_eq!(items.len(), 12);
        assert!(items.iter().any(|&i| i > 500));
    }

    #[test]
    fn collected_positions_replay_and_detect_drift() {
        let t = mirror();
        let seed = 7;
        let mut s = State::new(&t, seed, Profile::SMART).unwrap();
        let mut x = Scratch::new();
        let policy = RolloutPolicy::with(&t, DEEP_SEARCHER.knowledge);
        let mut history = Vec::new();
        for _ in 0..200 {
            let Step::Decide(d) = next(&mut s, &t, &mut x, &mut Replay) else {
                break;
            };
            let choice = policy.choose(&t, observe(&s, &t, &mut x, d.player), &d);
            let state = *view(&s, &t, &mut x);
            let p = Packet {
                seed,
                decision: history.len(),
                turn: state.turn().number(),
                seat: d.player.get(),
                kind: format!("{:?}", d.kind),
                history: history.clone(),
                labels: labels(&t, &state, &d),
                chosen: choice.0,
                ..Packet::default()
            };
            let (mut root, replayed) = reconstruct(&t, &p).unwrap();
            assert_eq!(replayed.len(), d.len());
            let mut bad = p;
            bad.labels[0].push_str(" drift");
            assert!(reconstruct(&t, &bad).is_err());
            history.push(Chosen::new(&t, &state, &d, choice));
            apply(&mut s, &t, &mut x, &mut Replay, choice);
            let mut replay_scratch = Scratch::new();
            apply(&mut root, &t, &mut replay_scratch, &mut Replay, choice);
            assert_eq!(format!("{root:?}"), format!("{s:?}"));
        }
        assert!(history.len() > 20);
    }

    #[test]
    fn ranking_requires_three_supported_independent_checks() {
        let mut p = Packet {
            labels: vec!["chosen".into(), "alternative".into()],
            alternatives: vec![1],
            ..Packet::default()
        };
        for seed in 0..2 {
            p.comparisons.push(Comparison {
                alternative: 1,
                seed,
                alternative_lead: 0.2,
                standard_error: 0.01,
                trees: 72,
            });
        }
        assert!(entry(&p).score.abs() < f64::EPSILON);
        p.comparisons.push(Comparison {
            alternative: 1,
            seed: 2,
            alternative_lead: 0.1,
            standard_error: 0.01,
            trees: 72,
        });
        assert!((entry(&p).score - 0.08).abs() < 1e-9);
        p.comparisons[2].trees = 1;
        assert!(entry(&p).score.abs() < f64::EPSILON);
    }

    #[test]
    fn clock_honors_interrupt_before_deadline() {
        let stopped = Arc::new(AtomicBool::new(false));
        let clock = Clock {
            start: Instant::now(),
            duration: Duration::from_secs(100),
            stopped: stopped.clone(),
        };
        assert!(!clock.done());
        stopped.store(true, Ordering::Relaxed);
        assert!(clock.done());
    }

    #[test]
    fn shortlist_keeps_audits_and_limits_repeated_choices() {
        let entries: Vec<_> = (0..40)
            .map(|i| Entry {
                file: i.to_string(),
                kind: "Priority".into(),
                chosen: if i < 30 { "same".into() } else { i.to_string() },
                audit: i >= 35,
                score: f64::from(40 - i),
                checks: 3,
            })
            .collect();
        let list = shortlist(&entries);
        assert_eq!(list.iter().filter(|e| e.chosen == "same").count(), 2);
        assert_eq!(list.iter().filter(|e| e.audit).count(), 4);
        assert!(list.len() <= 20);
    }
}
