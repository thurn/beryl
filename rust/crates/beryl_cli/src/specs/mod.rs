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

mod auto;
mod describe;
mod listing;
mod playground;
pub mod reviews;
mod script;
pub mod serve;
mod start;

use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use beryl_cards::CARDS;
use beryl_engine::card::CardDef;
use beryl_scenario::{Action, Scenario};
use beryl_session::Seat;

use describe::reach;
pub use describe::{Unreached, describe};
pub use script::scripts;
#[cfg(test)]
pub(crate) use start::FAULTS;
pub use start::{Phase, start};
use start::{at_checkpoint, review_position, start_guarded, start_review};

/// The specs, one per implemented card, each generated on its first request: asking for one
/// card's moments plays no other card's.
struct Library {
    specs: Vec<Spec>,
}

impl Library {
    fn new() -> Library {
        Library {
            specs: CARDS.cards().iter().map(|&def| Spec::new(def)).collect(),
        }
    }

    fn find(&self, card: &str) -> Option<&Spec> {
        self.specs.iter().find(|s| s.card == card || s.slug == card)
    }
}

fn library() -> &'static Library {
    static LIBRARY: OnceLock<Library> = OnceLock::new();
    LIBRARY.get_or_init(Library::new)
}

/// Every implemented card's spec, generating none.
pub fn cards() -> &'static [Spec] {
    &library().specs
}

/// The spec of the card `card` names, by its name or its slug, generating nothing.
pub fn spec(card: &str) -> Option<&'static Spec> {
    library().find(card)
}

/// New specs of `cards`, by name, apart from the library's: a test that changes how their games
/// play leaves the library's specs alone.
#[cfg(test)]
pub(crate) fn fresh(cards: &[&str]) -> Vec<Spec> {
    cards
        .iter()
        .map(|&card| Spec::new(CARDS.cards().iter().find(|d| d.name == card).unwrap()))
        .collect()
}

/// The specs `cards` name (by name or slug; every one if none), each generated, failing on an
/// unknown card. Generating every card is a sweep someone asked for (`specs-check` with no
/// card), so it runs on a pool the size of the machine, not a thread per card.
fn chosen(cards: &[String]) -> Result<Vec<&'static Spec>, String> {
    let chosen: Vec<&'static Spec> = if cards.is_empty() {
        self::cards().iter().collect()
    } else {
        cards
            .iter()
            .map(|c| spec(c).ok_or_else(|| format!("no card {c:?}")))
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

/// The scenarios in `card`'s file.
fn scenarios(card: &str) -> &'static [Scenario] {
    beryl_cards::SCENARIOS
        .iter()
        .find(|(name, _)| *name == card)
        .map_or(&[][..], |(_, scenarios)| *scenarios)
}

/// One card's spec.
pub struct Spec {
    pub card: &'static str,
    /// Its name in the page's URL ([`slug`]).
    pub slug: String,
    def: &'static CardDef,
    generated: OnceLock<auto::Generated>,
    checkpoints: OnceLock<auto::ReviewSet>,
    playground: OnceLock<playground::Playground>,
}

