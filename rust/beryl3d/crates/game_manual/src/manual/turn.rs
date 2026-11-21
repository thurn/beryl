//! The manual table's turn controls (docs/ui-conventions.md, "Manual table turn controls"), as
//! the web's `registerTurn` and `nextPhaseLabel` (`ui/src/table/turn.ts`) work them:
//!
//! - The step track beside each player's life orb (`screens::table`) lights the
//!   table's phase in the active player's rows. A press on an icon sets the phase to that
//!   icon's (`step_command`). Cleanup, which the manual table has no phase for, sets the end
//!   step. A press on the lit Untap or Draw icon in the viewer's own turn also untaps their
//!   permanents or draws them a card.
//! - The Next button in the lower right (`next`) names the phase after the table's, and
//!   advances to it with the Next phase command, wrapping from the end step to Untap.

use beryl_protocol::protocol::{PlayerView, Side, TableAction, TablePhase};
use glam::Vec2;

use super::TablePress;
use game_table::table::Window;
use kit_ui::element::{Anchor, Element};
use kit_ui::widgets::primary;

/// The phase each icon of the step track sets, by its index in the track
/// (`hud::steps::STEP_STOPS`): the last, cleanup, sets the end step, as the web maps it.
const PHASES: [TablePhase; 12] = [
    TablePhase::Untap,
    TablePhase::Upkeep,
    TablePhase::Draw,
    TablePhase::FirstMain,
    TablePhase::BeginningOfCombat,
    TablePhase::DeclareAttackers,
    TablePhase::DeclareBlockers,
    TablePhase::CombatDamage,
    TablePhase::EndOfCombat,
    TablePhase::SecondMain,
    TablePhase::End,
    TablePhase::End,
];

/// The index in the step track of the icon `phase` lights: the first whose phase it is.
#[must_use]
pub fn icon_of(phase: TablePhase) -> usize {
    PHASES.iter().position(|&each| each == phase).unwrap_or(0)
}

/// The phase after `phase`, wrapping from the end step to Untap (`nextPhase`).
#[must_use]
pub fn next_phase(phase: TablePhase) -> TablePhase {
    PHASES[(icon_of(phase) + 1) % (PHASES.len() - 1)]
}

/// What the Next button says under its label for the phase after `phase`: the name the web's
/// `STEP_NAMES` (`ui/src/game/stepLabels.ts`) gives its step.
#[must_use]
pub const fn caption(phase: TablePhase) -> &'static str {
    match phase {
        TablePhase::Untap => "Untap",
        TablePhase::Upkeep => "Upkeep",
        TablePhase::Draw => "Draw",
        TablePhase::FirstMain => "Main 1",
        TablePhase::BeginningOfCombat => "Beginning of combat",
        TablePhase::DeclareAttackers => "Attackers",
        TablePhase::DeclareBlockers => "Blockers",
        TablePhase::CombatDamage => "Combat damage",
        TablePhase::EndOfCombat => "End of combat",
        TablePhase::SecondMain => "Main 2",
        TablePhase::End => "End step",
    }
}

/// The id of the table command a press on the icon at `at` of the step track gives, where
/// `active` says whether the icon is in the active player's rows: the phase command of the
/// icon's phase, except that on the viewer's own turn the lit Untap icon untaps the active
/// player's permanents and the lit Draw icon draws them a card (`phaseAction`).
#[must_use]
pub fn step_command(view: &PlayerView, at: u8, active: bool) -> &'static str {
    let Some(table) = view.table.as_ref() else {
        return "";
    };
    let at = usize::from(at);
    let phase = PHASES.get(at).copied().unwrap_or(TablePhase::Untap);
    let lit = active && at == icon_of(table.phase);
    match phase {
        TablePhase::Untap if lit && view.turn.active == Side::You => "phase.untap_active",
        TablePhase::Draw if lit && view.turn.active == Side::You => "phase.draw_active",
        _ => phase_command(phase),
    }
}

/// The id of the table command that sets `phase`, whose part after `phase.` is the phase's
/// name in a table action.
const fn phase_command(phase: TablePhase) -> &'static str {
    match phase {
        TablePhase::Untap => "phase.untap",
        TablePhase::Upkeep => "phase.upkeep",
        TablePhase::Draw => "phase.draw",
        TablePhase::FirstMain => "phase.first_main",
        TablePhase::BeginningOfCombat => "phase.beginning_of_combat",
        TablePhase::DeclareAttackers => "phase.declare_attackers",
        TablePhase::DeclareBlockers => "phase.declare_blockers",
        TablePhase::CombatDamage => "phase.combat_damage",
        TablePhase::EndOfCombat => "phase.end_of_combat",
        TablePhase::SecondMain => "phase.second_main",
        TablePhase::End => "phase.end",
    }
}

/// The table actions of the Next phase command, or with `with_action` of Next phase with
/// action (`advancePhase`): the phase after the table's, and with an action, on the viewer's
/// own turn, untapping their permanents as it goes to Untap or drawing them a card as it goes
/// to Draw, both in one batch.
#[must_use]
pub fn advance(view: &PlayerView, with_action: bool) -> Vec<TableAction> {
    let Some(table) = view.table.as_ref() else {
        return Vec::new();
    };
    let next = next_phase(table.phase);
    let word = phase_command(next).trim_start_matches("phase.");
    let Ok(advance) =
        serde_json::from_value::<TableAction>(serde_json::json!({ "phase": { "phase": word } }))
    else {
        return Vec::new();
    };
    let action = match next {
        _ if !with_action || view.turn.active != Side::You => None,
        TablePhase::Untap => Some(super::commands::untap(view)),
        TablePhase::Draw => serde_json::from_value(serde_json::json!({
            "draw": { "seat": super::commands::seat_of(view, Side::You), "count": 1 }
        }))
        .ok(),
        _ => None,
    };
    match action {
        Some(action) => vec![TableAction::Batch {
            actions: vec![advance, action],
        }],
        None => vec![advance],
    }
}

/// The Next button and, under it, the name of the phase after `phase`, in the lower right of
/// `window`. On the desktop it stands as the web's table stands its `turn-action`, at the foot
/// of the game's action dock (`prompts::bar`), `layout::DOCK_RIGHT` and `DOCK_BOTTOM` in from
/// the corner. On the phone it stands the table's edge (`flat.table.edge`) in from the corner.
#[must_use]
pub fn next(
    phase: TablePhase,
    window: Window,
    profile: game_table::profiles::Profile<'_>,
) -> Element {
    use game_hud::prompts::bar::{captioned, captioned_dock, dock_sized, turn_action};
    use kit_ui::layout::{DOCK_BOTTOM, DOCK_RIGHT};
    let size = window.size;
    let button = primary("Next").on_press(TablePress::Next);
    let caption = caption(next_phase(phase));
    let (mut action, corner) = if let Some(slot) = profile.slot("manual.next") {
        let rect = slot.rect;
        (
            captioned(button, caption),
            Vec2::new(rect.x + rect.width, rect.y + rect.height),
        )
    } else {
        let dock = captioned_dock().child(turn_action(dock_sized(button), None, caption));
        (
            dock,
            Vec2::new(size.width - DOCK_RIGHT, size.height - DOCK_BOTTOM),
        )
    };
    action.anchor = Some(Anchor::Point(corner, Vec2::ONE));
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_next_phase_wraps_from_the_end_step_to_untap() {
        assert_eq!(
            next_phase(TablePhase::FirstMain),
            TablePhase::BeginningOfCombat
        );
        assert_eq!(next_phase(TablePhase::End), TablePhase::Untap);
    }
}
