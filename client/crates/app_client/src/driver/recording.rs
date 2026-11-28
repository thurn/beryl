//! A recording, and the session's recorded answers to the commands a test's presses send.
//!
//! The answers follow the web interface's fixture transport (`fixtureTransport` in
//! ui/src/ipc.ts): each command has a queue of recorded responses, the first answers it and is
//! used up unless it is the last, and a response recorded with its request answers that
//! request alone. A `choose`, or a `click` whose reply says it answered, plays the recording
//! on to its next views.
//!
//! The start screen asks for the saved tables each time it opens. A recording that holds no
//! `table_saved` response answers with none, as a profile that never hosted or joined a table
//! does, so that every recording that reaches the start screen need not record one. A
//! recorded `table_host`, `table_join` or `table_resume` response whose value is a
//! `TableConnectionError` (`{"reason": ...}`) is that command failing.

use crate::client;
use app_screens::screens::bug_report;
use app_screens::screens::settings::Message as SettingsMessage;
use app_screens::screens::testing::playground::Card;
use client::{Reply, SessionCommand};
use kit_ui::element::Action;
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use beryl_protocol::GameUpdate;
use beryl_protocol::card_records::CardRecords;
use beryl_protocol::ipc::{
    self, CancelRequest, ChooseRequest, ConcedeRequest, FixtureMessage, LobbyUpdate, PassOnRequest,
    SavedTable, SetStopsRequest, TableConnectionError, UndoRequest,
};
use beryl_protocol::prompt::{Click, ClickReply};
use beryl_protocol::{AgentView, CandidateIndex, SessionId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use game_hud::navigation::{Screen, Sheet};
use game_probe::driver::{OPEN, RECORDINGS};

/// One recording's messages for one seat: the session's views, as `game-update` events, and
/// its responses to commands. Other events are ignored.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recording(pub Vec<FixtureMessage>);

impl Recording {
    /// The recording's views, in order.
    ///
    /// # Errors
    ///
    /// When a `game-update` event does not hold a view.
    pub fn views(&self) -> Result<Vec<GameUpdate>, String> {
        self.0
            .iter()
            .enumerate()
            .filter_map(|(at, message)| match message {
                FixtureMessage::Event { event, payload } if event == ipc::GAME_UPDATE => Some(
                    GameUpdate::deserialize(payload)
                        .map_err(|error| format!("message {at} is not a view: {error}")),
                ),
                _ => None,
            })
            .collect()
    }
}

impl Recording {
    /// The card store's records that the recording answered `card_records` with, if it holds
    /// them: every recording in the pack starts with them (`card_records::fixture_message`).
    ///
    /// # Errors
    ///
    /// When the recorded answer is not card records.
    pub fn card_records(&self) -> Result<Option<CardRecords>, String> {
        let Some(value) = self.0.iter().find_map(|message| match message {
            FixtureMessage::Response { command, value, .. } if command == ipc::CARD_RECORDS => {
                Some(value)
            }
            _ => None,
        }) else {
            return Ok(None);
        };
        CardRecords::deserialize(value)
            .map(Some)
            .map_err(|error| format!("the recorded `card_records` answer is not one: {error}"))
    }
}

impl Recording {
    /// The agents the recording answered `list_agents` with, if it holds them: a screen's
    /// recording does, so that the start screen has opponents to offer.
    ///
    /// # Errors
    ///
    /// When the recorded answer is not a list of agents.
    pub fn agents(&self) -> Result<Option<Vec<AgentView>>, String> {
        let Some(value) = self.0.iter().find_map(|message| match message {
            FixtureMessage::Response { command, value, .. } if command == ipc::LIST_AGENTS => {
                Some(value)
            }
            _ => None,
        }) else {
            return Ok(None);
        };
        Vec::<AgentView>::deserialize(value)
            .map(Some)
            .map_err(|error| format!("the recorded `list_agents` answer is not one: {error}"))
    }
}

impl Recording {
    /// The names its `RECORDINGS` event lists, if it has one.
    #[must_use]
    pub fn recordings(&self) -> Option<Vec<String>> {
        self.0.iter().find_map(|message| match message {
            FixtureMessage::Event { event, payload } if event == RECORDINGS => {
                Vec::<String>::deserialize(payload).ok()
            }
            _ => None,
        })
    }

    /// The lobbies its `table-lobby` events hold, in order: a screen recording's lobby, built
    /// by hand.
    #[must_use]
    pub fn lobbies(&self) -> Vec<LobbyUpdate> {
        (self.0.iter())
            .filter_map(|message| match message {
                FixtureMessage::Event { event, payload } if event == ipc::TABLE_LOBBY => {
                    LobbyUpdate::deserialize(payload).ok()
                }
                _ => None,
            })
            .collect()
    }

    /// The presses that its `OPEN` events stand for, in order.
    ///
    /// # Panics
    ///
    /// When an `OPEN` event names no screen or sheet.
    #[must_use]
    pub fn opens(&self) -> Vec<Action> {
        let name_of = |payload: &Value| payload.as_str().unwrap_or_default().to_owned();
        (self.0.iter())
            .filter_map(|message| match message {
                FixtureMessage::Event { event, payload } if event == OPEN => Some(name_of(payload)),
                _ => None,
            })
            .map(|name| {
                let screen = Screen::ALL.into_iter().find(|screen| screen.name() == name);
                let sheet = Sheet::ALL.into_iter().find(|sheet| sheet.name() == name);
                match (screen, sheet) {
                    (Some(screen), _) => {
                        Action::new(app_screens::screens::start::Message::Open(screen))
                    }
                    (None, Some(Sheet::Settings)) => Action::new(SettingsMessage::Open),
                    (None, Some(Sheet::BugReport)) => Action::new(bug_report::Message::Open),
                    (None, None) => panic!("{OPEN} names no screen or sheet: {name:?}"),
                }
            })
            .collect()
    }
}

/// The recorded answer to a command.
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    pub reply: Reply,
    /// Whether the answer plays the recording on to its next views.
    pub plays_on: bool,
}

