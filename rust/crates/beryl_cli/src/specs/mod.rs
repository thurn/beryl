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
//! what holds there is recorded for `specs-describe`. A moment a card needs that nothing generates comes from
//! a scenario in its file, or a general capability of the generator: the `specs` skill
//! (`.agents/skills/specs/SKILL.md`) says which.

mod auto;
mod playground;
pub mod reviews;
pub mod serve;

use std::sync::{Arc, OnceLock};

use beryl_cards::CARDS;
use beryl_scenario::{Action, Scenario};
use beryl_search::GameAgent;
use beryl_session::{GameUpdate, Prepared, Seat, Sessions, Status, StopsSetting};
use serde::Serialize;

use crate::boards::{self, Scripted, Shared};

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
            let moments = g.moments;
            if !moments.is_empty() {
                specs.push(Spec {
                    card: def.name,
                    checkpoints: OnceLock::new(),
                    playground: OnceLock::new(),
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
    checkpoints: OnceLock<auto::ReviewSet>,
    playground: OnceLock<playground::Playground>,
}

impl Spec {
    fn playground(&self) -> &playground::Playground {
        self.playground
            .get_or_init(|| playground::generate(self.card))
    }

    pub fn checkpoints(&self) -> &auto::ReviewSet {
        self.checkpoints.get_or_init(|| {
            let def = CARDS
                .cards()
                .iter()
                .find(|d| d.name == self.card)
                .expect("spec card");
            let scenarios = beryl_cards::SCENARIOS
                .iter()
                .find(|(name, _)| *name == self.card)
                .map_or(&[][..], |(_, scenarios)| *scenarios);
            auto::checkpoints(def, scenarios)
        })
    }
    /// The original replay, if it shows this card being cast from hand.
    fn replay_from_hand(&self, group: &str) -> Option<&'static str> {
        self.moments
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
    // A trailing `AdvanceTo` holds the hand-over until the game reaches its turn.
    if let Some(Action::AdvanceTo { turn, .. }) =
        moment.board.actions.iter().chain(moment.play).last()
    {
        boards::lock(&progress).until = Some(*turn);
    }
    let watched = Arc::clone(&progress);
    let sessions = Sessions::new(None, move |update| {
        // The emitting thread is the one the scripts answer on, so this is the phase the view
        // was made in. Views are published before the stand-in answers, so the hand-over sees
        // the turn this one shows.
        let mut p = boards::lock(&watched);
        p.turn = update.view.turn.number;
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

/// Run the setup in one shared game, then publish both seats at every manual decision.
fn start_review(
    moment: &Moment,
    emit: impl Fn([GameUpdate; 2], bool) + Send + 'static,
) -> Result<Sessions, String> {
    let progress = Shared::default();
    let actions = || moment.board.actions.iter().chain(moment.play);
    if let Some(Action::AdvanceTo { turn, .. }) = actions().last() {
        boards::lock(&progress).until = Some(*turn);
    }
    let (table, state) =
        beryl_scenario::position(&CARDS, &moment.board).map_err(|e| e.to_string())?;
    let agent = |seat| {
        Scripted::with_actions(actions(), seat, &table, Arc::clone(&progress))
            .map(|a| Box::new(a) as Box<dyn GameAgent + Send>)
    };
    let watched = Arc::clone(&progress);
    let sessions = Sessions::new(None, |_| {});
    sessions
        .start_review(
            Prepared {
                opponent: agent(Seat::P1)?,
                stand_in: agent(Seat::P0)?,
                scripted: true,
                hand_over: Some(Box::new(move || boards::lock(&watched).done())),
                table,
                state,
                human: Seat::P0,
                stops: StopsSetting::SMART,
            },
            Box::new(move |session, views| {
                let mut p = boards::lock(&progress);
                let live = p.done() && p.failure.is_none();
                p.turn = views[0].turn.number;
                drop(p);
                emit(views.map(|view| GameUpdate { session, view }), live);
            }),
        )
        .map_err(|e| e.to_string())?;
    Ok(sessions)
}

fn at_checkpoint(updates: &[GameUpdate; 2], live: bool) -> bool {
    live && updates
        .iter()
        .any(|u| matches!(u.view.status, Status::Deciding { .. } | Status::Over { .. }))
}

fn review_position(
    moment: &Moment,
) -> Result<([beryl_session::PlayerView; 2], Option<String>), String> {
    use beryl_session::protocol::{GameEvent, Why};

    let (sent, received) = std::sync::mpsc::channel();
    let _sessions = start_review(moment, move |updates, live| {
        let _ = sent.send((updates, live));
    })?;
    let mut countered = None;
    loop {
        let (updates, live) = received
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| "setup never reached a checkpoint".to_owned())?;
        if live {
            let view = &updates[0].view;
            for event in &view.events {
                if let GameEvent::ZoneChange {
                    slot,
                    reason: Why::Counter,
                    ..
                } = *event
                {
                    countered = view
                        .objects
                        .iter()
                        .find(|o| o.slot == slot)
                        .and_then(|o| o.name.clone())
                        .or(countered);
                }
            }
        }
        if at_checkpoint(&updates, live) {
            return Ok((updates.map(|u| u.view), countered));
        }
        if matches!(
            updates[0].view.status,
            Status::Over { .. } | Status::Stopped
        ) {
            return Err("game ended during setup".to_owned());
        }
    }
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
    /// The scenario it comes from, empty for the card's own moments.
    group: &'static str,
    /// Whether it plays actions out once shown, which a replay plays again.
    plays: bool,
    replay: Option<&'static str>,
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
                    group: m.group,
                    plays: !m.play.is_empty(),
                    replay: s.replay_from_hand(m.group),
                })
                .collect(),
        })
        .collect()
}

