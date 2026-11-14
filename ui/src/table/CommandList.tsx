import {
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import { match } from "./filter";
import "./commandList.css";

export interface CommandListEntry {
  id: string;
  label: string;
  group: string;
  key?: string;
}

export interface CommandListProps {
  commands: readonly CommandListEntry[];
  acting: string;
  presentation: "panel" | "fullscreen";
  initialQuery?: string;
  onRun: (id: string) => void;
  onBind: (id: string) => void;
  onClose: () => void;
}

function groupCommands(commands: readonly CommandListEntry[]) {
  const groups = new Map<string, CommandListEntry[]>();
  for (const command of commands) {
    const group = groups.get(command.group);
    if (group) group.push(command);
    else groups.set(command.group, [command]);
  }
  return [...groups];
}

export function CommandList({
  commands,
  acting,
  presentation,
  initialQuery = "",
  onRun,
  onBind,
  onClose,
}: CommandListProps) {
  const id = useId();
  const dialog = useRef<HTMLDialogElement>(null);
  const activeOption = useRef<HTMLDivElement>(null);
  const [query, setQuery] = useState(initialQuery);
  const [selected, setSelected] = useState<string>();
  const groups = groupCommands(match(query, commands));
  // Keyboard movement follows the displayed rows after matches are grouped.
  const rows = groups.flatMap(([, commands]) => commands);
  const activeIndex = Math.max(
    0,
    rows.findIndex((row) => row.id === selected),
  );
  const active = rows[activeIndex];
  const optionId = (command: CommandListEntry) => `${id}-command-${command.id}`;

  useLayoutEffect(() => {
    const panel = dialog.current;
    if (!panel) return;
    panel.showModal();
    return () => panel.close();
  }, []);

  useLayoutEffect(() => {
    activeOption.current?.scrollIntoView({
      block: "nearest",
      inline: "nearest",
    });
  }, [active?.id, query]);

  const onKeyDown = (event: KeyboardEvent<HTMLDialogElement>) => {
    event.stopPropagation();
    if (event.nativeEvent.isComposing) return;
    switch (event.key) {
      case "Escape":
        event.preventDefault();
        onClose();
        break;
      case "ArrowUp":
      case "ArrowDown": {
        event.preventDefault();
        if (!rows.length) return;
        const step = event.key === "ArrowDown" ? 1 : -1;
        const next = (activeIndex + step + rows.length) % rows.length;
        setSelected(rows[next]?.id);
        break;
      }
      case "Enter":
        event.preventDefault();
        if (!active) return;
        if (event.altKey) onBind(active.id);
        else onRun(active.id);
        break;
    }
  };

  return (
    <dialog
      ref={dialog}
      className="command-list"
      aria-label="Command list"
      data-presentation={presentation}
      onKeyDown={onKeyDown}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
    >
      <div className="command-list-field">
        <input
          type="text"
          role="combobox"
          aria-label="Search commands"
          aria-describedby={`${id}-acting`}
          aria-autocomplete="list"
          aria-expanded="true"
          aria-controls={`${id}-results`}
          aria-activedescendant={active && optionId(active)}
          autoComplete="off"
          placeholder="Search commands"
          value={query}
          onChange={(event) => {
            setQuery(event.currentTarget.value);
            setSelected(undefined);
          }}
        />
        <span id={`${id}-acting`} className="command-list-acting">
          {acting}
        </span>
      </div>
      <div
        id={`${id}-results`}
        role="listbox"
        aria-label="Commands"
        className="command-list-results"
      >
        {groups.map(([group, commands]) => (
          <div key={group} role="group" aria-label={group}>
            <div className="command-list-heading" aria-hidden="true">
              {group}
            </div>
            {commands.map((command) => {
              const selected = command.id === active?.id;
              return (
                <div
                  key={command.id}
                  id={optionId(command)}
                  ref={selected ? activeOption : undefined}
                  role="option"
                  aria-selected={selected}
                  className="command-list-row"
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={() => onRun(command.id)}
                >
                  <span>{command.label}</span>
                  {command.key && <kbd>{command.key}</kbd>}
                </div>
              );
            })}
          </div>
        ))}
      </div>
      {rows.length === 0 && (
        <div className="command-list-empty" role="status">
          No matching commands
        </div>
      )}
      <footer className="command-list-hints">
        <span>↵ run</span>
        <span>⌥↵ bind a key</span>
        <span>Esc close</span>
      </footer>
    </dialog>
  );
}
