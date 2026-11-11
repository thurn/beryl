// The chrome of the phone's game screen around its cards (DESIGN.md of the approved prototype):
// each player's piles and life orb, the menu, the phase icon in the middle, and the dock
// answering the decisions the desktop's does. The phone's board draws its cards among these
// itself (PhoneBoard.tsx); behind `?stage` the stage does (stage/PhoneStageBoard.tsx), and
// these stay as they are.
import { useLayoutEffect, useRef, useState } from "react";
import type {
  CandidateIndex,
  ObjectView,
  PlayerView,
  Side,
} from "../generated/protocol";
import { CardBack } from "../components/Card";
import {
  Button,
  FastForwardIcon,
  PhaseIcon,
  TurnButton,
} from "../components/Controls";
import { LifeOrb } from "../components/Player";
import { attackPlan, blockPlan } from "../game/combat";
import { Incoming } from "../game/Life";
import {
  follow,
  suggestedDamage,
  type interactions,
  type Intent,
} from "../game/decisions";
import { HALT_BUTTON, passButton } from "../game/stepLabels";
import { PileTop } from "./faces";
import { phaseOf, PILE, type Band, type Viewport } from "./layout";

/** `side`'s library and graveyard in their strip: an offset edge behind each pile that holds
 * anything, and with `tops` its top card, a back for the library. Without, whoever draws the
 * cards draws the tops (the stage: stage/phone.ts). */
export function Piles({
  side,
  view,
  tops,
}: {
  side: Side;
  view: PlayerView;
  tops: boolean;
}) {
  const player = view.players[side];
  const graveTop = view.objects.find((o) => o.slot === player.graveyard.at(-1));
  return (
    <div className="phone-piles">
      <div className="phone-pile">
        {(tops || player.library.length > 0) && (
          <i className="phone-pile-edge" />
        )}
        {tops && <CardBack width={PILE.width} />}
      </div>
      <div className="phone-pile">
        {graveTop && (
          <>
            <i className="phone-pile-edge" />
            {tops && <PileTop object={graveTop} width={PILE.width} />}
          </>
        )}
      </div>
    </div>
  );
}

/** `side`'s life orb, on the center line of their strip, with the combat damage they are
 * about to take when `incoming` says it. */
export function Orb({
  side,
  view,
  incoming,
}: {
  side: Side;
  view: PlayerView;
  incoming?: number;
}) {
  const { life } = view.players[side];
  return (
    <div className="phone-orb" data-life={side}>
      <LifeOrb life={life} active={view.turn.active === side} seat={side} />
      {incoming !== undefined && (
        <Incoming amount={incoming} lethal={incoming > 0 && incoming >= life} />
      )}
    </div>
  );
}

/** The menu's button, at the end of the opponent's strip: it leaves the game, until the board
 * has a menu for it to open (`onOpen`: the stage's has, stage/PhoneChrome.tsx). */
export function MenuButton({
  onLeave,
  onOpen = onLeave,
}: {
  onLeave(): void;
  onOpen?: () => void;
}) {
  return (
    <button
      type="button"
      className="icon-button phone-menu"
      aria-label="Menu"
      onPointerDown={(e) => e.stopPropagation()}
      onClick={onOpen}
    >
      <MenuIcon />
    </button>
  );
}

/** The middle: the phase icon on the gold line, beside the stack when it holds anything. */
export function Middle({ band, view }: { band: Band; view: PlayerView }) {
  const yours = view.turn.active === "you";
  return (
    <div
      className={`phone-middle${view.stack.length ? " phone-middle-stack" : ""}`}
      style={band}
    >
      <div
        className={`icon-button phone-phase${yours ? " phone-phase-yours" : ""}`}
      >
        <PhaseIcon phase={phaseOf(view.turn.step)} lit={yours} />
      </div>
    </div>
  );
}

/** The board's size, measured: the viewport inside its top safe area, and the home bar's. */
export function useViewport() {
  const ref = useRef<HTMLDivElement>(null);
  const [viewport, setViewport] = useState<Viewport>();
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const measure = () => {
      // The board starts below the top safe area, and its padding is the bottom one's.
      setViewport({
        width: el.clientWidth,
        height: el.clientHeight,
        insetBottom: parseFloat(getComputedStyle(el).paddingBottom) || 0,
      });
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  return { ref, viewport };
}

/** The dock: the turn button, as the desktop's dock labels it, and the pass-on button where
 * passing on makes sense. A declaration's button declares what is picked on the table: for
 * now nothing (picking comes with the phone's gestures). */
export function PhoneDock({
  view,
  waiting,
  can,
  objects,
  choose,
  passOn,
}: {
  view: PlayerView;
  waiting: boolean;
  can: ReturnType<typeof interactions>;
  objects: Map<number, ObjectView>;
  choose(index: CandidateIndex, then?: Intent[]): void;
  passOn(): void;
}) {
  const decision =
    view.status.kind === "deciding" ? view.status.decision : undefined;
  const declare = (plan: Intent[]) => () => {
    if (!decision) return;
    const { index, rest } = follow(decision, plan);
    if (index !== undefined) choose(index, rest);
  };
  if (!waiting && decision?.kind === "attacker") {
    return (
      <div className="phone-dock">
        <TurnButton
          label="No Attacks"
          onClick={declare(attackPlan(new Map(), objects))}
        />
      </div>
    );
  }
  if (!waiting && decision?.kind === "blocker") {
    return (
      <div className="phone-dock">
        <TurnButton
          label="No Blocks"
          onClick={declare(blockPlan(new Map(), objects))}
        />
      </div>
    );
  }
  if (!waiting && decision?.kind === "damageDivision") {
    const preferred = suggestedDamage(decision, view.stops.fullControl);
    const done = can.done;
    const primary = preferred ?? done;
    return (
      <div className="phone-dock">
        <TurnButton
          label="Assign Damage"
          disabled={primary === undefined}
          onClick={() => primary !== undefined && choose(primary)}
        />
        {preferred !== undefined && done !== undefined && (
          <Button variant="secondary" onClick={() => choose(done)}>
            Done
          </Button>
        )}
      </div>
    );
  }
  if (!waiting && decision?.kind !== "priority") return null;
  const stop = decision?.halt
    ? HALT_BUTTON
    : decision?.passTo
      ? passButton(view, decision.passTo)
      : { label: "Pass", caption: "" };
  const passesOn =
    !waiting && decision?.passOn != null && can.pass !== undefined;
  return (
    <div className="phone-dock">
      <TurnButton
        label={waiting ? "Pass" : stop.label}
        disabled={waiting || can.pass === undefined}
        onClick={() => can.pass !== undefined && choose(can.pass)}
      />
      {passesOn && decision?.passOn && (
        <button
          type="button"
          className="pass-on-button"
          aria-label={passButton(view, decision.passOn).caption}
          onClick={passOn}
        >
          <span className="pass-on-face button-primary">
            <FastForwardIcon />
          </span>
        </button>
      )}
    </div>
  );
}

function MenuIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      aria-hidden
    >
      <path d="M5 7h14M5 12h14M5 17h14" />
    </svg>
  );
}
