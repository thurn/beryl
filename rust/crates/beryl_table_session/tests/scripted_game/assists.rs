use super::{GUEST, Game, HOST, reference, replay, start_linked_game};
use beryl_session::protocol::{Answer, ProposalView, Side, TargetRef, ZoneName};
use beryl_table::action::{Adjustment, SeenObject, Target};
use beryl_table::state::{Assist, Phase, Zone};
use beryl_table::{Action, Refusal};

#[test]
fn each_seat_can_preview_apply_and_undo_combat() {
    let (mut game, _connection) = start_linked_game(false);
    let switch = |assist, on| Action::SetAssist { assist, on };
    game.accept(HOST, switch(Assist::TurnUpkeep, true));
    game.accept(GUEST, switch(Assist::TurnUpkeep, true));
    game.accept(HOST, switch(Assist::Combat, true));
    game.opening();
    assert!(game.table_view(HOST).proposals.is_empty());
    game.accept(HOST, Action::NextTurn {});
    assert!(game.table_view(HOST).proposals.is_empty());
    assert_eq!(game.proposal(GUEST).title, "Turn upkeep");
    game.accept(HOST, Action::NextTurn {});
    assert!(game.table_view(GUEST).proposals.is_empty());
    assert_eq!(game.proposal(HOST).title, "Turn upkeep");

    game.accept(
        HOST,
        Action::Phase {
            phase: Phase::DeclareAttackers,
        },
    );
    game.arrow(HOST, "Grizzly Bears", Target::Player(GUEST));
    assert_eq!(game.proposal(HOST).title, "Combat");
    assert!(game.table_view(GUEST).proposals.is_empty());
    game.accept(GUEST, switch(Assist::Combat, true));
    let before_block = game.proposal(GUEST).clone();
    let advance = before_block
        .lines
        .iter()
        .find(|line| line.text == "Go to end of combat")
        .unwrap();
    game.answer(
        GUEST,
        Answer::ProposalLine {
            id: before_block.id,
            line: advance.id,
            on: false,
        },
        false,
    );
    assert!(game.proposal(HOST).lines.iter().all(|line| line.on));
    let unblocked_revision = game.proposal(GUEST).revision;

    game.accept(
        HOST,
        Action::Phase {
            phase: Phase::DeclareBlockers,
        },
    );
    let attacker = reference(&game.views[1], "Grizzly Bears", ZoneName::Battlefield);
    game.arrow(
        GUEST,
        "Runeclaw Bear",
        Target::Object(SeenObject {
            id: attacker,
            zone: Zone::Battlefield,
        }),
    );

    let proposed = game.proposal(GUEST).clone();
    assert_eq!(proposed.id, before_block.id);
    assert_ne!(proposed.revision, unblocked_revision);
    assert_eq!(
        proposed
            .lines
            .iter()
            .find(|line| line.id == advance.id)
            .map(|line| line.on),
        Some(false)
    );
    assert!(
        proposed
            .lines
            .iter()
            .any(|line| line.text == "Runeclaw Bear dies" && line.whose == Side::You)
    );
    assert!(
        proposed
            .lines
            .iter()
            .any(|line| line.text == "Grizzly Bears dies" && line.whose == Side::Opponent)
    );
    assert_eq!(
        game.guest.answer(Answer::ApplyProposal {
            id: before_block.id,
            revision: unblocked_revision
        }),
        Err(Refusal::Stale)
    );
    assert_combat_batch_and_undo(&mut game, &proposed);

    game.accept(GUEST, switch(Assist::Combat, false));
    assert!(game.table_view(GUEST).proposals.is_empty());
    assert_eq!(game.proposal(HOST).title, "Combat");
    game.accept(
        HOST,
        Action::ClearArrows {
            from: None,
            to: None,
        },
    );
    assert!(game.table_view(HOST).proposals.is_empty());
}

