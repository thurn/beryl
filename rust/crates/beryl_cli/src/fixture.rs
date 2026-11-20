//! UI fixtures (decision D11): a seeded game between named agents, or a board of
//! [`crate::boards`] its script plays, played through the app's [`Sessions`] with an agent
//! standing in for the human seat, recorded as the messages the app would have sent the UI up
//! to a named stopping condition, and through the clicks the definition then makes on the
//! human's prompts ([`clicks`]). A manual table instead starts from a sample deck and
//! plays a fixed list of actions through the solo session.

mod clicks;
mod condition;
mod prop;
mod table;
mod until;
mod view_predicates;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvError, RecvTimeoutError, channel};
use std::sync::{Arc, Mutex, PoisonError};

use beryl_card_names::decks::WU_CONTROL;
use beryl_engine::two_player::TwoPlayerSeat as _;
use beryl_search::GameAgent;
use beryl_session::ipc::{Fixture, FixtureMessage};
use beryl_session::protocol::PlayerView;
use beryl_session::{
    GameUpdate, HandOver, Prepared, Seat, SessionError, SessionId, Sessions, StartGame, Status,
    StopsSetting, agents, ipc,
};
use serde::Deserialize;

use crate::Catalog;
use crate::boards::{self, Scripted, Shared};

use self::clicks::Written;
pub(crate) use self::condition::Condition;
use self::prop::{Prop, Standing};
use self::until::Until;

/// The definitions file.
#[derive(Debug, Deserialize)]
struct Definitions {
    fixtures: Vec<Definition>,
}

/// One fixture: the game to play and where to stop.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    name: String,
    /// A seeded game's seed, with `decks`; or none, with `board`.
    seed: Option<u64>,
    /// Each seat's deck: `wu-control` (the mirror) or `fuzz:<seed>` for a fuzz deck.
    decks: Option<[String; 2]>,
    /// The board the game starts from, by its name in [`crate::boards::BOARDS`], in place of
    /// a seeded deal.
    board: Option<String>,
    table: Option<table::Definition>,
    /// The agent playing each seat of a seeded game. The stopping condition's seat is the
    /// human's. A board's own actions play it.
    agents: Option<[String; 2]>,
    stops: Option<StopsSetting>,
    until: Option<String>,
    /// What else must hold of the view the recording stops at (see [`Condition::parse`]).
    #[serde(default, rename = "where")]
    conditions: Vec<String>,
    /// How many more of the human's decisions to record past the stop, as the stand-in plays
    /// on: what the decision there led to.
    #[serde(default)]
    after: usize,
    /// The clicks made on the human's prompts from the stop on, each as [`Written::parse`]
    /// reads it, in place of the stand-in's answers: a seeded game's, or a board's whose
    /// scripts are done by its stop, where the seat is handed over as a spec's moment hands it
    /// to its player. The stand-in answers what the human is asked before the stop.
    #[serde(default)]
    clicks: Vec<String>,
    /// A prompt no card asks yet, stood on the board in place of what its first player is
    /// first asked ([`Prop::parse`]): the clicks are made on it, or with none, the seat across
    /// the table is shown that player asked it.
    prop: Option<String>,
}

impl Definition {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The seat its recording is seen from: its human's.
    pub fn seat(&self) -> Result<Seat, String> {
        if self.table.is_some() {
            return Ok(Seat::P0);
        }
        Ok(Until::parse(self.stop()?)?.seat())
    }

    /// The seats its recording can be seen from: its human's, and for a game with no prop the
    /// seat across the table too ([`record_from`]). A manual table has one viewer, and a prop
    /// stands only in its human's view.
    pub fn seats(&self) -> Result<Vec<Seat>, String> {
        let seat = self.seat()?;
        if self.table.is_some() || self.prop.is_some() {
            return Ok(vec![seat]);
        }
        Ok(vec![seat, seat.opponent()])
    }

    fn stop(&self) -> Result<&str, String> {
        self.until
            .as_deref()
            .ok_or_else(|| "a game fixture needs an `until` stop".to_owned())
    }
}

