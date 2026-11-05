// The table's piles of cards, as a player reaches for them: a library or an edge of cards lies
// where layout.ts puts it, a graveyard or exile pile is one button that opens it, never a card
// to point at alone, and the zone viewer lists every card in the pile, each shown whole on
// hover. A card the open decision offers something for (a target, a cast) is picked there.
import { useEffect, useState, type ReactNode } from "react";
import { ASPECT, Card } from "../components/Card";
import { cardEntry, useCardArt } from "../cards";
import type { ObjectView } from "../generated/protocol";
import type { CardState } from "./CardLayer";
import { BLANK, useFace } from "./faces";
import type { PileLook, ZonePile } from "./layout";

/** A pile lying on the table as `look` says: its top card's top-left corner at the look's
 * corner, the edges beneath on whichever side they show. */
export function Lying({
  look,
  className = "hud",
  children,
}: {
  look: PileLook;
  className?: string;
  children: ReactNode;
}) {
  return (
    <div
      className={className}
      style={{
        left: look.x - Math.max(0, -look.depth.x),
        top: look.y,
        transformOrigin: "0 0",
        transform: `skewX(${look.skew}deg)`,
      }}
    >
      {children}
    </div>
  );
}

/** What a pile is called: "Your graveyard", "Opponent's exile". */
export function pileName({ side, zone }: Pick<ZonePile, "side" | "zone">) {
  return `${side === "you" ? "Your" : "Opponent's"} ${zone}`;
}

const cardCount = (n: number) => `${n} card${n === 1 ? "" : "s"}`;

/** How a pile's cards are marked: one is a target (to pick, or of the top of the stack), one
 * has something to play, or neither. */
export function pileMark(
  pile: ZonePile,
  states: Map<number, CardState>,
): "target" | "playable" | undefined {
  const marks = pile.cards.map((o) => states.get(o.slot));
  if (marks.some((s) => s?.highlight === "target")) return "target";
  if (marks.some((s) => s?.playable)) return "playable";
  return undefined;
}

/** A pile's button, over its top card, with its zone's name over it and its count: it opens
 * the zone viewer. It glows as its cards are marked (pileMark). */
export function PileButton({
  pile,
  mark,
  onOpen,
}: {
  pile: ZonePile;
  mark?: "target" | "playable";
  onOpen(): void;
}) {
  const { look, cards } = pile;
  return (
    <Lying look={look} className="pile-at">
      <button
        type="button"
        className={["pile-button", mark && `pile-button-${mark}`]
          .filter(Boolean)
          .join(" ")}
        style={{
          marginLeft: Math.max(0, -look.depth.x),
          width: look.width,
          height: look.width / ASPECT.full,
        }}
        aria-label={`${pileName(pile)}, ${cardCount(cards.length)}`}
        data-pile={pile.zone}
        data-side={pile.side}
        onClick={onOpen}
      >
        <span className="pile-label" aria-hidden>
          {pile.zone}
        </span>
        <span className="pile-count">{cards.length}</span>
      </button>
    </Lying>
  );
}

/** A card listed in the zone viewer. */
const VIEWER_CARD = 150;
/** The most cards in a row of the viewer, leaving room for the whole card at its right. */
const VIEWER_COLUMNS = 5;

/** Every card in a pile, the top one first, over the dimmed table, marked as on the table:
 * hovering one shows it whole, and one the open decision `offers` something for is picked with
 * a click. Escape, the
 * close button or a click beside it closes it. */
export function ZoneViewer({
  pile,
  states,
  offers,
  onPick,
  onClose,
}: {
  pile: ZonePile;
  states: Map<number, CardState>;
  offers(slot: number): boolean;
  onPick(slot: number): void;
  onClose(): void;
}) {
  const [previewing, setPreviewing] = useState<number>();
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  const cards = [...pile.cards].reverse();
  const columns = Math.min(VIEWER_COLUMNS, Math.max(1, cards.length));
  const name = pileName(pile);
  return (
    <>
      <div className="viewer-scrim" onPointerDown={onClose} />
      <div
        className="zone-viewer"
        role="dialog"
        aria-label={name}
        data-pile={pile.zone}
        data-side={pile.side}
      >
        <header className="zone-viewer-header">
          <h2 className="zone-viewer-title">{name}</h2>
          <span className="zone-viewer-count">
            {cardCount(cards.length)}
            {cards.length > 1 && " · top first"}
          </span>
          <button
            type="button"
            className="zone-viewer-close"
            aria-label="Close"
            onClick={onClose}
          >
            ×
          </button>
        </header>
        {cards.length === 0 ? (
          <p className="zone-viewer-empty">Nothing here.</p>
        ) : (
          <div
            className="zone-viewer-cards"
            style={{
              gridTemplateColumns: `repeat(${columns}, ${VIEWER_CARD}px)`,
            }}
          >
            {cards.map((o) => (
              <ViewerCard
                key={o.slot}
                object={o}
                state={states.get(o.slot) ?? {}}
                offered={offers(o.slot)}
                onPick={() => onPick(o.slot)}
                onHover={(on) =>
                  setPreviewing((p) =>
                    on ? o.slot : p === o.slot ? undefined : p,
                  )
                }
              />
            ))}
          </div>
        )}
      </div>
      {previewing !== undefined && (
        <Inspect
          object={cards.find((o) => o.slot === previewing)}
          left={1600 - 330 - 24}
        />
      )}
    </>
  );
}

function ViewerCard({
  object,
  state,
  offered,
  onPick,
  onHover,
}: {
  object: ObjectView;
  state: CardState;
  offered: boolean;
  onPick(): void;
  onHover(on: boolean): void;
}) {
  const face = useFace(object);
  return (
    <div
      className={
        offered
          ? "zone-viewer-card zone-viewer-card-offered"
          : "zone-viewer-card"
      }
      data-viewer-slot={object.slot}
      data-card={object.name ?? undefined}
      role={offered ? "button" : undefined}
      onClick={offered ? onPick : undefined}
      onPointerEnter={() => onHover(true)}
      onPointerLeave={() => onHover(false)}
    >
      <Card
        card={face}
        width={VIEWER_CARD}
        highlight={state.highlight}
        className={
          state.playable && !state.highlight ? "card-playable" : undefined
        }
      />
    </div>
  );
}

/** A card's full Scryfall scan, which carries the artist credit, large beside the table, its
 * left edge at `left`; outside the app, its frame stands in for the scan. */
export function Inspect({
  object,
  left,
}: {
  object?: ObjectView;
  left: number;
}) {
  const name = object?.name;
  const scan = useCardArt(name ?? "", undefined, "normal");
  const face = useFace(object ?? BLANK);
  if (!object || !name) return null;
  const entry = cardEntry(name);
  return (
    <div className="inspect" style={{ left }} aria-label={`${name}, inspected`}>
      {scan?.src ? (
        <img className="inspect-scan" src={scan.src} alt={name} />
      ) : (
        <Card card={face} width={330} />
      )}
      {entry && <p className="inspect-artist">Illustrated by {entry.artist}</p>}
    </div>
  );
}
