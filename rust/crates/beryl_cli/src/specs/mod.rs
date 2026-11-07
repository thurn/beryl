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
//! The `specs` skill (`.agents/specs/SKILL.md`) says which moments a card needs.

mod currency_converter;
pub mod serve;

use std::sync::Arc;

use beryl_cards::CARDS;
use beryl_scenario::{Action, Scenario};
use beryl_search::GameAgent;
use beryl_session::{GameUpdate, Prepared, Seat, Sessions, Status, StopsSetting};
use serde::Serialize;

use crate::boards::{self, Scripted, Shared};
use crate::fixture::Condition;

/// Every spec, by its card.
pub const SPECS: &[Spec] = &[currency_converter::SPEC];

/// One card's spec.
pub struct Spec {
    pub card: &'static str,
    pub moments: &'static [Moment],
}

/// One moment of play a spec shows.
pub struct Moment {
    /// Its name in the page's URL, unique within the spec: `stops-with-exile`.
    pub name: &'static str,
    /// What it shows, in a few words: "Something exiled: it stops".
    pub title: &'static str,
    /// What to look at or try, in a few words.
    pub look: &'static str,
    /// The game, the human in seat 0: its actions set the moment up before the page shows it.
    pub board: Scenario,
    /// The actions it plays out once shown, when the viewer presses play.
    pub play: &'static [Action],
    /// What must hold of the human's first decision once both have been taken, in the fixture
    /// conditions' words ([`Condition::parse`]).
    pub expect: &'static [&'static str],
    /// What the opponent should see of it, in a few words.
    pub theirs: &'static str,
    /// What must hold of the opponent's first decision once both have been taken, seated in
    /// their place ([`View::Theirs`]).
    pub expect_theirs: &'static [&'static str],
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

    const fn seat(self) -> Seat {
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
            look: "",
            board,
            play: &[],
            expect: &[],
            theirs: "",
            expect_theirs: &[],
        }
    }

    #[must_use]
    pub const fn look(mut self, look: &'static str) -> Moment {
        self.look = look;
        self
    }

    #[must_use]
    pub const fn play(mut self, play: &'static [Action]) -> Moment {
        self.play = play;
        self
    }

    #[must_use]
    pub const fn expect(mut self, expect: &'static [&'static str]) -> Moment {
        self.expect = expect;
        self
    }

    /// What the opponent should see of it, and what must hold of their first decision, seated
    /// in their place.
    #[must_use]
    pub const fn theirs(mut self, look: &'static str, expect: &'static [&'static str]) -> Moment {
        self.theirs = look;
        self.expect_theirs = expect;
        self
    }

    /// What must hold of the viewer's first live decision, seen from `view`.
    pub const fn expected(&self, view: View) -> &'static [&'static str] {
        match view {
            View::Yours => self.expect,
            View::Theirs => self.expect_theirs,
        }
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
    let spec = SPECS.iter().find(|s| self::slug(s.card) == slug)?;
    Some((spec, spec.moments.iter().find(|m| m.name == name)?))
}

/// Where a moment's game stood when it emitted a view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Still being set up: the page never shows it.
    Setup,
    /// Playing out the moment's own actions, which the page animates.
    Play,
    /// Every action taken: the human plays on.
    Live,
}

/// Starts `moment` in its own [`Sessions`], seen from `view`, each view sent to `emit` with the
/// phase it was emitted in. Scripts that fail report to the view's phase as `Live` with the
/// failure.
pub fn start(
    moment: &'static Moment,
    view: View,
    emit: impl Fn(GameUpdate, Phase) + Send + Sync + 'static,
) -> Result<(Sessions, Shared), String> {
    let progress = Shared::default();
    let setup = moment.board.actions.len();
    let watched = Arc::clone(&progress);
    let sessions = Sessions::new(None, move |update| {
        // The emitting thread is the one the scripts answer on, so this is the phase the view
        // was made in.
        let p = boards::lock(&watched);
        let phase = if p.taken < setup {
            Phase::Setup
        } else if p.done() {
            Phase::Live
        } else {
            Phase::Play
        };
        drop(p);
        let mut update = update;
        if phase != Phase::Live && matches!(update.view.status, Status::Deciding { .. }) {
            // A stand-in answers the viewer's decisions until the actions are taken, at once:
            // the viewer watches the game go on through them, each held as long as its events
            // take to follow (the opponent's ability on the stack, read before it resolves), as
            // the app holds the opponent's play.
            update.view.status = Status::Playing;
        }
        emit(update, phase);
    });
    let (table, state) =
        beryl_scenario::position(&CARDS, &moment.board).map_err(|e| e.to_string())?;
    let actions = || moment.board.actions.iter().chain(moment.play);
    let agent = |seat: Seat| {
        Scripted::with_actions(actions(), seat, &table, Arc::clone(&progress))
            .map(|a| Box::new(a) as Box<dyn GameAgent + Send>)
    };
    let handed = Arc::clone(&progress);
    sessions
        .start_prepared(Prepared {
            opponent: agent(view.other())?,
            stand_in: agent(view.seat())?,
            scripted: true,
            hand_over: Some(Box::new(move || boards::lock(&handed).done())),
            table,
            state,
            human: view.seat(),
            stops: StopsSetting::SMART,
        })
        .map_err(|e| e.to_string())?;
    Ok((sessions, progress))
}