/// The fixtures the file `definitions` defines.
pub fn definitions(definitions: &Path) -> Result<Vec<Definition>, String> {
    let text = std::fs::read_to_string(definitions)
        .map_err(|e| format!("{}: {e}", definitions.display()))?;
    let defs: Definitions =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", definitions.display()))?;
    Ok(defs.fixtures)
}

/// Records the fixtures in `definitions` named in `only`, or all of them, into `out`.
pub fn record_all(
    catalog: &'static Catalog,
    definitions: &Path,
    out: &Path,
    only: &[String],
) -> Result<(), String> {
    let fixtures = self::definitions(definitions)?;
    if let Some(name) = only
        .iter()
        .find(|n| !fixtures.iter().any(|f| &f.name == *n))
    {
        return Err(format!("no fixture is named {name:?}"));
    }
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    for def in &fixtures {
        if !only.is_empty() && !only.contains(&def.name) {
            continue;
        }
        let mut messages =
            record(catalog, def, definitions).map_err(|e| format!("fixture {}: {e}", def.name))?;
        messages.insert(
            0,
            beryl_session::card_records::fixture_message(&messages, catalog.cards),
        );
        // The stage asks for the timing of its own steps once, whatever the game.
        messages.insert(
            1,
            FixtureMessage::response(ipc::STAGE_TIMINGS, &beryl_session::script::own_timings()),
        );
        let path = out.join(format!("{}.json", def.name));
        let json = serde_json::to_string(&Fixture { messages }).expect("a fixture serializes");
        std::fs::write(&path, json).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("{}", path.display());
    }
    Ok(())
}

/// How many messages a recording opens with before its first view.
const OPENING: usize = 3;

/// Where a recording stops: its stopping condition, and what else must hold of the view.
struct Stop {
    until: Until,
    conditions: Vec<Condition>,
}

impl Stop {
    fn parse(def: &Definition) -> Result<Stop, String> {
        Ok(Stop {
            until: Until::parse(def.stop()?)?,
            conditions: (def.conditions.iter())
                .map(|c| Condition::parse(c))
                .collect::<Result<_, _>>()?,
        })
    }

    /// Whether `view` is the stop.
    fn reached(&self, view: &PlayerView) -> bool {
        self.until.reached(view) && self.conditions.iter().all(|c| c.holds(view))
    }
}

/// The messages of `def`'s recording, or why it could not be made.
pub fn record(
    catalog: &'static Catalog,
    def: &Definition,
    source: &Path,
) -> Result<Vec<FixtureMessage>, String> {
    if let Some(table) = &def.table {
        if def.seed.is_some()
            || def.decks.is_some()
            || def.board.is_some()
            || def.agents.is_some()
            || def.stops.is_some()
            || def.until.is_some()
            || !def.conditions.is_empty()
            || def.after != 0
            || !def.clicks.is_empty()
            || def.prop.is_some()
        {
            return Err("a table fixture cannot have game fixture options".to_owned());
        }
        return table::record(table, source);
    }
    record_game(catalog, def, false)
}

/// The messages of `def`'s recording as `seat`, one of [`Definition::seats`], sees it. From the
/// seat across the table from its human, it is the same game, with the same stand-in answers,
/// the same clicks and the same stop, read from the human's views: only the views recorded are
/// the other seat's. That seat never decides, so nothing answers them.
pub fn record_from(
    catalog: &'static Catalog,
    def: &Definition,
    source: &Path,
    seat: Seat,
) -> Result<Vec<FixtureMessage>, String> {
    if seat == def.seat()? {
        return record(catalog, def, source);
    }
    if !def.seats()?.contains(&seat) {
        return Err(format!("it is not seen from seat {}", seat.index()));
    }
    record_game(catalog, def, true)
}

/// A view sent to the human, and with it the same update as the seat across the table sees it
/// when that is recorded.
type Seen = (GameUpdate, Option<GameUpdate>);

