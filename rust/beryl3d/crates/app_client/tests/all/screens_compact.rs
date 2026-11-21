//! The portrait screens of the phone layout (docs/beryl-3d.md, "Sizes"), driven through the
//! `Page` test driver at the three phone windows with a notch's safe area: the start screen's
//! opponents and seed, the opening hand in rows of four, the result's way back to the start
//! screen and the bug report's sheet. A window shortened by a few hundred points stands for one
//! with the system keyboard up.

use app_client::Input;
use app_client::command::Request;
use app_client::driver::{Page, Recording};
use beryl_card_records::{TokenMakers, fixture_message};
use beryl_protocol::GameUpdate;
use beryl_protocol::ipc::{FixtureMessage, LIST_AGENTS, START_GAME};
use beryl_protocol::prompt::{PartName, PromptView, Purpose, ShapeBody, Words};
use beryl_protocol::protocol::{
    DecisionId, DecisionView, Kind, LossReason, OutcomeView, SessionId, Side, Status, ZoneName,
};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use beryl_protocol::{AgentView, StartRequest};
use kit_frame::frame::StateBits;
use kit_probe::agent::Rule;
use kit_ui::input::Input as DeviceInput;
use kit_ui::input::{Insets, Rect, Size};

/// The three phone windows the interface tests play.
const PHONES: [Size; 3] = [
    Size {
        width: 393.0,
        height: 759.0,
    },
    Size {
        width: 375.0,
        height: 647.0,
    },
    Size {
        width: 430.0,
        height: 932.0,
    },
];

/// A status bar at the top and a home indicator at the bottom, as on a phone with a notch.
const SAFE: Insets = Insets {
    top: 44.0,
    right: 0.0,
    bottom: 34.0,
    left: 0.0,
};

/// How much of the window the system keyboard takes, about an iPhone's.
const KEYBOARD: f32 = 336.0;

/// The safe area with the keyboard up, which covers the home indicator.
const KEYBOARD_SAFE: Insets = Insets {
    bottom: 0.0,
    ..SAFE
};

/// `size` with the keyboard up: as tall as what the keyboard leaves.
fn with_keyboard(size: Size) -> Size {
    Size {
        width: size.width,
        height: size.height - KEYBOARD,
    }
}

/// `messages` played in the phone layout at `size`, settled.
fn page(messages: Vec<FixtureMessage>, size: Size) -> Page {
    page_in(messages, size, SAFE)
}

/// `messages` played in the phone layout at `size` with the safe area `safe`, settled.
fn page_in(messages: Vec<FixtureMessage>, size: Size, safe: Insets) -> Page {
    let mut page = Page::new(
        crate::test_data::data(),
        Recording(messages).into(),
        Side::You,
        crate::COMPACT,
        size,
    )
    .expect("the recording plays")
    .with_safe_area(safe);
    page.still();
    page
}

/// The faults of the rules that hold a screen's own elements: inside the safe area, apart,
/// with text that fits, and large enough to touch.
fn layout_faults(page: &mut Page) -> Vec<String> {
    let rules = [
        Rule::OnScreen,
        Rule::Overlap,
        Rule::TextFits,
        Rule::MinimumSizes,
    ];
    (page.faults().into_iter())
        .filter(|fault| rules.contains(&fault.rule))
        .map(|fault| fault.message)
        .collect()
}

/// Whether `rect` lies wholly inside the safe area of a window of `size`.
fn inside(rect: Rect, size: Size) -> bool {
    inside_of(rect, size, SAFE)
}

/// Whether `rect` lies wholly inside the safe area `safe` of a window of `size`.
fn inside_of(rect: Rect, size: Size, safe: Insets) -> bool {
    rect.x >= safe.left
        && rect.y >= safe.top
        && rect.x + rect.width <= size.width - safe.right
        && rect.y + rect.height <= size.height - safe.bottom
}