/// The specs as the page lists them.
#[derive(Serialize)]
pub struct Listing {
    card: &'static str,
    slug: String,
    moments: Vec<MomentListing>,
}

#[derive(Serialize)]
pub struct MomentListing {
    name: &'static str,
    title: &'static str,
    look: &'static str,
    /// What the opponent should see of it.
    theirs: &'static str,
    /// Whether it plays actions out once shown, which a replay plays again.
    plays: bool,
}

pub fn listing() -> Vec<Listing> {
    SPECS
        .iter()
        .map(|s| Listing {
            card: s.card,
            slug: slug(s.card),
            moments: s
                .moments
                .iter()
                .map(|m| MomentListing {
                    name: m.name,
                    title: m.title,
                    look: m.look,
                    theirs: m.theirs,
                    plays: !m.play.is_empty(),
                })
                .collect(),
        })
        .collect()
}

/// Plays `moment` headless, seen from `view`, to the viewer's first live decision and checks
/// what it expects there: an error says what failed.
pub fn check(moment: &'static Moment, view: View) -> Result<(), String> {
    let (sent, received) = std::sync::mpsc::channel();
    let (_sessions, progress) = start(moment, view, move |update, phase| {
        let _ = sent.send((update, phase));
    })?;
    let expected = moment.expected(view);
    let conditions = expected
        .iter()
        .map(|c| Condition::parse(c))
        .collect::<Result<Vec<_>, _>>()?;
    let mut shown = false;
    loop {
        let (update, phase) = received
            .recv_timeout(std::time::Duration::from_secs(30))
            .map_err(|_| "no live decision within 30 s".to_owned())?;
        if let Some(failure) = boards::lock(&progress).failure.take() {
            return Err(failure);
        }
        shown |= phase != Phase::Setup;
        match update.view.status {
            Status::Over { .. } | Status::Stopped => {
                return Err("the game ended before a live decision".to_owned());
            }
            Status::Deciding { .. } if phase == Phase::Live => {
                let failed: Vec<&str> = conditions
                    .iter()
                    .zip(expected)
                    .filter(|(c, _)| !c.holds(&update.view))
                    .map(|(_, text)| *text)
                    .collect();
                if !shown {
                    return Err("nothing was shown".to_owned());
                }
                return if failed.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "at turn {} {:?}: expected {}",
                        update.view.turn.number,
                        update.view.turn.step,
                        failed.join(", ")
                    ))
                };
            }
            _ => {}
        }
    }
}

/// Checks every moment of the specs for `cards` (by name or slug; all of them if none),
/// printing each, and fails if any fails.
pub fn check_all(cards: &[String]) -> Result<(), String> {
    let wanted =
        |s: &Spec| cards.is_empty() || cards.iter().any(|c| *c == s.card || *c == slug(s.card));
    if let Some(c) = cards
        .iter()
        .find(|c| !SPECS.iter().any(|s| s.card == **c || slug(s.card) == **c))
    {
        return Err(format!("no spec for {c:?}"));
    }
    let mut failed = 0;
    for spec in SPECS.iter().filter(|s| wanted(s)) {
        for m in spec.moments {
            for view in View::ALL {
                let name = format!("{} / {} ({})", slug(spec.card), m.name, view.name());
                match check(m, view) {
                    Ok(()) => println!("ok    {name}"),
                    Err(e) => {
                        failed += 1;
                        println!("FAIL  {name}: {e}");
                    }
                }
            }
        }
    }
    if failed == 0 {
        Ok(())
    } else {
        Err(format!("{failed} moments failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_name_cards() {
        assert_eq!(slug("Currency Converter"), "currency-converter");
        assert_eq!(slug("Jace, Vryn's Prodigy"), "jace-vryns-prodigy");
    }

    #[test]
    fn moments_are_named_uniquely() {
        for spec in SPECS {
            for (i, m) in spec.moments.iter().enumerate() {
                assert!(
                    spec.moments[..i].iter().all(|n| n.name != m.name),
                    "{}: two moments are named {}",
                    spec.card,
                    m.name
                );
                assert!(
                    !m.expect.is_empty() && !m.expect_theirs.is_empty(),
                    "{}: {} expects nothing of a seat",
                    spec.card,
                    m.name
                );
                assert!(
                    !m.theirs.is_empty(),
                    "{}: {} says nothing of what the opponent sees",
                    spec.card,
                    m.name
                );
            }
        }
    }

    /// Every moment sets up, plays out, and reaches a live decision where its expectations hold,
    /// from both seats.
    #[test]
    fn every_moment_holds() {
        let failures: Vec<String> = SPECS
            .iter()
            .flat_map(|s| s.moments.iter().map(move |m| (s, m)))
            .flat_map(|(s, m)| View::ALL.map(|v| (s, m, v)))
            .filter_map(|(s, m, v)| {
                check(m, v)
                    .err()
                    .map(|e| format!("{} / {} ({}): {e}", s.card, m.name, v.name()))
            })
            .collect();
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
