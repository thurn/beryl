import { useReducer, useState } from "react";
import { CommandList, type CommandListEntry } from "./CommandList";
import { commands as registry } from "./commands";
import { describe, bindingFor } from "./shortcuts";
import type { TableCommands } from "./TableMenu";
import { kindOf, menuRows } from "./menuRows";
import type { MenuContext } from "./menuRows";

const reset = "table.reset_shortcuts";

export function TablePalette({
  commands,
  context,
  phone,
  settings,
  contextual,
}: {
  commands: TableCommands;
  context: MenuContext;
  phone: boolean;
  settings?: boolean;
  contextual?: boolean;
}) {
  const [, changed] = useReducer((revision: number) => revision + 1, 0);
  const [notice, setNotice] = useState("");
  const { bindings } = commands;
  const entries = commands.list(context, contextual);
  const rows = entries.map<CommandListEntry>(
    ({ entry, context: actionContext }) => {
      const { target } = actionContext;
      const detail =
        actionContext !== context && "side" in target
          ? target.kind === "player"
            ? "You"
            : `Your ${target.kind}`
          : undefined;
      return {
        id: entry.id,
        label: entry.label,
        detail,
        group: entry.command.group,
        key: describe(bindings.get(entry.command.id), bindings.platform),
      };
    },
  );
  if (settings)
    rows.unshift({
      id: reset,
      label: "Reset shortcuts",
      group: "Shortcuts",
      key: "",
    });

  return (
    <CommandList
      acting={menuRows(kindOf(context.target), context).header.title}
      commands={rows}
      presentation={phone ? "fullscreen" : "panel"}
      onClose={commands.close}
      onRun={(id, shift) => {
        if (id === reset) {
          bindings.reset();
          setNotice("Shortcuts reset.");
          changed();
          return;
        }
        const selected = entries.find(({ entry }) => entry.id === id);
        if (!selected) return;
        commands.close();
        void commands
          .run(selected.entry, selected.context, shift)
          .catch(commands.reportError);
      }}
      captureBinding={(id, event) => {
        const selected = entries.find(({ entry }) => entry.id === id);
        const binding = bindingFor(event, bindings.platform);
        if (!selected || !binding || event.repeat) return false;
        const displaced = bindings.bind(selected.entry.command.id, binding);
        const label = registry.find(({ id }) => id === displaced)?.label;
        setNotice(label ? `${label} lost its shortcut.` : "Shortcut saved.");
        changed();
        return true;
      }}
      bindable={(id) => id !== reset}
      notice={notice}
    />
  );
}
