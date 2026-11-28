//! Specs: for one card, every moment of play its UI must get right, each a real game state the
//! spec page (`/specs/<card>`, `just specs`) opens live and fully playable. A moment is a
//! board, the actions that set it up before the page shows it, the actions it then plays out
//! when the viewer presses play (a replay button starts it over), and what must hold of the
//! viewer's first live decision after that, which a test checks.
//!
//! The page shows each moment twice, side by side and played in step: from the human's seat,
//! and from the opponent's ([`View::Theirs`]), the same game with the viewer seated across the
//! table. What the opponent sees of a card (which ability was activated, which card a choice
//! took, where a token came from) matters as much as what its controller sees.
//!
//! Every implemented card has a spec: its moments are generated from its definition and its
//! scenarios ([`auto`]), each played headless from both seats to its first live decision, and
//! what holds there is recorded for `specs-describe`. A card's spec is generated when it is
//! first asked for, and no other card's with it ([`spec`]). A moment a card needs that nothing
//! generates comes from a scenario in its file, or a general capability of the generator: the
//! `specs` skill (`.agents/skills/specs/SKILL.md`) says which.

pub mod auto;
pub mod describe;
pub mod playground;
pub mod script;
pub mod start;

use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use beryl_engine::card::CardDef;
use beryl_scenario::{Action, Scenario};
use beryl_session::Seat;

use crate::Catalog;

use describe::reach;
pub use describe::{Unreached, describe, trace};
pub use script::scripts;
pub(crate) use script::text as script_text;
pub use start::{FAULTS, Phase, recorded_cast_moves, start};
use start::{review_position, start_guarded, start_review};

/// The specs, one per implemented card, each generated on its first request: asking for one
/// card's moments plays no other card's.
pub struct Library {
    catalog: &'static Catalog,
    pub specs: Vec<Spec>,
}

impl Library {
    pub fn new(catalog: &'static Catalog) -> Library {
        Library {
            catalog,
            specs: (catalog.cards.cards().iter())
                .map(|&def| Spec::new(catalog, def))
                .collect(),
        }
    }

    pub fn find(&self, card: &str) -> Option<&Spec> {
        self.specs.iter().find(|s| s.card == card || s.slug == card)
    }
}

/// The library of `catalog`'s cards, made on the first call. A process plays with one catalog,
/// the one its binary links, so the library made for the first is kept for every later call.
fn library(catalog: &'static Catalog) -> &'static Library {
    static LIBRARY: OnceLock<Library> = OnceLock::new();
    let library = LIBRARY.get_or_init(|| Library::new(catalog));
    debug_assert!(
        std::ptr::eq(library.catalog, catalog),
        "one catalog per process"
    );
    library
}

/// Every implemented card's spec, generating none.
pub fn cards(catalog: &'static Catalog) -> &'static [Spec] {
    &library(catalog).specs
}

/// The spec of the card `card` names, by its name or its slug, generating nothing. A card
/// complete from the card store, which has no file, gets its spec on its first request, and no
/// list of every card's spec ([`cards`]) holds it: there are hundreds.
pub fn spec(catalog: &'static Catalog, card: &str) -> Option<&'static Spec> {
    library(catalog)
        .find(card)
        .or_else(|| from_store(catalog, card))
}

/// The spec of the card complete from the store that `card` names, by its name or its slug,
/// made once and kept.
fn from_store(catalog: &'static Catalog, card: &str) -> Option<&'static Spec> {
    static MADE: std::sync::Mutex<Vec<&'static Spec>> = std::sync::Mutex::new(Vec::new());
    let def = (catalog.cards.from_store())
        .iter()
        .find(|d| d.name == card || slug(d.name) == card)?;
    let mut made = MADE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(&spec) = made.iter().find(|s| s.card == def.name) {
        return Some(spec);
    }
    let spec: &'static Spec = Box::leak(Box::new(Spec::new(catalog, def)));
    made.push(spec);
    Some(spec)
}

/// New specs of `cards`, by name, apart from the library's: a test that changes how their games
/// play leaves the library's specs alone.
pub fn fresh(catalog: &'static Catalog, cards: &[&str]) -> Vec<Spec> {
    cards
        .iter()
        .map(|&card| {
            let defs = catalog.cards.cards();
            Spec::new(catalog, defs.iter().find(|d| d.name == card).unwrap())
        })
        .collect()
}

/// The specs `cards` name (by name or slug; every one if none), each generated, failing on an
/// unknown card. Generating every card is a sweep someone asked for (`specs-check` with no
/// card), so it runs on a pool the size of the machine, not a thread per card.
pub fn chosen(catalog: &'static Catalog, cards: &[String]) -> Result<Vec<&'static Spec>, String> {
    let chosen: Vec<&'static Spec> = if cards.is_empty() {
        self::cards(catalog).iter().collect()
    } else {
        cards
            .iter()
            .map(|c| spec(catalog, c).ok_or_else(|| format!("no card {c:?}")))
            .collect::<Result<_, _>>()?
    };
    generate(&chosen);
    Ok(chosen)
}

/// Generates each of `specs` not yet generated, on as many threads as the machine has cores.
pub fn generate(specs: &[&Spec]) {
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .min(specs.len());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                while let Some(spec) = specs.get(next.fetch_add(1, Ordering::Relaxed)) {
                    spec.moments();
                }
            });
        }
    });
}

