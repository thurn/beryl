//! `script-check`: the script checks (plan K24) as checks of their own, one per moment, seat and
//! recording, as agreed with the CI track (epic by-w8gv.21). It plays every card's every spec
//! candidate from both seats to its first live decision (each kept moment is one) and records
//! every UI fixture, in a debug build, where each script the session builds is held to the
//! replay check and the rule invariants. A panic there fails the one check it happened in and
//! the others go on: `script:<card>/<moment>/<seat>`, or `script:fixtures/<fixture>/<seat>`
//! for a recording, which also fails when it cannot reach its stop.
//!
//! Each check's result goes to the run log through the file `$BERYL_CHECK_RESULTS` names
//! (`tools/src/beryl_tools/runlog.py`), one JSON line each. A failure prints `FAILED: <check>`
//! with its panic, the line a rerun narrows to (`tools/src/beryl_tools/reruns.py`).
//!
//! `--dump <dir>` also writes every script as text under the directory, one file per moment
//! and seat (`<card>/<moment>/<seat>.txt`) and per recording (`fixtures/<fixture>.txt`): two
//! dumps, one from before a change to the builder and one from after, compared with `diff -r`,
//! say which scripts the change changed.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use beryl_session::GameUpdate;
use beryl_session::ipc::{FixtureMessage, GAME_UPDATE};

use crate::fixture::{self, Definition};
use crate::specs::{self, View};

/// The run-log id's prefix for these checks.
const LANE: &str = "script";
/// What stands in a recording's id where a moment's has its card.
const FIXTURES: &str = "fixtures";

/// One check's result: its seconds and, failed, why.
#[derive(Default)]
struct Check {
    seconds: f64,
    failure: Option<String>,
}

/// Runs the checks: every one, or those `filters` name, each a check's id or what one begins
/// with (`script:` optional): `counterspell` is every check of its spec, and
/// `fixtures/stack-two` both seats of a recording. `fixtures` is the definitions file. With
/// `--dump <dir>` first, the scripts of the checks run are written under `dir`.
pub fn main(fixtures: &Path, filters: &[String]) -> Result<(), String> {
    let (dump, filters) = match filters {
        [flag, dir, rest @ ..] if flag == "--dump" => (Some(Path::new(dir)), rest),
        _ => (None, filters),
    };
    if cfg!(not(debug_assertions)) {
        return Err("script-check needs a debug build: the script checks are assertions".into());
    }
    let started = Instant::now();
    let filters: Vec<&str> = filters
        .iter()
        .map(|f| f.strip_prefix("script:").unwrap_or(f).trim_end_matches('/'))
        .collect();
    let mut checks = BTreeMap::new();
    let chosen: Vec<&specs::Spec> = specs::cards()
        .iter()
        .filter(|s| group_wanted(&filters, &s.slug))
        .collect();
    moments(&chosen, &filters, &mut checks);
    let definitions = fixture::definitions(fixtures)?;
    let recorded = recordings(&definitions, &filters, &mut checks);
    if let Some(dir) = dump {
        dump_moments(dir, &chosen, &filters)?;
        for (name, text) in recorded {
            write(&dir.join(FIXTURES).join(format!("{name}.txt")), &text)?;
        }
    }
    let unwatched = crate::panics::unwatched();
    if !unwatched.is_empty() {
        // A session that panicked before its first view: which moment's, nothing can say.
        let failure = Some(unwatched.join("\n"));
        checks.insert(
            "unwatched".to_owned(),
            Check {
                seconds: 0.0,
                failure,
            },
        );
    }
    report(&checks, started.elapsed().as_secs_f64())
}

/// Whether `filters` take the check `id`.
fn wanted(filters: &[&str], id: &str) -> bool {
    filters.is_empty()
        || filters
            .iter()
            .any(|f| id == *f || id.strip_prefix(*f).is_some_and(|r| r.starts_with('/')))
}

/// Whether `filters` take a check under `group`, a card's slug or [`FIXTURES`].
fn group_wanted(filters: &[&str], group: &str) -> bool {
    filters.is_empty() || filters.iter().any(|f| f.split('/').next() == Some(group))
}

