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
//! scenarios ([`auto`]), and what holds at each is recorded and checked against the approved
//! record in `data/specs/<slug>.json`. A card may add moments written by hand, whose
//! expectations are written too. The `specs` skill (`.agents/specs/SKILL.md`) says when.

mod auto;
mod currency_converter;
pub mod serve;

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use beryl_cards::CARDS;
use beryl_scenario::{Action, Scenario};
use beryl_search::GameAgent;
use beryl_session::{GameUpdate, Prepared, Seat, Sessions, Status, StopsSetting};
use serde::{Deserialize, Serialize};

use crate::boards::{self, Scripted, Shared};
use crate::fixture::Condition;

/// The moments written by hand, by card: added to the card's generated ones.
const WRITTEN: &[Spec] = &[currency_converter::SPEC];

/// Every card's spec, generated once: those with no moments left out.
pub fn specs() -> &'static [Spec] {
    &generation().0
}

/// The candidates each card's generation dropped, each with why, by card.
fn dropped() -> &'static [(&'static str, Vec<(String, String)>)] {
    &generation().1
}

type Generation = (Vec<Spec>, Vec<(&'static str, Vec<(String, String)>)>);

fn generation() -> &'static Generation {
    static GENERATION: OnceLock<Generation> = OnceLock::new();
    GENERATION.get_or_init(|| {
        let cards = CARDS.cards();
        let generated: Vec<auto::Generated> = std::thread::scope(|scope| {
            let handles: Vec<_> = cards
                .iter()
                .map(|&def| {
                    let scenarios = beryl_cards::SCENARIOS
                        .iter()
                        .find(|(name, _)| *name == def.name)
                        .map_or(&[][..], |(_, s)| *s);
                    scope.spawn(move || auto::generate(def, scenarios))
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("generating a spec panicked"))
                .collect()
        });
        let mut specs = Vec::new();
        let mut dropped = Vec::new();
        for (def, g) in cards.iter().zip(generated) {
            let mut moments = g.moments;
            if let Some(w) = WRITTEN.iter().find(|w| w.card == def.name) {
                moments.extend(w.moments.iter().map(Moment::copy));
            }
            if !moments.is_empty() {
                specs.push(Spec {
                    card: def.name,
                    moments: Box::leak(moments.into_boxed_slice()),
                });
            }
            dropped.push((def.name, g.dropped));
        }
        (specs, dropped)
    })
}

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
    /// A generated moment's record: what held at each seat's first live decision ([`View`]
    /// order), checked against the approved record. None for a moment written by hand.
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
            recorded: None,
        }
    }

    /// A copy: every field is shared static data.
    fn copy(&self) -> Moment {
        Moment {
            name: self.name,
            title: self.title,
            look: self.look,
            board: self.board,
            play: self.play,
            expect: self.expect,
            theirs: self.theirs,
            expect_theirs: self.expect_theirs,
            recorded: self.recorded,
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
    let spec = specs().iter().find(|s| self::slug(s.card) == slug)?;
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
    // An `AdvanceTo` holds the scripts rather than being taken.
    let setup = moment
        .board
        .actions
        .iter()
        .filter(|a| !matches!(a, Action::AdvanceTo { .. }))
        .count();
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
    specs()
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

/// Plays `moment` headless, seen from `view`, to the viewer's first live decision (or the
/// game's end, once every action is taken), and returns what the viewer sees there: an error
/// says what failed on the way.
pub fn reach(moment: &'static Moment, view: View) -> Result<beryl_session::PlayerView, String> {
    let (sent, received) = std::sync::mpsc::channel();
    let (_sessions, progress) = start(moment, view, move |update, phase| {
        let _ = sent.send((update, phase));
    })?;
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
            // A game won or lost once shown is a moment too: its end.
            Status::Over { .. } if shown && boards::lock(&progress).done() => {
                return Ok(update.view);
            }
            Status::Over { .. } | Status::Stopped => {
                let next = boards::lock(&progress).next;
                return Err(match next {
                    [None, None] => "the game ended before a live decision".to_owned(),
                    next => format!(
                        "the game ended with the scripts waiting on {:?}",
                        next.iter().flatten().collect::<Vec<_>>()
                    ),
                });
            }
            Status::Deciding { .. } if phase == Phase::Live => {
                if !shown {
                    return Err("nothing was shown".to_owned());
                }
                return Ok(update.view);
            }
            _ => {}
        }
    }
}

