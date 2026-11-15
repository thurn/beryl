//! A recording, and the session's recorded answers to the commands a test's presses send.
//!
//! The answers follow the web interface's fixture transport (`fixtureTransport` in
//! ui/src/ipc.ts): each command has a queue of recorded responses, the first answers it and is
//! used up unless it is the last, and a response recorded with its request answers that
//! request alone. A `choose`, or a `click` whose reply says it answered, plays the recording
//! on to its next views.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use beryl_protocol::CandidateIndex;
use beryl_protocol::GameUpdate;
use beryl_protocol::card_records::CardRecords;
use beryl_protocol::ipc::{
    self, CancelRequest, ChooseRequest, ConcedeRequest, FixtureMessage, PassOnRequest,
    SetStopsRequest, UndoRequest,
};
use beryl_protocol::prompt::{Click, ClickReply};
use beryl_protocol::protocol::Status;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::command::{Reply, SessionCommand};
use crate::element::Press;
use crate::screens::{Screen, Sheet};

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

/// The event that a recording made with no engine, such as a screen's, holds to put the client
/// on a screen or open a sheet over it before its first view. Its payload is the name of the
/// screen or the sheet (`Screen::name`, `Sheet::name`).
pub const OPEN: &str = "beryl3d-open";

impl Recording {
    /// The presses that its `OPEN` events stand for, in order.
    ///
    /// # Panics
    ///
    /// When an `OPEN` event names no screen or sheet.
    #[must_use]
    pub fn opens(&self) -> Vec<Press> {
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
                    (Some(screen), _) => Press::Open(screen),
                    (None, Some(sheet)) => Press::Sheet(Some(sheet)),
                    (None, None) => panic!("{OPEN} names no screen or sheet: {name:?}"),
                }
            })
            .collect()
    }
}

/// Whether the session follows `update` with its next view unasked, because the game plays on
/// without the viewer, as the fixture transport's `playsOn` says.
#[must_use]
pub fn plays_on(update: &GameUpdate) -> bool {
    matches!(update.view.status, Status::Thinking | Status::Playing)
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
        let queue = self
            .queues
            .get_mut(name)
            .filter(|queue| !queue.is_empty())
            .ok_or_else(|| format!("the recording has no response for `{name}` ({command:?})"))?;
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
        Ok(Answer {
            reply: Reply::Done(Ok(())),
            plays_on: name == ipc::CHOOSE,
        })
    }
}

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
    }
}
