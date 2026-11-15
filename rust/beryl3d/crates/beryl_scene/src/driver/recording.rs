//! A recording, and the session's recorded answers to the commands a test's presses send.
//!
//! The answers follow the web interface's fixture transport (`fixtureTransport` in
//! ui/src/ipc.ts): each command has a queue of recorded responses, the first answers it and is
//! used up unless it is the last, and a response recorded with its request answers that
//! request alone. A `choose`, or a `click` whose reply says it answered, plays the recording
//! on to its next views.

use std::collections::{BTreeMap, VecDeque};

use beryl_session::GameUpdate;
use beryl_session::ipc::{
    self, CancelRequest, ConcedeRequest, FixtureMessage, PassOnRequest, SetStopsRequest,
    UndoRequest,
};
use beryl_session::prompt::ClickReply;
use beryl_session::protocol::Status;
use serde::Serialize;
use serde_json::Value;

use crate::command::{Reply, SessionCommand};

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
                    serde_json::from_value(payload.clone())
                        .map_err(|error| format!("message {at} is not a view: {error}")),
                ),
                _ => None,
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
#[derive(Clone, Debug, Default)]
pub struct Responses {
    queues: BTreeMap<String, VecDeque<(Option<Value>, Value)>>,
}

impl Responses {
    #[must_use]
    pub fn new(recording: &Recording) -> Self {
        let mut queues: BTreeMap<String, VecDeque<(Option<Value>, Value)>> = BTreeMap::new();
        for message in &recording.0 {
            if let FixtureMessage::Response {
                command,
                request,
                value,
            } = message
            {
                queues
                    .entry(command.clone())
                    .or_default()
                    .push_back((request.clone(), value.clone()));
            }
        }
        Self { queues }
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
        let queue = self
            .queues
            .get_mut(name)
            .filter(|queue| !queue.is_empty())
            .ok_or_else(|| format!("the recording has no response for `{name}` ({command:?})"))?;
        let (recorded, value) = queue.front().cloned().expect("the queue is not empty");
        if let Some(recorded) = recorded
            && Some(&recorded) != request.as_ref()
        {
            let asked = request.map_or_else(|| "nothing".to_owned(), |r| r.to_string());
            return Err(format!("`{name}` was recorded for {recorded}, not {asked}"));
        }
        if queue.len() > 1 {
            queue.pop_front();
        }
        if name == ipc::CLICK {
            let reply: ClickReply = serde_json::from_value(value)
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