/// The recorded responses, one queue per command.
///
/// A queue holds the indices of its responses in the recording rather than copies of them. A
/// test plays its recording several times and makes a new `Responses` each time, and copying
/// every response, the card store's records among them, took about a tenth of a test's time.
#[derive(Clone, Debug, Default)]
pub struct Responses {
    recording: Arc<Recording>,
    queues: BTreeMap<String, VecDeque<usize>>,
}

impl Responses {
    #[must_use]
    pub fn new(recording: &Recording) -> Self {
        Self::shared(Arc::new(recording.clone()))
    }

    /// The responses of a recording that other pages share.
    #[must_use]
    pub fn shared(recording: Arc<Recording>) -> Self {
        let mut queues: BTreeMap<String, VecDeque<usize>> = BTreeMap::new();
        for (at, message) in recording.0.iter().enumerate() {
            if let FixtureMessage::Response { command, .. } = message {
                queues.entry(command.clone()).or_default().push_back(at);
            }
        }
        Self { recording, queues }
    }

    /// The recorded answer to `command`.
    ///
    /// # Errors
    ///
    /// When the recording has no response for the command, when its response was recorded for
    /// another request, or when the command is an undo or a cancel, which the driver does not
    /// replay.
    pub fn answer(&mut self, command: &SessionCommand) -> Result<Answer, String> {
        let (name, request) = wire(command);
        if matches!(name, ipc::UNDO | ipc::CANCEL) {
            return Err(format!(
                "the recording cannot replay `{name}`: the test driver does not rewind a \
                 recording"
            ));
        }
        if let SessionCommand::Click(click) = command
            && matches!(click.click, Click::Button { .. })
            && !self.queues.contains_key(ipc::CLICK)
            && self.queues.contains_key(ipc::CHOOSE)
        {
            // A recording made without clicks (`pay-default`) holds the stand-in's answer to
            // each prompt as a `choose`, and the views that answer led to. A press on one of
            // the prompt's buttons answers the prompt, so it takes the recorded answer and
            // plays on to those views, as any candidate's `choose` would.
            self.answer(&SessionCommand::Choose(ChooseRequest {
                decision: click.decision,
                index: CandidateIndex(0),
                continuation: None,
            }))?;
            return Ok(Answer {
                reply: Reply::Click(Ok(ClickReply::Answered)),
                plays_on: true,
            });
        }
        if name == ipc::TABLE_SAVED && !self.queues.contains_key(name) {
            return Ok(Answer {
                reply: Reply::TableSaved(Ok(Vec::new())),
                plays_on: false,
            });
        }
        let queue = self
            .queues
            .get_mut(name)
            .filter(|queue| !queue.is_empty())
            .ok_or_else(|| format!("the recording has no response for `{name}` ({command:?})"))?;
        // A click on a later decision than the recorded clicks at the front of the queue was
        // made by a test that let the recording play past the decisions those answered, which
        // the recording's own views answered instead: they are skipped.
        if let SessionCommand::Click(click) = command {
            let recording = &self.recording;
            while queue.len() > 1
                && seq_of(&recording.0[queue[0]])
                    .is_some_and(|seq| seq < u64::from(click.decision.seq))
            {
                queue.pop_front();
            }
        }
        let at = *queue.front().expect("the queue is not empty");
        let FixtureMessage::Response {
            request: recorded,
            value,
            ..
        } = &self.recording.0[at]
        else {
            unreachable!("only responses are queued");
        };
        if let Some(recorded) = recorded
            && Some(recorded) != request.as_ref()
        {
            let asked = request.map_or_else(|| "nothing".to_owned(), |r| r.to_string());
            return Err(format!("`{name}` was recorded for {recorded}, not {asked}"));
        }
        if queue.len() > 1 {
            queue.pop_front();
        }
        if name == ipc::CLICK {
            let reply = ClickReply::deserialize(value)
                .map_err(|error| format!("the recorded `click` reply is not one: {error}"))?;
            let plays_on = reply == ClickReply::Answered;
            return Ok(Answer {
                reply: Reply::Click(Ok(reply)),
                plays_on,
            });
        }
        if name == ipc::TABLE_ACT {
            // A table's answer says whether it took the action, which the client acts on: a
            // look it took opens the library's viewer.
            let reply = ipc::TableActReply::deserialize(value)
                .map_err(|error| format!("the recorded `table_act` reply is not one: {error}"))?;
            return Ok(Answer {
                reply: Reply::TableAct(reply),
                plays_on: false,
            });
        }
        let recorded = |what: &str, error: serde_json::Error| {
            format!("the recorded `{what}` answer is not one: {error}")
        };
        let reply = match (name, command) {
            (PLAYGROUND_FIND, SessionCommand::PlaygroundFind(text)) => Reply::PlaygroundFound(
                text.clone(),
                Option::<String>::deserialize(value).map_err(|error| recorded(name, error))?,
            ),
            (PLAYGROUND_EXAMPLES, _) => Reply::PlaygroundExamples(Ok(
                Card::deserialize(value).map_err(|error| recorded(name, error))?
            )),
            (PLAYGROUND_START, _) => Reply::PlaygroundStarted(Ok(
                SessionId::deserialize(value).map_err(|error| recorded(name, error))?
            )),
            _ => reply(name, value)?,
        };
        Ok(Answer {
            reply,
            plays_on: name == ipc::CHOOSE,
        })
    }
}