/// A session whose thread panicked (in a debug build, a script that breaks its checks)
/// says where. With `across`, the views recorded are the seat's across the table from the
/// human, and none of the human's answers is.
fn record_game(
    catalog: &'static Catalog,
    def: &Definition,
    across: bool,
) -> Result<Vec<FixtureMessage>, String> {
    let until = def.stop()?;
    let stop = Arc::new(Stop::parse(def)?);
    let clicks = (def.clicks.iter())
        .map(|c| Written::parse(c))
        .collect::<Result<Vec<_>, _>>()?;
    if !clicks.is_empty() && def.after > 0 {
        return Err("clicks begin at the stop, with nothing recorded `after` it".to_owned());
    }
    let prop = standing(def)?;
    let human = stop.until.seat();
    let Listening {
        sessions,
        updates,
        view_progress,
        progress,
        at_stop,
        guard,
    } = listen(catalog, &stop, across);
    let before = crate::panics::mark();
    let (session, unused) = start(catalog, def, &sessions, human, &progress, &at_stop, guard)?;
    // A seeded game owns no guard, so nothing tells of a panic before its first view.
    let mut unguarded = unused.is_some();
    let mut messages = vec![
        FixtureMessage::response(ipc::LIST_AGENTS, &agents()),
        FixtureMessage::response(ipc::START_GAME, &session),
        // A stops change returns nothing; its view is the open decision's again.
        FixtureMessage::response(ipc::SET_STOPS, &()),
    ];
    // The human's decisions still to record once the stop is reached.
    let mut left = None;
    loop {
        let update = if unguarded {
            first_view(&updates, before)
        } else {
            updates.recv()
        };
        unguarded = false;
        if let Some(failure) = boards::lock(&progress).failure.take() {
            return Err(failure);
        }
        let (update, other) = update
            .map_err(|_| "the session ended without a view".to_owned())?
            .map_err(|panic| format!("the session {panic}"))?;
        let (handed, waiting) = view_progress.recv().expect("script progress for this view");
        let reached = left.is_none() && stop.reached(&update.view);
        let status = &update.view.status;
        let (deciding, ended) = (
            matches!(status, Status::Deciding { .. }),
            matches!(status, Status::Over { .. } | Status::Stopped),
        );
        // The seat across the table from a prop is shown it in its first view.
        let watches = clicks.is_empty() && messages.len() == OPENING;
        let shown = match other {
            Some(other) => other,
            None if across => return Err("the session sent no view across the table".to_owned()),
            None => prop::watched(catalog.cards, prop.filter(|_| watches), human, &update)?,
        };
        messages.push(FixtureMessage::game_update(&shown));
        if !clicks.is_empty() && deciding {
            // The seat is the recording's from its stop, which the scripts must be done by:
            // a decision handed over anywhere else would wait for an answer nobody gives.
            if reached != handed {
                return Err(format!(
                    "the scripts must end at {until:?}, where its clicks begin"
                ));
            }
            if reached {
                answer(
                    &clicks,
                    prop,
                    &sessions,
                    &update,
                    &updates,
                    &mut messages,
                    across,
                )?;
                return Ok(messages);
            }
        }
        if reached {
            left = Some(def.after);
        } else if let Some(n) = left.filter(|_| deciding) {
            left = Some(n - 1);
        }
        if left == Some(0) || (ended && left.is_some()) {
            // A board's actions all lead up to where its recording ends.
            if let Some(action) = waiting {
                return Err(format!("the board's {action:?} was never taken"));
            }
            if !deciding && !ended && !across {
                // It ends as the human watches (`first handling`): a replay releases views
                // only on an answer, so one plays it on to the end.
                messages.push(FixtureMessage::response(ipc::CHOOSE, &()));
            }
            return Ok(messages);
        }
        if ended {
            let why = boards::lock(&progress).stalled();
            return Err(match why {
                Some(why) => format!("the game ended before {until:?}, with {why}"),
                None => format!("the game ended before {until:?}"),
            });
        }
        if deciding && !across {
            // The stand-in answered; the UI's `choose` would have returned nothing.
            messages.push(FixtureMessage::response(ipc::CHOOSE, &()));
        }
    }
}

/// The sessions a recording plays its game in, and what it hears from them.
struct Listening {
    sessions: Sessions,
    /// Each view sent to the human, or the panic that came in its place.
    updates: Receiver<Result<Seen, String>>,
    /// With each view, whether it is the stop, and the board's action still to be taken.
    view_progress: Receiver<(bool, Option<beryl_scenario::Action>)>,
    /// How far the board's scripts have got.
    progress: Shared,
    /// Whether the view last sent was the stop.
    at_stop: Arc<AtomicBool>,
    /// The guard that tells of a panic on the session's thread, for that thread to own.
    guard: crate::panics::Guard,
}

