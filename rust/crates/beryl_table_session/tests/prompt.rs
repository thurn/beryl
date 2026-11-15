use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use beryl_session::protocol::{
    Answer, CardHandling, ObjectId as Slot, PileName, Side, TablePrompt, ZoneName,
};
use beryl_session::script::{Act, Span};
use beryl_session::{GameUpdate, SessionIds};
use beryl_table::action::{Adjustment, Flow};
use beryl_table::assist::text::{Flow as Effect, flow_result};
use beryl_table::deck::Deck;
use beryl_table::state::Zone;
use beryl_table::{Action, Log, Refusal, Table, save};
use beryl_table_session::TableSessions;
use beryl_table_session::prompt::PromptState;

mod prompt_common;

use prompt_common::{GUEST, HOST, Linked};
const PRIVATE: &str = "Ancestral Recall";

fn sort(update: &GameUpdate) -> (&[Slot], &[Vec<Slot>]) {
    let Some(TablePrompt::Sort { cards, placed, .. }) = &update.view.table.as_ref().unwrap().prompt
    else {
        panic!("expected the seat's sort prompt");
    };
    (cards, placed)
}

fn has_lift(update: &GameUpdate, span: Span) -> bool {
    update
        .view
        .script
        .as_ref()
        .unwrap()
        .beats
        .iter()
        .flatten()
        .any(|step| {
            step.act
                == Act::Lift {
                    span,
                    handling: CardHandling::Pile {
                        side: Side::Opponent,
                        pile: ZoneName::Library,
                        count: 3,
                        bottom: false,
                    },
                }
        })
}

fn open_guest_scry(game: &Linked) -> GameUpdate {
    game.host
        .open_library_prompt(GUEST, "Scry".into(), Flow::Scry, 3, |prompt| {
            let PromptState::Sort(sort) = prompt else {
                panic!("expected a sort")
            };
            flow_result(Effect::Scry(3), sort.cards(), &sort.choice())
        })
        .unwrap();
    let (observer, opening) = game.updates();
    assert!(has_lift(&observer, Span::Begin));
    assert!(!serde_json::to_string(&observer).unwrap().contains(PRIVATE));
    assert!(observer.view.table.unwrap().prompt.is_none());
    opening
}

fn replay_save(directory: &Path, id: &str) -> (Table, Log) {
    let path = directory.join("tables").join(format!("{id}.jsonl"));
    save::open(BufReader::new(File::open(path).unwrap())).unwrap()
}