fn assert_combat_batch_and_undo(game: &mut Game, proposed: &ProposalView) {
    let length = game.table_view(GUEST).log.len();
    game.answer(
        GUEST,
        Answer::ApplyProposal {
            id: proposed.id,
            revision: proposed.revision,
        },
        true,
    );
    assert!(game.table_view(HOST).proposals.is_empty());
    assert!(game.table_view(GUEST).proposals.is_empty());
    let (table, log) = replay(&game.path);
    let entry = log.entries().last().unwrap();
    assert!(matches!(&entry.action, Action::Batch { actions } if actions.len() == 2));
    assert_eq!(entry.by, GUEST);
    assert_eq!(game.table_view(GUEST).log.len(), length + 1);
    assert_eq!(table.phase, Phase::DeclareBlockers);
    assert_eq!(
        table
            .objects
            .values()
            .filter(|card| matches!(card.zone, Zone::Graveyard(_)))
            .count(),
        2
    );
    game.accept(
        HOST,
        Action::Undo {
            entry: Some(entry.n),
        },
    );
    assert_ne!(game.proposal(GUEST).id, proposed.id);
}

#[test]
fn queued_combat_apply_is_stale_after_another_seat_applies_or_restores() {
    let (mut game, _connection) = start_linked_game(false);
    game.opening();
    for seat in [HOST, GUEST] {
        game.accept(
            seat,
            Action::SetAssist {
                assist: Assist::Combat,
                on: true,
            },
        );
    }
    game.accept(
        HOST,
        Action::Phase {
            phase: Phase::DeclareAttackers,
        },
    );
    game.arrow(HOST, "Grizzly Bears", Target::Player(GUEST));
    let restore_point = game.table_view(HOST).log.last().unwrap().n;
    let host = game.proposal(HOST).clone();
    let advance = host.lines.last().unwrap();
    game.answer(
        HOST,
        Answer::ProposalLine {
            id: host.id,
            line: advance.id,
            on: false,
        },
        false,
    );
    let apply = |proposal: &ProposalView| Answer::ApplyProposal {
        id: proposal.id,
        revision: proposal.revision,
    };

    let guest = game.proposal(GUEST).clone();
    let host_apply = apply(game.proposal(HOST));
    game.answer(HOST, host_apply, true);
    assert_eq!(game.proposal(GUEST).lines, guest.lines);
    assert_eq!(game.guest.answer(apply(&guest)), Err(Refusal::Stale));
    let (table, log) = replay(&game.path);
    assert_eq!(table.players[usize::from(GUEST.0)].life, 18);

    let after_apply = apply(game.proposal(GUEST));
    game.accept(
        GUEST,
        Action::Undo {
            entry: Some(log.entries().last().unwrap().n),
        },
    );
    assert_eq!(game.guest.answer(after_apply), Err(Refusal::Stale));

    let guest = game.proposal(GUEST).clone();
    game.accept(HOST, Action::Rewind { to: restore_point });
    assert_ne!(game.proposal(GUEST).id, guest.id);
    assert_eq!(game.guest.answer(apply(&guest)), Err(Refusal::Stale));
}

#[test]
fn only_the_eligible_seat_can_click_a_concession_nudge() {
    let (mut game, _connection) = start_linked_game(false);
    game.opening();
    game.accept(
        HOST,
        Action::SetAssist {
            assist: Assist::StateNudges,
            on: true,
        },
    );
    game.accept(
        GUEST,
        Action::SetAssist {
            assist: Assist::StateNudges,
            on: true,
        },
    );
    game.accept(
        HOST,
        Action::Life {
            seat: GUEST,
            adjustment: Adjustment::Set(0),
        },
    );
    assert!(game.table_view(HOST).labels.is_empty());
    let labels = &game.table_view(GUEST).labels;
    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0].on, TargetRef::Player { player: Side::You });
    assert_eq!(labels[0].text, "Concede");
    let answer = Answer::ApplyLabel { id: labels[0].id };
    assert_eq!(game.host.answer(answer.clone()), Err(Refusal::Stale));
    assert!(replay(&game.path).0.game_match.in_game);
    game.answer(GUEST, answer, true);
    assert!(!replay(&game.path).0.game_match.in_game);
    assert!(game.table_view(GUEST).labels.is_empty());
}
