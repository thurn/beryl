// The floating prompt: a short title over a row of buttons, centered over the board, with no
// panel and no card. Every question the board asks in its own words (a cast's mode, a yes or
// no, a target) wears it.
import type { ReactNode } from "react";

export function FloatPrompt({
  title,
  label,
  children,
}: {
  title: ReactNode;
  /** The dialog's accessible name, when the title is not text. */
  label?: string;
  children?: ReactNode;
}) {
  return (
    <div
      className="prompt"
      role="dialog"
      aria-label={label ?? (typeof title === "string" ? title : undefined)}
    >
      <h2 className="prompt-title">{title}</h2>
      {children}
    </div>
  );
}

/** A row of the prompt's buttons, all one height. */
export function PromptRow({ children }: { children: ReactNode }) {
  return <div className="prompt-row prompt-choices">{children}</div>;
}
