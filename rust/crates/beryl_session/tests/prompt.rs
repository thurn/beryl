//! A prompt's state: its text form, one worked state per shape, and the click command's wire
//! form. The expected texts are written by hand from what the screen shows.

use beryl_engine::card::{Purpose, Shape};
use beryl_session::prompt::{
    ButtonId, Click, ClickReply, ClickRequest, Marked, ObjectId, Pair, PartName, Pile, PlaceName,
    PromptButton, PromptView, ShapeBody, Share, Words,
};
use beryl_session::protocol::Side;
use beryl_session::script::{Arrow, Mark, Subject};
use beryl_session::{DecisionId, SessionId};

/// The objects in `slots`, as the wire names them.
fn ids(slots: &[u16]) -> Vec<ObjectId> {
    slots.iter().copied().map(ObjectId).collect()
}

const fn card(slot: u16) -> Subject {
    Subject::Object {
        slot: ObjectId(slot),
    }
}

fn marked(mark: Mark, on: &[Subject]) -> impl Iterator<Item = Marked> + '_ {
    on.iter().map(move |&on| Marked { on, mark })
}

fn button(id: ButtonId, label: &str) -> PromptButton {
    PromptButton {
        id,
        label: label.to_owned(),
        caption: String::new(),
        enabled: true,
        primary: false,
    }
}

fn primary(id: ButtonId, label: &str) -> PromptButton {
    PromptButton {
        primary: true,
        ..button(id, label)
    }
}

fn disabled(id: ButtonId, label: &str) -> PromptButton {
    PromptButton {
        enabled: false,
        ..button(id, label)
    }
}

const fn option(index: u16) -> ButtonId {
    ButtonId::Option { index }
}

fn prompt(
    purpose: Purpose,
    title: &str,
    marks: Vec<Marked>,
    buttons: Vec<PromptButton>,
    shape: ShapeBody,
) -> PromptView {
    assert_eq!(
        shape.shape(),
        Some(purpose.shape()),
        "{purpose:?} is not that shape"
    );
    PromptView {
        revision: 0,
        purpose,
        part: PartName::Pick,
        title: title.to_owned(),
        words: Words::default(),
        count: None,
        about: Vec::new(),
        from: None,
        marks,
        buttons,
        body: shape,
    }
}