/// Three agents, none of them the one offered first, so that the first is chosen at the start.
fn agents() -> Vec<AgentView> {
    ["random", "greedy", "expert"]
        .map(|name| AgentView {
            name: name.to_owned(),
            title: name.to_uppercase(),
            description: format!("The {name} agent, which plays as its name says it does"),
        })
        .to_vec()
}

/// The start screen at `size`, whose start is answered for `agent` with `seed`.
fn start(size: Size, agent: &str, seed: Option<u32>) -> Page {
    start_in(size, SAFE, agent, seed)
}

/// The start screen at `size` with the safe area `safe`, whose start is answered for `agent`
/// with `seed`.
fn start_in(size: Size, safe: Insets, agent: &str, seed: Option<u32>) -> Page {
    let request = StartRequest {
        agent: agent.to_owned(),
        seed,
        fair: false,
    };
    page_in(
        vec![
            FixtureMessage::response(LIST_AGENTS, &agents()),
            FixtureMessage::asked(START_GAME, &request, &()),
        ],
        size,
        safe,
    )
}

#[test]
fn the_start_screen_fits_each_compact_and_starts_with_the_chosen_agent_and_seed() {
    for size in PHONES {
        let mut page = start(size, "greedy", Some(1234));
        assert_eq!(layout_faults(&mut page), Vec::<String>::new(), "{size:?}");
        for locator in ["toggle \"GREEDY\"", "field \"seed\"", "button \"Start\""] {
            let rect = page.find(locator).rect();
            assert!(inside(rect, size), "{locator} {rect:?} in {size:?}");
        }
        page.press("toggle \"GREEDY\"");
        page.still();
        assert!(
            page.find("toggle \"GREEDY\"")
                .states()
                .has(StateBits::SELECTED)
        );
        page.type_text("field \"seed\"", "12x");
        page.still();
        assert!(
            page.find("button \"Start\"")
                .states()
                .has(StateBits::DISABLED),
            "a seed that is not a number disables Start"
        );
        page.type_text("field \"seed\"", "1234");
        page.still();
        assert_eq!(page.field("seed").text, "1234");
        // A start with any other agent or seed would not match the recorded request.
        page.press("button \"Start\"");
        page.still();
        assert!(
            page.try_find("button \"Start\"").is_err(),
            "the game screen replaced the start screen at {size:?}"
        );
    }
}

#[test]
fn with_the_keyboard_up_the_seed_stays_in_view_and_the_rest_scrolls_into_reach() {
    use kit_ui::input::{Button, Modifiers, Pointer, PointerKind, PointerPhase};
    for size in PHONES.map(with_keyboard) {
        let mut page = start_in(size, KEYBOARD_SAFE, "random", None);
        page.press("field \"seed\"");
        page.still();
        let seed = page.find("field \"seed\"").rect();
        assert!(inside_of(seed, size, KEYBOARD_SAFE), "{seed:?} in {size:?}");
        assert!(
            page.find("list \"Opponent\"").rect().height > 0.0,
            "the opponents keep a row at {size:?}"
        );
        // The column scrolls to its foot, which brings every action under the seed into view.
        page.give_input(Input::Device(DeviceInput::Pointer(Pointer {
            kind: PointerKind::Mouse,
            phase: PointerPhase::Wheel,
            position: glam::Vec2::new(size.width / 2.0, seed.y),
            button: Button::None,
            wheel: glam::Vec2::new(0.0, -2000.0),
            id: 0,
            modifiers: Modifiers::default(),
        })));
        page.still();
        for locator in [
            "button \"Start\"",
            "button \"Solo table\"",
            "button \"Export Bug Reports\"",
        ] {
            let rect = page.find(locator).rect();
            assert!(
                inside_of(rect, size, KEYBOARD_SAFE),
                "{locator} {rect:?} in {size:?}"
            );
        }
    }
}

