// Actions and status: buttons, the priority pill, the phase tracker and hover tips.
import type { ButtonHTMLAttributes, ReactNode, Ref } from "react";
import type { Step } from "../generated/protocol";
import { useTextFit } from "./fit";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "primary" | "secondary";
  icon?: ReactNode;
  size?: "regular" | "small";
  /** The label's element, for fitting it to the button. */
  labelRef?: Ref<HTMLSpanElement>;
}

/** Primary (ember), secondary (outlined) or, with `disabled`, the muted disabled button. */
export function Button({
  variant = "primary",
  icon,
  size = "regular",
  children,
  className,
  labelRef,
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
      <span ref={labelRef}>{children}</span>
    </button>
  );
}

/** The button that advances the turn: one size and type style for every label and state, a long
 * label shrinking to fit (screens.css). */
export function TurnButton({
  label,
  ...rest
}: { label: string } & ButtonHTMLAttributes<HTMLButtonElement>) {
  const labelRef = useTextFit(label);
  return (
    <Button icon={<FastForwardIcon />} labelRef={labelRef} {...rest}>
      {label}
    </Button>
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

/** Who acts now, after a sparkle: a long line shrinks to fit a pill held narrow (fit.ts). */
export function PriorityPill({ children }: { children: string }) {
  const ref = useTextFit(children);
  return (
    <div className="priority-pill" role="status">
      <Sparkle />
      <span ref={ref}>{children}</span>
    </div>
  );
}

export type Phase = "beginning" | "main1" | "combat" | "main2" | "end";

/** A main phase's peak: the first main phase's icon, and the front of the second's. */
const PEAK = (
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
);

/** One sword, its point up and to the right; crossed with its mirror image for combat. */
const SWORD = (
  <>
    <path
      className="phase-blade"
      d="M22.32 1.68 L20.41 2.03 L7.97 14.47 L8.11 14.62 L4.58 18.15 L4.15 17.73 L2.60 19.28 L4.72 21.40 L6.27 19.85 L5.85 19.42 L9.38 15.89 L9.53 16.03 L21.97 3.59 Z"
    />
    <path d="M19.78 4.22 L10.23 13.77 M5.92 12.14 L11.86 18.08" />
  </>
);

/** Combat damage: the inspector's damage burst, at the tracker's scale. */
const BURST = (
  <path
    className="phase-dark"
    strokeWidth="1.1"
    transform="scale(1.5)"
    d="M8 0.5 L9.8 5.2 L15 4 L11.4 8 L15 12 L9.8 10.8 L8 15.5 L6.2 10.8 L1 12 L4.6 8 L1 4 L6.2 5.2 Z"
  />
);

/** Each step of combat's icon, where the tracker tells them apart: crossed swords as combat
 * begins, one sword raised for attackers, a shield for blockers, a burst for damage and the
 * sword lowered as combat ends. */
const COMBAT_STEPS: Partial<Record<Step, ReactNode>> = {
  declareAttackers: SWORD,
  declareBlockers: (
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
  firstStrikeDamage: BURST,
  combatDamage: BURST,
  endOfCombat: <g transform="rotate(180 12 12)">{SWORD}</g>,
};

/** Each phase's icon, cream outlines over dark or stone fills, drawn by both trackers. */
const PHASES: { phase: Phase; label: string; icon: ReactNode }[] = [
  {
    phase: "beginning",
    label: "Upkeep",
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
  },
  { phase: "main1", label: "First main phase", icon: PEAK },
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
        {SWORD}
      </>
    ),
  },
  {
    // The first main phase's peak before a second one, the part of it in view only.
    phase: "main2",
    label: "Second main phase",
    icon: (
      <>
        <path
          className="phase-dark"
          d="M0.5 21.5 L5.5 7 L8.17 14.81 L3.9 21.7 Z"
        />
        <g transform="translate(3.5 3.4) scale(0.85)">{PEAK}</g>
      </>
    ),
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
  },
];

/** The turn's phases, the current one lit gold. The compact variant sits on the table above
 * the pass button: smaller, with no pill or arrows, and a gold rule under the current phase.
 * Given `stops`, it shows a turn's phase stops, each phase with one lit and pipped, the others
 * dimmed, and a click on a phase sets or clears its stop (`onStop`). */
export function PhaseTracker({
  current,
  step,
  compact = false,
  now,
  stops,
  onStop,
  whose,
  className,
}: {
  current?: Phase;
  /** The current step, which picks combat's icon among its steps' (COMBAT_STEPS). */
  step?: Step;
  compact?: boolean;
  /** What the current phase's tip says, such as "Turn 9 · Blockers"; its label otherwise. */
  now?: string;
  /** The phases with a stop. */
  stops?: Set<Phase>;
  onStop?: (phase: Phase) => void;
  /** Whose turn the row stands for, before each phase's label in its tip. */
  whose?: string;
  className?: string;
}) {
  return (
    <ol
      className={[
        "phase-tracker",
        compact && "phase-tracker-compact",
        stops && "phase-tracker-stops",
        className,
      ]
        .filter(Boolean)
        .join(" ")}
    >
      {PHASES.map(({ phase, label, icon }, i) => {
        const glyph = (
          <svg
            className="phase-icon"
            viewBox="0 0 24 24"
            role="img"
            aria-label={label}
          >
            {(phase === current && step && COMBAT_STEPS[step]) || icon}
          </svg>
        );
        const stopped = stops?.has(phase);
        return (
          <li
            key={phase}
            className={[
              "phase has-tip",
              phase === current && "phase-current",
              stopped === true && "phase-stop",
              stopped === false && "phase-no-stop",
            ]
              .filter(Boolean)
              .join(" ")}
          >
            <Tip side="above">
              {(phase === current && now) ||
                (whose ? `${whose} ${label.toLowerCase()}` : label)}
            </Tip>
            {i > 0 && !compact && (
              <svg className="phase-arrow" viewBox="0 0 9 11" aria-hidden>
                <path d="M0.5 0.5 L8.5 5.5 L0.5 10.5 Z" />
              </svg>
            )}
            {onStop ? (
              <button
                type="button"
                className="phase-toggle"
                aria-pressed={stopped}
                onPointerDown={(e) => e.stopPropagation()}
                onClick={() => onStop(phase)}
              >
                {glyph}
              </button>
            ) : (
              glyph
            )}
          </li>
        );
      })}
    </ol>
  );
}

/** A few words on hover or focus of its parent, which must be `.has-tip`: what the table shows
 * without words, such as a pile's size, said only when asked. */
export function Tip({
  side,
  children,
}: {
  /** Where it opens: toward the middle of the table from the parent's edge of it, or over the
   * parent's middle where nothing beside it is free (your piles, under your badge). */
  side: "above" | "below" | "over";
  children: ReactNode;
}) {
  return (
    <span className={`tip tip-${side}`} role="tooltip">
      {children}
    </span>
  );
}
