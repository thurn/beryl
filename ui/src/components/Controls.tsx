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
    <svg viewBox="0 0 24 16" aria-hidden>
      <path fill="currentColor" d="M1 1 L11 8 L1 15 Z M12 1 L22 8 L12 15 Z" />
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

const DIAMOND = <path d="M12 5 L19 12 L12 19 L5 12 Z" />;
/** The current diamond on the compact tracker: a gold rim, a dark ring and a bright core. */
const DIAMOND_LIT = (
  <>
    <path className="lit-rim" d="M12 4.6 L19.4 12 L12 19.4 L4.6 12 Z" />
    <path className="lit-ring" d="M12 7 L17 12 L12 17 L7 12 Z" />
    <path className="lit-rim" d="M12 8.4 L15.6 12 L12 15.6 L8.4 12 Z" />
    <path className="lit-core" d="M12 9.3 L14.7 12 L12 14.7 L9.3 12 Z" />
  </>
);
const CARD = <rect x="4.5" y="3" width="15" height="18" rx="2.2" />;

/** Each phase's icon (the full tracker, ui_system.png section 02: cream outlines over dark or
 * stone fills) and glyph (the compact one on the table, as in battlefield.png). */
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
          className="phase-dark"
          x="1.5"
          y="5"
          width="11"
          height="16.5"
          rx="1.2"
          transform="rotate(-20 7 13.25)"
        />
        <rect
          className="phase-dark"
          x="5"
          y="3.5"
          width="11"
          height="16.5"
          rx="1.2"
          transform="rotate(-8 10.5 11.75)"
        />
        <rect
          className="phase-card"
          x="10"
          y="2"
          width="11.5"
          height="17"
          rx="1.2"
          transform="rotate(8 15.75 10.5)"
        />
      </>
    ),
    glyph: DIAMOND,
  },
  {
    phase: "main1",
    label: "First main phase",
    icon: (
      <>
        <path
          className="phase-dark"
          d="M0.5 21.5 L7 11 L9 12.5 L12 3.5 L15 12.5 L17 11 L23.5 21.5 Z"
        />
        <path
          className="phase-stone"
          stroke="none"
          d="M4.5 20 L8.8 14 L10.3 15.3 L12 11 L13.7 15.3 L15.2 14 L19.5 20 Z"
        />
        <path
          strokeWidth="1"
          d="M7.6 13.4 L9.6 15.2 L12 9.6 L14.4 15.2 L16.4 13.4"
        />
      </>
    ),
    glyph: CARD,
  },
  {
    phase: "combat",
    label: "Combat",
    icon: (
      <>
        <path
          className="phase-blade"
          d="M1.68 1.68 L3.59 2.03 L16.03 14.47 L15.89 14.62 L19.42 18.15 L19.85 17.73 L21.40 19.28 L19.28 21.40 L17.73 19.85 L18.15 19.42 L14.62 15.89 L14.47 16.03 L2.03 3.59 Z"
        />
        <path d="M4.22 4.22 L13.77 13.77 M18.08 12.14 L12.14 18.08" />
        <path
          className="phase-blade"
          d="M22.32 1.68 L20.41 2.03 L7.97 14.47 L8.11 14.62 L4.58 18.15 L4.15 17.73 L2.60 19.28 L4.72 21.40 L6.27 19.85 L5.85 19.42 L9.38 15.89 L9.53 16.03 L21.97 3.59 Z"
        />
        <path d="M19.78 4.22 L10.23 13.77 M5.92 12.14 L11.86 18.08" />
      </>
    ),
    glyph: (
      <>
        <path
          className="phase-tab"
          d="M-3.6 25.6 L2.2 20.4 L21.8 20.4 L27.6 25.6"
        />
        <path d="M2.67 2.67 L5.71 1.61 L10.30 5.21 L17.87 13.20 L19.71 11.36 L20.98 12.64 L17.87 15.75 L20.56 18.43 L20.91 18.08 L21.62 18.08 L22.39 18.58 L22.89 19.35 L22.89 20.06 L22.32 20.91 L21.69 21.69 L20.91 22.32 L20.06 22.89 L19.35 22.89 L18.58 22.39 L18.08 21.62 L18.08 20.91 L18.43 20.56 L15.75 17.87 L12.64 20.98 L11.36 19.71 L13.20 17.87 L5.21 10.30 L1.61 5.71 Z" />
        <path d="M21.33 2.67 L18.29 1.61 L13.70 5.21 L6.13 13.20 L4.29 11.36 L3.02 12.64 L6.13 15.75 L3.44 18.43 L3.09 18.08 L2.38 18.08 L1.61 18.58 L1.11 19.35 L1.11 20.06 L1.68 20.91 L2.31 21.69 L3.09 22.32 L3.94 22.89 L4.65 22.89 L5.42 22.39 L5.92 21.62 L5.92 20.91 L5.57 20.56 L8.25 17.87 L11.36 20.98 L12.64 19.71 L10.80 17.87 L18.79 10.30 L22.39 5.71 Z" />
        <path
          className="phase-ridge"
          d="M4.58 4.58 L14.97 14.97 M19.42 4.58 L9.03 14.97"
        />
      </>
    ),
  },
  {
    phase: "main2",
    label: "Second main phase",
    icon: (
      <>
        <path
          className="phase-shield"
          d="M12 3 C15 4.5 18 5 21.4 5 L21.4 11 C21.4 16.5 17 20 12 21.6 C7 20 2.6 16.5 2.6 11 L2.6 5 C6 5 9 4.5 12 3 Z"
        />
        <path
          className="phase-shield-dark"
          stroke="none"
          d="M12 4.2 C9.2 5.5 6.5 6 3.8 6.1 L3.8 11 C3.8 15.6 7.4 18.8 12 20.3 Z"
        />
      </>
    ),
    glyph: CARD,
  },
  {
    phase: "end",
    label: "End",
    icon: (
      <path
        className="phase-heavy"
        d="M9.8 5.5 A8 8 0 1 1 5 15.7 M13 2.5 L9.6 5.5 L13 8.5"
      />
    ),
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
          {i > 0 && !compact && (
            <svg className="phase-arrow" viewBox="0 0 9 11" aria-hidden>
              <path d="M0.5 0.5 L8.5 5.5 L0.5 10.5 Z" />
            </svg>
          )}
          <svg
            className="phase-icon"
            viewBox="0 0 24 24"
            role="img"
            aria-label={label}
          >
            {compact && phase === current && (
              <linearGradient id="phase-gold" x1="0" y1="0" x2="0" y2="1">
                <stop offset="0" stopColor="#ffe9a0" />
                <stop offset="0.55" stopColor="#f6b13c" />
                <stop offset="1" stopColor="#d0701c" />
              </linearGradient>
            )}
            {!compact
              ? icon
              : glyph === DIAMOND && phase === current
                ? DIAMOND_LIT
                : glyph}
          </svg>
        </li>
      ))}
    </ol>
  );
}
