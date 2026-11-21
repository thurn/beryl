//! The numbers a `count` step changes that are not on a card: each player's life total and
//! unspent mana (docs/beryl-3d.md, "Effects" and "Floats").
//!
//! - The verb writes each `Tally::Life` and `Tally::Pool` as a `CountChange` at the start of its
//!   step, and the show applies the change to its `Counts` when the script's clock reaches it.
//!   As a script ends, `Counts` takes the numbers of the view it leads to. So a total changes as
//!   its step begins, not when the view that holds it arrives.
use beryl_protocol::protocol::{ManaPool, PlayerView, Side};
use glam::Vec2;
use kit_ui::input::Size;

/// A number a `count` step sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Count {
    Life(i32),
    Pool(ManaPool),
}

/// One player's number set at `at`: on the script's clock as a verb writes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CountChange {
    pub player: Side,
    pub count: Count,
    pub at: u64,
    /// The number of the step that wrote it.
    pub step: usize,
}

/// One player's numbers as the table shows them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlayerCounts {
    pub life: i32,
    pub pool: ManaPool,
    /// How much the last change to `life` changed it by, which a `loss` or `gain` shows.
    pub change: i32,
}

/// Both players' numbers as the table shows them, the viewer's first; none before the first
/// script has ended.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    players: Option<[PlayerCounts; 2]>,
}

impl Counts {
    /// `side`'s numbers, once the table shows a view.
    #[must_use]
    pub fn player(&self, side: Side) -> Option<&PlayerCounts> {
        Some(&self.players.as_ref()?[index(side)])
    }

    /// Takes the life totals and pools of `view`, which the table now shows at rest. The last
    /// change is kept, because the `loss` or `gain` that shows it may outlast its script.
    pub fn take_view(&mut self, view: &PlayerView) {
        let players = self.players.get_or_insert_default();
        for (counts, state) in players
            .iter_mut()
            .zip([&view.players.you, &view.players.opponent])
        {
            counts.life = state.life;
            counts.pool = state.mana;
        }
    }

    /// Applies one change a `count` step wrote.
    pub fn apply(&mut self, change: &CountChange) {
        let Some(players) = &mut self.players else {
            return;
        };
        let counts = &mut players[index(change.player)];
        match change.count {
            Count::Life(life) => {
                counts.change = life - counts.life;
                counts.life = life;
            }
            Count::Pool(mana) => counts.pool = mana,
        }
    }
}

/// The index of `side` in pairs that put the viewer first.
pub const fn index(side: Side) -> usize {
    match side {
        Side::You => 0,
        Side::Opponent => 1,
    }
}

/// A point in normalized device coordinates as logical points of a window of `size`.
pub fn points(ndc: Vec2, size: Size) -> Vec2 {
    Vec2::new(
        f32::midpoint(ndc.x, 1.0) * size.width,
        f32::midpoint(1.0, -ndc.y) * size.height,
    )
}
