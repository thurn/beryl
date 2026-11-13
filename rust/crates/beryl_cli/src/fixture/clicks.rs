//! The clicks a fixture's recording makes on its human's prompts once it has reached its stop
//!: each goes to the session as the UI's would, and the recording keeps the
//! request with its reply, so a replay answers the UI's click only if it is that one. A click
//! that answers its prompt plays the game on to the human's next decision, whose views the
//! recording takes as it takes any others.

use std::sync::mpsc::Receiver;

use beryl_session::ipc::{self, FixtureMessage};
use beryl_session::prompt::{ButtonId, Click, ClickReply, ClickRequest, PromptView};
use beryl_session::protocol::{PlayerView, Side};
use beryl_session::script::Subject;
use beryl_session::{DecisionId, GameUpdate, Status};

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
    /// A card, by its name, dropped into the pile `pile` before what lies at `at` there.
    Drop { card: String, pile: u8, at: u16 },
    /// The item `item` of an order moved to before what lies at `at`.
    Move { item: u32, at: u16 },
    /// The number `n`: a number prompt's value, or the share of the card named, a recipient
    /// of a division.
    Number { n: u8, card: Option<String> },
    /// The entry `index` of a catalog.
    Entry(u16),
}

impl Written {
    /// Parses "press confirm", "press decline", "press auto", "press passOn" or "press
    /// option <n>" for a button, "player you" or "player opponent" for a player, "drop
    /// <pile> <at> <card>" for a card dropped into a pile, "move <item> <at>" for an item of
    /// an order moved, "number <n>" for a number prompt's value or "number <n> <card>" for
    /// that card's share of a division, "entry <index>" for an entry of a catalog, and
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
            ["drop", pile, at, card @ ..] if !card.is_empty() => Ok(Written::Drop {
                card: card.join(" "),
                pile: super::number(text, pile)?,
                at: super::number(text, at)?,
            }),
            ["drop", ..] => Err(format!("{text:?}: a drop is of a card, into a pile")),
            ["move", item, at] => Ok(Written::Move {
                item: super::number(text, item)?,
                at: super::number(text, at)?,
            }),
            ["move", ..] => Err(format!("{text:?}: a move is of an item, to a place")),
            ["number", n, card @ ..] => Ok(Written::Number {
                n: super::number(text, n)?,
                card: (!card.is_empty()).then(|| card.join(" ")),
            }),
            ["number"] => Err(format!("{text:?}: which number")),
            ["entry", index] => Ok(Written::Entry(super::number(text, index)?)),
            ["entry", ..] => Err(format!("{text:?}: an entry is named by its place")),
            [] => Err("an empty click".to_owned()),
            _ => Ok(Written::Card(text.to_owned())),
        }
    }

    /// The click this is on `prompt`, open in `view`.
    fn click(&self, view: &PlayerView, prompt: &PromptView) -> Result<Click, String> {
        // The card named `name`: the one the open prompt marks, or the first the view shows.
        let card = |name: &String| {
            let named = || {
                (view.objects.iter())
                    .filter(|o| o.name.as_deref() == Some(name))
                    .map(|o| o.slot)
            };
            let marked =
                |slot: &u16| (prompt.marks.iter()).any(|m| m.on == Subject::Object { slot: *slot });
            named()
                .find(marked)
                .or_else(|| named().next())
                .map(|slot| Subject::Object { slot })
                .ok_or_else(|| format!("the view shows no {name}"))
        };
        Ok(match self {
            Written::Press(button) => Click::Button { button: *button },
            Written::Player(player) => Click::On {
                on: Subject::Player { player: *player },
            },
            Written::Card(name) => Click::On { on: card(name)? },
            Written::Drop {
                card: name,
                pile,
                at,
            } => Click::Drop {
                on: card(name)?,
                pile: *pile,
                at: *at,
            },
            Written::Move { item, at } => Click::Drop {
                on: Subject::Ability { id: *item },
                pile: 0,
                at: *at,
            },
            Written::Number { n, card: name } => Click::Number {
                n: *n,
                on: name.as_ref().map(card).transpose()?,
            },
            Written::Entry(index) => Click::Entry { index: *index },
        })
    }
}

/// Makes `clicks` in order from the human's decision open in `stop`, the view the recording
/// stopped at, and adds what the session said to `messages`: each click's reply, and after
/// one that answered, the views up to the human's next decision. `send` gives the session's
/// reply to a click, and `next` its next view, or why there is none.
pub(super) fn play(
    clicks: &[Written],
    send: &mut dyn FnMut(ClickRequest) -> Result<ClickReply, String>,
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
        let reply = send(request).map_err(|e| format!("click {i}, {written:?}: {e}"))?;
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
    use beryl_session::prompt::ShapeBody;

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
            Written::parse("number 2"),
            Ok(Written::Number { n: 2, card: None })
        );
        assert_eq!(
            Written::parse("number 3 Trinket Mage"),
            Ok(Written::Number {
                n: 3,
                card: Some("Trinket Mage".to_owned())
            })
        );
        assert_eq!(Written::parse("entry 4"), Ok(Written::Entry(4)));
        assert!(Written::parse("entry").is_err());
        assert!(Written::parse("number").is_err());
        assert_eq!(
            Written::parse("Jace, the Mind Sculptor"),
            Ok(Written::Card("Jace, the Mind Sculptor".to_owned()))
        );
        assert_eq!(
            Written::parse("drop 1 0 Jace, the Mind Sculptor"),
            Ok(Written::Drop {
                card: "Jace, the Mind Sculptor".to_owned(),
                pile: 1,
                at: 0
            })
        );
        assert_eq!(
            Written::parse("move 2 0"),
            Ok(Written::Move { item: 2, at: 0 })
        );
        assert!(Written::parse("drop 1 0").is_err());
        assert!(Written::parse("move 2").is_err());
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

    /// A board's own play may lead on from its scripts to the stop, the stand-in answering
    /// what the human is asked on the way (a cost to pay, a pass): the clicks begin at the
    /// stop, and a drop there is the session's to place.
    #[test]
    fn clicks_begin_at_the_stop_a_boards_play_leads_to() {
        let def: super::super::Definition = serde_json::from_value(serde_json::json!({
            "name": "t",
            "board": "shape-split",
            "until": "first chooseUpTo decision for seat 0",
            "clicks": ["drop 0 0 Island", "press confirm"],
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
        let [(drop, sorted), (_, done)] = &replies[..] else {
            panic!("two clicks, not {replies:?}");
        };
        let Click::Drop { on, pile: 0, at: 0 } = drop.click else {
            panic!("a drop into the first pile, not {:?}", drop.click);
        };
        let ClickReply::Open { prompt } = sorted else {
            panic!("the scry stays open, not {sorted:?}");
        };
        let ShapeBody::Split { piles } = &prompt.body else {
            panic!("a split, not {:?}", prompt.body);
        };
        let Subject::Object { slot } = on else {
            panic!("a card, not {on:?}");
        };
        assert_eq!(piles[0].cards, [slot.into()]);
        assert_eq!(*done, ClickReply::Answered);
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