fn add(checks: &mut BTreeMap<String, Check>, id: String, seconds: f64, failure: Option<String>) {
    let check = checks.entry(id).or_default();
    check.seconds += seconds;
    check.failure = check.failure.take().or(failure);
}

/// The candidates of `chosen`'s specs, played from both seats as their generation does: a kept
/// moment's seat is a check, and so is any seat that panicked, kept or not.
fn moments(chosen: &[&specs::Spec], filters: &[&str], checks: &mut BTreeMap<String, Check>) {
    specs::generate(chosen);
    for spec in chosen {
        for played in spec.played() {
            let kept = spec.moments().iter().any(|m| m.name == played.name);
            let id = format!("{}/{}/{}", spec.slug, played.name, played.view.name());
            if (kept || played.panic.is_some()) && wanted(filters, &id) {
                add(checks, id, played.seconds, played.panic.clone());
            }
        }
    }
}

/// Each fixture recorded, on as many threads as the machine has cores: the scripts of each
/// that could be, as text, by its name.
fn recordings(
    definitions: &[Definition],
    filters: &[&str],
    checks: &mut BTreeMap<String, Check>,
) -> Vec<(String, String)> {
    if !group_wanted(filters, FIXTURES) {
        return Vec::new();
    }
    let chosen: Vec<(String, &Definition)> = definitions
        .iter()
        .map(|def| {
            let seat = def.seat().ok();
            let view = View::ALL.into_iter().find(|v| Some(v.seat()) == seat);
            let seat = view.unwrap_or(View::Yours).name();
            (format!("{FIXTURES}/{}/{seat}", def.name()), def)
        })
        .filter(|(id, _)| wanted(filters, id))
        .collect();
    let done = Mutex::new(Vec::new());
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .min(chosen.len());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                while let Some((id, def)) = chosen.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let started = Instant::now();
                    let recorded = fixture::record(def);
                    let seconds = started.elapsed().as_secs_f64();
                    let scripts = recorded.as_deref().ok().map(scripts);
                    let mut done = done
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    done.push((id.clone(), def.name(), seconds, recorded.err(), scripts));
                }
            });
        }
    });
    let mut texts = Vec::new();
    for (id, name, seconds, failure, scripts) in done
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
    {
        add(checks, id, seconds, failure);
        texts.extend(scripts.map(|text| (name.to_owned(), text)));
    }
    texts
}

/// The script of each view a recording sent, as text, headed by the view's number.
fn scripts(messages: &[FixtureMessage]) -> String {
    let views = messages.iter().filter_map(|message| match message {
        FixtureMessage::Event { event, payload } if event == GAME_UPDATE => {
            serde_json::from_value::<GameUpdate>(payload.clone()).ok()
        }
        _ => None,
    });
    let mut text = String::new();
    for (n, update) in views.enumerate() {
        let _ = writeln!(text, "-- view {}", n + 1);
        if let Some(script) = &update.view.script {
            for line in script.to_string().lines() {
                let _ = writeln!(text, "  {line}");
            }
        }
    }
    text
}

/// Writes under `dir` the scripts of each kept moment of `chosen` that `filters` take, from
/// each seat.
fn dump_moments(
    dir: &Path,
    chosen: &[&'static specs::Spec],
    filters: &[&str],
) -> Result<(), String> {
    for spec in chosen {
        for moment in spec.moments() {
            for view in View::ALL {
                let id = format!("{}/{}/{}", spec.slug, moment.name, view.name());
                if wanted(filters, &id) {
                    let text = specs::script_text(moment, view)
                        .unwrap_or_else(|unreached| format!("{unreached}\n"));
                    write(&dir.join(format!("{id}.txt")), &text)?;
                }
            }
        }
    }
    Ok(())
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    let said = |e| format!("{}: {e}", path.display());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(said)?;
    }
    std::fs::write(path, text).map_err(said)
}

