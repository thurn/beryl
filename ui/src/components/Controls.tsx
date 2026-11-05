// Actions and status: buttons, the priority pill and the phase tracker.
import type { ButtonHTMLAttributes, ReactNode } from "react";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "primary" | "secondary";
  icon?: ReactNode;
  size?: "regular" | "small";
}

/** Primary (ember), secondary (outlined) or, with `disabled`, the muted disabled button. */
export function Button({
  variant = "primary",
  icon,
  size = "regular",
  children,
  className,
  ...rest
}: ButtonProps) {
  return (
    <button
      type="button"
      className={["button", `button-${variant}`, `button-${size}`, className]
        .filter(Boolean)
        .join(" ")}
      {...rest}
    >
      {icon && <span className="button-icon">{icon}</span>}
      <span>{children}</span>
    </button>
  );
}

export function FastForwardIcon() {
  return (
    <svg viewBox="0 0 24 14" aria-hidden>
      <path fill="currentColor" d="M1 1 L11 7 L1 13 Z M12 1 L22 7 L12 13 Z" />
    </svg>
  );
}

function Sparkle() {
  return (
    <svg className="sparkle" viewBox="0 0 20 20" aria-hidden>
      <path
        fill="currentColor"
        d="M10 0 C11 7 13 9 20 10 C13 11 11 13 10 20 C9 13 7 11 0 10 C7 9 9 7 10 0 Z"
      />
    </svg>
  );
}

export function PriorityPill({ children }: { children: ReactNode }) {
  return (
    <div className="priority-pill" role="status">
      <Sparkle />
      <span>{children}</span>
    </div>
  );
}

export type Phase = "beginning" | "main1" | "combat" | "main2" | "end";

const DIAMOND = <path d="M12 3 L20.5 12 L12 21 L3.5 12 Z" />;
const CARD = <rect x="6.5" y="3" width="11" height="18" rx="2" />;

/** Each phase's outline icon (the full tracker) and filled glyph (the compact one on the
 * table, as in battlefield.png). */
const PHASES: {
  phase: Phase;
  label: string;
  icon: ReactNode;
  glyph: ReactNode;
}[] = [
  {
    phase: "beginning",
    label: "Beginning",
    icon: (
      <>
        <rect
          x="5"
          y="3"
          width="11"
          height="15"
          rx="1.5"
          transform="rotate(-12 10 10)"
        />
        <rect x="9" y="5" width="11" height="15" rx="1.5" />
      </>
    ),
    glyph: DIAMOND,
  },
  {
    phase: "main1",
    label: "First main phase",
    icon: (
      <path d="M2 20 L10 6 L14 12 L16 9 L22 20 Z M10 6 L8 11 L10 10 L12 12" />
    ),
    glyph: CARD,
  },
  {
    phase: "combat",
    label: "Combat",
    icon: (
      <path d="M3 3 L13 13 M21 3 L11 13 M3 3 L6 3 M3 3 L3 6 M21 3 L18 3 M21 3 L21 6 M8 15 L4 19 M16 15 L20 19 M6 13 L10 17 M18 13 L14 17" />
    ),
    glyph: (
      <>
        <path d="M3 2 L6 2 L17 13 L13 17 L2 6 L2 3 Z" />
        <path d="M21 2 L18 2 L7 13 L11 17 L22 6 L22 3 Z" />
        <path d="M5 13 L11 19 L9 21 L3 15 Z M19 13 L13 19 L15 21 L21 15 Z" />
        <path d="M3.5 19 L5 20.5 L3 22.5 L1.5 21 Z M20.5 19 L19 20.5 L21 22.5 L22.5 21 Z" />
      </>
    ),
  },
  {
    phase: "main2",
    label: "Second main phase",
    icon: (
      <path d="M12 2 L20 5 L20 11 C20 16 16 20 12 22 C8 20 4 16 4 11 L4 5 Z" />
    ),
    glyph: CARD,
  },
  {
    phase: "end",
    label: "End",
    icon: <path d="M19 12 A7 7 0 1 1 12 5 L17 5 M14 2 L17 5 L14 8" />,
    glyph: DIAMOND,
  },
];

/** The turn's phases, the current one lit gold. The compact variant sits on the table above
 * the pass button. */
export function PhaseTracker({
  current,
  compact = false,
}: {
  current: Phase;
  compact?: boolean;
}) {
  return (
    <ol
      className={
        compact ? "phase-tracker phase-tracker-compact" : "phase-tracker"
      }
    >
      {PHASES.map(({ phase, label, icon, glyph }, i) => (
        <li
          key={phase}
          className={phase === current ? "phase phase-current" : "phase"}
        >
          {i > 0 && !compact && <span className="phase-arrow" aria-hidden />}
          <svg viewBox="0 0 24 24" role="img" aria-label={label}>
            {compact ? glyph : icon}
          </svg>
        </li>
      ))}
    </ol>
  );
}
