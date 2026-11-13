// The chrome of the phone's game screen around its cards (DESIGN.md of the approved prototype):
// each player's piles and life orb, the menu, the phase icon in the middle, and the dock
// answering the decisions the desktop's does. The stage draws the cards among these
// (stage/PhoneStageBoard.tsx).
import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import type { PlayerView, Side } from "../generated/protocol";
import { CardBack } from "../components/Card";
import { PhaseIcon } from "../components/Controls";
import { LifeOrb } from "../components/Player";
import { Incoming, LifeChange } from "../game/Life";
import { zonePiles } from "../game/zoneViews";
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
  const exile = zonePiles(view).find(
    (pile) => pile.side === side && pile.zone === "exile",
  );
  return (
    <div className="phone-piles">
      <div className="phone-pile">
        {(tops || player.library.length > 0) && (
          <i className="phone-pile-edge" />
        )}
        {tops && <CardBack width={PILE.width} />}
      </div>
      <div className="phone-pile">
        {(graveTop || exile?.cards.length) && (
          <>
            <i className="phone-pile-edge" />
            {tops && graveTop && (
              <PileTop object={graveTop} width={PILE.width} />
            )}
          </>
        )}
      </div>
    </div>
  );
}

/** `side`'s life orb, on the center line of their strip, with the combat damage they are
 * about to take when `incoming` says it, and by how much their total is changing while
 * `change` says it. */
export function Orb({
  side,
  view,
  incoming,
  change,
  children,
}: {
  side: Side;
  view: PlayerView;
  incoming?: number;
  change?: number;
  /** What lies over the orb: where a prompt takes a touch on the player (the stage's). */
  children?: ReactNode;
}) {
  const { life } = view.players[side];
  return (
    <div className="phone-orb" data-life={side}>
      <LifeOrb life={life} active={view.turn.active === side} seat={side} />
      {incoming !== undefined && (
        <Incoming amount={incoming} lethal={incoming > 0 && incoming >= life} />
      )}
      {change !== undefined && <LifeChange by={change} />}
      {children}
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
