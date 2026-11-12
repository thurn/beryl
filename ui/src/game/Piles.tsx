// The table's piles of cards, as a player reaches for them: a library or an edge of cards lies
// where layout.ts puts it, a graveyard or exile pile is one button that opens it, never a card
// to point at alone, and the zone viewer lists every card in the pile, each shown whole on
// hover. A card the open decision offers something for (a target, a cast) is picked there.
import { useOpenSound } from "../audio/SoundControls";
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type PointerEvent,
  type ReactNode,
} from "react";
import {
  CARD_ASPECT,
  Card,
  CardBack,
  MarkBadge,
  toneClass,
  type MarkIcon,
} from "../components/Card";
import { Tip } from "../components/Controls";
import { cardEntry, useCardArt, type CardFace } from "../cards";
import { ManaSymbol } from "../components/Mana";
import type { CardType, ObjectView, RefusalView } from "../generated/protocol";
import type { CardState } from "./CardLayer";
import { BLANK, printedFace, SCAN, tone, useFace } from "./faces";
import { STAGE } from "./perspective";
import { type Box } from "./placement";
import { type ZonePile } from "./zoneViews";
import { useReducedMotion } from "motion/react";
import { BORROW_FLIGHT_MS, useBorrowedCard } from "./choiceFlight";