/// A card's own scenarios and the rules library's examples for it. Generated
/// scenarios supplement cards with no scenarios of their own.
fn scenarios(catalog: &'static Catalog, def: &'static CardDef) -> &'static [Scenario] {
    type CardScenarios = (&'static Catalog, &'static str, &'static [Scenario]);
    static MADE: std::sync::Mutex<Vec<CardScenarios>> = std::sync::Mutex::new(Vec::new());
    let card = def.name;
    let mut made = MADE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(&(_, _, scenarios)) = made
        .iter()
        .find(|(source, name, _)| std::ptr::eq(*source, catalog) && *name == card)
    {
        return scenarios;
    }
    let written = (catalog.scenarios)()
        .iter()
        .find_map(|&(name, scenarios)| (name == card).then_some(scenarios))
        .unwrap_or_default();
    let shared = (catalog.library_scenarios)()
        .into_iter()
        .filter(|&(name, _)| name == card)
        .flat_map(|(_, scenarios)| scenarios.iter().copied())
        .filter(|scenario| written.iter().all(|own| own.name != scenario.name));
    let mut all: Vec<_> = written.iter().copied().chain(shared).collect();
    if written.is_empty() {
        all.extend((catalog.derived_scenarios)(def));
    }
    let scenarios: &'static [Scenario] = if !written.is_empty() && all.len() == written.len() {
        written
    } else {
        all.leak()
    };
    made.push((catalog, card, scenarios));
    scenarios
}

/// One card's spec.
pub struct Spec {
    pub card: &'static str,
    /// Its name in the page's URL ([`slug`]).
    pub slug: String,
    /// The cards its games are played with.
    catalog: &'static Catalog,
    def: &'static CardDef,
    pub generated: OnceLock<auto::Generated>,
    checkpoints: OnceLock<auto::ReviewSet>,
    playground: OnceLock<playground::Playground>,
}

impl Spec {
    fn new(catalog: &'static Catalog, def: &'static CardDef) -> Spec {
        Spec {
            card: def.name,
            slug: slug(def.name),
            catalog,
            def,
            generated: OnceLock::new(),
            checkpoints: OnceLock::new(),
            playground: OnceLock::new(),
        }
    }

    /// Its moments, generated on the first call by playing each candidate from both seats to
    /// its first live decision: empty if none got there.
    pub fn moments(&self) -> &[Moment] {
        &self.generated().moments
    }

    /// The candidates its generation dropped, each with why.
    fn dropped(&self) -> &[(String, String)] {
        &self.generated().dropped
    }

    /// Every candidate its generation played, from each seat.
    pub fn played(&self) -> &[auto::Played] {
        &self.generated().played
    }

    fn generated(&self) -> &auto::Generated {
        self.generated.get_or_init(|| {
            auto::generate(self.catalog, self.def, scenarios(self.catalog, self.def))
        })
    }

    pub fn playground(&self) -> &playground::Playground {
        self.playground
            .get_or_init(|| playground::generate(self.catalog, self.card))
    }

    pub fn checkpoints(&self) -> &auto::ReviewSet {
        self.checkpoints.get_or_init(|| {
            auto::checkpoints(self.catalog, self.def, scenarios(self.catalog, self.def))
        })
    }
}

