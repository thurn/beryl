// Actions and status: buttons, the priority pill, the phase tracker and hover tips.
import type { ButtonHTMLAttributes, ReactNode, Ref } from "react";
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

/** The action dock (decision: every action on the game screen in one place): a column of
 * buttons in the table's bottom-right corner, built up from where the turn button stands, the
 * question they answer briefly over them. Every button in it is one size, the primary at the
 * foot and the others stacked over it (screens.css .dock). It eases in as it appears. */
export function Dock({
  title,
  className,
  children,
}: {
  /** The question the buttons answer, in a few words. */
  title?: ReactNode;
  className?: string;
  children?: ReactNode;
}) {
  return (
    <div className={["dock", className].filter(Boolean).join(" ")}>
      {title !== undefined && <h2 className="prompt-title">{title}</h2>}
      {children}
    </div>
  );
}

/** A dock's buttons, the first (the primary, if any) at the foot and the rest stacked over it,
 * all as wide as the widest. */
export function DockButtons({
  className,
  children,
}: {
  className?: string;
  children: ReactNode;
}) {
  return (
    <div className={["dock-buttons", className].filter(Boolean).join(" ")}>
      {children}
    </div>
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
    <Button labelRef={labelRef} {...rest}>
      {label}
    </Button>
  );
}

/** Passing on to the next stop (Board.tsx' pass-on button). */
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

/** The phase icons are one set: each drawn in a 24-unit box to one optical size, its ink
 * spanning about 18 units both ways about the box's middle (3 to 21), in cream outlines of one
 * weight over dark or stone fills, so that a row of them reads evenly spaced. */

/** A main phase's peak, a snow line under its summit: the first main phase's icon. */
const PEAK = (
  <>
    <path className="phase-dark" d="M3 19.5 L12 4.5 L21 19.5 Z" />
    <path
      className="phase-stone"
      stroke="none"
      d="M6.6 18 L12 9.3 L17.4 18 Z"
    />
    <path
      strokeWidth="1.1"
      d="M8.6 10.4 L10.4 11.8 L12 9.6 L13.6 11.8 L15.4 10.4"
    />
  </>
);

/** One sword, its point up and to the right; crossed with its mirror image for combat. */
const SWORD = (
  <>
    <path
      className="phase-blade"
      d="M20.88 3.12 L19.23 3.43 L8.53 14.12 L8.65 14.25 L5.62 17.29 L5.25 16.93 L3.92 18.26 L5.74 20.08 L7.07 18.75 L6.71 18.38 L9.75 15.35 L9.88 15.47 L20.57 4.77 Z"
    />
    <path d="M18.69 5.31 L10.48 13.52 M6.77 12.12 L11.88 17.23" />
  </>
);

/** Each phase's icon (one set, as above): two cards for the beginning,
 * a peak for the first main phase, crossed swords for combat, a second peak before the first
 * for the second main phase, and the sun setting for the end. */
const PHASES: { phase: Phase; label: string; icon: ReactNode }[] = [
  {
    phase: "beginning",
    label: "Upkeep",
    icon: (
      <>
        <rect
          className="phase-dark"
          x="4.5"
          y="5.5"
          width="10"
          height="14"
          rx="1.4"
          transform="rotate(-14 9.5 12.5)"
        />
        <rect
          className="phase-card"
          x="9.5"
          y="4.5"
          width="10"
          height="14"
          rx="1.4"
          transform="rotate(10 14.5 11.5)"
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
          d="M3.12 3.12 L4.77 3.43 L15.47 14.12 L15.35 14.25 L18.38 17.29 L18.75 16.93 L20.08 18.26 L18.26 20.08 L16.93 18.75 L17.29 18.38 L14.25 15.35 L14.12 15.47 L3.43 4.77 Z"
        />
        <path d="M5.31 5.31 L13.52 13.52 M17.23 12.12 L12.12 17.23" />
        {SWORD}
      </>
    ),
  },
  {
    // The first main phase's peak before a second, lower one behind it.
    phase: "main2",
    label: "Second main phase",
    icon: (
      <>
        <path className="phase-dark" d="M3 18.5 L8.5 9 L14 18.5 Z" />
        <path className="phase-dark" d="M8 18.5 L14.5 6 L21 18.5 Z" />
        <path
          className="phase-stone"
          stroke="none"
          d="M11.2 17 L14.5 10.6 L17.8 17 Z"
        />
        <path
          strokeWidth="1.1"
          d="M11.9 10.9 L13.3 12 L14.5 10.3 L15.7 12 L17.1 10.9"
        />
      </>
    ),
  },
  {
    phase: "end",
    label: "End",
    icon: (
      <>
        <path className="phase-dark" d="M5.5 17.5 A6.5 6.5 0 0 1 18.5 17.5 Z" />
        <path d="M3 17.5 H21 M12 4.5 V7 M5.6 7.6 L7.3 9.3 M18.4 7.6 L16.7 9.3 M7 20.5 H17" />
      </>
    ),
  },
];

/** The turn's phases between small arrows in a dark pill, the current one lit gold. (On the
 * table, the step stops about each life orb stand for it: Steps.tsx.) */
export function PhaseTracker({
  current,
  className,
}: {
  current?: Phase;
  className?: string;
}) {
  return (
    <ol className={["phase-tracker", className].filter(Boolean).join(" ")}>
      {PHASES.map(({ phase, label, icon }, i) => (
        <li
          key={phase}
          className={phase === current ? "phase phase-current" : "phase"}
        >
          {i > 0 && (
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
            {icon}
          </svg>
        </li>
      ))}
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
  side: "above" | "below" | "over" | "left";
  children: ReactNode;
}) {
  return (
    <span className={`tip tip-${side}`} role="tooltip">
      {children}
    </span>
  );
}