/// Sessions that send each view to a recording that stops at `stop`, with the view across the
/// table beside it when `across`.
fn listen(catalog: &'static Catalog, stop: &Arc<Stop>, across: bool) -> Listening {
    let (sent, updates) = channel::<Result<Seen, String>>();
    let told = sent.clone();
    let guard = crate::panics::Guard::new(move |panic| drop(told.send(Err(panic))));
    let progress = Shared::default();
    // Snapshot script progress with each view, before the session can answer its next decision.
    // The session also uses this snapshot to know when to hand the seat over.
    let (sent_progress, view_progress) = channel();
    let at_stop = Arc::new(AtomicBool::new(false));
    let (scripts, at, said) = (
        Arc::clone(&progress),
        Arc::clone(stop),
        Arc::clone(&at_stop),
    );
    // The session sends the other seat's view of an update just before the human's.
    let other = Arc::new(Mutex::new(None));
    let drawn = Arc::clone(&other);
    let sessions = Sessions::new(catalog.cards, None, move |update| {
        crate::panics::watch(|| {
            let told = sent.clone();
            move |panic| drop(told.send(Err(panic)))
        });
        let scripts = boards::lock(&scripts);
        let stopped = scripts.done() && at.reached(&update.view);
        said.store(stopped, Ordering::SeqCst);
        let _ = sent_progress.send((stopped, scripts.next.into_iter().flatten().next()));
        // The recording stops listening once it has what it came for.
        let other = other.lock().unwrap_or_else(PoisonError::into_inner).take();
        let _ = sent.send(Ok((update, other)));
    });
    let sessions = if across {
        sessions.with_across(move |update| {
            *drawn.lock().unwrap_or_else(PoisonError::into_inner) = Some(update);
        })
    } else {
        sessions
    };
    Listening {
        sessions,
        updates,
        view_progress,
        progress,
        at_stop,
        guard,
    }
}

/// The prop `def` stands on its board, if it has one.
fn standing(def: &Definition) -> Result<Option<Standing>, String> {
    let stops = def.stops.unwrap_or(StopsSetting::SMART);
    match (&def.prop, &def.board) {
        (Some(prop), Some(board)) => Ok(Some(Standing {
            prop: Prop::parse(prop)?,
            board: boards::named(board)?,
            stops,
        })),
        (Some(_), None) => Err("a prop stands on a board".to_owned()),
        (None, _) => Ok(None),
    }
}

/// Makes `clicks` from `stop`, the view a recording stopped at and the last of `messages`, and
/// adds what they led to: on the session's own prompts, or on `prop`, stood on its board in
/// place of what `stop` asks, whose answer leads to `stop` itself. With `across`, it adds only
/// the views across the table that the clicks led to.
fn answer(
    clicks: &[Written],
    prop: Option<Standing>,
    sessions: &Sessions,
    stop: &GameUpdate,
    updates: &Receiver<Result<Seen, String>>,
    messages: &mut Vec<FixtureMessage>,
    across: bool,
) -> Result<(), String> {
    let Some(prop) = prop else {
        let mut others = Vec::new();
        let mut next = || {
            let (update, other) = clicks::next_view(updates)?;
            others.extend(other);
            Ok(update)
        };
        let mut click = |request| sessions.click(request).map_err(|e| e.to_string());
        let mut heard = Vec::new();
        let said = if across { &mut heard } else { &mut *messages };
        clicks::play(clicks, &mut click, stop, &mut next, said)?;
        messages.extend(others.iter().map(FixtureMessage::game_update));
        return Ok(());
    };
    // A prop stands where the board's seat is first asked anything.
    if messages.len() != OPENING + 1 {
        return Err("a prop stands at its board's first view".to_owned());
    }
    let (asks, mut stood) = prop::stand(sessions.cards(), prop, stop)?;
    messages.pop();
    messages.push(FixtureMessage::game_update(&asks));
    let mut after = Some(prop::after(stop));
    let mut next = || {
        after
            .take()
            .ok_or("a prop's answer ends its clicks".to_owned())
    };
    let mut click = |request| stood.click(request);
    clicks::play(clicks, &mut click, &asks, &mut next, messages)
}

