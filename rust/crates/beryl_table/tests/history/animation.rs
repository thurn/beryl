use super::*;
use beryl_table::redact::{AnimationEvent, ShownCard, animation_seen_by};
use beryl_table::state::Face;

#[test]
fn draw_and_undo_animate_in_inverse_order_without_linking_ids() {
    let mut game = Game::new(fixtures());
    let before_draw = game.table.clone();
    let drawn = game.act(HOST, &draw(2));
    let before_undo = game.table.clone();
    let undone = game.act(HOST, &UNDO);
    let hand = before_undo.zone(HAND).unwrap();
    let shown = |index: usize, name: &str| {
        Some(ShownCard {
            id: hand[index],
            card: Card::Named(name.into()),
            face: Face::Front,
        })
    };
    let drawn_ids: Vec<_> = drawn
        .changes
        .iter()
        .filter_map(|change| {
            if let beryl_table::Change::ReplaceId { from, to } = change {
                Some((*from, *to))
            } else {
                None
            }
        })
        .collect();
    for viewer in [HOST, GUEST] {
        let owner = viewer == HOST;
        let events = |entry, before| {
            let projection = animation_seen_by(entry, before, viewer);
            assert_eq!(projection.members.len(), 1);
            projection.members.into_iter().next().unwrap().events
        };
        assert_eq!(
            events(&drawn, &before_draw),
            [
                AnimationEvent::HandMove {
                    owner_transfer: owner.then_some(drawn_ids[0]),
                    from: LIBRARY,
                    to: HAND,
                    hand_card: if owner { shown(1, "Island") } else { None }
                },
                AnimationEvent::HandMove {
                    owner_transfer: owner.then_some(drawn_ids[1]),
                    from: LIBRARY,
                    to: HAND,
                    hand_card: if owner { shown(0, "Brainstorm") } else { None }
                },
            ]
        );
        assert_eq!(
            events(&undone, &before_undo),
            [
                AnimationEvent::HandMove {
                    owner_transfer: owner.then_some((drawn_ids[1].1, drawn_ids[1].0)),
                    from: HAND,
                    to: LIBRARY,
                    hand_card: if owner { shown(0, "Brainstorm") } else { None }
                },
                AnimationEvent::HandMove {
                    owner_transfer: owner.then_some((drawn_ids[0].1, drawn_ids[0].0)),
                    from: HAND,
                    to: LIBRARY,
                    hand_card: if owner { shown(1, "Island") } else { None }
                },
            ]
        );
    }
    game.assert_reopens();
}
