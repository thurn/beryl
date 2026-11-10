// The choice of how to use a card, as full frames with art side by side mid-table, each sprung
// out of the card it is a use of: a card's casts (its own cost, an alternative or additional
// cost: evoke, overload, kicker), playing it as a land, and the abilities it is activated by,
// from the hand (cycling) or on the battlefield. A cast shows the card, its cost said under it;
// an ability shows its paragraph as the stack would. A use that cannot be made now is dimmed.
// The X beside the gear, Cancel in the action dock, Escape, a right-click or a click off the frames
// closes it.
import { useEffect, useLayoutEffect, useRef, type ReactNode } from "react";
import {
  animate,
  AnimatePresence,
  motion,
  useIsPresent,
  usePresenceData,
  useMotionValue,
  useReducedMotion,
} from "motion/react";
import { CARD_ASPECT, Card } from "../components/Card";
import { useOpenSound } from "../audio/SoundControls";
import { Button, Dock, DockButtons } from "../components/Controls";
import { StackItem } from "../components/Stack";
import type { Candidate, ObjectView } from "../generated/protocol";
import { useAbilityFace, useFace } from "./faces";
import { STAGE } from "./layout";
import { dissolve } from "./dissolve";
import { stageBox, type CardHandoff } from "./choiceFlight";

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
  primary?: boolean;
  /** Unset: the use cannot be made now. */
  onPick?(): void;
  action?: Candidate;
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
  id,
  object,
  modes,
  from,
  split,
  selected,
  onPick,
  onClose,
}: {
  id: number;
  object: ObjectView;
  modes: Mode[];
  from?: Origin;
  split?: boolean;
  selected?: string;
  onPick(mode: Mode, from: CardHandoff): void;
  onClose(returned?: {
    key: string;
    from: CardHandoff;
    faceName: string | null;
  }): void;
}) {
  useOpenSound();
  const present = useIsPresent();
  const handoff = usePresenceData() as { id: number; key: string } | undefined;
  const handed = handoff?.id === id ? handoff?.key : undefined;
  const active = present && selected === undefined;
  const reduce = useReducedMotion() ?? false;
  const group = useRef<HTMLDivElement>(null);
  const close = () => {
    // A dropped card lent its place to the chooser. Return one frame to the table
    // as that original and let the other projections dissolve.
    const cards = split ? modes.filter((m) => m.look.kind === "card") : [];
    const returning =
      cards.find(
        (m) =>
          m.look.kind === "card" &&
          (m.look.object ?? object).name === object.name,
      ) ?? cards[0];
    const element =
      returning &&
      group.current?.querySelector<HTMLElement>(
        `[data-mode="${returning.key}"] .ability-choice-lift`,
      );
    const stage = element?.closest<HTMLElement>(".stage");
    if (element && stage && returning?.look.kind === "card") {
      onClose({
        key: returning.key,
        from: stageBox(stage, element.getBoundingClientRect()),
        faceName: (returning.look.object ?? object).name,
      });
    } else onClose();
  };
  useEffect(() => {
    if (!active) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        close();
        return;
      }
      if (
        e.defaultPrevented ||
        e.repeat ||
        e.shiftKey ||
        (e.key !== " " && e.key !== "Enter")
      )
        return;
      if (
        e.target instanceof Element &&
        e.target.closest("button, input, select, textarea, a")
      )
        return;
      const primary = group.current?.querySelector<HTMLButtonElement>(
        "[data-primary]:enabled",
      );
      if (primary) {
        e.preventDefault();
        primary.click();
      }
    };
    const onMenu = (e: MouseEvent) => {
      e.preventDefault();
      close();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("contextmenu", onMenu);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("contextmenu", onMenu);
    };
  }, [active, close]);
  const name = object.name ?? "";
  const xs = centers(modes.length);
  return (
    <>
      <AnimatePresence propagate>
        {active && <ChooserScrim close={close} reduce={reduce} />}
      </AnimatePresence>
      <div
        ref={group}
        className="ability-chooser"
        role="group"
        aria-label={name}
        inert={!active}
      >
        <AnimatePresence custom={handed} propagate>
          {modes.map(
            (m, i) =>
              (active || (present && selected === m.key)) && (
                <Frame
                  key={m.key}
                  mode={m}
                  object={object}
                  x={xs[i]!}
                  from={from}
                  split={split}
                  selected={!active}
                  onPick={onPick}
                  delay={i * 0.04}
                />
              ),
          )}
        </AnimatePresence>
      </div>
      {active && (
        <>
          <motion.button
            type="button"
            className="icon-button chooser-close"
            aria-label="Close"
            onClick={close}
            initial={{ opacity: 0, scale: 0.6, rotate: -90 }}
            animate={{ opacity: 1, scale: 1, rotate: 0 }}
            transition={reduce ? { duration: 0 } : SPRING}
          >
            <svg viewBox="0 0 24 24" role="img" aria-hidden>
              <path d="M5.6 3.5 12 9.9l6.4-6.4 2.1 2.1-6.4 6.4 6.4 6.4-2.1 2.1-6.4-6.4-6.4 6.4-2.1-2.1 6.4-6.4-6.4-6.4z" />
            </svg>
          </motion.button>
          <Dock className="chooser-dock">
            <DockButtons>
              <Button variant="secondary" onClick={close}>
                Cancel
              </Button>
            </DockButtons>
          </Dock>
        </>
      )}
    </>
  );
}