/// A mulligan's prompt, as the session's decision on the opening hand.
fn keep_or_mulligan() -> PromptView {
    PromptView {
        revision: 1,
        purpose: Purpose::Choose,
        part: PartName::Pick,
        title: "Keep or mulligan?".to_owned(),
        words: Words::default(),
        count: None,
        about: Vec::new(),
        from: None,
        marks: Vec::new(),
        previews: Vec::new(),
        buttons: Vec::new(),
        body: ShapeBody::YesNo {
            about: Vec::new(),
            places: None,
        },
    }
}

/// A game before its first turn, with seven cards in your hand, you on the play and the
/// decision on your hand open.
fn opening(size: Size) -> Page {
    let mut builder = ViewBuilder::new();
    for name in [
        "Plains",
        "Island",
        "Counterspell",
        "Brainstorm",
        "Swords to Plowshares",
        "Island",
        "Plains",
    ] {
        builder.card(ZoneName::Hand, Side::You, name);
    }
    builder.script(ScriptBuilder::reset().build());
    let mut update: GameUpdate = builder.update();
    update.view.turn.number = 0;
    update.view.turn.starting = Some(Side::You);
    update.view.status = Status::Deciding {
        decision: DecisionView {
            id: DecisionId {
                session: SessionId(1),
                seq: 1,
            },
            kind: Kind::KeepOrMulligan,
            candidates: Vec::new(),
            resolving: None,
            prompt: Box::new(keep_or_mulligan()),
            pass_to: None,
            pass_on: None,
            source: None,
            halt: false,
            cancellable: false,
            undo: 0,
            undo_depth: 0,
            refused: Vec::new(),
            acting_for: None,
        },
    };
    let mut messages = vec![FixtureMessage::game_update(&update)];
    messages.insert(0, fixture_message(&messages, TokenMakers::NONE));
    page(messages, size)
}

#[test]
fn the_opening_hand_stands_in_rows_of_four_above_the_dock() {
    for size in PHONES {
        let mut page = opening(size);
        page.find("text \"Opening hand\"");
        page.find("text \"On the play\"");
        let tree = page.tree();
        let boxes: Vec<Rect> = (tree.lines())
            .filter(|line| line.trim_start().starts_with("region \"") && line.contains(" #"))
            .filter_map(|line| {
                let name = line.trim_start().split('"').nth(1)?;
                let card = ["Plains", "Island", "Counterspell", "Brainstorm", "Swords"]
                    .iter()
                    .any(|card| name.starts_with(card));
                card.then(|| page_rect(line))
            })
            .collect();
        assert_eq!(
            boxes.len(),
            7,
            "every card of the hand at {size:?}:\n{tree}"
        );
        let mut tops: Vec<f32> = boxes.iter().map(|rect| rect.y).collect();
        tops.dedup_by(|a, b| (*a - *b).abs() < 1.0);
        assert_eq!(
            tops.len(),
            2,
            "two rows, four and three, at {size:?}: {boxes:?}"
        );
        // The dock's buttons, a gutter and the safe area stand below the hand.
        let dock = size.height - SAFE.bottom - 16.0 - 48.0;
        for rect in &boxes {
            assert!(inside(*rect, size), "{rect:?} in {size:?}");
            assert!(
                rect.y + rect.height <= dock,
                "{rect:?} above {dock} at {size:?}"
            );
        }
    }
}

/// The rectangle a tree line prints after its name and id ("... #12 16,44 361x48 ..."): the
/// first word that is two numbers split by a comma, and the word after it.
fn page_rect(line: &str) -> Rect {
    let words: Vec<&str> = line.split_whitespace().collect();
    let numbers = |word: &str, by: char| {
        let (a, b) = word.split_once(by)?;
        Some((a.parse::<f32>().ok()?, b.parse::<f32>().ok()?))
    };
    let at = (words.iter())
        .position(|word| numbers(word, ',').is_some())
        .expect("a place");
    let (x, y) = numbers(words[at], ',').expect("x,y");
    let (width, height) = numbers(words[at + 1], 'x').expect("wxh");
    Rect {
        x,
        y,
        width,
        height,
    }
}