/// Plays a moment written by hand headless, seen from `view`, to the viewer's first live
/// decision and checks what it expects there: an error says what failed.
pub fn check(moment: &'static Moment, view: View) -> Result<(), String> {
    let expected = moment.expected(view);
    let conditions = expected
        .iter()
        .map(|c| Condition::parse(c))
        .collect::<Result<Vec<_>, _>>()?;
    let seen = reach(moment, view)?;
    let failed: Vec<&str> = conditions
        .iter()
        .zip(expected)
        .filter(|(c, _)| !c.holds(&seen))
        .map(|(_, text)| *text)
        .collect();
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "at turn {} {:?}: expected {}",
            seen.turn.number,
            seen.turn.step,
            failed.join(", ")
        ))
    }
}

/// One generated moment's approved record.
#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Approved {
    moment: String,
    yours: Vec<String>,
    theirs: Vec<String>,
}

/// Where the approved records are: `data/specs`, in the checkout of the crate under test
/// (`CARGO_MANIFEST_DIR` at run time, for tests) or else of this binary, four up from it in
/// `rust/target/<profile>/`. Never `env!`: a `rust/target` cloned into another worktree keeps
/// binaries built with the old worktree's path.
fn approved_dir() -> PathBuf {
    let checkout = match std::env::var_os("CARGO_MANIFEST_DIR") {
        Some(dir) => PathBuf::from(dir).join("../../.."),
        None => std::env::current_exe()
            .ok()
            .and_then(|exe| exe.ancestors().nth(4).map(std::path::Path::to_path_buf))
            .unwrap_or_default(),
    };
    checkout.join("data/specs")
}

fn approved_path(card: &str) -> PathBuf {
    approved_dir().join(format!("{}.json", slug(card)))
}