/// A state of a prompt and the text it prints as.
type Worked = (PromptView, &'static str);

/// A search for up to two cards, three found, one picked.
fn search() -> Worked {
    (
        prompt(
            Purpose::Search,
            "Search",
            marked(Mark::Eligible, &[card(40), card(31)])
                .chain(marked(Mark::Selected, &[card(32)]))
                .collect(),
            vec![
                primary(ButtonId::Confirm, "Find 1/2"),
                button(ButtonId::Decline, "Find Nothing"),
            ],
            ShapeBody::Pick {
                tray: ids(&[31, 32, 40]),
                least: 0,
                most: 2,
                picked: vec![card(32)],
                warned: Vec::new(),
            },
        ),
        "pick search: Search\n\
         tray #31 #32 #40\n\
         picked 1 of 0..2: #32\n\
         eligible #31 #40\n\
         selected #32\n\
         confirm[Find 1/2]* decline[Find Nothing]\n",
    )
}

/// Two triggers to order, the second moved ahead of the first.
fn trigger_order() -> Worked {
    (
        prompt(
            Purpose::TriggerOrder,
            "Order Triggers",
            Vec::new(),
            vec![primary(ButtonId::Confirm, "Done")],
            ShapeBody::Order {
                items: vec![Subject::Ability { id: 2 }, Subject::Ability { id: 1 }],
            },
        ),
        "order trigger-order: Order Triggers\n\
         order a2 a1\n\
         confirm[Done]*\n",
    )
}

/// A scry of two as it opens: both on top, as they lie.
fn scry() -> Worked {
    (
        prompt(
            Purpose::Scry,
            "Scry",
            marked(Mark::Eligible, &[card(8), card(9)]).collect(),
            vec![primary(ButtonId::Confirm, "Done")],
            ShapeBody::Split {
                piles: vec![
                    Pile {
                        place: Some(PlaceName::Bottom),
                        label: "Bottom".to_owned(),
                        cards: Vec::new(),
                        exactly: None,
                        closed: false,
                    },
                    Pile {
                        place: Some(PlaceName::Top),
                        label: "Top".to_owned(),
                        cards: ids(&[9, 8]),
                        exactly: None,
                        closed: false,
                    },
                ],
            },
        ),
        "split scry: Scry\n\
         pile bottom \"Bottom\":\n\
         pile top \"Top\": #9 #8\n\
         eligible #8 #9\n\
         confirm[Done]*\n",
    )
}

/// The top five dealt out: two for the hand so far short of its two, so no confirm.
fn arrange() -> Worked {
    (
        prompt(
            Purpose::Arrange,
            "Top Five",
            Vec::new(),
            vec![disabled(ButtonId::Confirm, "Done")],
            ShapeBody::Split {
                piles: vec![
                    Pile {
                        place: Some(PlaceName::Hand),
                        label: "Hand".to_owned(),
                        cards: ids(&[5]),
                        exactly: Some(2),
                        closed: false,
                    },
                    Pile {
                        place: Some(PlaceName::Bottom),
                        label: "Bottom".to_owned(),
                        cards: ids(&[7, 6, 4, 3]),
                        exactly: None,
                        closed: false,
                    },
                ],
            },
        ),
        "split arrange: Top Five\n\
         pile hand \"Hand\" exactly 2: #5\n\
         pile bottom \"Bottom\": #7 #6 #4 #3\n\
         confirm(Done)\n",
    )
}

/// A spell going to the top or the bottom of its owner's library: neither is put forward.
fn top_or_bottom() -> Worked {
    (
        prompt(
            Purpose::TopOrBottom,
            "Top or Bottom?",
            Vec::new(),
            vec![button(option(0), "Top"), button(option(1), "Bottom")],
            ShapeBody::YesNo {
                about: ids(&[12]),
                places: Some([PlaceName::Top, PlaceName::Bottom]),
            },
        ),
        "yesNo top-or-bottom: Top or Bottom?\n\
         about #12\n\
         0[Top] 1[Bottom]\n",
    )
}

/// X at two of up to three.
fn x() -> Worked {
    (
        prompt(
            Purpose::X,
            "Choose X",
            Vec::new(),
            vec![primary(ButtonId::Confirm, "Done")],
            ShapeBody::Number {
                values: vec![0, 1, 2, 3],
                value: 2,
                detail: Some("{2}{U}".to_owned()),
            },
        ),
        "number x: Choose X\n\
         number 2 of 0 1 2 3 ({2}{U})\n\
         confirm[Done]*\n",
    )
}

/// A prompt of the shape's plainest kind.
fn play_or_draw() -> Worked {
    (
        prompt(
            Purpose::PlayOrDraw,
            "Play or Draw?",
            Vec::new(),
            vec![primary(option(0), "Play"), button(option(1), "Draw")],
            ShapeBody::Mode { about: Vec::new() },
        ),
        "mode play-or-draw: Play or Draw?\n\
         0[Play]* 1[Draw]\n",
    )
}

/// No card asks for a color yet: the state is built here.
fn color() -> Worked {
    (
        prompt(
            Purpose::Color,
            "Choose a Color",
            Vec::new(),
            vec![primary(ButtonId::Confirm, "Done")],
            ShapeBody::Catalog {
                entries: ["White", "Blue", "Black", "Red", "Green"]
                    .map(str::to_owned)
                    .into(),
                chosen: Some(1),
            },
        ),
        "catalog color: Choose a Color\n\
         catalog White | Blue* | Black | Red | Green\n\
         confirm[Done]*\n",
    )
}

/// Nor to divide damage: five among a creature and a player, three given out.
fn divide_damage() -> Worked {
    (
        prompt(
            Purpose::DivideDamage,
            "Divide 5 Damage",
            marked(
                Mark::HostileTarget,
                &[
                    Subject::Player {
                        player: Side::Opponent,
                    },
                    card(20),
                ],
            )
            .collect(),
            vec![disabled(ButtonId::Confirm, "Done")],
            ShapeBody::Divide {
                total: 5,
                left: 2,
                shares: vec![
                    Share {
                        on: card(20),
                        amount: 2,
                    },
                    Share {
                        on: Subject::Player {
                            player: Side::Opponent,
                        },
                        amount: 1,
                    },
                ],
            },
        ),
        "divide divide-damage: Divide 5 Damage\n\
         divide 5, 2 left: #20=2 opponent=1\n\
         hostile-target #20 opponent\n\
         confirm(Done)\n",
    )
}

/// A crew of three with a 2/2 tapped: one power to go, and another creature can pay it.
fn crew() -> Worked {
    (
        prompt(
            Purpose::Crew,
            "Crew 3",
            marked(Mark::Paying, &[card(14)])
                .chain(marked(Mark::Eligible, &[card(15), card(16)]))
                .collect(),
            vec![disabled(ButtonId::Confirm, "Crew")],
            ShapeBody::Pay {
                cost: "3".to_owned(),
                left: "1".to_owned(),
            },
        ),
        "pay crew: Crew 3\n\
         pay 3, 1 left\n\
         eligible #15 #16\n\
         paying #14\n\
         confirm(Crew)\n",
    )
}

/// Attackers: one aimed at the player, one at a planeswalker, a third awaiting its aim.
fn attack() -> Worked {
    (
        prompt(
            Purpose::Attack,
            "Declare Attackers",
            marked(Mark::Selected, &[card(4), card(6), card(8)])
                .chain(marked(
                    Mark::Eligible,
                    &[
                        card(20),
                        Subject::Player {
                            player: Side::Opponent,
                        },
                    ],
                ))
                .collect(),
            vec![
                primary(ButtonId::Confirm, "Attack"),
                button(ButtonId::Decline, "No Attacks"),
            ],
            ShapeBody::Declare {
                arrow: Arrow::Attack,
                pairs: vec![
                    Pair {
                        from: ObjectId(4),
                        to: Subject::Player {
                            player: Side::Opponent,
                        },
                    },
                    Pair {
                        from: ObjectId(6),
                        to: card(20),
                    },
                ],
                lined: true,
                aiming: ids(&[8]),
            },
        ),
        "declare attack: Declare Attackers\n\
         declare attack: #4>opponent #6>#20\n\
         aiming #8\n\
         eligible #20 opponent\n\
         selected #4 #6 #8\n\
         confirm[Attack]* decline[No Attacks]\n",
    )
}

/// One state of a prompt of each shape.
fn worked() -> Vec<Worked> {
    vec![
        search(),
        trigger_order(),
        scry(),
        arrange(),
        top_or_bottom(),
        x(),
        play_or_draw(),
        color(),
        divide_damage(),
        crew(),
        attack(),
    ]
}

#[test]
fn a_state_of_each_shape_prints_as_its_text() {
    let worked = worked();
    for (view, text) in &worked {
        assert_eq!(view.to_string(), *text);
    }
    for &shape in Shape::ALL {
        assert!(
            worked
                .iter()
                .any(|(view, _)| view.body.shape() == Some(shape)),
            "no worked state of shape {shape:?}"
        );
    }
}

#[test]
fn a_state_survives_the_wire() {
    for (view, _) in worked() {
        let json = serde_json::to_string(&view).unwrap();
        assert_eq!(serde_json::from_str::<PromptView>(&json).unwrap(), view);
    }
}

#[test]
fn a_click_and_its_reply_on_the_wire() {
    let request = ClickRequest {
        decision: DecisionId {
            session: SessionId(1),
            seq: 4,
        },
        revision: 2,
        click: Click::Drop {
            on: card(31),
            pile: 1,
            at: 0,
        },
    };
    assert_eq!(
        serde_json::to_value(request).unwrap(),
        serde_json::json!({
            "decision": { "session": 1, "seq": 4 },
            "revision": 2,
            "click": {
                "kind": "drop",
                "on": { "kind": "object", "slot": 31 },
                "pile": 1,
                "at": 0,
            },
        })
    );
    let clicks = [
        (
            Click::On {
                on: Subject::Player {
                    player: Side::Opponent,
                },
            },
            serde_json::json!({ "kind": "on", "on": { "kind": "player", "player": "opponent" } }),
        ),
        (
            Click::Button {
                button: ButtonId::Option { index: 1 },
            },
            serde_json::json!({ "kind": "button", "button": { "kind": "option", "index": 1 } }),
        ),
        (
            Click::Button {
                button: ButtonId::PassOn,
            },
            serde_json::json!({ "kind": "button", "button": { "kind": "passOn" } }),
        ),
        (
            Click::Number { n: 3, on: None },
            serde_json::json!({ "kind": "number", "n": 3, "on": null }),
        ),
        (
            Click::Entry { index: 4 },
            serde_json::json!({ "kind": "entry", "index": 4 }),
        ),
    ];
    for (click, json) in clicks {
        assert_eq!(serde_json::to_value(click).unwrap(), json);
        assert_eq!(serde_json::from_value::<Click>(json).unwrap(), click);
    }
    assert_eq!(
        serde_json::to_value(ClickReply::Answered).unwrap(),
        serde_json::json!({ "kind": "answered" })
    );
    let (view, _) = worked().swap_remove(0);
    let reply = serde_json::to_value(ClickReply::Open {
        prompt: Box::new(view),
    })
    .unwrap();
    assert_eq!(reply["kind"], "open");
    assert_eq!(reply["prompt"]["body"]["shape"], "pick");
    assert_eq!(
        reply["prompt"]["body"]["tray"],
        serde_json::json!([31, 32, 40])
    );
    assert_eq!(reply["prompt"]["purpose"], "search");
    assert_eq!(
        reply["prompt"]["marks"][0],
        serde_json::json!({ "on": { "kind": "object", "slot": 40 }, "mark": "eligible" })
    );
}
