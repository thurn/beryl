use std::fs::File;
use std::io::BufReader;

use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::{
    Answer, CardHandling, PileName, Side, TableAction, TablePrompt, ZoneName,
};
use beryl_protocol::script::{Act, Span, Subject};
use beryl_table::action::{Flow, SeenObject};
use beryl_table::state::{Assist, Zone};
use beryl_table::{Action, Refusal, save};
mod prompt_common;

use prompt_common::{GUEST, Linked};

impl Linked {
    fn enable_guest(&self) -> GameUpdate {
        self.guest
            .act(Action::SetAssist {
                assist: Assist::LibraryFlows,
                on: true,
            })
            .unwrap();
        self.updates().1
    }
}

fn request(flow: Flow, count: u16) -> TableAction {
    Action::Flow {
        seat: GUEST,
        flow,
        count,
        source: None,
    }
}

fn assert_lift(update: &GameUpdate, span: Span) {
    let handling = CardHandling::Pile {
        side: Side::Opponent,
        pile: ZoneName::Library,
        count: 2,
        bottom: false,
    };
    assert_eq!(
        update.view.handling,
        (span == Span::Begin).then_some(handling)
    );
    assert!(
        update
            .view
            .script
            .as_ref()
            .unwrap()
            .beats
            .iter()
            .flatten()
            .any(|step| { step.act == (Act::Lift { span, handling }) })
    );
}

fn verify_result(flow: Flow, pile: PileName, line: &str) {
    const PRIVATE: &str = "Ancestral Recall";
    let dir = tempfile::tempdir().unwrap();
    let game = Linked::new(dir.path(), PRIVATE, 9);
    game.enable_guest();
    // A short library still offers the two cards it has.
    game.guest.act(request(flow, 5)).unwrap();
    let (observer, player) = game.updates();
    let original = observer.view.players.opponent.library.clone();
    assert!(observer.view.table.as_ref().unwrap().prompt.is_none());
    assert_lift(&observer, Span::Begin);
    assert!(!serde_json::to_string(&observer).unwrap().contains(PRIVATE));
    let manual = player.view.table.unwrap();
    let Some(TablePrompt::Sort { cards, .. }) = manual.prompt else {
        panic!("expected a sort");
    };
    assert_eq!(cards.len(), 2);
    let card = if pile == PileName::Top {
        cards[1]
    } else {
        cards[0]
    };
    game.guest
        .answer(Answer::Place {
            card,
            pile,
            index: 0,
        })
        .unwrap();
    assert_eq!(
        game.guest_views.recv().unwrap().view.table.unwrap().log,
        manual.log
    );
    game.guest.answer(Answer::Done).unwrap();
    let (observer, player) = game.updates();
    let actual = player.view.table.unwrap();
    assert!(actual.prompt.is_none());
    assert_eq!(actual.log.len(), manual.log.len() + 1);
    let result = actual.log.last().unwrap();
    let line = format!("{line}.");
    assert_eq!(result.text, line);
    assert!(result.undoable);
    assert_eq!(
        observer
            .view
            .table
            .as_ref()
            .unwrap()
            .log
            .last()
            .unwrap()
            .text,
        line
    );
    assert_lift(&observer, Span::End);
    let expected_library = if flow == Flow::Surveil {
        vec![original[0]]
    } else {
        vec![original[1], original[0]]
    };
    assert_eq!(observer.view.players.opponent.library, expected_library);
    let public = observer
        .view
        .objects
        .iter()
        .filter(|card| card.name.as_deref() == Some(PRIVATE))
        .collect::<Vec<_>>();
    if flow == Flow::Surveil {
        assert_eq!(public.len(), 1);
        assert_eq!(public[0].zone, ZoneName::Graveyard);
    } else {
        assert!(!serde_json::to_string(&observer).unwrap().contains(PRIVATE));
    }

    let id = &game.host.list_saved().unwrap()[0].id;
    let saved = dir.path().join("tables").join(format!("{id}.jsonl"));
    let (replayed, log) = save::open(BufReader::new(File::open(saved).unwrap())).unwrap();
    assert_eq!(log.entries().last().unwrap().n, result.n);
    assert_eq!(
        replayed.zone(Zone::Graveyard(GUEST)).unwrap().len(),
        usize::from(flow == Flow::Surveil),
    );
    game.host
        .act(Action::Undo {
            entry: Some(result.n),
        })
        .unwrap();
    let (undo, _) = game.updates();
    assert_eq!(undo.view.players.opponent.library, original);
    assert!(undo.view.players.opponent.graveyard.is_empty());
}

#[test]
fn scry_moves_the_second_pile_to_the_bottom() {
    verify_result(
        Flow::Scry,
        PileName::Bottom,
        "Scried 2: 1 on top, 1 on the bottom",
    );
}

#[test]
fn surveil_reveals_the_graveyard_card() {
    verify_result(
        Flow::Surveil,
        PileName::Graveyard,
        "Surveilled 2: 1 on top, 1 in the graveyard",
    );
}

#[test]
fn look_reorders_the_top() {
    verify_result(Flow::LookAtTop, PileName::Top, "Looked at 2: 2 on top");
}

#[test]
fn guest_flow_requires_its_assist_and_validates_the_printed_source() {
    let dir = tempfile::tempdir().unwrap();
    let game = Linked::new(dir.path(), "Preordain", 9);
    assert_eq!(
        game.guest.act(request(Flow::Scry, 2)),
        Err(Refusal::Permission)
    );
    let view = game.enable_guest().view.table.unwrap();
    assert!(
        game.host
            .current()
            .unwrap()
            .view
            .table
            .unwrap()
            .card_commands
            .values()
            .all(|card| card.flows.is_empty())
    );
    let slot = view.orders.you.hand[0];
    let reference = view
        .object_actions
        .iter()
        .find(|action| action.subject == Subject::Object { slot })
        .unwrap();
    let open = |count| Action::Flow {
        flow: Flow::Scry,
        seat: GUEST,
        count,
        source: Some(SeenObject {
            id: reference.reference.clone(),
            zone: reference.zone,
        }),
    };
    assert_eq!(view.card_commands[&slot].flows[0].count, 2);
    assert_eq!(game.guest.act(open(1)), Err(Refusal::INVALID));
    assert_eq!(
        game.guest.act(Action::Batch {
            actions: vec![open(2)]
        }),
        Err(Refusal::Permission),
    );
    assert_eq!(
        game.host.act(request(Flow::Scry, 2)),
        Err(Refusal::Permission)
    );
    game.guest.act(open(2)).unwrap();
    assert!(game.updates().1.view.table.unwrap().prompt.is_some());
    game.guest.answer(Answer::Cancel).unwrap();
    let (_, closed) = game.updates();
    assert!(closed.view.table.unwrap().prompt.is_none());
    assert_eq!(game.guest.act(open(2)), Err(Refusal::Stale));
}