#[test]
fn guest_sorts_privately_and_done_records_one_undoable_batch() {
    let dir = tempfile::tempdir().unwrap();
    let game = Linked::new(dir.path(), PRIVATE, 12);
    let opening = open_guest_scry(&game);
    let Linked {
        host,
        guest,
        host_views,
        guest_views,
    } = &game;
    let (cards, placed) = sort(&opening);
    assert_eq!(placed, &[vec![], cards.to_vec()]);
    assert!(serde_json::to_string(&opening).unwrap().contains(PRIVATE));
    assert_eq!(host.answer(Answer::Done), Err(Refusal::Stale));

    let place = |card, pile, index| Answer::Place { card, pile, index };
    for (answer, expected) in [
        (
            place(cards[0], PileName::Bottom, 0),
            vec![vec![cards[0]], cards[1..].to_vec()],
        ),
        (
            Answer::Unplace { card: cards[1] },
            vec![vec![cards[0]], vec![cards[2]]],
        ),
    ] {
        guest.answer(answer).unwrap();
        assert_eq!(sort(&guest_views.recv().unwrap()).1, expected);
        assert!(host_views.try_recv().is_err());
    }
    assert_eq!(guest.answer(Answer::Done), Err(Refusal::INVALID));
    assert_eq!(
        guest.answer(place(cards[2], PileName::Bottom, 10)),
        Err(Refusal::INVALID)
    );
    guest.answer(place(cards[1], PileName::Top, 1)).unwrap();
    assert_eq!(
        sort(&guest_views.recv().unwrap()).1,
        &[vec![cards[0]], vec![cards[2], cards[1]]]
    );

    let id = &host.list_saved().unwrap()[0].id;
    let replay = || replay_save(dir.path(), id);
    let (before, before_log) = replay();
    let library = before.zone(Zone::Library(GUEST)).unwrap();

    guest.answer(Answer::Done).unwrap();
    let observer = host_views.recv().unwrap();
    assert!(has_lift(&observer, Span::End));
    assert!(!serde_json::to_string(&observer).unwrap().contains(PRIVATE));
    let done = guest_views.recv().unwrap().view.table.unwrap();
    assert!(done.prompt.is_none());
    assert!(done.log.last().unwrap().undoable);
    let (after, log) = replay();
    assert_eq!(log.entries().len(), before_log.entries().len() + 1);
    assert!(matches!(
        log.entries().last().unwrap().action,
        Action::FlowResult {
            flow: Flow::Scry,
            ..
        }
    ));
    let expected = [library[2], library[1]]
        .into_iter()
        .chain(library[3..].iter().copied())
        .chain([library[0]])
        .collect::<Vec<_>>();
    assert_eq!(after.zone(Zone::Library(GUEST)).unwrap(), expected);
    guest
        .act(Action::Undo {
            entry: Some(log.entries().last().unwrap().n),
        })
        .unwrap();
    game.updates();
    assert_eq!(replay().0.zone(Zone::Library(GUEST)).unwrap(), library);
}

#[test]
fn guest_can_cancel_scry_without_changing_the_table() {
    let dir = tempfile::tempdir().unwrap();
    let game = Linked::new(dir.path(), PRIVATE, 12);
    let opening = open_guest_scry(&game);
    let log_len = opening.view.table.unwrap().log.len();
    let id = &game.host.list_saved().unwrap()[0].id;
    let replay = || replay_save(dir.path(), id);
    let (before_cancel, _) = replay();
    game.guest.answer(Answer::Cancel).unwrap();
    let (observer, cancelled) = game.updates();
    assert!(has_lift(&observer, Span::End));
    let manual = cancelled.view.table.as_ref().unwrap();
    assert!(manual.prompt.is_none());
    assert_eq!(manual.log.len(), log_len);
    assert_eq!(replay().0, before_cancel);
    assert_eq!(
        cancelled
            .view
            .objects
            .iter()
            .filter(|object| object.zone == ZoneName::Library
                && object.name.as_deref() == Some(PRIVATE))
            .count(),
        3,
    );
}

#[test]
fn choose_applies_only_the_selected_option() {
    let table = TableSessions::new(SessionIds::default(), |_| {});
    table
        .solo_seeded(
            Deck {
                main: vec![("Island".into(), 12)],
                ..Deck::default()
            },
            1,
        )
        .unwrap();
    let prompt = PromptState::Choose {
        title: "Play or draw?".into(),
        options: vec!["Play".into(), "Draw".into()],
        selected: None,
    };
    table
        .open_prompt(HOST, prompt, |state| {
            assert!(matches!(
                state,
                PromptState::Choose {
                    selected: Some(1),
                    ..
                }
            ));
            Ok(Action::Life {
                seat: HOST,
                adjustment: Adjustment::Add(1),
            })
        })
        .unwrap();
    assert_eq!(table.answer(Answer::Done), Err(Refusal::INVALID));
    table.answer(Answer::Pick { option: 1 }).unwrap();
    assert!(matches!(
        table.current().unwrap().view.table.unwrap().prompt,
        Some(TablePrompt::Choose {
            selected: Some(1),
            ..
        })
    ));
    table.answer(Answer::Done).unwrap();
    let finished = table.current().unwrap().view;
    assert_eq!(finished.players.you.life, 21);
    assert!(finished.table.unwrap().prompt.is_none());
}
