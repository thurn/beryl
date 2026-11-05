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
          d="M4.5 20 L8.8 14 L10.3 15.3 L12 11 L13.7 15.3 L15.2 14 L19.5 20 Z M8.4 13.2 L10 14.6 L12 9.2 L14 14.6 L15.6 13.2"
        />
      </>
    ),
    glyph: CARD,
  },
  {
    phase: "combat",
    label: "Combat",
    icon: (
      <g transform="translate(12 11) scale(1.1) translate(-12 -12)">
        <path d="M1.5 1.5 L5 2.4 L15.2 12.6 L12.6 15.2 L2.4 5 Z M11 18.6 L18.6 11 M15.6 15.6 L19.4 19.4" />
        <path d="M22.5 1.5 L19 2.4 L8.8 12.6 L11.4 15.2 L21.6 5 Z M13 18.6 L5.4 11 M8.4 15.6 L4.6 19.4" />
      </g>
    ),
    glyph: (
      <g transform="translate(12 13) scale(1.1) translate(-12 -12)">
        <path d="M0.5 0.5 L7 2.2 L16.8 12 L12 16.8 L2.2 7 Z M9 17.6 L17.6 9 L19.8 11.2 L11.2 19.8 Z M15 17.4 L17.4 15 L20 17.6 L17.6 20 Z" />
        <circle cx="20.4" cy="20.4" r="1.9" />
        <path d="M23.5 0.5 L17 2.2 L7.2 12 L12 16.8 L21.8 7 Z M15 17.6 L6.4 9 L4.2 11.2 L12.8 19.8 Z M9 17.4 L6.6 15 L4 17.6 L6.4 20 Z" />
        <circle cx="3.6" cy="20.4" r="1.9" />
      </g>
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
              <radialGradient id="phase-glow">
                <stop offset="0" stopColor="#fffbe0" />
                <stop offset="0.35" stopColor="#ffe68f" />
                <stop offset="0.55" stopColor="#f0b040" />
                <stop offset="0.7" stopColor="#3a2810" />
                <stop offset="0.85" stopColor="#d99a3a" />
                <stop offset="1" stopColor="#f2c060" />
              </radialGradient>
            )}
            {compact ? glyph : icon}
          </svg>
        </li>
      ))}
    </ol>
  );
}