/// A card's approved records, none if it has no file.
fn approved(card: &str) -> Result<Vec<Approved>, String> {
    let path = approved_path(card);
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// The records of a spec's generated moments, as an approval writes them.
fn records(spec: &Spec) -> Vec<Approved> {
    let owned = |c: &[&str]| c.iter().map(|&c| c.to_owned()).collect();
    spec.moments
        .iter()
        .filter_map(|m| {
            let [yours, theirs] = m.recorded?;
            Some(Approved {
                moment: m.name.to_owned(),
                yours: owned(yours),
                theirs: owned(theirs),
            })
        })
        .collect()
}

/// What differs between a moment's record and its approved one, from one seat.
fn differs(seat: &str, recorded: &[String], approved: &[String]) -> Option<String> {
    let gone: Vec<&str> = approved
        .iter()
        .filter(|c| !recorded.contains(c))
        .map(String::as_str)
        .collect();
    let new: Vec<&str> = recorded
        .iter()
        .filter(|c| !approved.contains(c))
        .map(String::as_str)
        .collect();
    let mut parts = Vec::new();
    if !gone.is_empty() {
        parts.push(format!("no longer {}", gone.join(", ")));
    }
    if !new.is_empty() {
        parts.push(format!("now {}", new.join(", ")));
    }
    (!parts.is_empty()).then(|| format!("{seat}: {}", parts.join("; ")))
}

/// Checks a spec's generated moments against their approved records: each failure, by the
/// moment's name (or the file's, for a stale record).
fn check_generated(spec: &Spec) -> Result<Vec<Checked>, String> {
    let approved = approved(spec.card)?;
    let mut out = Vec::new();
    for r in records(spec) {
        let result = match approved.iter().find(|a| a.moment == r.moment) {
            None => Err("not approved: `just specs --approve`".to_owned()),
            Some(a) => {
                let diffs: Vec<String> = [
                    differs("yours", &r.yours, &a.yours),
                    differs("theirs", &r.theirs, &a.theirs),
                ]
                .into_iter()
                .flatten()
                .collect();
                if diffs.is_empty() {
                    Ok(())
                } else {
                    Err(diffs.join("; "))
                }
            }
        };
        out.push((r.moment, result));
    }
    for a in &approved {
        if !spec.moments.iter().any(|m| m.name == a.moment) {
            out.push((
                a.moment.clone(),
                Err("approved but no longer generated".to_owned()),
            ));
        }
    }
    Ok(out)
}

/// Writes a spec's generated records as its approved ones.
fn approve(spec: &Spec) -> Result<(), String> {
    let records = records(spec);
    let path = approved_path(spec.card);
    if records.is_empty() {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
    }
    std::fs::create_dir_all(approved_dir()).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(&records).map_err(|e| e.to_string())? + "\n";
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Every failure of the specs for `cards` (by name or slug; all of them if none): moments
/// written by hand played and checked, generated ones checked against their approved records,
/// and approved records no longer generated. Each moment is sent to `report` with its result
/// as it is checked; candidates generation dropped are sent as skipped.
fn failures(
    cards: &[String],
    approving: bool,
    report: &mut dyn FnMut(Line),
) -> Result<usize, String> {
    let wanted =
        |card: &str| cards.is_empty() || cards.iter().any(|c| c == card || *c == slug(card));
    if let Some(c) = cards.iter().find(|c| {
        !CARDS
            .cards()
            .iter()
            .any(|d| d.name == **c || slug(d.name) == **c)
    }) {
        return Err(format!("no card {c:?}"));
    }
    let mut failed = 0;
    for spec in specs().iter().filter(|s| wanted(s.card)) {
        let card = slug(spec.card);
        if approving {
            approve(spec)?;
        }
        for (name, result) in check_generated(spec)? {
            failed += usize::from(result.is_err());
            report(Line::Checked(format!("{card} / {name}"), result));
        }
        for m in spec.moments.iter().filter(|m| m.recorded.is_none()) {
            for view in View::ALL {
                let result = check(m, view);
                failed += usize::from(result.is_err());
                report(Line::Checked(
                    format!("{card} / {} ({})", m.name, view.name()),
                    result,
                ));
            }
        }
    }
    for (card, dropped) in dropped().iter().filter(|(c, _)| wanted(c)) {
        for (name, why) in dropped {
            report(Line::Skipped(
                format!("{} / {name}", slug(card)),
                why.clone(),
            ));
        }
    }
    // A card whose moments are all gone leaves a stale file.
    if !approving {
        for def in CARDS.cards().iter().filter(|d| wanted(d.name)) {
            if !specs().iter().any(|s| s.card == def.name) && !approved(def.name)?.is_empty() {
                failed += 1;
                report(Line::Checked(
                    slug(def.name),
                    Err("approved but no longer generated".to_owned()),
                ));
            }
        }
    }
    Ok(failed)
}

/// A moment's name and how its check went.
type Checked = (String, Result<(), String>);

/// One line of a check's report.
enum Line {
    Checked(String, Result<(), String>),
    Skipped(String, String),
}

/// Checks every moment of the specs for `cards` (by name or slug; all of them if none),
/// printing each, and fails if any fails. With `approve`, first writes each generated record
/// as the approved one.
pub fn check_all(cards: &[String], approve: bool) -> Result<(), String> {
    let failed = failures(cards, approve, &mut |line| match line {
        Line::Checked(name, Ok(())) => println!("ok    {name}"),
        Line::Checked(name, Err(e)) => println!("FAIL  {name}: {e}"),
        Line::Skipped(name, why) => println!("skip  {name}: {why}"),
    })?;
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
        for spec in specs() {
            for (i, m) in spec.moments.iter().enumerate() {
                assert!(
                    spec.moments[..i].iter().all(|n| n.name != m.name),
                    "{}: two moments are named {}",
                    spec.card,
                    m.name
                );
                if m.recorded.is_some() {
                    continue;
                }
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

    /// Every written moment sets up, plays out, and reaches a live decision where its
    /// expectations hold, from both seats; every generated one records what was approved; and
    /// no approved record is stale.
    #[test]
    fn every_moment_holds() {
        let mut failed = Vec::new();
        failures(&[], false, &mut |line| {
            if let Line::Checked(name, Err(e)) = line {
                failed.push(format!("{name}: {e}"));
            }
        })
        .expect("the specs check");
        assert!(failed.is_empty(), "{}", failed.join("\n"));
    }
}