impl Spec {
    fn new(def: &'static CardDef) -> Spec {
        Spec {
            card: def.name,
            slug: slug(def.name),
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
        self.generated
            .get_or_init(|| auto::generate(self.def, scenarios(self.card)))
    }

    fn playground(&self) -> &playground::Playground {
        self.playground
            .get_or_init(|| playground::generate(self.card))
    }

    pub fn checkpoints(&self) -> &auto::ReviewSet {
        self.checkpoints
            .get_or_init(|| auto::checkpoints(self.def, scenarios(self.card)))
    }

    /// The original replay, if it shows this card being cast from hand.
    fn replay_from_hand(&self, group: &str) -> Option<&str> {
        self.moments()
            .iter()
            .find(|m| {
                m.group == group
                    && m.title == "Start"
                    && m.play.iter().any(|a| {
                        matches!(a, Action::Cast { player, card, .. }
                        if *card == self.card
                            && m.board.setup.players[player.index()].hand.contains(card))
                    })
            })
            .map(|m| m.name)
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

    pub(crate) const fn seat(self) -> Seat {
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
pub fn find(slug: &str, name: &str) -> Option<(&'static Spec, &'static Moment)> {
    let spec = spec(slug)?;
    Some((spec, spec.moments().iter().find(|m| m.name == name)?))
}

/// Lists every moment of the specs for `cards` (by name or slug; all of them if none), each
/// generated by playing it headless from both seats to its first live decision, then the
/// candidates generation could not play, with why. Fails on an unknown card.
pub fn check_all(cards: &[String]) -> Result<(), String> {
    let chosen = chosen(cards)?;
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
#[cfg(test)]
fn sample() -> Vec<&'static Spec> {
    const SAMPLE: usize = 64;
    // FNV-1a: stable across runs and platforms, unlike std's hasher.
    let hash = |name: &str| {
        name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
        })
    };
    let mut specs: Vec<&'static Spec> = cards().iter().collect();
    if specs.len() > SAMPLE {
        specs.sort_by_key(|s| (hash(s.card), s.card));
        specs.truncate(SAMPLE);
    }
    specs
}

#[cfg(test)]
mod tests {
    use super::*;
    use beryl_session::Status;

    #[test]
    fn slugs_name_cards() {
        assert_eq!(slug("Currency Converter"), "currency-converter");
        assert_eq!(slug("Jace, Vryn's Prodigy"), "jace-vryns-prodigy");
    }

    #[test]
    fn review_shares_the_payment_choice_and_its_result() {
        use beryl_cards::names::{MANA_TITHE, OVERLORD_OF_THE_MISTMOORS};
        use beryl_session::CandidateIndex;
        use beryl_session::protocol::{Candidate, ZoneName};

        let def = CARDS
            .cards()
            .iter()
            .copied()
            .find(|d| d.name == MANA_TITHE)
            .unwrap();
        let (_, scenarios) = beryl_cards::SCENARIOS
            .iter()
            .find(|(n, _)| *n == MANA_TITHE)
            .unwrap();
        let checkpoints = auto::checkpoints(def, scenarios).examples;
        assert_eq!(checkpoints.len(), 3);
        let payment = checkpoints
            .iter()
            .find(|c| c.moment.title == "Pay {1}.")
            .unwrap();
        assert_eq!(payment.first, View::Yours);
        for (answer, expected) in [
            (Candidate::Yes, ZoneName::Battlefield),
            (Candidate::No, ZoneName::Graveyard),
        ] {
            let (send, receive) = std::sync::mpsc::channel();
            let sessions = start_review(&payment.moment, move |updates, live| {
                if at_checkpoint(&updates, live) {
                    let _ = send.send(updates);
                }
            })
            .unwrap();
            let [payer, caster] = receive
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            assert_eq!(caster.view.status, Status::Thinking);
            let Status::Deciding { decision } = payer.view.status else {
                panic!("payer has no prompt")
            };
            assert!(decision.prompt.is(beryl_engine::card::Purpose::UnlessPays));
            let index = decision
                .candidates
                .iter()
                .position(|c| c.option == answer)
                .unwrap();
            sessions
                .choose(decision.id, CandidateIndex(u16::try_from(index).unwrap()))
                .unwrap();
            let views = receive
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            for update in views {
                assert!(update.view.objects.iter().any(|o| {
                    o.name.as_deref() == Some(OVERLORD_OF_THE_MISTMOORS) && o.zone == expected
                }));
            }
        }
    }

    #[test]
    fn counter_review_starts_before_the_target_spell_is_cast() {
        use beryl_cards::names::{MANA_CRYPT, MANA_TITHE};
        use beryl_session::protocol::ZoneName;

        let def = CARDS
            .cards()
            .iter()
            .copied()
            .find(|d| d.name == MANA_TITHE)
            .unwrap();
        let (_, scenarios) = beryl_cards::SCENARIOS
            .iter()
            .find(|(n, _)| *n == MANA_TITHE)
            .unwrap();
        let checkpoints = auto::checkpoints(def, scenarios).examples;
        let counter = checkpoints
            .iter()
            .find(|c| !c.review_casts.is_empty())
            .unwrap();
        assert_eq!(counter.first, View::Yours);

        let (send, receive) = std::sync::mpsc::channel();
        let casts = std::sync::Mutex::new(counter.review_casts);
        let sessions = start_review(&counter.moment, move |updates, live| {
            if live {
                let steps = serve::recorded_cast_moves(&mut casts.lock().unwrap(), &updates);
                if at_checkpoint(&updates, live) {
                    let _ = send.send((updates, steps));
                }
            }
        })
        .unwrap();
        let next = || {
            receive
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap()
        };
        for (actor, card) in [(0, MANA_CRYPT), (1, MANA_TITHE)] {
            let (views, steps) = next();
            let Status::Deciding { decision } = &views[actor].view.status else {
                panic!("expected the {card} caster to act");
            };
            assert!(steps[actor].is_none());
            let (id, index) = steps[1 - actor].expect("the observer can play the cast");
            assert_eq!(id, decision.id);
            assert_eq!(
                decision.candidates[usize::from(index.0)].label,
                format!("Cast {card}")
            );
            sessions.choose(id, index).unwrap();
        }
        let (views, steps) = next();
        assert!(steps.iter().all(Option::is_none));
        for update in views {
            assert!(!matches!(update.view.status, Status::Deciding { decision }
                if decision.prompt.is(beryl_engine::card::Purpose::UnlessPays)));
            assert!(update.view.objects.iter().any(|o| {
                o.name.as_deref() == Some(MANA_CRYPT) && o.zone == ZoneName::Graveyard
            }));
        }
    }

    /// A scripted action that never fits says so when the game ends without it: which action
    /// waited, and the decision it fit no candidate of, not just that the game ended.
    #[test]
    fn a_stalled_script_names_the_decision_it_missed() {
        use beryl_cards::names::{COUNTERSPELL, ISLAND};
        use beryl_scenario::{P0, Permanent};
        static MOMENT: Moment = Moment::new(
            "stalled",
            "Stalled",
            Scenario::new("Counterspell with nothing to counter")
                .battlefield(P0, &[Permanent::new(ISLAND), Permanent::new(ISLAND)])
                .hand(P0, &[COUNTERSPELL])
                .library(P0, &[ISLAND])
                .library(beryl_scenario::P1, &[ISLAND])
                .at(2, beryl_scenario::P1, beryl_engine::TurnStep::PrecombatMain),
        )
        .play(&[Action::cast(P0, COUNTERSPELL)]);
        let error = reach(&MOMENT, View::Yours)
            .expect_err("Counterspell has no target")
            .to_string();
        assert!(error.contains("Counterspell"), "{error}");
        assert!(error.contains("fit no candidate"), "{error}");
    }

    /// Asking for one card's spec generates that card and no other.
    #[test]
    fn a_spec_generates_only_its_card() {
        use beryl_cards::names::COUNTERSPELL;
        let library = Library::new();
        let spec = library
            .find(&slug(COUNTERSPELL))
            .expect("Counterspell has a spec");
        assert!(library.specs.iter().all(|s| s.generated.get().is_none()));
        assert!(!spec.moments().is_empty());
        let generated: Vec<&str> = library
            .specs
            .iter()
            .filter(|s| s.generated.get().is_some())
            .map(|s| s.card)
            .collect();
        assert_eq!(generated, [COUNTERSPELL]);
    }

    /// Generating a spec plays every moment from both seats: a moment is kept only once it
    /// reaches a live decision from each.
    #[test]
    fn moments_are_named_uniquely() {
        let specs = sample();
        generate(&specs);
        for spec in specs {
            let moments = spec.moments();
            for (i, m) in moments.iter().enumerate() {
                assert!(
                    moments[..i].iter().all(|n| n.name != m.name),
                    "{}: two moments are named {}",
                    spec.card,
                    m.name
                );
            }
        }
    }
}
