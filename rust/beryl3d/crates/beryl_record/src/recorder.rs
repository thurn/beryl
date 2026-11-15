//! `beryl_cli`'s recorders, which make the web interface's fixtures and card spec moments, run
//! for every recording the pack holds.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use beryl_cli::fixture;
use beryl_cli::specs::{self, Moment, View};
use beryl_session::ipc::FixtureMessage;
use beryl_session::{GameUpdate, Seat, SessionId, card_records};

use beryl_inspect::pack::{Recorded, Recorder};

/// `beryl_cli`'s recorders, reading the fixture definitions in the repository at `root`.
pub struct Cli {
    pub root: PathBuf,
}

/// One recording still to make.
enum Job<'a> {
    Fixture(&'a fixture::Definition, Seat),
    Moment(&'a str, &'static Moment, View),
}

impl Recorder for Cli {
    /// Every fixture in `ui/fixtures.json` and every card spec moment, from both seats, on as
    /// many threads as the machine has cores. A fixture that has one viewer (a manual table,
    /// or a board with a prop, which stands only in its human's view) is recorded from that
    /// seat alone ([`fixture::Definition::seats`]).
    fn record(&self) -> Vec<Recorded> {
        let source = self.root.join("ui/fixtures.json");
        let mut done = Vec::new();
        let definitions = match fixture::definitions(&source) {
            Ok(definitions) => definitions,
            Err(why) => {
                // No fixture can be named, so the failure is a recording of its own.
                done.push(Recorded {
                    name: "fixtures".to_owned(),
                    seat: Seat::P0,
                    messages: Err(why),
                });
                Vec::new()
            }
        };
        let cards: Vec<&specs::Spec> = specs::cards().iter().collect();
        // Plays every candidate moment of every card, which decides which are kept.
        specs::generate(&cards);
        let mut jobs = Vec::new();
        for def in &definitions {
            match def.seats() {
                Ok(seats) => jobs.extend(seats.into_iter().map(|seat| Job::Fixture(def, seat))),
                Err(why) => done.push(Recorded {
                    name: def.name().to_owned(),
                    seat: Seat::P0,
                    messages: Err(format!("fixture {}: {why}", def.name())),
                }),
            }
        }
        for spec in &cards {
            for moment in spec.moments() {
                jobs.extend(View::ALL.map(|view| Job::Moment(&spec.slug, moment, view)));
            }
        }
        let recorded = Mutex::new(done);
        let next = AtomicUsize::new(0);
        let threads = std::thread::available_parallelism()
            .map_or(1, std::num::NonZero::get)
            .min(jobs.len().max(1));
        std::thread::scope(|scope| {
            for _ in 0..threads {
                scope.spawn(|| {
                    while let Some(job) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) {
                        let one = record(job, &source);
                        recorded
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .push(one);
                    }
                });
            }
        });
        recorded
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn record(job: &Job, source: &std::path::Path) -> Recorded {
    match job {
        Job::Fixture(def, seat) => Recorded {
            name: def.name().to_owned(),
            seat: *seat,
            messages: fixture::record_from(def, source, *seat)
                .map(with_card_records)
                .map_err(|why| format!("fixture {}: {why}", def.name())),
        },
        Job::Moment(card, moment, view) => Recorded {
            name: format!("{card}/{}", moment.name),
            seat: match view {
                View::Yours => Seat::P0,
                View::Theirs => Seat::P1,
            },
            messages: specs::trace(moment, *view)
                .map(|(_, views)| {
                    let session = SessionId(1);
                    let messages = (views.into_iter())
                        .map(|view| FixtureMessage::game_update(&GameUpdate { session, view }))
                        .collect();
                    with_card_records(messages)
                })
                .map_err(|why| format!("{card}/{}: {why}", moment.name)),
        },
    }
}

/// `messages` headed by the card records of every card they name, as `beryl_cli`'s fixture
/// files are.
fn with_card_records(mut messages: Vec<FixtureMessage>) -> Vec<FixtureMessage> {
    messages.insert(0, card_records::fixture_message(&messages));
    messages
}
