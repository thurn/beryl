//! A player game that outgrows the `Small` layout goes on in `Large` (architecture §4.1):
//! it never ends stalled for a capacity, and plays on to its end.

use std::collections::VecDeque;

use beryl_cards::CARDS;
use beryl_cards::names::{COUNTERSPELL, ISLAND, PLAINS, SNAPCASTER_MAGE};
use beryl_engine::{Choice, Decision, DecisionKind, GameTable, Layout, Observation, TurnStep};
use beryl_scenario::{Action, P0, P1, Permanent, Scenario};
use beryl_search::{Agent, Budget};
use beryl_session::protocol::{OutcomeView, Status};
use beryl_session::{Game, Prepared, Seat, SessionId, StopsSetting};

/// One more Snapcaster Mage than a `Small` state has cast permissions (8): each one's trigger
/// gives the Counterspell in the human's graveyard flashback until end of turn, a permission
/// of its own. Short libraries end the game soon after.
const NINE_SNAPCASTERS: Scenario = Scenario::new("nine-snapcasters")
    .at(1, P0, TurnStep::PrecombatMain)
    .battlefield(P0, &[Permanent::new(ISLAND); 18])
    .hand(P0, &[SNAPCASTER_MAGE; 9])
    .graveyard(P0, &[COUNTERSPELL])
    .library(P0, &[ISLAND, ISLAND, ISLAND])
    .battlefield(P1, &[Permanent::new(PLAINS)])
    .library(P1, &[PLAINS, PLAINS]);

/// Casts each card its script names at its first priority to cast it, and otherwise takes
/// the first candidate: passing priority, the first target.
struct Script {
    table: GameTable,
    casts: VecDeque<Action>,
}

impl<L: Layout> Agent<L> for Script {
    fn choose(&mut self, obs: Observation<'_, L>, d: &Decision) -> Choice {
        if d.kind == DecisionKind::Priority
            && let Some(action) = self.casts.front()
            && let Some(c) = beryl_scenario::answer(&self.table, obs.unmasked_root(), d, action)
        {
            self.casts.pop_front();
            return c;
        }
        Choice(0)
    }
}

#[test]
fn a_game_past_a_small_capacity_plays_on_in_large() {
    let (table, state) = beryl_scenario::position(&CARDS, &NINE_SNAPCASTERS).unwrap();
    let random = beryl_search::agent("random").unwrap();
    let mut game = Game::prepared(
        Prepared {
            opponent: (random.new)(&table, 1, Budget::DEFAULT),
            stand_in: Box::new(Script {
                table: table.clone(),
                casts: [Action::cast(P0, SNAPCASTER_MAGE); 9].into(),
            }),
            table,
            state,
            human: Seat::P0,
            stops: StopsSetting::default(),
        },
        SessionId(1),
    );
    let mut most_permissions = 0;
    let outcome = loop {
        let Some(view) = game.next_view() else {
            game.answer_by_stand_in();
            continue;
        };
        if let Some(s) = game.large() {
            most_permissions = most_permissions.max(s.permissions().len());
        }
        match view.status {
            Status::Over { outcome } => break outcome,
            Status::Deciding { .. } => {
                game.answer_by_stand_in();
            }
            _ => {}
        }
    };
    assert!(
        matches!(outcome, OutcomeView::Win { .. }),
        "the game ends in a win, not a stall: {outcome:?}"
    );
    assert_eq!(
        most_permissions, 9,
        "every Snapcaster's permission was kept"
    );
}
