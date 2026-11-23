//! Builds the native recordings pack with `beryl_cli`'s fixture and card spec recorders.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use beryl_cli::specs::{self, Moment, View};
use beryl_cli::{Catalog, fixture};
use beryl_session::ipc::FixtureMessage;
use beryl_session::{GameUpdate, Seat, SessionId, card_records};

use beryl_inspect::pack::{Recorded, Recorder};
use beryl_inspect::recordings::FIXTURE_DEFINITIONS;

/// Every implemented card and what is built from their files, which the recorders play with.
/// This program links the cards and hands them to `beryl_cli`'s library, which names no card
/// crate, so an edit to a card file recompiles none of that library.
static CATALOG: Catalog = Catalog {
    cards: &beryl_cards::CARDS,
    scenarios: beryl_cards::scenarios,
    library_scenarios: || beryl_cards::library::by_card().collect(),
    derived_scenarios: |def| beryl_cards::derive::for_card(def).scenarios,
    fuzz_deck_from: beryl_cards::decks::fuzz_deck_from,
    keystone_family: beryl_cards::decks::keystone_family,
};

/// `beryl_cli`'s recorders, reading the fixture definitions in the repository at `root`.
pub struct Cli {
    pub root: PathBuf,
    pub selection: Selection,
}

/// Which fixtures and card spec moments an invocation records.
pub enum Selection {
    Corpus(BTreeSet<String>),
    All,
    Prefix(String),
    Cards(BTreeSet<String>),
}

impl Selection {
    pub fn parse(root: &Path, arguments: &[String]) -> Result<Self, String> {
        match arguments {
            [] => beryl_inspect::corpus::names(root).map(Self::Corpus),
            [all] if all == "--all" => Ok(Self::All),
            [switch, prefix] if switch == "--prefix" && !prefix.is_empty() => {
                Ok(Self::Prefix(prefix.trim_start_matches("b3d:").to_owned()))
            }
            [switch, cards @ ..] if switch == "--cards" => {
                Ok(Self::Cards(cards.iter().cloned().collect()))
            }
            _ => Err("usage: record [--all | --prefix <recording> | --cards <card>...]".into()),
        }
    }

    fn fixture(&self, name: &str) -> bool {
        match self {
            Self::All | Self::Corpus(_) => true,
            Self::Prefix(prefix) => matches_prefix(name, prefix),
            Self::Cards(_) => false,
        }
    }

    fn card(&self, spec: &specs::Spec) -> bool {
        match self {
            Self::All => true,
            Self::Corpus(names) => {
                let prefix = format!("{}/", spec.slug);
                names
                    .range(prefix.clone()..)
                    .next()
                    .is_some_and(|name| name.starts_with(&prefix))
            }
            Self::Prefix(prefix) => {
                spec.slug.starts_with(prefix.as_str())
                    || prefix
                        .strip_prefix(&spec.slug)
                        .is_some_and(|rest| rest.starts_with('/'))
            }
            Self::Cards(cards) => cards.contains(spec.card),
        }
    }

    fn moment(&self, name: &str) -> bool {
        match self {
            Self::Corpus(names) => names.contains(name),
            Self::Prefix(prefix) => matches_prefix(name, prefix),
            Self::All | Self::Cards(_) => true,
        }
    }
}

/// A test prefix may extend past the recording's name into the seat and window.
fn matches_prefix(name: &str, prefix: &str) -> bool {
    name.starts_with(prefix)
        || prefix
            .strip_prefix(name)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// One recording still to make.
enum Job<'a> {
    Fixture(&'a fixture::Definition, Seat),
    Moment(&'a str, &'static Moment, View),
}

impl Recorder for Cli {
    /// Every selected fixture and card spec moment, from both seats, on as many threads as
    /// Ignite grants. A fixture that has one viewer (a manual table,
    /// or a board with a prop, which stands only in its human's view) is recorded from that
    /// seat alone ([`fixture::Definition::seats`]).
    fn record(&self) -> Vec<Recorded> {
        let source = self.root.join(FIXTURE_DEFINITIONS);
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
        let mut cards: Vec<&specs::Spec> = specs::cards(&CATALOG)
            .iter()
            .filter(|spec| self.selection.card(spec))
            .collect();
        // A specific spec can also come from a card implemented directly by the store.
        if let Selection::Prefix(prefix) = &self.selection
            && let Some((slug, _)) = prefix.split_once('/')
            && cards.is_empty()
            && let Some(spec) = specs::spec(&CATALOG, slug)
        {
            cards.push(spec);
        }
        // Play only the selected cards' candidates to discover their available moments.
        let next = AtomicUsize::new(0);
        beryl_inspect::runner::spread(cards.len(), &next, || {
            while let Some(spec) = cards.get(next.fetch_add(1, Ordering::Relaxed)) {
                spec.moments();
            }
        });
        let mut jobs = Vec::new();
        for def in definitions
            .iter()
            .filter(|def| self.selection.fixture(def.name()))
        {
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
                if self
                    .selection
                    .moment(&format!("{}/{}", spec.slug, moment.name))
                {
                    jobs.extend(View::ALL.map(|view| Job::Moment(&spec.slug, moment, view)));
                }
            }
        }
        let recorded = Mutex::new(done);
        let next = AtomicUsize::new(0);
        beryl_inspect::runner::spread(jobs.len(), &next, || {
            while let Some(job) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) {
                let one = record(&CATALOG, job, &source);
                recorded
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(one);
            }
        });
        recorded
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn record(catalog: &'static Catalog, job: &Job, source: &std::path::Path) -> Recorded {
    match job {
        Job::Fixture(def, seat) => Recorded {
            name: def.name().to_owned(),
            seat: *seat,
            messages: fixture::record_from(catalog, def, source, *seat)
                .map(|messages| with_card_records(catalog, messages))
                .map_err(|why| format!("fixture {}: {why}", def.name())),
        },
        Job::Moment(card, moment, view) => Recorded {
            name: format!("{card}/{}", moment.name),
            seat: match view {
                View::Yours => Seat::P0,
                View::Theirs => Seat::P1,
            },
            messages: specs::trace(catalog, moment, *view)
                .map(|(_, views)| {
                    let session = SessionId(1);
                    let messages = (views.into_iter())
                        .map(|view| FixtureMessage::game_update(&GameUpdate { session, view }))
                        .collect();
                    with_card_records(catalog, messages)
                })
                .map_err(|why| format!("{card}/{}: {why}", moment.name)),
        },
    }
}

/// `messages` headed by the card records of every card they name, as `beryl_cli`'s fixture
/// files are.
fn with_card_records(catalog: &Catalog, mut messages: Vec<FixtureMessage>) -> Vec<FixtureMessage> {
    messages.insert(0, card_records::fixture_message(&messages, catalog.cards));
    messages
}
