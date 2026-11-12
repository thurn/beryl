//! The clicks a fixture's recording makes on its human's prompts once it has reached its stop
//! (plan K26): each goes to the session as the UI's would, and the recording keeps the
//! request with its reply, so a replay answers the UI's click only if it is that one. A click
//! that answers its prompt plays the game on to the human's next decision, whose views the
//! recording takes as it takes any others.

use std::sync::mpsc::Receiver;

use beryl_session::ipc::{self, FixtureMessage};
use beryl_session::prompt::{ButtonId, Click, ClickReply, ClickRequest, PromptView};
use beryl_session::protocol::{PlayerView, Side};
use beryl_session::script::Subject;
use beryl_session::{DecisionId, GameUpdate, Sessions, Status};

/// One click, as a definition writes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Written {
    /// On a card, by its name: the one of that name the open prompt marks, or with none
    /// marked, the first the view shows.
    Card(String),
    /// On a player.
    Player(Side),
    /// On one of the prompt's buttons.
    Press(ButtonId),
}

impl Written {
    /// Parses "press confirm", "press decline", "press auto", "press passOn" or "press
    /// option <n>" for a button, "player you" or "player opponent" for a player, and
    /// anything else as the name of a card.
    pub(super) fn parse(text: &str) -> Result<Written, String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        match words.as_slice() {
            ["press", "confirm"] => Ok(Written::Press(ButtonId::Confirm)),
            ["press", "decline"] => Ok(Written::Press(ButtonId::Decline)),
            ["press", "auto"] => Ok(Written::Press(ButtonId::Auto)),
            ["press", "passOn"] => Ok(Written::Press(ButtonId::PassOn)),
            ["press", "option", n] => Ok(Written::Press(ButtonId::Option {
                index: super::number(text, n)?,
            })),
            ["press", ..] => Err(format!("{text:?}: no such button")),
            ["player", "you"] => Ok(Written::Player(Side::You)),
            ["player", "opponent"] => Ok(Written::Player(Side::Opponent)),
            ["player", ..] => Err(format!("{text:?}: no such player")),
            [] => Err("an empty click".to_owned()),
            _ => Ok(Written::Card(text.to_owned())),
        }
    }

    /// The click this is on `prompt`, open in `view`.
    fn click(&self, view: &PlayerView, prompt: &PromptView) -> Result<Click, String> {
        let on = match self {
            Written::Press(button) => return Ok(Click::Button { button: *button }),
            Written::Player(player) => Subject::Player { player: *player },
            Written::Card(name) => {
                let named = || {
                    (view.objects.iter())
                        .filter(|o| o.name.as_deref() == Some(name))
                        .map(|o| o.slot)
                };
                let marked = |slot: &u16| {
                    (prompt.marks.iter()).any(|m| m.on == Subject::Object { slot: *slot })
                };
                let slot = named()
                    .find(marked)
                    .or_else(|| named().next())
                    .ok_or_else(|| format!("the view shows no {name}"))?;
                Subject::Object { slot }
            }
        };
        Ok(Click::On { on })
    }
}

/// Makes `clicks` in order from the human's decision open in `stop`, the view the recording
/// stopped at, and adds what the session said to `messages`: each click's reply, and after
/// one that answered, the views up to the human's next decision. `next` gives the session's
/// next view, or why there is none.
pub(super) fn play(
    clicks: &[Written],
    sessions: &Sessions,
    stop: &GameUpdate,
    next: &mut dyn FnMut() -> Result<GameUpdate, String>,
    messages: &mut Vec<FixtureMessage>,
) -> Result<(), String> {
    let mut view = stop.view.clone();
    let mut open = opened(&view);
    for (i, written) in clicks.iter().enumerate() {
        let Some((decision, prompt)) = &mut open else {
            return Err(format!("no decision is open for click {i}, {written:?}"));
        };
        let request = ClickRequest {
            decision: *decision,
            revision: prompt.revision,
            click: written.click(&view, prompt)?,
        };
        let reply = sessions
            .click(request)
            .map_err(|e| format!("click {i}, {written:?}: {e}"))?;
        messages.push(FixtureMessage::asked(ipc::CLICK, &request, &reply));
        match reply {
            ClickReply::Open { prompt: after } => *prompt = *after,
            ClickReply::Answered => {
                // The game plays on to the human's next decision, or to its end.
                loop {
                    let update = next()?;
                    messages.push(FixtureMessage::game_update(&update));
                    view = update.view;
                    if !matches!(view.status, Status::Thinking | Status::Playing) {
                        break;
                    }
                }
                open = opened(&view);
            }
        }
    }
    Ok(())
}