/// The reply a recorded answer `value` to the command `name` stands for, for every command
/// whose answer the client reads no more of than whether it failed.
fn reply(name: &str, value: &Value) -> Result<Reply, String> {
    Ok(match name {
        ipc::TABLE_SAVED => Reply::TableSaved(Ok(Vec::<SavedTable>::deserialize(value)
            .map_err(|error| format!("the recorded `table_saved` answer is not one: {error}"))?)),
        ipc::TABLE_HOST | ipc::TABLE_JOIN | ipc::TABLE_RESUME => Reply::TableOpen(
            TableConnectionError::deserialize(value).map_or(Ok(()), |error| Err(error.reason)),
        ),
        _ => Reply::Done(Ok(())),
    })
}

/// The sequence number of the decision a recorded click was made on, if `message` is one.
fn seq_of(message: &FixtureMessage) -> Option<u64> {
    let FixtureMessage::Response {
        request: Some(request),
        ..
    } = message
    else {
        return None;
    };
    request.get("decision")?.get("seq")?.as_u64()
}

/// The names a story's recorded answers to the playground's commands go by: the web app's IPC
/// has none, its playground page having had a server of its own.
pub const PLAYGROUND_FIND: &str = "playground_find";
pub const PLAYGROUND_EXAMPLES: &str = "playground_examples";
pub const PLAYGROUND_START: &str = "playground_start";