/// One moment of play a spec shows.
pub struct Moment {
    /// Its name in the page's URL, unique within the spec: `card-exiled`.
    pub name: &'static str,
    /// What made it, in a few words: "Stops: card exiled", "{T}".
    pub title: &'static str,
    /// The scenario it comes from, by its name, which the page heads its moments with; empty
    /// for the moments of the card's definition.
    pub group: &'static str,
    /// The game, the human in seat 0: its actions set the moment up before the page shows it.
    pub board: Scenario,
    /// The actions it plays out once shown, when the viewer presses play.
    pub play: &'static [Action],
    /// What held at each seat's first live decision ([`View`] order), in the fixture
    /// conditions' words ([`crate::fixture::Condition::parse`]): what `specs-describe` prints.
    /// None while it is being recorded.
    pub recorded: Option<[&'static [&'static str]; 2]>,
}

/// Whose seat a moment is seen from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// The human's, seat 0: the one its board is written for.
    Yours,
    /// The opponent's, seat 1: the same game, the viewer seated across the table, playing on
    /// live as the opponent once every action is taken.
    Theirs,
}

impl View {
    pub const ALL: [View; 2] = [View::Yours, View::Theirs];

    /// Its name in the page's URL.
    pub const fn name(self) -> &'static str {
        match self {
            View::Yours => "yours",
            View::Theirs => "theirs",
        }
    }

    pub fn parse(name: &str) -> Option<View> {
        View::ALL.into_iter().find(|v| v.name() == name)
    }

    pub const fn seat(self) -> Seat {
        match self {
            View::Yours => Seat::P0,
            View::Theirs => Seat::P1,
        }
    }

    /// The other seat's.
    const fn other(self) -> Seat {
        match self {
            View::Yours => Seat::P1,
            View::Theirs => Seat::P0,
        }
    }
}

impl Moment {
    #[allow(clippy::large_types_passed_by_value)] // a `Scenario` is a `Copy` value, built by value
    pub const fn new(name: &'static str, title: &'static str, board: Scenario) -> Moment {
        Moment {
            name,
            title,
            group: "",
            board,
            play: &[],
            recorded: None,
        }
    }

    #[must_use]
    pub const fn play(mut self, play: &'static [Action]) -> Moment {
        self.play = play;
        self
    }
}

/// A card name in a URL: `currency-converter`.
pub fn slug(card: &str) -> String {
    let mut out = String::new();
    for c in card.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() && (c == ' ' || c == '-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

/// The spec for the card `slug` names, and its moment `name`.
pub fn find(
    catalog: &'static Catalog,
    slug: &str,
    name: &str,
) -> Option<(&'static Spec, &'static Moment)> {
    let spec = spec(catalog, slug)?;
    Some((spec, spec.moments().iter().find(|m| m.name == name)?))
}

/// Lists every moment of the specs for `cards` (by name or slug; all of them if none), each
/// generated by playing it headless from both seats to its first live decision, then the
/// candidates generation could not play, with why. Fails on an unknown card.
pub fn check_all(catalog: &'static Catalog, cards: &[String]) -> Result<(), String> {
    let chosen = chosen(catalog, cards)?;
    let mut moments = 0;
    for spec in &chosen {
        for m in spec.moments() {
            moments += 1;
            println!("ok    {} / {}  ({})", spec.slug, m.name, m.title);
        }
    }
    let mut skipped = 0;
    for spec in &chosen {
        for (name, why) in spec.dropped() {
            skipped += 1;
            println!("skip  {} / {name}: {why}", spec.slug);
        }
    }
    println!("{moments} moments, {skipped} candidates skipped");
    Ok(())
}

/// The specs a test that walks every spec takes: every card while there are at most 64, and
/// otherwise a fixed 64 chosen by a stable hash of the name, so a failure reproduces. The full
/// sweep is `beryl_cli specs-check` with no card, which runs nightly (`by nightly`).
pub fn sample(catalog: &'static Catalog) -> Vec<&'static Spec> {
    const SAMPLE: usize = 64;
    // FNV-1a: stable across runs and platforms, unlike std's hasher.
    let hash = |name: &str| {
        name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
        })
    };
    let mut specs: Vec<&'static Spec> = cards(catalog).iter().collect();
    if specs.len() > SAMPLE {
        specs.sort_by_key(|s| (hash(s.card), s.card));
        specs.truncate(SAMPLE);
    }
    specs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_name_cards() {
        assert_eq!(slug("Currency Converter"), "currency-converter");
        assert_eq!(slug("Jace, Vryn's Prodigy"), "jace-vryns-prodigy");
    }
}