#[test]
fn the_result_returns_to_the_start_screen_from_its_dock() {
    for size in PHONES {
        let mut builder = ViewBuilder::new();
        builder.card(ZoneName::Battlefield, Side::You, "Island");
        builder.script(ScriptBuilder::reset().build());
        let mut update = builder.update();
        update.view.status = Status::Over {
            outcome: OutcomeView::Win {
                winner: Side::You,
                reason: LossReason::Concede,
                by: None,
            },
        };
        let mut page = page(vec![FixtureMessage::game_update(&update)], size);
        page.find("text \"Victory\"");
        page.find("text \"Conceded\"");
        let button = page.find("button \"New Game\"").rect();
        assert!(inside(button, size), "{button:?} in {size:?}");
        assert!(
            button.width >= size.width - 2.0 * 16.0 - 1.0 || button.width >= 398.0 - 1.0,
            "the button spans the dock: {button:?} at {size:?}"
        );
        page.press("button \"New Game\"");
        page.still();
        assert!(page.try_find("region \"start\"").is_ok(), "{}", page.tree());
    }
}

/// The sample table at `size`, with the bug report open in its describe stage over the whole
/// window.
fn describing(size: Size) -> Page {
    let mut view = ViewBuilder::sample();
    view.script(ScriptBuilder::reset().build());
    let mut page = page(vec![FixtureMessage::game_update(&view.update())], size);
    page.chord("Cmd+B");
    page.still();
    let whole = page.find("button \"Whole window\"").rect();
    assert!(
        inside(whole, size),
        "the tools stand in the safe area: {whole:?}"
    );
    assert!(whole.width >= 44.0 && whole.height >= 44.0, "{whole:?}");
    page.press("button \"Whole window\"");
    page.still();
    page
}

/// Every `SaveBug` request the client has made since the last call.
fn saves(page: &mut Page) -> Vec<Request> {
    std::iter::from_fn(|| page.next_request())
        .filter(|request| matches!(request, Request::SaveBug { .. }))
        .collect()
}

#[test]
fn the_bug_report_is_a_sheet_at_the_foot_that_keeps_its_draft_when_the_keyboard_opens() {
    for size in PHONES {
        let mut page = describing(size);
        let panel = page.find("region \"Report bug\"").rect();
        assert!(
            (panel.width - size.width).abs() < 1.0
                && (panel.y + panel.height - size.height).abs() < 1.0,
            "a sheet as wide as the window at its foot: {panel:?} at {size:?}"
        );
        // The game's own controls around the report are the game screen's to place.
        let report = ["Whole window", "Close", "Submit", "AI play", "description"];
        let faults: Vec<String> = (layout_faults(&mut page).into_iter())
            .filter(|fault| {
                report
                    .iter()
                    .any(|name| fault.contains(&format!("\"{name}\"")))
            })
            .collect();
        assert_eq!(faults, Vec::<String>::new(), "{size:?}");
        page.type_text("field \"description\"", "the angel never untapped");
        page.still();

        // The keyboard opens: the window is shorter, and the sheet stands on the keyboard.
        let short = with_keyboard(size);
        page.give_input(Input::Device(DeviceInput::Surface {
            size: short,
            scale: 1.0,
            safe_area: SAFE,
        }));
        page.still();
        assert_eq!(page.field("description").text, "the angel never untapped");
        let submit = page.find("button \"Submit\"").rect();
        assert!(inside(submit, short), "{submit:?} in {short:?}");
        page.press("button \"Submit\"");
        assert_eq!(
            saves(&mut page).len(),
            1,
            "Submit sends the report at {size:?}"
        );
    }
}

#[test]
fn closing_the_bug_report_sends_nothing() {
    let size = PHONES[1];
    let mut page = describing(size);
    page.type_text("field \"description\"", "not a bug after all");
    page.still();
    page.press("button \"Close\"");
    page.still();
    assert!(saves(&mut page).is_empty());
    assert!(page.try_find("region \"bug report\"").is_err());
}
