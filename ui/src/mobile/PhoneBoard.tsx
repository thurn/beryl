// The game screen on a portrait phone (DESIGN.md of the approved prototype): the cards placed
// by mobile/layout.ts as full scans, only live state drawn over them in the desktop's badges,
// the desktop's life orbs, piles in each player's strip, the phase icon and the stack in the
// middle, and the dock answering the same decisions the desktop's does. No names, captions,
// counts or turn number: a card's scan says what it is, and a touch will say the rest.
import {
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import type {
  CandidateIndex,
  ObjectView,
  PlayerView,
  Side,
  StackItem,
} from "../generated/protocol";
import {
  Card,
  CardBack,
  Loyalty,
  type CardFace,
  type CardProps,
  type Highlight,
} from "../components/Card";
import { FastForwardIcon, PhaseIcon, TurnButton } from "../components/Controls";
import { LifeOrb } from "../components/Player";
import { attackPlan, blockPlan } from "../game/combat";
import { follow, interactions, tapsOnly, type Intent } from "../game/decisions";
import { faceDownFace, marksOf, SCAN, useFace } from "../game/faces";
import { HALT_BUTTON, passButton } from "../game/layout";
import { useCardArt } from "../cards";
import {
  phaseOf,
  phoneLayout,
  type Spot,
  type StackSpot,
  type Viewport,
} from "./layout";
import "./phone.css";

/** A card's glow: gold or cyan for what the stack names, green for what can be played. */
interface Glow {
  highlight?: Highlight;
  playable?: boolean;
}

/** The em of the badges on a card `width` wide (phone.css `--badge`): a loyalty or a face-down
 * card's P/T numeral stays at least 12px however small the scan. */
const badgeEm = (width: number) =>
  `${Math.min(11, Math.max(7.2, width / 34))}px`;

/** How a phone draws `face`: its scan alone, with no stats over it but the badges passed in;
 * or, until the scan loads, a battlefield frame with its name and art, all a card this small
 * has room for. */
function scanOrFrame(face: CardFace): Pick<CardProps, "card" | "variant"> {
  return face.scan
    ? { card: { ...face, loyalty: undefined }, variant: "full" }
    : {
        card: { name: face.name, typeLine: face.typeLine, colors: face.colors },
        variant: "field",
      };
}

export function PhoneBoard({
  view,
  waiting,
  choose,
  passOn,
  onLeave,
}: {
  view: PlayerView;
  /** An answer is on its way, or the game plays on without the player. */
  waiting: boolean;
  choose(index: CandidateIndex, then?: Intent[]): void;
  passOn(): void;
  onLeave(): void;
}) {
  const { ref, viewport } = useViewport();
  const placed = useMemo(
    () => viewport && phoneLayout(view, viewport),
    [view, viewport],
  );
  const objects = useMemo(
    () => new Map(view.objects.map((o) => [o.slot, o])),
    [view],
  );
  const can = useMemo(() => interactions(view), [view]);
  const status = view.status;
  const decision =
    status.kind === "deciding" && !waiting ? status.decision : undefined;

  const glows = new Map<number, Glow>();
  if (decision) {
    for (const [slot, acts] of can.bySlot) {
      const single = can.picking && acts.length === 1;
      glows.set(slot, {
        highlight: single ? "target" : undefined,
        playable: !single && !tapsOnly(acts),
      });
    }
  }
  // The top of the stack glows gold, and what it names cyan.
  const top = view.stack[view.stack.length - 1];
  const marks: [number | null | undefined, Highlight][] = [
    [top?.kind === "spell" ? top.slot : top?.source, "source"],
    [can.source, "source"],
    ...(top?.targets ?? []).flatMap((t): [number, Highlight][] =>
      t.kind === "object" ? [[t.slot, "target"]] : [],
    ),
  ];
  for (const [slot, highlight] of marks) {
    if (slot == null || glows.get(slot)?.highlight) continue;
    glows.set(slot, { ...glows.get(slot), highlight });
  }

  const { you, opponent } = view.players;
  const active = view.turn.active;
  const theirHand = view.objects.filter(
    (o) => o.zone === "hand" && o.owner === "opponent",
  );
  const pile = (side: Side) => {
    const player = view.players[side];
    const graveTop = player.graveyard[player.graveyard.length - 1];
    return (
      <div className="phone-piles">
        <div className="phone-pile">
          <i className="phone-pile-edge" />
          <CardBack width={26} />
        </div>
        <div className="phone-pile">
          {graveTop !== undefined && objects.get(graveTop) && (
            <>
              <i className="phone-pile-edge" />
              <PileTop object={objects.get(graveTop)!} />
            </>
          )}
        </div>
      </div>
    );
  };
  return (
    <div className="phone-board" ref={ref}>
      {placed && (
        <>
          <div
            className="phone-strip phone-strip-opponent"
            style={placed.bands.opponentStrip}
          >
            {pile("opponent")}
            <div className="phone-orb" data-life="opponent">
              <LifeOrb
                life={opponent.life}
                active={active === "opponent"}
                seat="opponent"
              />
            </div>
            <div className="phone-backs">
              {theirHand.map((o) => (
                <CardBack key={o.slot} width={24} />
              ))}
            </div>
            <button
              type="button"
              className="icon-button phone-menu"
              aria-label="Menu"
              onClick={onLeave}
            >
              <MenuIcon />
            </button>
          </div>
          <div
            className={`phone-middle${view.stack.length ? " phone-middle-stack" : ""}`}
            style={placed.bands.middle}
          >
            <div
              className={`icon-button phone-phase${active === "you" ? " phone-phase-yours" : ""}`}
            >
              <PhaseIcon
                phase={phaseOf(view.turn.step)}
                lit={active === "you"}
              />
            </div>
          </div>
          <div
            className="phone-strip phone-strip-you"
            style={placed.bands.yourStrip}
          >
            {pile("you")}
            <div className="phone-orb" data-life="you">
              <LifeOrb life={you.life} active={active === "you"} seat="you" />
            </div>
            <PhoneDock
              view={view}
              waiting={waiting}
              can={can}
              objects={objects}
              choose={choose}
              passOn={passOn}
            />
          </div>
          <div className="card-layer phone-cards">
            {placed.cards.map((spot) => (
              <PhoneCard
                key={spot.slot}
                object={objects.get(spot.slot)!}
                spot={spot}
                glow={glows.get(spot.slot) ?? {}}
              />
            ))}
            {placed.stack.map((spot) => (
              <StackCard
                key={stackKey(spot)}
                spot={spot}
                object={
                  spot.item.kind === "spell"
                    ? objects.get(spot.item.slot)
                    : undefined
                }
                top={spot.index === view.stack.length - 1}
              />
            ))}
          </div>
        </>
      )}
    </div>
  );
}

/** The board's size, measured: the viewport inside its top safe area, and the home bar's. */
function useViewport() {
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

/** A card on the table or in your hand, by its slot. */
function PhoneCard({
  object,
  spot,
  glow,
}: {
  object: ObjectView;
  spot: Spot;
  glow: Glow;
}) {
  const face = useFace(object);
  const field = spot.zone === "battlefield";
  const down = object.facing === "down";
  const loyalty = field
    ? object.counters.find((c) => c.kind === "loyalty")?.count
    : undefined;
  return (
    <div
      className={`card-el phone-card card-el-${object.controller}`}
      data-slot={object.slot}
      data-zone={object.zone}
      data-card={object.name ?? undefined}
      style={
        {
          left: spot.x,
          top: spot.y,
          zIndex: spot.z,
          transform: spot.rotate ? `rotate(${spot.rotate}deg)` : undefined,
          "--badge": badgeEm(spot.width),
        } as CSSProperties
      }
    >
      {down || object.name !== null ? (
        <Card
          // A face-down card shows its back and its P/T; a face-up one its scan, its loyalty
          // drawn here over the printed shield.
          {...(down
            ? { card: faceDownFace(object), variant: "full" }
            : scanOrFrame(face))}
          back={down}
          width={spot.width}
          tapped={field && object.tapped}
          highlight={glow.highlight}
          className={
            glow.playable && !glow.highlight ? "card-playable" : undefined
          }
          marks={
            field
              ? { counters: marksOf(object).counters, sick: false }
              : undefined
          }
        >
          {loyalty !== undefined && !down && (
            <Loyalty loyalty={String(loyalty)} />
          )}
        </Card>
      ) : (
        <CardBack width={spot.width} />
      )}
    </div>
  );
}

/** A spell or ability on the stack, the top one in the source's gold. */
function StackCard({
  spot,
  object,
  top,
}: {
  spot: StackSpot;
  object?: ObjectView;
  top: boolean;
}) {
  const style = {
    left: spot.x,
    top: spot.y,
    zIndex: spot.z,
    "--badge": badgeEm(spot.width),
  } as CSSProperties;
  return (
    <div
      className="card-el phone-card"
      data-slot={object?.slot}
      data-zone="stack"
      data-card={object?.name ?? undefined}
      style={style}
    >
      {object ? (
        <StackFace object={object} width={spot.width} top={top} />
      ) : (
        <AbilityFace item={spot.item} width={spot.width} top={top} />
      )}
    </div>
  );
}

function StackFace({
  object,
  width,
  top,
}: {
  object: ObjectView;
  width: number;
  top: boolean;
}) {
  const face = useFace(object);
  return (
    <Card
      {...scanOrFrame(face)}
      width={width}
      highlight={top ? "source" : undefined}
    />
  );
}

/** An ability on the stack: its source's scan, by name, since the source may have left. */
function AbilityFace({
  item,
  width,
  top,
}: {
  item: StackItem;
  width: number;
  top: boolean;
}) {
  const name = item.kind === "ability" ? item.name : "";
  const art = useCardArt(name, undefined, SCAN);
  return (
    <Card
      {...scanOrFrame({
        name,
        typeLine: art?.face.type_line ?? "",
        colors: [],
        scan: art?.src,
      })}
      width={width}
      highlight={top ? "source" : undefined}
    />
  );
}

/** A graveyard's top card, as small as the library beside it. */
function PileTop({ object }: { object: ObjectView }) {
  const face = useFace(object);
  return <Card {...scanOrFrame(face)} width={26} />;
}

/** The key of a stack item's element: a spell's slot, or an ability's place. */
const stackKey = (spot: StackSpot) =>
  spot.item.kind === "spell"
    ? `spell-${spot.item.slot}`
    : `ability-${spot.index}`;

/** The dock: the turn button, as the desktop's dock labels it, and the pass-on button where
 * passing on makes sense. A declaration's button declares what is picked on the table: for
 * now nothing (picking comes with the phone's gestures). */
function PhoneDock({
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