/// The first message of a session that owns no guard: its first view, or a panic nothing watched
/// since `before` that no guard has told of a second later. A guard tells of its own as its thread
/// unwinds, and a seeded game's first view comes at once, so the panic is taken for this game's
/// (in a run that fails all the same).
fn first_view(
    updates: &Receiver<Result<Seen, String>>,
    before: u64,
) -> Result<Result<Seen, String>, RecvError> {
    let mut seen = None;
    loop {
        match updates.recv_timeout(std::time::Duration::from_secs(1)) {
            Ok(update) => return Ok(update),
            Err(RecvTimeoutError::Disconnected) => return Err(RecvError),
            Err(RecvTimeoutError::Timeout) => match crate::panics::unwatched_since(before) {
                Some((number, panic)) if seen == Some(number) => {
                    crate::panics::claim(number);
                    return Ok(Err(format!("{panic} (before its first view)")));
                }
                untold => seen = untold.map(|(number, _)| number),
            },
        }
    }
}

/// Starts the fixture's game in `sessions`: its seeded deal, or its board, whose scripts
/// report to `progress` and whose thread owns `guard`; the guard back if nothing owns it. A
/// fixture with clicks hands its human seat over once `at_stop` says the view last sent was
/// its stop.
fn start(
    catalog: &'static Catalog,
    def: &Definition,
    sessions: &Sessions,
    human: Seat,
    progress: &Shared,
    at_stop: &Arc<AtomicBool>,
    guard: crate::panics::Guard,
) -> Result<(SessionId, Option<crate::panics::Guard>), String> {
    let mut guard = Some(guard);
    let stops = def.stops.unwrap_or(StopsSetting::SMART);
    let started: Result<SessionId, SessionError> =
        match (&def.board, def.seed, &def.decks, &def.agents) {
            (None, Some(seed), Some(decks), Some(agents)) => {
                let start = StartGame {
                    seed,
                    decks: [deck(catalog, &decks[0])?, deck(catalog, &decks[1])?],
                    human,
                    opponent: agents[human.opponent().index()].clone(),
                    stops,
                };
                let stand_in = &agents[human.index()];
                if def.clicks.is_empty() {
                    sessions.start_game(&start, Some(stand_in))
                } else {
                    // Its clicks are the human's answers from the stop on.
                    let stopped = Arc::clone(at_stop);
                    let hand_over = Box::new(move || stopped.load(Ordering::SeqCst)) as HandOver;
                    sessions.start_game_handing_over(&start, stand_in, hand_over)
                }
            }
            (Some(name), None, None, None) => {
                let board = boards::named(name)?;
                let (table, state) =
                    beryl_scenario::position(catalog.cards, board).map_err(|e| e.to_string())?;
                let agent = |seat: Seat| {
                    Scripted::new(board, seat, &table, Arc::clone(progress))
                        .map(|a| Box::new(a) as Box<dyn GameAgent + Send>)
                };
                let opponent =
                    Scripted::new(board, human.opponent(), &table, Arc::clone(progress))?;
                let guard = guard.take().expect("the guard is given once");
                // Its clicks are the human's answers from the stop on.
                let stopped = Arc::clone(at_stop);
                let hand_over = (!def.clicks.is_empty())
                    .then(|| Box::new(move || stopped.load(Ordering::SeqCst)) as HandOver);
                sessions.start_prepared(Prepared {
                    opponent: Box::new(opponent.guarded(guard)),
                    stand_in: agent(human)?,
                    scripted: true,
                    hand_over,
                    table,
                    state,
                    human,
                    stops,
                })
            }
            _ => {
                return Err("a fixture has a seed, decks and agents, or a board".to_owned());
            }
        };
    Ok((started.map_err(|e| e.to_string())?, guard))
}

