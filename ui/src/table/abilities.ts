import type { AskPromptProps } from "./AskPrompt";
import type { TableClient } from "./client";
import type { MenuCommand, MenuContext } from "./menuRows";

export function cardAbilities({ target, view, cards }: MenuContext) {
  return (target.kind === "permanent" || target.kind === "their_permanent") &&
    target.object &&
    target.action &&
    cards.length === 1
    ? (view.table?.cardCommands?.[target.object.slot]?.abilities ?? [])
    : [];
}

export function activateAbility(
  client: TableClient,
  ability: NonNullable<MenuCommand["ability"]>,
  { target, view, element }: MenuContext,
  close: () => void,
  report: (error: unknown) => void,
  ask: (prompt: AskPromptProps) => void,
) {
  if (!("name" in target) || !target.action) return;
  const { reference } = target.action;
  const send = (x: number | null) => {
    close();
    if (client.view !== view) return;
    void client
      .answerAssist(
        { kind: "activateAbility", source: reference, row: ability.row, x },
        element ?? target.name,
      )
      .catch(report);
  };
  if (!ability.requiresX) {
    send(null);
    return;
  }
  ask({
    kind: "number",
    start: 0,
    min: 0,
    max: 65535,
    fieldLabel: "Choose X",
    label: (number) => `Set X to ${number}`,
    onCancel: close,
    onConfirm: send,
  });
}