/// Plays `moment` headless, seen from `view`, to the viewer's first live decision (or the
/// game's end, once every action is taken), and returns what the viewer sees there: an error
/// says what failed on the way.
pub fn reach(moment: &'static Moment, view: View) -> Result<beryl_session::PlayerView, String> {
    trace(moment, view).map(|(view, _)| view)
}

/// [`reach`], with the views the viewer was shown on the way once the setup was done, each
/// with the events it animates.
fn trace(
    moment: &'static Moment,
    view: View,
) -> Result<(beryl_session::PlayerView, Vec<beryl_session::PlayerView>), String> {
    let (sent, received) = std::sync::mpsc::channel();
    let (_sessions, progress) = start(moment, view, move |update, phase| {
        let _ = sent.send((update, phase));
    })?;
    let mut shown = false;
    let mut events = Vec::new();
    loop {
        let (update, phase) = received
            .recv_timeout(std::time::Duration::from_secs(30))
            .map_err(|_| "no live decision within 30 s".to_owned())?;
        if let Some(failure) = boards::lock(&progress).failure.take() {
            return Err(failure);
        }
        // The first view shown animates nothing (`specs-serve` clears its events).
        if shown {
            events.push(update.view.clone());
        }
        shown |= phase != Phase::Setup;
        match update.view.status {
            // A game won or lost once shown is a moment too: its end.
            Status::Over { .. } if shown && boards::lock(&progress).done() => {
                return Ok((update.view, events));
            }
            Status::Over { .. } | Status::Stopped => {
                return Err(match boards::lock(&progress).stalled() {
                    None => "the game ended before a live decision".to_owned(),
                    Some(why) => format!("the game ended with {why}"),
                });
            }
            Status::Deciding { .. } if phase == Phase::Live => {
                if !shown {
                    return Err("nothing was shown".to_owned());
                }
                return Ok((update.view, events));
            }
            _ => {}
        }
    }
}

/// A view's events in a line, each its kind and fields, objects by name: "turn 5 your
/// postcombatMain: zoneChange Island exile->graveyard (effect); tokensCreated you [Treasure]".
/// None if it has none but steps beginning, which every view of a turn passing has.
fn events_line(view: &beryl_session::PlayerView) -> Option<String> {
    let name = |slot: &serde_json::Value| {
        slot.as_u64()
            .and_then(|s| view.objects.iter().find(|o| u64::from(o.slot) == s))
            .and_then(|o| o.name.clone())
            .unwrap_or_else(|| format!("#{slot}"))
    };
    let text = |v: &serde_json::Value| match v {
        serde_json::Value::String(s) => s.clone(),
        v => v.to_string(),
    };
    let events: Vec<String> = view
        .events
        .iter()
        .filter(|e| !matches!(e, beryl_session::protocol::GameEvent::StepBegins { .. }))
        .map(|e| {
            let serde_json::Value::Object(fields) = serde_json::to_value(e).unwrap_or_default()
            else {
                return format!("{e:?}");
            };
            let mut parts = vec![fields.get("kind").map(text).unwrap_or_default()];
            if let (Some(from), Some(to)) = (fields.get("from"), fields.get("to"))
                && fields.contains_key("slot")
            {
                parts.push(format!(
                    "{} {}->{}",
                    name(&fields["slot"]),
                    text(from),
                    text(to)
                ));
                if let Some(why) = fields.get("reason") {
                    parts.push(format!("({})", text(why)));
                }
                return parts.join(" ");
            }
            for (k, v) in &fields {
                match k.as_str() {
                    "kind" => {}
                    "slot" | "source" => parts.push(name(v)),
                    // A stack item by its name: "Currency Converter ability 2".
                    "item" => {
                        parts.push(v.get("name").map(text).unwrap_or_default());
                        if let Some(k) = v.get("ability") {
                            parts.push(format!("ability {}", text(k)));
                        }
                    }
                    "loyalty" if v.is_null() => {}
                    "tokens" | "attackers" => {
                        let names: Vec<String> =
                            v.as_array().into_iter().flatten().map(name).collect();
                        parts.push(format!("[{}]", names.join(", ")));
                    }
                    _ => parts.push(format!("{k}={}", text(v))),
                }
            }
            parts.join(" ")
        })
        .collect();
    let turn = &view.turn;
    let whose = if turn.active == beryl_session::protocol::Side::You {
        "your"
    } else {
        "their"
    };
    let step = serde_json::to_value(turn.step)
        .ok()
        .map(|v| text(&v))
        .unwrap_or_default();
    (!events.is_empty())
        .then(|| format!("turn {} {whose} {step}: {}", turn.number, events.join("; ")))
}

