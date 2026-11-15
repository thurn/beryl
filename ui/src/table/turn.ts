import { STEP_NAMES } from "../game/stepLabels.ts";
import type {
  PlayerView,
  Step,
  TableAction,
  TableActionPhase,
  TablePhase,
} from "../generated/protocol";
import type { TableClient } from "./client";
import { seatOf } from "./menuRows.ts";

// Space and Tab cycle through these phases. The Next turn command advances the turn.
const phases = {
  untap: ["untap", "untap"],
  upkeep: ["upkeep", "upkeep"],
  draw: ["draw", "draw"],
  firstMain: ["precombatMain", "first_main"],
  beginningOfCombat: ["beginningOfCombat", "beginning_of_combat"],
  declareAttackers: ["declareAttackers", "declare_attackers"],
  declareBlockers: ["declareBlockers", "declare_blockers"],
  combatDamage: ["combatDamage", "combat_damage"],
  endOfCombat: ["endOfCombat", "end_of_combat"],
  secondMain: ["postcombatMain", "second_main"],
  end: ["end", "end"],
} as const satisfies Record<TablePhase, readonly [Step, TableActionPhase]>;

const order = Object.values(phases);

function nextPhase(phase: TablePhase) {
  return order[order.indexOf(phases[phase]) + 1] ?? phases.untap;
}

export function nextPhaseLabel(phase: TablePhase) {
  return STEP_NAMES[nextPhase(phase)[0]];
}

export function untapPermanents(view: PlayerView): TableAction {
  const tapped = new Set(
    view.objects
      .filter(
        (object) =>
          object.zone === "battlefield" &&
          object.controller === "you" &&
          object.tapped &&
          !object.doesntUntap,
      )
      .map(({ slot }) => slot),
  );
  const objects = (view.table?.objectActions ?? []).flatMap(
    ({ subject, reference, zone }) =>
      zone === "battlefield" &&
      subject.kind === "object" &&
      tapped.has(subject.slot)
        ? [reference]
        : [],
  );
  return { tap: { objects, in: "battlefield", tapped: false } };
}

function phaseAction(view: PlayerView, phase: TableActionPhase) {
  if (view.turn.active !== "you") return;
  if (phase === "untap") return untapPermanents(view);
  if (phase === "draw")
    return { draw: { seat: seatOf(view, "you"), count: 1 } };
}

export function advancePhase(
  view: PlayerView,
  withAction: boolean,
): TableAction[] {
  if (!view.table) return [];
  const [, phase] = nextPhase(view.table.phase);
  const advance = { phase: { phase } };
  const action = withAction ? phaseAction(view, phase) : undefined;
  return action ? [advance, action] : [advance];
}

export function registerTurn(
  client: TableClient,
  report: (error: unknown) => void,
) {
  client.onClick("step", ({ side, step }, _keys, element) => {
    const view = client.view;
    if (!view?.table) return;

    // Cleanup has no manual phase. The damage icon represents either engine damage step.
    const mapped =
      step === "cleanup"
        ? "end"
        : step === "firstStrikeDamage"
          ? "combatDamage"
          : step;
    const phase = order.find(([engineStep]) => engineStep === mapped)?.[1];
    if (!phase) return;

    const action =
      side === view.turn.active && step === phases[view.table.phase][0]
        ? phaseAction(view, phase)
        : undefined;
    void client.send(action ?? { phase: { phase } }, element).catch(report);
  });
}