/// The command's name as the app's IPC knows it, and its request as the app would receive it.
fn wire(command: &SessionCommand) -> (&'static str, Option<Value>) {
    fn value(request: &impl Serialize) -> Option<Value> {
        serde_json::to_value(request).ok()
    }
    match command {
        SessionCommand::Start(request) => (ipc::START_GAME, value(request)),
        SessionCommand::Resume => (ipc::RESUME_GAME, None),
        SessionCommand::Choose(request) => (ipc::CHOOSE, value(request)),
        SessionCommand::Click(request) => (ipc::CLICK, value(request)),
        &SessionCommand::Cancel(decision) => (ipc::CANCEL, value(&CancelRequest { decision })),
        &SessionCommand::Undo(decision) => (ipc::UNDO, value(&UndoRequest { decision })),
        &SessionCommand::PassOn(decision) => (ipc::PASS_ON, value(&PassOnRequest { decision })),
        &SessionCommand::SetStops(session, stops) => {
            (ipc::SET_STOPS, value(&SetStopsRequest { session, stops }))
        }
        &SessionCommand::Concede(session) => (ipc::CONCEDE, value(&ConcedeRequest { session })),
        SessionCommand::TableSolo(request) => (ipc::TABLE_SOLO, value(request)),
        SessionCommand::TableAct(action) => (
            ipc::TABLE_ACT,
            value(&ipc::TableActRequest {
                action: action.clone(),
            }),
        ),
        SessionCommand::TableAnswer(answer) => (
            ipc::TABLE_ANSWER,
            value(&ipc::TableAnswerRequest {
                answer: answer.clone(),
            }),
        ),
        SessionCommand::TableLeave => (ipc::TABLE_LEAVE, None),
        SessionCommand::TableHost { name, link } => (
            ipc::TABLE_HOST,
            value(&ipc::TableHostRequest {
                link: *link,
                settings: ipc::TableHostSettings {
                    name: name.clone(),
                    starting_life: None,
                    starting_hand_size: None,
                },
            }),
        ),
        SessionCommand::TableJoin { invite, name } => (
            ipc::TABLE_JOIN,
            value(&ipc::TableJoinRequest {
                invite: invite.clone(),
                name: name.clone(),
            }),
        ),
        SessionCommand::TableResume(id) => (
            ipc::TABLE_RESUME,
            value(&ipc::TableResumeRequest { id: id.clone() }),
        ),
        SessionCommand::TableSaved => (ipc::TABLE_SAVED, None),
        SessionCommand::TableLobby(change) => (
            ipc::TABLE_LOBBY_COMMAND,
            value(&ipc::TableLobbyRequest {
                change: change.clone(),
            }),
        ),
        SessionCommand::CardSearch(text) => (
            ipc::CARD_SEARCH,
            value(&game_manual::manual::tokens::request(text.clone())),
        ),
        SessionCommand::DeckList => (ipc::DECK_LIST, None),
        SessionCommand::DeckImport(request) => (ipc::DECK_IMPORT, value(request)),
        // The playground has no command of the web app's: a recording never answers one.
        SessionCommand::PlaygroundFind(text) => (PLAYGROUND_FIND, value(text)),
        SessionCommand::PlaygroundExamples(card) => (PLAYGROUND_EXAMPLES, value(card)),
        SessionCommand::PlaygroundStart { card, variant } => {
            (PLAYGROUND_START, value(&[card, variant]))
        }
    }
}