/// One view as `specs-describe --views` prints it: its turn and events, what the viewer is
/// asked (or the game's status), the stack bottom first, and each side's battlefield, hand,
/// graveyard and exile by name (a card the viewer cannot see as `?`, tapped ones marked), and
/// their library's size.
fn view_lines(view: &beryl_session::PlayerView) -> Vec<String> {
    let v = serde_json::to_value(view).unwrap_or_default();
    let objects = v["objects"].as_array().cloned().unwrap_or_default();
    let named = |slot: &serde_json::Value| {
        objects
            .iter()
            .find(|o| o["slot"] == *slot)
            .and_then(|o| o["name"].as_str())
            .unwrap_or("?")
            .to_owned()
    };
    let mut lines = vec![events_line(view).unwrap_or_else(|| {
        format!(
            "turn {} {:?} {:?}: no events",
            view.turn.number, view.turn.active, view.turn.step
        )
    })];
    lines.push(match &v["status"] {
        s if s["kind"] == "deciding" => format!(
            "asks   {} ({} candidates)",
            s["decision"]["kind"].as_str().unwrap_or("?"),
            s["decision"]["candidates"].as_array().map_or(0, Vec::len)
        ),
        s => format!("status {}", s["kind"].as_str().unwrap_or("?")),
    });
    let stack: Vec<String> = v["stack"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| match item["kind"].as_str() {
            Some("spell") => named(&item["slot"]),
            _ => item["name"].as_str().map_or_else(
                || format!("{} ability", named(&item["source"])),
                str::to_owned,
            ),
        })
        .collect();
    if !stack.is_empty() {
        lines.push(format!("stack  {}", stack.join(" < ")));
    }
    for side in ["you", "opponent"] {
        let mut zones = Vec::new();
        for zone in ["battlefield", "hand", "graveyard", "exile"] {
            // A permanent is on its controller's side; a card elsewhere, its owner's.
            let whose = if zone == "battlefield" {
                "controller"
            } else {
                "owner"
            };
            let cards: Vec<String> = objects
                .iter()
                .filter(|o| o["zone"] == zone && o[whose] == side)
                .map(|o| {
                    let name = o["name"].as_str().unwrap_or("?");
                    // A permanent's status, power and toughness, counters and keywords, as
                    // the view gives them: what its frame shows beyond its card.
                    let mut marks = Vec::new();
                    if o["tapped"] == true {
                        marks.push("tapped".to_owned());
                    }
                    if let (Some(p), Some(t)) =
                        (o["pt"]["power"].as_i64(), o["pt"]["toughness"].as_i64())
                    {
                        marks.push(format!("{p}/{t}"));
                    }
                    for c in o["counters"].as_array().into_iter().flatten() {
                        if let (Some(kind), Some(n)) = (c["kind"].as_str(), c["count"].as_u64()) {
                            marks.push(format!("{n} {kind}"));
                        }
                    }
                    for k in o["keywords"].as_array().into_iter().flatten() {
                        marks.extend(k.as_str().map(str::to_owned));
                    }
                    if zone == "battlefield" && !marks.is_empty() {
                        format!("{name} ({})", marks.join(", "))
                    } else if o["tapped"] == true {
                        format!("{name} (tapped)")
                    } else {
                        name.to_owned()
                    }
                })
                .collect();
            if !cards.is_empty() {
                zones.push(format!("{zone}: {}", cards.join(", ")));
            }
        }
        let library = v["players"][side]["library"].as_array().map_or(0, Vec::len);
        zones.push(format!("library: {library}"));
        lines.push(format!("{side:<6} {}", zones.join(" | ")));
    }
    lines
}