/** The arrange tray borrows the whole library. Measure the envelope of its skewed cards. */
export function LibraryAnchor({ side, look }: Pick<ZonePile, "side" | "look">) {
  const height = look.width / CARD_ASPECT;
  const drop = height * Math.tan((look.skew * Math.PI) / 180);
  return (
    <div
      className="hud"
      data-table-library={side}
      style={{
        left: look.x + Math.min(0, look.depth.x) + Math.min(0, drop),
        top: look.y + look.depth.y,
        width: look.width + Math.abs(look.depth.x) + Math.abs(drop),
        height: height - look.depth.y,
      }}
    />
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
  const height = look.width / CARD_ASPECT;
  const drop = height * Math.tan((look.skew * Math.PI) / 180);
  return (
    <div
      className="pile-at has-tip"
      style={
        { left: look.x, top: look.y, "--drop": `${drop}px` } as CSSProperties
      }
    >
      <button
        type="button"
        className={["pile-button", mark && `pile-button-${mark}`]
          .filter(Boolean)
          .join(" ")}
        style={{
          width: look.width,
          height,
          transformOrigin: "0 0",
          transform: `skewX(${look.skew}deg)`,
        }}
        aria-label={`${pileName(pile)}, ${cardCount(pile.count ?? cards.length)}`}
        data-pile={pile.zone}
        data-side={pile.side}
        onClick={onOpen}
      />
      <Tip side={pile.side === "you" ? "over" : "below"}>
        {pileTip(pile.zone, pile.count ?? cards.length)}
      </Tip>
    </div>
  );
}

/** A card listed in the zone viewer. */
const VIEWER_CARD = 150;
/** The most cards in a row of the viewer, leaving room for the whole card at its right. */
const VIEWER_COLUMNS = 5;
/** Matches the chrome transition in screens.css. */
const VIEWER_FADE_MS = 200;

/** Every card in a pile, the top one first, over the dimmed table, marked as on the table:
 * hovering one shows it whole, and one the open decision `offers` something for is picked with
 * a click. Escape, the
 * close button or a click beside it closes it. */
export function ZoneViewer({
  pile,
  title,
  states,
  offers,
  onPick,
  onClose,
}: {
  /** A zone's pile, or cards that lie in no one zone, which `title` names. */
  pile: ZonePile | { cards: ObjectView[] };
  /** What the viewer is called, if not the pile's name: the permanent whose exiled cards it
   * shows. */
  title?: string;
  states: Map<number, CardState>;
  offers(slot: number): boolean;
  onPick(slot: number): void;
  onClose(): void;
}) {
  useOpenSound();
  const [inspector, inspect] = useHoverInspect();
  const cards = [...pile.cards].reverse();
  const columns = Math.min(VIEWER_COLUMNS, Math.max(1, cards.length));
  const zone = "zone" in pile ? pile : undefined;
  const name = title ?? (zone ? pileName(zone) : "");
  const [list, more, onScroll] = useScrollEdges();
  const panel = useRef<HTMLDivElement>(null);
  const reduce = useReducedMotion();
  const [closing, setClosing] = useState(false);
  const closingTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const close = () => {
    if (reduce) return onClose();
    if (closingTimer.current) return;
    setClosing(true);
    closingTimer.current = setTimeout(onClose, VIEWER_FADE_MS);
  };
  useEffect(() => () => clearTimeout(closingTimer.current), []);
  useLayoutEffect(() => {
    const element = panel.current;
    if (reduce || !element) return;
    element.classList.add("zone-viewer-rising", "zone-viewer-flying");
    const rise = requestAnimationFrame(() =>
      element.classList.remove("zone-viewer-rising"),
    );
    const settle = setTimeout(
      () => element.classList.remove("zone-viewer-flying"),
      BORROW_FLIGHT_MS,
    );
    return () => {
      cancelAnimationFrame(rise);
      clearTimeout(settle);
    };
  }, [reduce]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [close]);
  return (
    <>
      <div className="viewer-scrim" onPointerDown={close} />
      <div
        ref={panel}
        className={closing ? "zone-viewer zone-viewer-going" : "zone-viewer"}
        role="dialog"
        aria-label={name}
        data-pile={zone?.zone}
        data-side={zone?.side}
      >
        <header className="zone-viewer-header">
          <h2 className="zone-viewer-title">{name}</h2>
          <span className="zone-viewer-count">
            {cardCount(cards.length)}
            {cards.length > 1 && title === undefined && " · top first"}
          </span>
          <button
            type="button"
            className="icon-button zone-viewer-close"
            aria-label="Close"
            onClick={close}
          >
            <svg viewBox="0 0 24 24" aria-hidden>
              <path d="M5.6 3.5 12 9.9l6.4-6.4 2.1 2.1-6.4 6.4 6.4 6.4-2.1 2.1-6.4-6.4-6.4 6.4-2.1-2.1 6.4-6.4-6.4-6.4z" />
            </svg>
          </button>
        </header>
        {cards.length === 0 ? (
          <p className="zone-viewer-empty">Nothing here.</p>
        ) : (
          <div
            ref={list}
            className="zone-viewer-cards"
            data-more={more}
            onScroll={onScroll}
            style={{
              gridTemplateColumns: `repeat(${columns}, ${VIEWER_CARD}px)`,
            }}
          >
            {cards.map((o) => (
              <ViewerCard
                key={o.slot}
                object={o}
                closing={closing}
                state={states.get(o.slot) ?? {}}
                offered={offers(o.slot)}
                onPick={() => onPick(o.slot)}
                hover={inspect(o)}
              />
            ))}
          </div>
        )}
      </div>
      {!closing && inspector}
    </>
  );
}

/** Which ends of a scrolling list have more beyond them: "below", "above", "both" or "none". */
type Beyond = "none" | "above" | "below" | "both";

const beyond = (e: HTMLElement): Beyond => {
  const above = e.scrollTop > 1;
  const below = e.scrollTop + e.clientHeight < e.scrollHeight - 1;
  return above ? (below ? "both" : "above") : below ? "below" : "none";
};

/** A scrolling list's ref, which of its ends have more beyond them (for a fade there, so a
 * list that overflows says so though the system hides its scroll bar), and its scroll
 * handler. */
function useScrollEdges() {
  const ref = useRef<HTMLDivElement>(null);
  const [more, setMore] = useState<Beyond>("none");
  const measure = () => {
    if (ref.current) setMore(beyond(ref.current));
  };
  useLayoutEffect(() => {
    measure();
    const el = ref.current;
    if (!el) return;
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  return [ref, more, measure] as const;
}

function ViewerCard({
  object,
  closing,
  state,
  offered,
  onPick,
  hover,
}: {
  object: ObjectView;
  closing: boolean;
  state: CardState;
  offered: boolean;
  onPick(): void;
  hover: HoverHandlers;
}) {
  const face = useFace(object);
  const ref = useRef<HTMLDivElement>(null);
  useBorrowedCard(ref, closing ? undefined : object.slot);
  return (
    <div
      ref={ref}
      className={
        offered
          ? "zone-viewer-card zone-viewer-card-offered"
          : "zone-viewer-card"
      }
      data-viewer-slot={object.slot}
      data-card={object.name ?? undefined}
      role={offered ? "button" : undefined}
      onClick={offered ? onPick : undefined}
      {...hover}
    >
      {object.name ? (
        <Card
          card={face}
          width={VIEWER_CARD}
          highlight={state.highlight}
          className={
            state.playable && !state.highlight ? "card-playable" : undefined
          }
        />
      ) : (
        <CardBack width={VIEWER_CARD} />
      )}
    </div>
  );
}

/** The pointer handlers that inspect a card while it is hovered. */
export interface HoverHandlers {
  onPointerEnter(e: PointerEvent<HTMLElement>): void;
  onPointerLeave(): void;
}

/** Hovering a card the table does not place, in an overlay over it, a pile's viewer or the
 * opening hand, shows it whole in the inspector: at `place` when given, else on the stage's
 * side away from the card, as hovering one on the table does. Returns the inspector to draw
 * (or nothing) and, for each card, the handlers for its element. A card the viewer doesn't
 * know is not inspected. */
export function useHoverInspect(
  place?: InspectPlace,
): [ReactNode, (object: ObjectView | undefined) => HoverHandlers] {
  const [shown, setShown] = useState<{ object: ObjectView; left: number }>();
  const handlers = (object: ObjectView | undefined): HoverHandlers => ({
    onPointerEnter: (e) => {
      if (!object?.name) return;
      const box = e.currentTarget.getBoundingClientRect();
      const stage = e.currentTarget.closest(".stage")?.getBoundingClientRect();
      const left =
        stage && (box.left + box.right) / 2 - stage.left < stage.width / 2
          ? STAGE.width - INSPECT_WIDTH - 24
          : 24;
      setShown({ object, left });
    },
    onPointerLeave: () =>
      setShown((s) => (s?.object.slot === object?.slot ? undefined : s)),
  });
  return [
    shown && (
      <Inspect object={shown.object} place={place ?? { left: shown.left }} />
    ),
    handlers,
  ];
}

/** The inspector's width, top and inner margin (screens.css .inspect). */
export const INSPECT_WIDTH = 330;
const INSPECT_TOP = 118;
export const INSPECT_PAD = 8;
/** The artist's line under the scan, with its margin. */
const INSPECT_ARTIST = 30;

/** The box the inspector covers at `place`, for a card without marks under its scan. */
export function inspectorBox({
  left,
  top = INSPECT_TOP,
  width = INSPECT_WIDTH,
}: InspectPlace): Box {
  const inner = width - 2 * INSPECT_PAD;
  return {
    left,
    top,
    right: left + width,
    bottom: top + inner / CARD_ASPECT + 2 * INSPECT_PAD + INSPECT_ARTIST,
  };
}

/** Where the inspector stands: its left edge, and its top and width when not the usual ones
 * (screens.css .inspect). */
export interface InspectPlace {
  left: number;
  top?: number;
  width?: number;
}

/** A card's full Scryfall scan, which carries the artist credit, large beside the table where
 * `place` puts it; outside the app, its frame stands in for the scan. On the battlefield,
 * what the scan cannot say follows it: its card types when effects have changed them, the
 * printed ones struck out, its marks spelled out, and what is `attached` to it. With a
 * paragraph `lit`, the ability an option uses, its frame shows, that paragraph marked. */
export function Inspect({
  object,
  place: { left, top, width = INSPECT_WIDTH },
  attached = [],
  lit,
  refusal,
}: {
  object?: ObjectView;
  place: InspectPlace;
  attached?: ObjectView[];
  lit?: number;
  /** Why a hand card a click found nothing to do with can't be played, with the name of the
   * card a timing refusal names. */
  refusal?: { reason: RefusalView; by?: string };
}) {
  const name = object?.name;
  const scan = useCardArt(name ?? "", undefined, SCAN);
  const face = useFace(object ?? BLANK);
  if (!object || !name) return null;
  const entry = cardEntry(name);
  const printed = cardTypes(face.typeLine);
  const changed =
    object.zone === "battlefield" &&
    face.typeLine !== "" &&
    printed.join() !== object.types.join();
  return (
    <div
      className="inspect"
      style={{ left, top, width }}
      aria-label={`${name}, inspected`}
    >
      {scan?.src && lit === undefined ? (
        <img className="inspect-scan" src={scan.src} alt={name} />
      ) : (
        <Card card={{ ...face, lit }} width={width - 2 * INSPECT_PAD} />
      )}
      {refusal && <InspectRefusal {...refusal} />}
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

/** Why a card can't be played: an icon and a few words, naming the card that holds its
 * controller to sorcery speed (by the part of its name before a comma). */
function InspectRefusal({ reason, by }: { reason: RefusalView; by?: string }) {
  const words =
    reason.kind === "timing" && by
      ? `Sorcery speed: ${by.split(",")[0]}`
      : REFUSAL_WORDS[reason.kind];
  return (
    <p className="inspect-refusal" aria-label={`Can't play: ${words}`}>
      <RefusalIcon kind={reason.kind} />
      {words}
    </p>
  );
}

const REFUSAL_WORDS: Record<RefusalView["kind"], string> = {
  timing: "Sorcery speed",
  landPlayed: "Land played",
  mana: "No mana",
  cost: "Can't pay",
  target: "No target",
};

/** A refusal's icon: the Mana font's sorcery and land symbols, a mana symbol, a reticle. */
function RefusalIcon({ kind }: { kind: RefusalView["kind"] }) {
  switch (kind) {
    case "timing":
      return <span className="refusal-glyph">{"\ue624"}</span>;
    case "landPlayed":
      return <span className="refusal-glyph">{"\ue622"}</span>;
    case "mana":
    case "cost":
      return <ManaSymbol symbol="C" />;
    case "target":
      return (
        <svg className="refusal-reticle" viewBox="0 0 16 16" aria-hidden>
          <circle cx="8" cy="8" r="5.5" />
          <path d="M8 0.5 V4.5 M8 11.5 V15.5 M0.5 8 H4.5 M11.5 8 H15.5" />
        </svg>
      );
  }
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

/** Two attachments side by side fill the inspector's width inside its margin. */
const INSPECT_ATTACHED = 152;

const MARK_NAMES: Record<MarkIcon, string> = {
  plusOne: "+1/+1",
  minusOne: "\u22121/\u22121",
  loyalty: "Loyalty",
  time: "Time",
  story: "Story",
  chorus: "Chorus",
  damage: "Damage",
  sick: "Summoning sick",
  token: "Token",
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
