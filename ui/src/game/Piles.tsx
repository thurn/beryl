// The table's piles of cards, as a player reaches for them: a library or an edge of cards lies
// where layout.ts puts it, a graveyard or exile pile is one button that opens it, never a card
// to point at alone, and the zone viewer lists every card in the pile, each shown whole on
// hover. A card the open decision offers something for (a target, a cast) is picked there.
import { useEffect, useState, type CSSProperties, type ReactNode } from "react";
import {
  ASPECT,
  Card,
  MarkBadge,
  toneClass,
  type MarkIcon,
} from "../components/Card";
import { Tip } from "../components/Controls";
import { cardEntry, useCardArt, type CardFace } from "../cards";
import type { CardType, ObjectView } from "../generated/protocol";
import type { CardState } from "./CardLayer";
import { BLANK, printedFace, tone, useFace } from "./faces";
import type { PileLook, ZonePile } from "./layout";

/** A pile lying on the table as `look` says: its top card's top-left corner at the look's
 * corner, the edges beneath on whichever side they show. Its `tip` (Controls.tsx' Tip), shown
 * on hover, stands upright beside the leaning pile: from its top edge, which stays put, or
 * from its foot, which the lean moves sideways by `--drop`. */
export function Lying({
  look,
  className = "hud",
  tip,
  children,
}: {
  look: PileLook;
  className?: string;
  tip?: ReactNode;
  children: ReactNode;
}) {
  const at = { left: look.x - Math.max(0, -look.depth.x), top: look.y };
  const lean = {
    transformOrigin: "0 0",
    transform: `skewX(${look.skew}deg)`,
  };
  if (!tip) {
    return (
      <div className={className} style={{ ...at, ...lean }}>
        {children}
      </div>
    );
  }
  const height = look.width / ASPECT.full + Math.max(0, look.depth.y);
  const drop = height * Math.tan((look.skew * Math.PI) / 180);
  return (
    <div
      className={`${className} has-tip`}
      style={{ ...at, "--drop": `${drop}px` } as CSSProperties}
    >
      <div style={lean}>{children}</div>
      {tip}
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

/** What a pile's tip says on hover: its zone and how many cards, "Graveyard · 5". */
export const pileTip = (zone: string, count: number) =>
  `${zone[0]!.toUpperCase()}${zone.slice(1)} · ${count}`;

/** A pile's button, over its top card: it opens the zone viewer. Its thickness shows its size
 * at a glance; hovering it names its zone and count. It glows as its cards are marked
 * (pileMark). */
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
    <Lying
      look={look}
      className="pile-at"
      tip={
        <Tip side={pile.side === "you" ? "above" : "below"}>
          {pileTip(pile.zone, cards.length)}
        </Tip>
      }
    >
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
      />
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
 * left edge at `left`; outside the app, its frame stands in for the scan. On the battlefield,
 * what the scan cannot say follows it: its card types when effects have changed them, the
 * printed ones struck out, its marks spelled out, and what is `attached` to it. With a
 * paragraph `lit`, the ability an option uses, its frame shows, that paragraph marked. */
export function Inspect({
  object,
  left,
  attached = [],
  lit,
}: {
  object?: ObjectView;
  left: number;
  attached?: ObjectView[];
  lit?: number;
}) {
  const name = object?.name;
  const scan = useCardArt(name ?? "", undefined, "normal");
  const face = useFace(object ?? BLANK);
  if (!object || !name) return null;
  const entry = cardEntry(name);
  const printed = cardTypes(face.typeLine);
  const changed =
    object.zone === "battlefield" &&
    face.typeLine !== "" &&
    printed.join() !== object.types.join();
  return (
    <div className="inspect" style={{ left }} aria-label={`${name}, inspected`}>
      {scan?.src && lit === undefined ? (
        <img className="inspect-scan" src={scan.src} alt={name} />
      ) : (
        <Card card={{ ...face, lit }} width={330} />
      )}
      {entry && <p className="inspect-artist">Illustrated by {entry.artist}</p>}
      {changed && (
        <p className="inspect-types" aria-label="Types now">
          <s>{printed.map(titled).join(" ")}</s>{" "}
          {object.types.map(titled).join(" ")}
        </p>
      )}
      <InspectMarks
        object={object}
        printed={scan && printedFace(object, scan.face)}
      />
      {attached.length > 0 && (
        <div className="inspect-attached" aria-label="Attached">
          {attached.map((o) => (
            <InspectAttached key={o.slot} object={o} />
          ))}
        </div>
      )}
    </div>
  );
}

/** The card types a type line names, in the protocol's order. */
function cardTypes(typeLine: string): CardType[] {
  const words = new Set(
    (typeLine.split("—")[0] ?? "").toLowerCase().split(/\s+/),
  );
  return CARD_TYPES.filter((t) => words.has(t));
}

const CARD_TYPES: CardType[] = [
  "artifact",
  "creature",
  "enchantment",
  "instant",
  "land",
  "planeswalker",
  "sorcery",
];

const titled = (t: string) => t[0]!.toUpperCase() + t.slice(1);

/** An Aura or Equipment beneath the permanent it is attached to in the inspector. */
function InspectAttached({ object }: { object: ObjectView }) {
  const face = useFace(object);
  return (
    <div data-attached={object.slot}>
      <Card card={face} variant="field" width={INSPECT_ATTACHED} />
    </div>
  );
}

/** Two attachments side by side fill the inspector's width. */
const INSPECT_ATTACHED = 160;

const MARK_NAMES: Record<MarkIcon, string> = {
  plusOne: "+1/+1",
  minusOne: "\u22121/\u22121",
  loyalty: "Loyalty",
  time: "Time",
  story: "Story",
  chorus: "Chorus",
  damage: "Damage",
  sick: "Summoning sick",
};

/** A permanent's marks, which the scan does not show: its owner when another player controls
 * it, its power and toughness against the printed ones when they differ, each kind of counter,
 * marked damage and summoning sickness. */
function InspectMarks({
  object,
  printed,
}: {
  object: ObjectView;
  printed?: CardFace;
}) {
  if (object.zone !== "battlefield") return null;
  const { pt, counters, damage, summoningSick } = object;
  const tones =
    pt && printed
      ? [tone(pt.power, printed.power), tone(pt.toughness, printed.toughness)]
      : [];
  const marks: { icon: MarkIcon; count?: number }[] = [
    ...counters.map((c) => ({ icon: c.kind, count: c.count })),
    ...(damage > 0 ? [{ icon: "damage" as const, count: damage }] : []),
    ...(summoningSick ? [{ icon: "sick" as const }] : []),
  ];
  const changed = pt && printed && tones.some(Boolean);
  const stolen = object.owner !== object.controller;
  if (!changed && !stolen && marks.length === 0) return null;
  return (
    <ul className="inspect-marks">
      {stolen && (
        <li className="inspect-owner">
          Owner: {object.owner === "you" ? "You" : "Opponent"}
        </li>
      )}
      {changed && (
        <li className="inspect-pt">
          <s>
            {printed.power}/{printed.toughness}
          </s>
          {" \u2192 "}
          <b className={toneClass(tones[0])}>{pt.power}</b>/
          <b className={toneClass(tones[1])}>{pt.toughness}</b>
        </li>
      )}
      {marks.map(({ icon, count }) => (
        <li key={icon}>
          <MarkBadge icon={icon} count={count} />
          {MARK_NAMES[icon]}
        </li>
      ))}
    </ul>
  );
}