fn deck(catalog: &'static Catalog, name: &str) -> Result<Vec<(u8, String)>, String> {
    let list = if name == "wu-control" {
        WU_CONTROL.to_vec()
    } else if let Some(seed) = name.strip_prefix("fuzz:") {
        catalog.fuzz_deck(
            seed.parse()
                .map_err(|_| format!("bad fuzz seed {seed:?}"))?,
        )
    } else {
        return Err(format!("no deck is named {name:?}"));
    };
    Ok(list.into_iter().map(|(n, c)| (n, c.to_owned())).collect())
}

fn number<T: std::str::FromStr>(text: &str, n: &str) -> Result<T, String> {
    n.parse().map_err(|_| format!("{text:?}: bad number {n:?}"))
}

#[cfg(test)]
mod tests {
    use crate::catalog::TEST;
    use beryl_session::GameUpdate;
    use beryl_session::protocol::{Kind, Step, ZoneName};

    use super::condition::Mark;
    use super::*;

    #[test]
    fn stopping_conditions_parse() {
        assert_eq!(
            Until::parse("first chooseUpTo decision for seat 1"),
            Ok(Until::First {
                kind: Kind::ChooseUpTo,
                seat: Seat::P1,
                turn: 0,
            })
        );
        assert_eq!(
            Until::parse("first priority decision for seat 0 from turn 9"),
            Ok(Until::First {
                kind: Kind::Priority,
                seat: Seat::P0,
                turn: 9,
            })
        );
        assert_eq!(
            Until::parse("first halt for seat 1 from turn 4"),
            Ok(Until::Halt {
                seat: Seat::P1,
                turn: 4,
            })
        );
        assert_eq!(
            Until::parse("first handling for seat 1"),
            Ok(Until::Handling {
                seat: Seat::P1,
                turn: 0,
            })
        );
        assert!(Until::parse("game over from turn 3").is_err());
        assert_eq!(Until::parse("game over"), Ok(Until::GameOver));
        assert!(Until::parse("first scrying decision for seat 0").is_err());
        assert!(Until::parse("first target decision for seat 2").is_err());
    }

    #[test]
    fn conditions_parse() {
        assert_eq!(
            Condition::parse("step declareBlockers"),
            Ok(Condition::Step(Step::DeclareBlockers))
        );
        assert_eq!(
            Condition::parse("castable Mana Drain"),
            Ok(Condition::Castable("Mana Drain".to_owned()))
        );
        assert_eq!(Condition::parse("blocking"), Ok(Condition::Blocking));
        assert_eq!(
            Condition::parse("playable Sink into Stupor"),
            Ok(Condition::Playable("Sink into Stupor".to_owned()))
        );
        assert_eq!(
            Condition::parse("abilities of The Wandering Emperor"),
            Ok(Condition::Abilities("The Wandering Emperor".to_owned()))
        );
        assert_eq!(
            Condition::parse("namesake candidates"),
            Ok(Condition::NamesakeCandidates)
        );
        assert_eq!(
            Condition::parse("resolving Mana Tithe"),
            Ok(Condition::Resolving("Mana Tithe".to_owned()))
        );
        assert_eq!(
            Condition::parse("casting Thieving Skydiver"),
            Ok(Condition::Casting("Thieving Skydiver".to_owned()))
        );
        assert_eq!(
            Condition::parse("playable pair in hand"),
            Ok(Condition::PlayablePair)
        );
        assert_eq!(
            Condition::parse("exiled beneath a permanent"),
            Ok(Condition::ExiledBeneath)
        );
        assert_eq!(
            Condition::parse("exiled with a permanent"),
            Ok(Condition::ExiledWith)
        );
        assert_eq!(
            Condition::parse("attached across the table"),
            Ok(Condition::AttachedAcross)
        );
        assert_eq!(
            Condition::parse("damage beside a twin"),
            Ok(Condition::Marked(Mark::Damage))
        );
        assert_eq!(
            Condition::parse("castable from exile"),
            Ok(Condition::CastableFrom(ZoneName::Exile))
        );
        assert_eq!(
            Condition::parse("known card in the opponent's hand"),
            Ok(Condition::KnownOpponentsHand)
        );
        assert_eq!(
            Condition::parse("ability targeting beside its source"),
            Ok(Condition::NeighbourTarget)
        );
        assert_eq!(Condition::parse("token twins"), Ok(Condition::TokenTwins));
        assert_eq!(Condition::parse("emblems a side"), Ok(Condition::Emblems));
        assert_eq!(
            Condition::parse("changed token"),
            Ok(Condition::ChangedToken)
        );
        assert_eq!(
            Condition::parse("token beside its card"),
            Ok(Condition::TokenBesideCard)
        );
        assert_eq!(Condition::parse("stolen permanent"), Ok(Condition::Stolen));
        assert_eq!(
            Condition::parse("mana floating"),
            Ok(Condition::ManaFloating)
        );
        assert_eq!(Condition::parse("X on the stack"), Ok(Condition::XOnStack));
        assert_eq!(
            Condition::parse("8 nonland permanents a side"),
            Ok(Condition::Crowded(8))
        );
        assert_eq!(
            Condition::parse("spell on the stack"),
            Ok(Condition::SpellOnStack)
        );
        assert!(Condition::parse("many nonland permanents a side").is_err());
        assert!(Condition::parse("step lunch").is_err());
        assert!(Condition::parse("raining").is_err());
    }

