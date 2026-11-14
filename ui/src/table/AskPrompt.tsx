import { useLayoutEffect, useRef, useState } from "react";
import { Button, Dock } from "../components/Controls";
import { clamp, parse, parseStats, type Stats } from "./ask";
import "./ask.css";

interface Entry<T> {
  /** The value selected when the prompt opens. Mount a new prompt for each command. */
  start: T;
  label: (value: T) => string;
  onConfirm: (value: T) => void;
  onCancel: () => void;
  fieldLabel?: string;
  autoFocus?: boolean;
}

interface Bounds {
  min: number;
  max: number;
}

export type AskPromptProps =
  | (Entry<number> & Bounds & { kind: "number"; current?: number })
  | (Entry<string> & { kind: "text" })
  | (Entry<Stats> & Bounds & { kind: "stats"; current?: Stats });

/** The manual table's typed entry, in the same dock as the engine's number choices. */
export function AskPrompt(props: AskPromptProps) {
  switch (props.kind) {
    case "text":
      return (
        <AskEntry
          key={props.kind}
          {...props}
          fieldLabel={props.fieldLabel ?? "Text"}
          read={(text) => text}
          format={String}
        />
      );
    case "number": {
      const within = (value: number) => clamp(value, props.min, props.max);
      return (
        <AskEntry
          key={props.kind}
          {...props}
          fieldLabel={props.fieldLabel ?? "Number"}
          start={within(props.start)}
          read={(text) => {
            const value = parse(text, props.current);
            return value === undefined ? undefined : within(value);
          }}
          format={String}
          step={(value, by) => within(value + by)}
        />
      );
    }
    case "stats": {
      const within = ({ power, toughness }: Stats) => ({
        power: clamp(power, props.min, props.max),
        toughness: clamp(toughness, props.min, props.max),
      });
      return (
        <AskEntry
          key={props.kind}
          {...props}
          fieldLabel={props.fieldLabel ?? "Power and toughness"}
          start={within(props.start)}
          read={(text) => {
            const value = parseStats(text, props.current ?? props.start);
            return value === undefined ? undefined : within(value);
          }}
          format={({ power, toughness }) => `${power}/${toughness}`}
          step={({ power, toughness }, by) =>
            within({ power: power + by, toughness: toughness + by })
          }
        />
      );
    }
  }
}

function AskEntry<T>({
  start,
  label,
  onConfirm,
  onCancel,
  fieldLabel,
  autoFocus = true,
  read,
  format,
  step,
}: Entry<T> & {
  read: (text: string) => T | undefined;
  format: (value: T) => string;
  step?: (value: T, by: number) => T;
}) {
  // The initial value and the result of a step are absolute even when displayed with a minus.
  const [{ text, value }, setEntry] = useState<{
    text: string;
    value: T | undefined;
  }>(() => ({ text: format(start), value: start }));
  const form = useRef<HTMLFormElement>(null);
  const field = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => {
    if (!autoFocus) return;
    const previous = document.activeElement;
    const prompt = form.current;
    field.current?.focus({ preventScroll: true });
    field.current?.select();
    return () => {
      const active = document.activeElement;
      if (
        previous instanceof HTMLElement &&
        (active === document.body || prompt?.contains(active))
      ) {
        previous.focus({ preventScroll: true });
      }
    };
  }, [autoFocus]);
  const stepButton = (by: number, sign: string, name: string) => {
    const from = value ?? start;
    const to = step?.(from, by);
    if (to === undefined) return null;
    const next = format(to);
    return (
      <button
        type="button"
        className="prompt-step"
        aria-label={name}
        disabled={value !== undefined && next === format(from)}
        onClick={() => {
          setEntry({ text: next, value: to });
        }}
      >
        {sign}
      </button>
    );
  };
  return (
    <Dock>
      <form
        ref={form}
        className="table-ask-form"
        onPointerDown={(event) => event.stopPropagation()}
        onKeyDown={(event) => {
          event.stopPropagation();
          if (event.nativeEvent.isComposing) return;
          if (event.key === "Escape") {
            event.preventDefault();
            onCancel();
          }
        }}
        onSubmit={(event) => {
          event.preventDefault();
          if (value !== undefined) onConfirm(value);
        }}
      >
        <Button variant="secondary" onClick={onCancel}>
          Cancel
        </Button>
        <div className="prompt-stepper">
          {stepButton(-1, "−", "Less")}
          <input
            className="prompt-value table-ask-input"
            aria-label={fieldLabel}
            aria-invalid={value === undefined || undefined}
            autoComplete="off"
            ref={field}
            value={text}
            onChange={(event) => {
              const text = event.target.value;
              setEntry({ text, value: read(text) });
            }}
          />
          {stepButton(1, "+", "More")}
        </div>
        <Button type="submit" disabled={value === undefined}>
          {value === undefined ? "Confirm" : label(value)}
        </Button>
      </form>
    </Dock>
  );
}
