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

const DIAMOND = <path d="M12 5 L19 12 L12 19 L5 12 Z" />;
const CARD = (
  <path d="M5.5 3.2 Q12 2.6 18.6 3.4 Q19.2 12 18.8 20.6 Q12 21.2 5.7 20.8 Q5 12 5.5 3.2 Z" />
);

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
          transform="rotate(-14 7 13.25)"
        />
        <rect
          className="phase-dark"
          x="5"
          y="3.5"
          width="11"
          height="16.5"
          rx="1.2"
          transform="rotate(-6 10.5 11.75)"
        />
        <rect
          className="phase-card"
          x="10"
          y="2"
          width="11.5"
          height="17"
          rx="1.2"
          transform="rotate(5 15.75 10.5)"
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
          d="M0.5 21.5 L8.5 9 L10.5 11 L12 2.8 L15.2 9 L17.5 7.5 L23.5 21.5 Z"
        />
        <path
          className="phase-stone"
          strokeWidth="1"
          d="M4.5 20 L9.3 13.3 L11 14.8 L12.2 10 L14.2 13.8 L16.3 12.3 L19.8 20 Z"
        />
      </>
    ),
    glyph: CARD,
  },
  {
    phase: "combat",
    label: "Combat",
    icon: (
      <g
        className="phase-heavy"
        transform="translate(12 12) scale(1.22) translate(-12 -12)"
      >
        <path d="M2.5 2.5 L15 15 M11.5 18.5 L18.5 11.5 M15.5 15.5 L19.5 19.5 M21 21 L21.2 21.2" />
        <path d="M21.5 2.5 L9 15 M12.5 18.5 L5.5 11.5 M8.5 15.5 L4.5 19.5 M3 21 L2.8 21.2" />
      </g>
    ),
    glyph: (
      <>
        <path d="M1.5 1.5 L6 2.8 L15.6 12.4 L12.4 15.6 L2.8 6 Z M10 17 L17 10 L19 12 L12 19 Z M15 17.2 L17.2 15 L20 17.8 L17.8 20 Z M18.8 20.5 L20.5 18.8 L22.6 20.9 L20.9 22.6 Z" />
        <path d="M22.5 1.5 L18 2.8 L8.4 12.4 L11.6 15.6 L21.2 6 Z M14 17 L7 10 L5 12 L12 19 Z M9 17.2 L6.8 15 L4 17.8 L6.2 20 Z M5.2 20.5 L3.5 18.8 L1.4 20.9 L3.1 22.6 Z" />
      </>
    ),
  },
  {
    phase: "main2",
    label: "Second main phase",
    icon: (
      <>
        <path
          className="phase-dark"
          d="M12 3 C15 4.5 18 5 21.4 5 L21.4 11 C21.4 16.5 17 20 12 21.6 C7 20 2.6 16.5 2.6 11 L2.6 5 C6 5 9 4.5 12 3 Z"
        />
        <path
          className="phase-light"
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
                <stop offset="0.55" stopColor="#ffd77a" />
                <stop offset="1" stopColor="#eea33c" />
              </radialGradient>
            )}
            {compact ? glyph : icon}
          </svg>
        </li>
      ))}
    </ol>
  );
}