/// Prints each moment of the specs for `cards` (by name or slug; all of them if none), or the
/// one `moment` names: its page, the actions it plays once shown, and from each seat the
/// events shown on the way and what holds at its first live decision, abilities by their cost.
/// With `views`, each view shown is printed whole instead: its events, what it asks, the stack
/// and each side's zones ([`view_lines`]).
pub fn describe(cards: &[String], moment: Option<&str>, views: bool) -> Result<(), String> {
    let wanted =
        |card: &str| cards.is_empty() || cards.iter().any(|c| c == card || *c == slug(card));
    let mut found = false;
    for spec in specs().iter().filter(|s| wanted(s.card)) {
        let def = CARDS
            .cards()
            .iter()
            .find(|d| d.name == spec.card)
            .ok_or_else(|| format!("no card {:?}", spec.card))?;
        let labels = auto::ability_labels(def);
        // An ability by its cost rather than its index: "offers Currency Converter {T}".
        let label = |c: &str| {
            labels.iter().fold(c.to_owned(), |c, (k, l)| {
                c.replace(
                    &format!("{} ability {k}", spec.card),
                    &format!("{} {l}", spec.card),
                )
            })
        };
        for m in spec
            .moments
            .iter()
            .filter(|m| moment.is_none_or(|n| n == m.name))
        {
            found = true;
            let card = slug(spec.card);
            println!("== {card} / {}  \"{}\"", m.name, m.title);
            if !m.group.is_empty() {
                println!("scenario: {}", m.group);
            }
            println!("page: /specs/{card}/{}", m.name);
            for a in m.play {
                println!("plays: {a:?}");
            }
            for view in View::ALL {
                let (shown, events) = trace(m, view)?;
                println!("{}:", view.name());
                for (n, v) in events.iter().enumerate() {
                    if views {
                        println!("  view {}", n + 1);
                        for line in view_lines(v) {
                            println!("    {}", label(&line));
                        }
                    } else if let Some(line) = events_line(v) {
                        println!("  shown  {}", label(&line));
                    }
                }
                let holds: Vec<String> = auto::snapshot(&shown, def)
                    .iter()
                    .map(|c| label(c))
                    .collect();
                println!("  holds  {}", holds.join(", "));
            }
            println!();
        }
    }
    if found {
        Ok(())
    } else {
        Err(format!(
            "no moment {cards:?} {}",
            moment.unwrap_or_default()
        ))
    }
}

/// Lists every moment of the specs for `cards` (by name or slug; all of them if none), each
/// generated by playing it headless from both seats to its first live decision, then the
/// candidates generation could not play, with why. Fails on an unknown card.
pub fn check_all(cards: &[String]) -> Result<(), String> {
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
    let mut moments = 0;
    for spec in specs().iter().filter(|s| wanted(s.card)) {
        for m in spec.moments {
            moments += 1;
            println!("ok    {} / {}  ({})", slug(spec.card), m.name, m.title);
        }
    }
    let mut skipped = 0;
    for (card, dropped) in dropped().iter().filter(|(c, _)| wanted(c)) {
        for (name, why) in dropped {
            skipped += 1;
            println!("skip  {} / {name}: {why}", slug(card));
        }
    }
    println!("{moments} moments, {skipped} candidates skipped");
    Ok(())
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
    fn review_shares_the_payment_choice_and_its_result() {
        use beryl_cards::names::{MANA_TITHE, OVERLORD_OF_THE_MISTMOORS};
        use beryl_session::CandidateIndex;
        use beryl_session::protocol::{AskView, Candidate, ZoneName};

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
            .find(|c| c.moment.title == "Pay {1}?")
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
            assert!(matches!(decision.ask, AskView::UnlessPays { .. }));
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
        use beryl_session::protocol::{AskView, ZoneName};

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
                if matches!(decision.ask, AskView::UnlessPays { .. })));
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
        let error = reach(&MOMENT, View::Yours).expect_err("Counterspell has no target");
        assert!(error.contains("Counterspell"), "{error}");
        assert!(error.contains("fit no candidate"), "{error}");
    }

    /// Generating every spec plays every moment from both seats: a moment is kept only once it
    /// reaches a live decision from each.
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
            }
        }
    }
}
