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
  detail?: string;
  group: string;
  key?: string;
}

export interface CommandListProps {
  commands: readonly CommandListEntry[];
  acting?: string;
  presentation: "panel" | "fullscreen";
  initialQuery?: string;
  onRun: (id: string, shift: boolean) => void;
  search?: {
    label: string;
    query: string;
    onQuery: (query: string) => void;
    busy?: boolean;
  };
  onBind?: (id: string) => void;
  captureBinding?: (id: string, event: globalThis.KeyboardEvent) => boolean;
  bindable?: (id: string) => boolean;
  notice?: string;
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
  search: externalSearch,
  onBind,
  captureBinding,
  bindable,
  notice,
  onClose,
}: CommandListProps) {
  const id = useId();
  const dialog = useRef<HTMLDialogElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const keyCapture = useRef<HTMLButtonElement>(null);
  const [capturing, setCapturing] = useState<string>();
  const activeOption = useRef<HTMLDivElement>(null);
  const [localQuery, setQuery] = useState(initialQuery);
  const query = externalSearch?.query ?? localQuery;
  const [selected, setSelected] = useState<string>();
  const groups = groupCommands(
    externalSearch ? commands : match(query, commands),
  );
  const name = externalSearch?.label ?? "Search commands";
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

  useLayoutEffect(() => {
    if (capturing) keyCapture.current?.focus();
    else search.current?.focus();
  }, [capturing]);

  const onKeyDown = (event: KeyboardEvent<HTMLDialogElement>) => {
    event.stopPropagation();
    if (event.nativeEvent.isComposing) return;
    if (capturing) {
      event.preventDefault();
      if (
        event.key === "Escape" ||
        captureBinding?.(capturing, event.nativeEvent)
      )
        setCapturing(undefined);
      return;
    }
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
        if (!active || (externalSearch?.busy && !selected)) return;
        if (!event.altKey) onRun(active.id, event.shiftKey);
        else if (bindable?.(active.id) !== false) {
          if (captureBinding) setCapturing(active.id);
          else onBind?.(active.id);
        }
        break;
    }
  };

  return (
    <dialog
      ref={dialog}
      className="command-list"
      aria-label={externalSearch ? name : "Command list"}
      data-presentation={presentation}
      onKeyDown={onKeyDown}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
    >
      <div className="command-list-field">
        <input
          ref={search}
          type="text"
          role="combobox"
          aria-label={name}
          aria-describedby={acting ? `${id}-acting` : undefined}
          aria-autocomplete="list"
          aria-expanded="true"
          aria-controls={`${id}-results`}
          aria-activedescendant={active && optionId(active)}
          autoComplete="off"
          placeholder={name}
          value={query}
          onChange={(event) => {
            (externalSearch?.onQuery ?? setQuery)(event.currentTarget.value);
            setSelected(undefined);
          }}
        />
        {acting && (
          <span id={`${id}-acting`} className="command-list-acting">
            {acting}
          </span>
        )}
      </div>
      <div
        id={`${id}-results`}
        role="listbox"
        aria-busy={externalSearch?.busy || undefined}
        aria-label={externalSearch ? name : "Commands"}
        className="command-list-results"
      >
        {groups.map(([group, commands]) => (
          <div key={group} role="group" aria-label={group}>
            {group && (
              <div className="command-list-heading" aria-hidden="true">
                {group}
              </div>
            )}
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
                  onClick={(event) => onRun(command.id, event.shiftKey)}
                >
                  <span>{command.label}</span>
                  {command.detail && <small>{command.detail}</small>}
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
      {capturing && (
        <button
          ref={keyCapture}
          type="button"
          className="command-list-capture"
          onClick={() => setCapturing(undefined)}
        >
          Press a key · Esc cancels
        </button>
      )}
      {notice && <div role="status">{notice}</div>}
      <footer className="command-list-hints">
        <span>{externalSearch ? "↵ choose" : "↵ run"}</span>
        {!externalSearch && <span>⌥↵ bind a key</span>}
        <span>Esc close</span>
      </footer>
    </dialog>
  );
}