    #[test]
    fn a_recording_stops_at_its_condition() {
        let def = Definition {
            name: "t".to_owned(),
            seed: Some(4),
            // Fixed decks: fuzz decks are drawn from every card, so a new card changes the game.
            decks: Some(["wu-control".to_owned(), "wu-control".to_owned()]),
            board: None,
            table: None,
            agents: Some(["random".to_owned(), "random".to_owned()]),
            stops: None,
            until: Some("first target decision for seat 1".to_owned()),
            conditions: vec![],
            after: 0,
            clicks: vec![],
            prop: None,
        };
        let messages = record_game(&TEST, &def, false).unwrap();
        let Some(FixtureMessage::Event { payload, .. }) = messages.last() else {
            panic!("a recording ends on a view");
        };
        let update: GameUpdate = serde_json::from_value(payload.clone()).unwrap();
        let Status::Deciding { decision } = update.view.status else {
            panic!("a recording stops at a decision");
        };
        assert_eq!(decision.kind, Kind::Target);
    }

    /// The views of `messages`, a recording.
    fn views(messages: &[FixtureMessage]) -> Vec<PlayerView> {
        (messages.iter())
            .filter_map(|message| match message {
                FixtureMessage::Event { payload, .. } => Some(
                    serde_json::from_value::<GameUpdate>(payload.clone())
                        .unwrap()
                        .view,
                ),
                FixtureMessage::Response { .. } => None,
            })
            .collect()
    }

    /// Seen from across the table, a recording is the same game to the same stop, a view of
    /// each update the human was shown, from the other side and with nothing to decide: for a
    /// seeded game, and for a board whose clicks play on past its stop.
    #[test]
    fn a_recording_from_across_the_table_shows_the_same_updates() {
        let seeded = serde_json::json!({
            "name": "t",
            "seed": 4,
            "decks": ["wu-control", "wu-control"],
            "agents": ["random", "random"],
            "until": "first target decision for seat 1",
        });
        let clicked = serde_json::json!({
            "name": "t",
            "board": "shape-pick",
            "until": "first priority decision for seat 0",
            "clicks": ["Counterspell", "Island", "press decline"],
        });
        for json in [seeded, clicked] {
            let def: Definition = serde_json::from_value(json).unwrap();
            let seats = def.seats().unwrap();
            assert_eq!(seats.len(), 2);
            let path = Path::new("unused");
            let human = views(&record_from(&TEST, &def, path, seats[0]).unwrap());
            let across = views(&record_from(&TEST, &def, path, seats[1]).unwrap());
            assert_eq!(human.len(), across.len(), "{}", def.name);
            for (mine, theirs) in human.iter().zip(&across) {
                assert_eq!(mine.turn.number, theirs.turn.number);
                assert_eq!(mine.turn.step, theirs.turn.step);
                assert_ne!(mine.turn.active, theirs.turn.active);
                assert!(!matches!(theirs.status, Status::Deciding { .. }));
            }
        }
    }
}