function ChooserScrim({ close, reduce }: { close(): void; reduce: boolean }) {
  const present = useIsPresent();
  return (
    <motion.div
      className="menu-scrim chooser-scrim"
      onPointerDown={present ? close : undefined}
      style={{ pointerEvents: present ? undefined : "none" }}
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      transition={{ duration: reduce ? 0 : 0.18 }}
    />
  );
}

/** One use's frame: sprung out of the card, from its place and size to the row. */
function Frame({
  mode: m,
  object,
  x: centerX,
  from,
  split,
  selected,
  onPick,
  delay,
}: {
  mode: Mode;
  object: ObjectView;
  x: number;
  from?: Origin;
  split?: boolean;
  selected: boolean;
  onPick(mode: Mode, from: CardHandoff): void;
  delay: number;
}) {
  const reduce = useReducedMotion() ?? false;
  const present = useIsPresent();
  const handed = usePresenceData() === m.key;
  const out = from && !reduce;
  const x = useMotionValue(out ? from.x - centerX : 0);
  const y = useMotionValue(out ? from.y - MIDDLE : 0);
  const scale = useMotionValue(out ? from.width / WIDTH : 1);
  const opacity = useMotionValue(out && !split ? 0.4 : 1);
  useLayoutEffect(() => {
    if (selected || !present) return;
    const how = { ...SPRING, delay };
    const runs = [
      animate(x, 0, how),
      animate(y, 0, how),
      animate(scale, 1, how),
      animate(opacity, 1, how),
    ];
    return () => runs.forEach((r) => r.stop());
  }, [x, y, scale, opacity, delay, selected, present]);
  const height = WIDTH / CARD_ASPECT;
  return (
    <motion.button
      type="button"
      className={present && !selected ? "ability-choice" : "mode-departing"}
      data-dissolving={!present && !handed && !reduce ? "" : undefined}
      data-mode={m.key}
      data-primary={m.primary ? "" : undefined}
      disabled={!m.onPick}
      tabIndex={present && !selected ? undefined : -1}
      aria-label={m.name}
      style={{
        left: centerX - WIDTH / 2,
        top: MIDDLE - height / 2,
        width: WIDTH,
        x,
        y,
        scale,
        opacity,
        visibility: !present && handed ? "hidden" : undefined,
      }}
      exit={
        handed ? { opacity: 0, transition: { duration: 0 } } : dissolve(reduce)
      }
      onClick={(e) => {
        const button = e.currentTarget;
        const stage = button.closest<HTMLElement>(".stage");
        const lift = button.querySelector<HTMLElement>(".ability-choice-lift");
        if (!stage || !lift) return;
        const box = stageBox(stage, lift.getBoundingClientRect());
        lift.style.transform = getComputedStyle(lift).transform;
        onPick(m, box);
      }}
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
