// The choice of how to use a card, as full frames with art side by side mid-table, each sprung
// out of the card it is a use of: a card's casts (its own cost, an alternative or additional
// cost: evoke, overload, kicker), playing it as a land, and the abilities it is activated by,
// from the hand (cycling) or on the battlefield. A cast shows the card, its cost said under it;
// an ability shows its paragraph as the stack would. A use that cannot be made now is dimmed.
// The X beside the gear, Cancel in the action dock, Escape, a right-click or a click off the frames
// closes it.
import { useEffect, type ReactNode } from "react";
import {
  animate,
  motion,
  useMotionValue,
  useReducedMotion,
} from "motion/react";
import { CARD_ASPECT, Card } from "../components/Card";
import { Button, Dock, DockButtons } from "../components/Controls";
import { StackItem } from "../components/Stack";
import type { ObjectView } from "../generated/protocol";
import { useAbilityFace, useFace } from "./faces";
import { STAGE } from "./layout";

/** One use of the card. */
export interface Mode {
  key: string;
  /** The card's own frame (a cast, a land played: by the face it is played by, `object`), or
   * an ability's paragraph framed as the stack frames it, under its label ("Activated
   * Ability"). */
  look:
    | { kind: "card"; object?: ObjectView }
    | { kind: "ability"; label: string; text: string };
  /** What the use costs or is, under the frame: "Cast {2}{U}{U}", "Evoke". */
  caption?: ReactNode;
  /** Its name, for a screen reader and a test. */
  name: string;
  /** Unset: the use cannot be made now. */
  onPick?(): void;
}

/** Where the card the uses spring from lies on the stage: its center and its width. */
export interface Origin {
  x: number;
  y: number;
  width: number;
}

/** The frames' width on the stage, and the room between them. */
const WIDTH = 250;
const GAP = 40;
/** The frames' center line, above the middle of the table, clear of the hand. */
const MIDDLE = 405;
const SPRING = { type: "spring", stiffness: 300, damping: 28 } as const;

/** Each of `count` frames' center, in a row across the stage's middle. */
function centers(count: number): number[] {
  const span = count * WIDTH + (count - 1) * GAP;
  const left = (STAGE.width - span) / 2 + WIDTH / 2;
  return Array.from({ length: count }, (_, i) => left + i * (WIDTH + GAP));
}

export function ModeChooser({
  object,
  modes,
  from,
  onClose,
}: {
  object: ObjectView;
  modes: Mode[];
  from?: Origin;
  onClose(): void;
}) {
  const reduce = useReducedMotion() ?? false;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      onClose();
    };
    const onMenu = (e: MouseEvent) => {
      e.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("contextmenu", onMenu);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("contextmenu", onMenu);
    };
  }, [onClose]);
  const name = object.name ?? "";
  const xs = centers(modes.length);
  return (
    <>
      <motion.div
        className="menu-scrim chooser-scrim"
        onPointerDown={onClose}
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ duration: reduce ? 0 : 0.18 }}
      />
      <div className="ability-chooser" role="group" aria-label={name}>
        {modes.map((m, i) => (
          <Frame
            key={m.key}
            mode={m}
            object={object}
            x={xs[i]!}
            from={from}
            delay={i * 0.04}
          />
        ))}
      </div>
      <button
        type="button"
        className="chooser-close"
        aria-label="Close"
        onClick={onClose}
      >
        <svg viewBox="0 0 24 24" role="img" aria-hidden>
          <path d="M5.6 3.5 12 9.9l6.4-6.4 2.1 2.1-6.4 6.4 6.4 6.4-2.1 2.1-6.4-6.4-6.4 6.4-2.1-2.1 6.4-6.4-6.4-6.4z" />
        </svg>
      </button>
      <Dock className="chooser-dock">
        <DockButtons>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
        </DockButtons>
      </Dock>
    </>
  );
}

/** One use's frame: sprung out of the card, from its place and size to the row. */
function Frame({
  mode: m,
  object,
  x: centerX,
  from,
  delay,
}: {
  mode: Mode;
  object: ObjectView;
  x: number;
  from?: Origin;
  delay: number;
}) {
  const reduce = useReducedMotion() ?? false;
  const out = from && !reduce;
  const x = useMotionValue(out ? from.x - centerX : 0);
  const y = useMotionValue(out ? from.y - MIDDLE : 0);
  const scale = useMotionValue(out ? from.width / WIDTH : 1);
  const opacity = useMotionValue(out ? 0.4 : 1);
  useEffect(() => {
    const how = { ...SPRING, delay };
    const runs = [
      animate(x, 0, how),
      animate(y, 0, how),
      animate(scale, 1, how),
      animate(opacity, 1, how),
    ];
    return () => runs.forEach((r) => r.stop());
  }, [x, y, scale, opacity, delay]);
  const height = WIDTH / CARD_ASPECT;
  return (
    <motion.button
      type="button"
      className="ability-choice"
      data-mode={m.key}
      disabled={!m.onPick}
      aria-label={m.name}
      style={{
        left: centerX - WIDTH / 2,
        top: MIDDLE - height / 2,
        width: WIDTH,
        x,
        y,
        scale,
        opacity,
      }}
      onClick={() => m.onPick?.()}
    >
      <span className="ability-choice-lift">
        {m.look.kind === "card" ? (
          <CardFrame object={m.look.object ?? object} />
        ) : (
          <AbilityFrame
            object={object}
            label={m.look.label}
            text={m.look.text}
          />
        )}
        {m.caption && (
          <span className="ability-choice-caption">{m.caption}</span>
        )}
      </span>
    </motion.button>
  );
}

function CardFrame({ object }: { object: ObjectView }) {
  return <Card card={useFace(object)} width={WIDTH} />;
}

function AbilityFrame({
  object,
  label,
  text,
}: {
  object: ObjectView;
  label: string;
  text: string;
}) {
  const face = useAbilityFace(object.name ?? "", object, object);
  return (
    <StackItem source={face} label={label} text={text} width={WIDTH} showArt />
  );
}
