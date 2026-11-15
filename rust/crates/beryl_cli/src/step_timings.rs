//! `beryl_cli step-timings <definitions.json> <pack.json> [--fixtures-only]`: the timing the
//! session sent with every step of every recording, beside the recordings themselves, for
//! `just timing-parity`. It records each UI fixture `definitions` defines, from its own seat,
//! and unless `--fixtures-only`, each card spec moment from both seats. It writes the
//! recordings' views to `pack.json`, which `ui/scripts/timings.mjs` times with the web
//! interface's rules, and prints one JSON line per step with the script's own
//! [`Script::timing`](beryl_session::script::Script::timing), in the same form as that
//! script's lines.

use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use beryl_session::ipc::{FixtureMessage, GAME_UPDATE};
use beryl_session::protocol::PlayerView;
use beryl_session::script::{Table, replay};
use beryl_session::{GameUpdate, Seat};
use serde::Serialize;

use crate::fixture;
use crate::specs::{self, View};

/// One seat's views of one fixture or spec moment, in the order the seat was sent them.
#[derive(Serialize)]
struct Recording {
    /// The fixture's name, or `<card>/<moment>` for a spec moment.
    name: String,
    /// `yours` or `theirs`.
    seat: &'static str,
    views: Vec<PlayerView>,
}

pub fn main(definitions: &Path, pack: &Path, fixtures_only: bool) -> Result<(), String> {
    let mut recordings = fixtures(definitions)?;
    if !fixtures_only {
        recordings.extend(moments());
    }
    let json = serde_json::to_string(&recordings).expect("views serialize");
    std::fs::write(pack, json).map_err(|e| format!("{}: {e}", pack.display()))?;
    let mut out = String::new();
    for recording in &recordings {
        // The table as the stage holds it between views: the last script's replay, which
        // keeps the numbers the scripts gave the abilities on its stack.
        let mut carried: Option<Table> = None;
        for (n, view) in recording.views.iter().enumerate() {
            let Some(script) = &view.script else {
                // The stage starts over from a view with no script, numbering no ability.
                carried = None;
                continue;
            };
            // With a reset the script starts over from its own view's table; with none, it
            // plays over the table the scripts before it left. A view the web cannot follow
            // is left out on both sides.
            let before = if script.reset.is_some() {
                Table::of(view)
            } else if let Some(table) = carried.take() {
                table
            } else {
                continue;
            };
            if script.timing.len() != script.steps() {
                return Err(format!(
                    "{} {} view {}: {} steps and {} timings",
                    recording.name,
                    recording.seat,
                    n + 1,
                    script.steps(),
                    script.timing.len()
                ));
            }
            for (k, timing) in script.timing.iter().enumerate() {
                let line = serde_json::json!({
                    "fixture": recording.name,
                    "seat": recording.seat,
                    "view": n + 1,
                    "step": k + 1,
                    "timing": timing,
                });
                out.push_str(&line.to_string());
                out.push('\n');
            }
            carried = replay(before, script).ok();
        }
    }
    print!("{out}");
    Ok(())
}

/// Each fixture's recording from its human's seat. A fixture that cannot be recorded fails
/// the run: the parity check would otherwise pass over it.
fn fixtures(definitions: &Path) -> Result<Vec<Recording>, String> {
    let defs = fixture::definitions(definitions)?;
    parallel(&defs, |def| {
        let seat = match def.seat()? {
            Seat::P0 => View::Yours,
            Seat::P1 => View::Theirs,
        };
        let messages = fixture::record(def, definitions)
            .map_err(|e| format!("fixture {}: {e}", def.name()))?;
        let views = messages
            .iter()
            .filter_map(|message| match message {
                FixtureMessage::Event { event, payload } if event == GAME_UPDATE => {
                    serde_json::from_value::<GameUpdate>(payload.clone()).ok()
                }
                _ => None,
            })
            .map(|update| update.view)
            .collect();
        Ok(Recording {
            name: def.name().to_owned(),
            seat: seat.name(),
            views,
        })
    })
    .into_iter()
    .collect()
}

/// Each card spec moment from both seats, as its page shows it. A moment no seat reaches is
/// `specs-check`'s to report, and is left out here.
fn moments() -> Vec<Recording> {
    let all: Vec<&specs::Spec> = specs::cards().iter().collect();
    specs::generate(&all);
    let chosen: Vec<_> = all
        .iter()
        .flat_map(|spec| spec.moments().iter().map(move |moment| (*spec, moment)))
        .flat_map(|(spec, moment)| View::ALL.map(|view| (spec, moment, view)))
        .collect();
    parallel(&chosen, |(spec, moment, view)| {
        let (_, views) = specs::trace(moment, *view).ok()?;
        Some(Recording {
            name: format!("{}/{}", spec.slug, moment.name),
            seat: view.name(),
            views,
        })
    })
    .into_iter()
    .flatten()
    .collect()
}

/// `make` of each of `items`, on every core, in the order of `items`.
fn parallel<T: Sync, R: Send>(items: &[T], make: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let done = Mutex::new(Vec::with_capacity(items.len()));
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .min(items.len().max(1));
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(i) else { break };
                    let result = make(item);
                    done.lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push((i, result));
                }
            });
        }
    });
    let mut done = done
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    done.sort_by_key(|(i, _)| *i);
    done.into_iter().map(|(_, made)| made).collect()
}
