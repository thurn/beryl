//! Card-specific judgments for the expert's combat planner.

use beryl_cards::names;
use beryl_engine::card::Types;

use beryl_engine::{GameTable, Layout, Seat, State};

use crate::combat::Judgments;
use crate::rollout::Policy;

pub(super) struct ExpertCombat;

impl<L: Layout> Judgments<L> for ExpertCombat {
    fn hold_back(t: &GameTable, s: &State<L>, slot: L::Slot) -> bool {
        // Looting is worth more than attacking except when attacking is lethal.
        s.definition_name(t, slot) == names::JACE_VRYNS_PRODIGY
    }

    fn life(t: &GameTable, s: &State<L>, player: Seat) -> i16 {
        // Each Mana Crypt flip costs 3 life half the time.
        let crypts = s
            .battlefield()
            .iter()
            .filter(|&&c| s.controller(c) == player && s.definition_name(t, c) == names::MANA_CRYPT)
            .count() as i16;
        s.player(player).life() - 2 * crypts
    }

    /// After our attack, what the opponent's The Wandering Emperor takes on their turn: "−2:
    /// Exile target tapped creature", our most valuable attacker still tapped, while she has
    /// the loyalty to survive it (at 2 she trades herself for it).
    fn tapped_risk(t: &GameTable, s: &State<L>, me: Seat, p: &Policy<'_, L>) -> i32 {
        let emperor = s.battlefield().iter().any(|&w| {
            s.controller(w) != me
                && s.definition_name(t, w) == names::THE_WANDERING_EMPEROR
                && s.object(w).counters().loyalty >= 3
        });
        if !emperor {
            return 0;
        }
        s.battlefield()
            .iter()
            .filter(|&&c| {
                s.controller(c) == me
                    && s.derived(c).types.intersects(Types::CREATURE)
                    && s.object(c).is_tapped()
            })
            .map(|&c| p.value(c))
            .max()
            .unwrap_or(0)
    }
}