/// Logs each check, prints each failure, and fails if one did.
fn report(checks: &BTreeMap<String, Check>, seconds: f64) -> Result<(), String> {
    let mut lines = String::new();
    let mut failed = 0;
    for (id, check) in checks {
        let outcome = if check.failure.is_some() {
            "fail"
        } else {
            "pass"
        };
        let line = serde_json::json!({
            "check": format!("{LANE}:{id}"),
            "outcome": outcome,
            "seconds": (check.seconds * 1000.0).round() / 1000.0,
        });
        lines.push_str(&line.to_string());
        lines.push('\n');
        if let Some(why) = &check.failure {
            failed += 1;
            println!("FAILED: {LANE}:{id}");
            for l in why.lines() {
                println!("    {l}");
            }
        }
    }
    if let Some(path) = std::env::var_os("BERYL_CHECK_RESULTS") {
        let logged = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| f.write_all(lines.as_bytes()));
        if let Err(e) = logged {
            eprintln!("run log: not written: {e}");
        }
    }
    println!(
        "script-check: {} checks, {failed} failed, in {seconds:.1} s",
        checks.len()
    );
    if failed == 0 {
        Ok(())
    } else {
        Err(format!("{failed} script checks failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_take_a_check_or_what_it_begins_with() {
        let id = "counterspell/start/yours";
        assert!(wanted(&[], id));
        assert!(wanted(&["counterspell"], id));
        assert!(wanted(&["counterspell/start"], id));
        assert!(wanted(&[id], id));
        assert!(!wanted(&["counter"], id));
        assert!(!wanted(&["counterspell/start/theirs"], id));
        assert!(group_wanted(&["counterspell/start/yours"], "counterspell"));
        assert!(!group_wanted(&["counterspell"], FIXTURES));
    }

    /// A moment whose session panics fails its own checks, from each seat, with the panic; the
    /// card's other moments and another card's pass.
    #[test]
    fn a_failing_moment_fails_only_its_own_checks() {
        use beryl_cards::names::{MANA_DRAIN, REPRIEVE};
        let scenarios = |card| {
            beryl_cards::SCENARIOS
                .iter()
                .find(|(name, _)| *name == card)
                .unwrap()
                .1
        };
        // A moment's name is its scenario's slug with the card's left off.
        let card = specs::slug(MANA_DRAIN);
        let [broken, whole] = [0, 1].map(|i| {
            let slug = specs::slug(scenarios(MANA_DRAIN)[i].name);
            slug.strip_prefix(&card)
                .unwrap()
                .trim_start_matches('-')
                .to_owned()
        });
        specs::FAULTS
            .lock()
            .unwrap()
            .push(scenarios(MANA_DRAIN)[0].name);
        let fresh = specs::fresh(&[MANA_DRAIN, REPRIEVE]);
        let chosen: Vec<&specs::Spec> = fresh.iter().collect();
        let mut checks = BTreeMap::new();
        moments(&chosen, &[], &mut checks);
        specs::FAULTS.lock().unwrap().clear();
        let under = |prefix: &str| {
            let prefix = format!("{card}/{prefix}");
            checks
                .iter()
                .filter(move |(id, _)| id.starts_with(&prefix))
                .collect::<Vec<_>>()
        };
        for seat in ["yours", "theirs"] {
            let failed: Vec<_> = under(&broken)
                .into_iter()
                .filter(|(id, _)| id.ends_with(seat))
                .collect();
            assert!(!failed.is_empty(), "no {seat} check of {broken}");
            for (id, check) in failed {
                let why = check.failure.as_deref().unwrap_or_default();
                assert!(why.contains("a fault injected into"), "{id}: {why:?}");
            }
        }
        let failed = format!("{card}/{broken}");
        let passed: Vec<&String> = checks
            .iter()
            .filter(|(id, _)| !id.starts_with(&failed))
            .map(|(id, check)| {
                assert!(check.failure.is_none(), "{id} failed");
                id
            })
            .collect();
        assert!(passed.iter().any(|id| id.contains(&whole)), "{passed:?}");
        assert!(passed.iter().any(|id| id.starts_with("reprieve/")));
    }
}