/// The decision open in `view` and its prompt as it opened.
fn opened(view: &PlayerView) -> Option<(DecisionId, PromptView)> {
    match &view.status {
        Status::Deciding { decision } => Some((decision.id, (*decision.prompt).clone())),
        _ => None,
    }
}

/// The session's next view from `updates`, or the panic or the end that came in its place.
pub(super) fn next_view(
    updates: &Receiver<Result<GameUpdate, String>>,
) -> Result<GameUpdate, String> {
    updates
        .recv()
        .map_err(|_| "the session ended without a view".to_owned())?
        .map_err(|panic| format!("the session {panic}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicks_parse() {
        assert_eq!(
            Written::parse("press decline"),
            Ok(Written::Press(ButtonId::Decline))
        );
        assert_eq!(
            Written::parse("press passOn"),
            Ok(Written::Press(ButtonId::PassOn))
        );
        assert_eq!(
            Written::parse("press option 2"),
            Ok(Written::Press(ButtonId::Option { index: 2 }))
        );
        assert_eq!(
            Written::parse("player opponent"),
            Ok(Written::Player(Side::Opponent))
        );
        assert_eq!(
            Written::parse("Jace, the Mind Sculptor"),
            Ok(Written::Card("Jace, the Mind Sculptor".to_owned()))
        );
        assert!(Written::parse("press harder").is_err());
        assert!(Written::parse("player three").is_err());
        assert!(Written::parse("").is_err());
    }

    /// A recording keeps each click with the session's reply to it, and after a click that
    /// answered, the views up to the human's next decision: what a replay answers the UI's
    /// own clicks from.
    #[test]
    fn a_recording_holds_its_clicks_and_what_they_led_to() {
        let def: super::super::Definition = serde_json::from_value(serde_json::json!({
            "name": "t",
            "board": "shape-pick",
            "until": "first priority decision for seat 0",
            "clicks": ["Counterspell", "Island", "press decline"],
        }))
        .unwrap();
        let messages = super::super::record(&def).unwrap();
        let replies: Vec<(ClickRequest, ClickReply)> = (messages.iter())
            .filter_map(|message| match message {
                FixtureMessage::Response {
                    command,
                    request: Some(request),
                    value,
                } if command == ipc::CLICK => Some((
                    serde_json::from_value(request.clone()).unwrap(),
                    serde_json::from_value(value.clone()).unwrap(),
                )),
                _ => None,
            })
            .collect();
        let [(nothing, same), (land, played), (pass, passed)] = &replies[..] else {
            panic!("three clicks, not {replies:?}");
        };
        // A card with nothing to do leaves the prompt as it was; a land is played by the
        // click; and the button passes, each on the decision then open.
        assert!(matches!(same, ClickReply::Open { prompt } if prompt.revision == nothing.revision));
        assert!(matches!(nothing.click, Click::On { .. }));
        assert_eq!(*played, ClickReply::Answered);
        assert_eq!(nothing.decision, land.decision);
        assert_eq!(*passed, ClickReply::Answered);
        assert_ne!(pass.decision, land.decision);
        assert_eq!(
            pass.click,
            Click::Button {
                button: ButtonId::Decline
            }
        );
        let Some(FixtureMessage::Event { payload, .. }) = messages.last() else {
            panic!("a recording ends on a view");
        };
        let update: GameUpdate = serde_json::from_value(payload.clone()).unwrap();
        assert!(matches!(update.view.status, Status::Deciding { .. }));
    }

    /// A seat its board's scripts still play is not the recording's to click for.
    #[test]
    fn clicks_wait_for_the_boards_scripts_to_end() {
        let def: super::super::Definition = serde_json::from_value(serde_json::json!({
            "name": "t",
            "board": "shape-pick-theirs",
            "until": "first priority decision for seat 0",
            "clicks": ["press decline"],
        }))
        .unwrap();
        let refused = super::super::record(&def).unwrap_err();
        assert!(refused.contains("scripts must end"), "{refused}");
    }
}
