// The floating prompt: every question the board asks in its own words (a cast's mode, a yes or
// no, a target). Its title and buttons stand in the action dock at the bottom right
// (Controls.tsx' Dock), where the turn button is; what it shows to choose among (cards off
// the table, triggers to order, a crew meter) stands mid-table, with no panel.
import type { ReactNode } from "react";
import { Dock, DockButtons } from "../components/Controls";

export function FloatPrompt({
  title,
  label,
  body,
  children,
}: {
  title: ReactNode;
  /** The dialog's accessible name, when the title is not text. */
  label?: string;
  /** What it shows mid-table to choose among, if anything. */
  body?: ReactNode;
  /** The dock's rows under the title: its buttons (PromptRow) and any stepper. */
  children?: ReactNode;
}) {
  return (
    <div
      className="prompt"
      role="dialog"
      aria-label={label ?? (typeof title === "string" ? title : undefined)}
    >
      {body && <div className="prompt-body">{body}</div>}
      <Dock title={title}>{children}</Dock>
    </div>
  );
}

/** The prompt's buttons, its first the primary at the dock's foot. */
export function PromptRow({ children }: { children: ReactNode }) {
  return (
    <DockButtons className="prompt-row prompt-choices">{children}</DockButtons>
  );
}
