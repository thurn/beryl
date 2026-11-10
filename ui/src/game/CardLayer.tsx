// The game screen's cards: one element per public slot, placed by layout.ts and animated with
// Motion from wherever it was to wherever the latest view puts it, so a card drawn, played,
// cast, resolved, killed, bounced or exiled travels rather than jumps (decision D10). Hand
// cards with something to play drag onto the battlefield, attackers forward and blockers onto
// attackers; a click on any card asks the board what it offers.
import { counterDeparture } from "./beats";
import { CARD_RETURN, takeMovedCard, type CardHandoff } from "./choiceFlight";
import { play } from "../audio/sound";
import {
  memo,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import {
  animate,
  AnimatePresence,
  motion,
  useIsPresent,
  useMotionValue,
  useReducedMotion,
  type MotionValue,
  type Transition,
} from "motion/react";
import {
  CARD_ASPECT,
  Card,
  CardBack,
  type Highlight,
} from "../components/Card";
import { ManaSymbol } from "../components/Mana";
import { StackMarks } from "../components/Stack";
import type {
  GameEvent,
  ObjectView,
  PaidCost,
  Side,
} from "../generated/protocol";
import { StageScale } from "./Stage";
import { faceDownFace, marksOf, useFace } from "./faces";
import {
  RAISE,
  isEmblem,
  type Box,
  type Layout,
  type Look,
  type Placement,
} from "./layout";
import { useCardPhysics } from "./physics";
import { dissolve } from "./dissolve";

/** Where on the stage a pointer is, in stage pixels. */
export interface Point {
  x: number;
  y: number;
}

/** What a dragged card was let go over: another card, or a player's life orb. */
export type Onto = { slot: number } | { player: Side };

/** Where a drag went: the pointer at its start and where it was let go, in stage pixels, and
 * what it was let go over. */
export interface Drop {
  start: Point;
  at: Point;
  card?: Point & { width: number };
  onto?: Onto;
}

/** The element ids arrows find a card by, and its combat mark by (`${id}-mark`). */
export const cardId = (slot: number) => `card-${slot}`;

/** How a card is shown beyond its placement. */
export interface CardState {
  highlight?: Highlight;
  /** It has something to play or activate now. */
  playable?: boolean;
  /** It can be dragged: onto the battlefield, forward to attack, onto an attacker to block. */
  draggable?: boolean;
  /** The payment of the cast being dragged or set up taps it. */
  paying?: boolean;
  /** A selected waterbend contributor, distinct from a mana source. */
  waterbending?: boolean;
  /** The dropped card is represented by its mode choices. */
  splitting?: boolean;
  /** It is attached to the card the pointer is over, or that card is attached to it. */
  linked?: boolean;
  /** On the stack, the X its spell was announced with. */
  x?: number;
  /** On the stack, the alternative and additional costs its spell was cast with. */
  paid?: PaidCost[];
}

export interface CardLayerProps {
  /** Changes with each view shown (the view itself will do): a card left where it was dropped
   * to be played stays there until the epoch changes, then goes to its placement. */
  epoch: unknown;
  /** What happened in the view shown: a spell it took off the stack other than by resolving
   * (countered, bounced, exiled) is struck, then carried to where it went. */
  events?: GameEvent[];
  objects: ObjectView[];
  placements: Layout;
  /** Where a card new to the table starts: a token flies out of the spell or permanent that made it,
   * rather than only fading in where it lands. */
  origins?: Map<number, Box>;
  modeHandoff?: { slot: number; from: CardHandoff; faceName?: string | null };
  /** The card the pointer is over: a hand card rises out of the fan (RAISE), drawn over the
   * rest of the hand while its element keeps its place in the fan, so that nothing else moves
   * and the pointer keeps the card. */
  hovered?: number;
  states: Map<number, CardState>;
  /** The element each object is drawn by, where it is not its own slot's: the card the
   * player handled of identical ones stands in for the one the engine moved (standIns.ts). */
  keys?: ReadonlyMap<number, number>;
  /** A card was clicked (or tapped without dragging) at `at`; returns whether that did
   * something. A hand card that offers nothing shakes. */
  onClick(slot: number, at: Point): boolean;
  /** A played drop holds until the next view moves it. A split holds until the chooser
   * closes or the played card moves. A refused drop returns to its place. */
  onDrop(slot: number, drop: Drop): boolean | "split";
  onHover(slot: number | undefined): void;
  /** A card was picked up to drag, or put down again (`undefined`). */
  onLift?(slot: number | undefined): void;
}

/** A drag starts once the pointer has moved this far, in stage pixels; less is a click. */
const DRAG_THRESHOLD = 6;

const SPRING: Transition = { type: "spring", stiffness: 210, damping: 28 };
/** A struck spell shakes in place, then crosses the table at a pace the eye follows. */
const STRUCK_SHAKE = [0, -12, 10, -8, 6, -3, 0];
const STRUCK_FOR = 0.45;
const CARRIED: Transition = {
  type: "tween",
  duration: 0.75,
  ease: [0.4, 0, 0.2, 1],
  delay: STRUCK_FOR,
};
/** A token flies out of what made it at a pace the eye follows, over the table. */
const EMERGING: Transition = {
  type: "tween",
  duration: 0.6,
  ease: [0.4, 0, 0.2, 1],
};
const EMERGING_FOR = 650;
/** A struck spell stays over the table until it has arrived, in ms. */
const CARRIED_FOR = (STRUCK_FOR + 0.75) * 1000 + 50;
/** A card going to or from a library or a pile, or into a showcase, crosses the table at a
 * pace the eye follows, rather than springing there: the zone it went to is what the player
 * needs to see. */
const TRAVEL: Transition = {
  type: "tween",
  duration: 0.65,
  ease: [0.4, 0, 0.2, 1],
};
/** A card going into a library stays in sight until it is there, then fades into the pile. */
const INTO_LIBRARY: Transition = { type: "tween", duration: 0.2, delay: 0.6 };
const TRAVEL_FOR = 700;
/** Looks a card travels to or from: the piles and libraries, which lie in the table's corners,
 * and the showcase over its middle. */
const TRAVELLED = new Set<Look>([
  "hidden",
  "pile",
  "shown",
  "handling",
  "scry-bottom",
]);
/** A hand card rises and settles back quickly and without bounce. */
const RAISING: Transition = { type: "tween", duration: 0.14, ease: "easeOut" };
/** A raised hand card is drawn over the rest of the hand. */
const RAISED_Z = 300;
/** A struck spell is carried over everything else on the table. */
const FLYING_Z = 900;
const DRAGGING_Z = 1000;
/** A refused click's shake, side to side in stage pixels. */
const SHAKE = [0, -7, 6, -4, 2, 0];

/** What a card element calls back: the same object for the layer's life, each call going to
 * the handler of the latest render, so that a card whose own props are unchanged (all but the
 * hovered one and those it links, when the pointer moves) skips rendering. */
type Callbacks = Pick<
  CardLayerProps,
  "onClick" | "onDrop" | "onHover" | "onLift"
>;

const NO_STATE: CardState = {};

export function CardLayer(props: CardLayerProps) {
  const { objects, placements } = props;
  // Cards there from the start are simply there; later ones (tokens) fade and grow in.
  const shown = useRef(false);
  useEffect(() => {
    shown.current = true;
  }, []);
  const latest = useRef(props);
  latest.current = props;
  // The pointer moving from card to card leaves one and enters the next in the same task:
  // the leave waits for the rest of the task, so that the enter, taking its place, makes one
  // render of the board rather than two.
  const leaving = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => () => clearTimeout(leaving.current), []);
  const [callbacks] = useState<Callbacks>(() => ({
    onClick: (slot, at) => latest.current.onClick(slot, at),
    onDrop: (slot, drop) => latest.current.onDrop(slot, drop),
    onHover: (slot) => {
      clearTimeout(leaving.current);
      if (slot === undefined) {
        leaving.current = setTimeout(() => latest.current.onHover(undefined));
      } else {
        latest.current.onHover(slot);
      }
    },
    onLift: (slot) => latest.current.onLift?.(slot),
  }));
  // The engine can recycle a token slot while the old token is still dissolving.
  const tokens = useRef({
    epoch: props.epoch,
    keys: new Map<number, number>(),
    next: 0,
  });
  const identities = tokens.current;
  if (identities.epoch !== props.epoch) {
    identities.epoch = props.epoch;
    for (const e of props.events ?? []) {
      if (
        e.kind === "zoneChange" &&
        e.from === "nowhere" &&
        e.reason === "created"
      ) {
        identities.keys.set(e.slot, ++identities.next);
      }
    }
  }
  const struck = new Set(
    (props.events ?? []).flatMap((e) =>
      e.kind === "zoneChange" &&
      e.from === "stack" &&
      e.to !== "battlefield" &&
      e.reason !== "resolve" &&
      e.reason !== "cast" &&
      e.reason !== "play"
        ? [e.slot]
        : [],
    ),
  );
  const events = props.events ?? [];
  const followingCounter = new Set(
    events.flatMap((event, index) => {
      const departure = counterDeparture(event, events[index + 1]);
      return departure ? [departure.slot] : [];
    }),
  );
  const shuffled = new Set(
    (props.events ?? []).flatMap((e) =>
      e.kind === "libraryShuffled" ? [e.player] : [],
    ),
  );
  const departures = (props.events ?? []).flatMap((e) => {
    if (e.kind === "draw") return e.slots ?? [];
    if (
      e.kind === "zoneChange" &&
      e.from !== e.to &&
      (e.from === "library" || e.to === "exile" || e.to === "library")
    )
      return [e.slot];
    return [];
  });
  const departureOrder = new Map(departures.map((slot, i) => [slot, i]));
  // The board builds each card's state afresh on every render: one equal to the last keeps
  // the last's identity.
  const states = useRef(new Map<number, CardState>());
  const stateOf = (slot: number) => {
    const given = props.states.get(slot) ?? NO_STATE;
    const last = states.current.get(slot);
    if (last && sameState(last, given)) return last;
    states.current.set(slot, given);
    return given;
  };
  return (
    <div className="card-layer">
      <AnimatePresence>
        {objects.map((o) => {
          const place = placements.get(o.slot);
          const departure = departureOrder.get(o.slot);
          const tokenLife = o.token
            ? (identities.keys.get(o.slot) ?? 0)
            : undefined;
          return (
            place && (
              <CardElement
                key={
                  tokenLife !== undefined
                    ? `token:${o.slot}:${tokenLife}`
                    : (props.keys?.get(o.slot) ?? o.slot)
                }
                object={o}
                tokenLife={tokenLife}
                shuffling={o.zone === "library" && shuffled.has(o.owner)}
                place={place}
                state={stateOf(o.slot)}
                raised={props.hovered === o.slot}
                struck={struck.has(o.slot)}
                followingCounter={followingCounter.has(o.slot)}
                departureDelay={
                  departure === undefined
                    ? 0
                    : (departure * 0.3) / Math.max(1, departures.length - 1)
                }
                departureLayer={
                  departure === undefined
                    ? 0
                    : Math.min(
                        DRAGGING_Z - FLYING_Z - 1,
                        departures.length - departure,
                      )
                }
                epoch={props.epoch}
                callbacks={callbacks}
                appearing={shown.current}
                origin={props.origins?.get(o.slot)}
                modeHandoff={
                  props.modeHandoff?.slot === o.slot
                    ? props.modeHandoff
                    : undefined
                }
              />
            )
          );
        })}
      </AnimatePresence>
    </div>
  );
}

/** Whether two card states show alike: their fields are the same values. */
function sameState(a: CardState, b: CardState) {
  const keys = Object.keys(a) as (keyof CardState)[];
  return (
    keys.length === Object.keys(b).length && keys.every((k) => a[k] === b[k])
  );
}

/** Animates `value` to `to` (`how`, a spring by default) whenever `to` or `restart` changes,
 * unless the pointer is `holding` the value where it is; once let go, the value glides to
 * `to`. */
function useGlide(
  value: MotionValue<number>,
  to: number,
  instant: boolean,
  { holding = false, restart, how = SPRING }: GlideOptions = {},
) {
  useEffect(() => {
    if (holding || value.get() === to) return;
    const controls = animate(value, to, instant ? { duration: 0 } : how);
    return () => controls.stop();
  }, [value, to, instant, holding, restart, how]);
}

interface GlideOptions {
  holding?: boolean;
  restart?: unknown;
  how?: Transition;
}

/** Where the pointer has a card: dragging it, or dropped where it was played until the view
 * it was played on (its epoch) is replaced. */
type Held =
  { dragging: true } | { dragging: false; epoch: unknown; split: boolean };

const CardElement = memo(function CardElement({
  object,
  tokenLife,
  shuffling,
  place,
  state,
  raised,
  struck,
  followingCounter,
  departureDelay,
  departureLayer,
  epoch,
  callbacks: layer,
  appearing,
  origin,
  modeHandoff,
}: {
  object: ObjectView;
  tokenLife?: number;
  shuffling: boolean;
  place: Placement;
  state: CardState;
  raised: boolean;
  struck: boolean;
  followingCounter: boolean;
  departureDelay: number;
  departureLayer: number;
  epoch: unknown;
  callbacks: Callbacks;
  appearing: boolean;
  origin?: Box;
  modeHandoff?: CardLayerProps["modeHandoff"];
}) {
  const reduce = useReducedMotion() ?? false;
  const present = useIsPresent();
  const temporary = object.token || isEmblem(object.slot);
  const stageScale = useContext(StageScale);
  const grow = appearing && !reduce;
  // A card new to the table grows in where it lands, or flies there from where it came from.
  const [emerging, setEmerging] = useState(() => grow && origin !== undefined);
  // Start the token small and centered over the spell or permanent that made it.
  const from = emerging ? origin : undefined;
  const sizeAtStart = place.width * place.scale * 0.6;
  const x = useMotionValue(
    from ? (from.left + from.right - sizeAtStart) / 2 : place.x,
  );
  const y = useMotionValue(
    from ? (from.top + from.bottom - sizeAtStart / CARD_ASPECT) / 2 : place.y,
  );
  const scale = useMotionValue(grow ? sizeAtStart / place.width : place.scale);
  const rotate = useMotionValue(place.rotate);
  const skewX = useMotionValue(place.skew);
  const opacity = useMotionValue(grow && !emerging ? 0 : place.opacity);
  const [held, setHeld] = useState<Held>();
  const dragging = held?.dragging === true;
  // The pointer places the card while it holds it, whatever its placement does meanwhile. A
  // dropped card is let go with the view it was played on, whatever the engine did with it (a
  // twin of it may be the card played), and from then on is drawn as if never held.
  const holding =
    held !== undefined &&
    (held.dragging ||
      (held.epoch === epoch && (!held.split || state.splitting)));
  // A struck spell leaves the stack for where it went: after its shake, slowly, over the
  // table.
  const returning = struck && (place.look === "hand" || place.look === "held");
  const carried =
    followingCounter || (struck && place.look !== "stack" && !returning);
  // A card changing looks to or from a pile, a library or the showcase travels there; the
  // transition holds for this view, so that a render meanwhile keeps it going.
  const moved = useRef({
    look: place.look,
    zone: object.zone,
    travel: false,
    tucking: false,
    epoch,
  });
  if (moved.current.look !== place.look || moved.current.zone !== object.zone) {
    const was = moved.current.look;
    const samePile =
      moved.current.zone === object.zone &&
      (was === "hidden" || was === "pile") &&
      (place.look === "hidden" || place.look === "pile");
    moved.current = {
      look: place.look,
      zone: object.zone,
      epoch,
      tucking: was === "scry-bottom" && place.librarySide === "opponent",
      travel:
        !samePile &&
        (returning || TRAVELLED.has(was) || TRAVELLED.has(place.look)),
    };
  }
  const change = moved.current;
  const moving = change.travel && change.epoch === epoch;
  const how = useMemo(() => {
    const base = carried
      ? CARRIED
      : emerging
        ? EMERGING
        : moving
          ? TRAVEL
          : SPRING;
    return departureDelay
      ? { ...base, delay: (base.delay ?? 0) + departureDelay }
      : base;
  }, [carried, emerging, moving, departureDelay]);
  const element = useRef<HTMLDivElement>(null);
  const handed = useRef<unknown>(undefined);
  const [handoff, setHandoff] = useState(0);
  const [travelling, setTravelling] = useState(false);
  const previousZone = useRef(object.zone);
  const lastModeHandoff = useRef<CardHandoff>(undefined);
  const fromLibrary = useRef(place.look === "hidden");
  useLayoutEffect(() => {
    const table = element.current;
    if (!table) return;
    const start = (from: CardHandoff) => {
      if (reduce || !from.width) return;
      handed.current = epoch;
      // The borrowed card has its own departure pose. Stop applying the
      // packet's translation before the first frame, while React unparks it.
      table.removeAttribute("data-library-pile");
      x.jump(from.x);
      y.jump(from.y);
      scale.jump(from.width / place.width);
      rotate.jump(0);
      skewX.jump(0);
      opacity.jump(1);
      setTravelling(true);
      setHandoff((count) => count + 1);
    };
    if (previousZone.current !== object.zone) {
      const from = takeMovedCard(table, object.slot);
      if (from) start(from);
      previousZone.current = object.zone;
    }
    if (modeHandoff && lastModeHandoff.current !== modeHandoff.from) {
      lastModeHandoff.current = modeHandoff.from;
      start(modeHandoff.from);
    }
    const onReturn = (event: Event) =>
      start((event as CustomEvent<CardHandoff>).detail);
    table.addEventListener(CARD_RETURN, onReturn);
    return () => table.removeEventListener(CARD_RETURN, onReturn);
  }, [
    epoch,
    modeHandoff,
    object.slot,
    object.zone,
    place.width,
    reduce,
    x,
    y,
    scale,
    rotate,
    skewX,
    opacity,
  ]);
  const restart = `${object.zone}:${handoff}`;
  useGlide(x, place.x, reduce, { holding, how, restart });
  useGlide(y, place.y, reduce, { holding, how, restart });
  // A card changing looks, or tapping, is drawn at another width (a back, a whole card, a
  // battlefield card): before that is painted, its scale makes up the difference, and then
  // glides, so the card's size changes smoothly too.
  const width = place.width;
  const frameKind = `${place.look}|${object.tapped}`;
  const drawn = useRef({ width, frameKind });
  useLayoutEffect(() => {
    const was = drawn.current;
    drawn.current = { width, frameKind };
    if (
      handed.current !== epoch &&
      was.width !== width &&
      was.frameKind !== frameKind
    ) {
      scale.jump((scale.get() * was.width) / width);
    }
  }, [scale, width, frameKind, epoch]);
  useGlide(scale, place.scale, reduce, { restart: `${width}:${restart}`, how });
  // A hand card is raised while hovered and while the pointer holds it; anything else dragged
  // straightens.
  const hand = place.look === "hand";
  const lifted = hand && (raised || holding);
  useGlide(rotate, dragging && !hand ? 0 : place.rotate, reduce, {
    how: returning ? TRAVEL : SPRING,
    restart,
  });
  useGlide(skewX, place.skew, reduce, { restart });
  // Out of a library, the card is in sight from the start, on the pile; into one, until it is
  // there.
  useLayoutEffect(() => {
    const was = fromLibrary.current;
    fromLibrary.current = place.look === "hidden";
    if (was && place.look !== "hidden" && !reduce) opacity.jump(place.opacity);
  }, [place.look, place.opacity, opacity, reduce]);
  useGlide(opacity, place.opacity, reduce, {
    how:
      (moving || handoff > 0) && place.look === "hidden"
        ? INTO_LIBRARY
        : SPRING,
    restart,
  });
  // Raised, the card turns back upright about its top center and rises straight up the screen,
  // inside its element, which stays turned in the fan.
  const lift = place.lift ?? RAISE.lift;
  const raise = useRaise(lifted ? place.rotate : undefined, lift, reduce);
  // Hovered but not held, a raised hand card is drawn over the hand by a copy the pointer
  // passes through (Ghost), while its element keeps the fan's order: each card's visible part
  // in the fan is what the pointer hits, the raised card included, and above the fan the raised
  // card's top, which its element reaches up to.
  const ghosted = hand && raised && !holding && !travelling;
  const parked = !travelling && !carried && !emerging;
  const rim = parked && place.rim;
  const concealed = parked && place.look === "hidden";
  const { turn, shown } = useCardTurn(
    object,
    place.back === true,
    // Anonymize the packet before it moves so a previously known card cannot be tracked.
    reduce || shuffling,
    modeHandoff,
  );
  // Over the felt, the card is a physical thing (physics.ts): held up while dragged, falling
  // where it is dropped, carried while it glides.
  const body = useRef<HTMLDivElement>(null);
  const shadow = useRef<HTMLSpanElement>(null);
  const shade = useRef<HTMLSpanElement>(null);
  const grip = useRef<[number, number]>([0.5, 0.5]);
  const lying =
    object.tapped &&
    ["field", "land", "beneath", "attached"].includes(place.look);
  const size: [number, number] = lying
    ? [width / CARD_ASPECT, width]
    : [width, width / CARD_ASPECT];
  useCardPhysics({
    motion: {
      x,
      y,
      scale,
      rotate,
      raiseScale: raise.scale,
      raiseRotate: raise.rotate,
    },
    handling: dragging ? "held" : holding ? "dropped" : "free",
    size,
    enabled: !rim && !concealed && !returning,
    reduce,
    body,
    shadow,
    shade,
    grip,
  });
  const shake = useMotionValue(0);
  const flash = useMotionValue("brightness(1)");
  const [flying, setFlying] = useState(false);
  useEffect(() => {
    if (!carried || reduce) {
      setFlying(false);
      return;
    }
    const controls = struck
      ? [
          animate(shake, STRUCK_SHAKE, {
            duration: STRUCK_FOR,
            ease: "easeInOut",
          }),
          animate(
            flash,
            ["brightness(1)", "brightness(2.4)", "brightness(1)"],
            {
              duration: STRUCK_FOR,
            },
          ),
        ]
      : [];
    setFlying(true);
    const timer = setTimeout(() => setFlying(false), CARRIED_FOR);
    return () => {
      controls.forEach((c) => c.stop());
      clearTimeout(timer);
    };
  }, [carried, struck, reduce, shake, flash]);
  // A travelling card crosses over the table, then lies where it went.
  const lastHandoff = useRef(handoff);
  useLayoutEffect(() => {
    const fromTray = handoff !== lastHandoff.current;
    lastHandoff.current = handoff;
    if ((!moving && !fromTray) || reduce) {
      setTravelling(false);
      return;
    }
    setTravelling(true);
    const timer = setTimeout(
      () => setTravelling(false),
      TRAVEL_FOR + departureDelay * 1000,
    );
    return () => clearTimeout(timer);
  }, [change, moving, reduce, departureDelay, handoff]);
  // An emerging token crosses over the table, then is a card like the rest.
  useEffect(() => {
    if (!emerging) return;
    const timer = setTimeout(() => setEmerging(false), EMERGING_FOR);
    return () => clearTimeout(timer);
  }, [emerging]);
  // `strayed`: the pointer went past a click's reach on a card that does not drag. Let go, it
  // does nothing: a drag never acts as a click (a land pulled aside must not tap).
  const drag = useRef<{
    start: Point;
    from: Point;
    moved: boolean;
    strayed?: boolean;
  }>(undefined);
  // A card in a library is unseen, and one on a graveyard or exile pile is the pile's: the
  // pile's button opens it (Piles.tsx). Neither is pointed at alone.
  const untouchable =
    place.look === "hidden" ||
    place.look === "pile" ||
    place.look === "handling" ||
    place.look === "scry-bottom";

  const stagePoint = (e: ReactPointerEvent) => {
    const stage = (e.currentTarget as HTMLElement).closest(".stage");
    const box = stage?.getBoundingClientRect();
    return {
      x: (e.clientX - (box?.left ?? 0)) / stageScale,
      y: (e.clientY - (box?.top ?? 0)) / stageScale,
    };
  };

  const onPointerDown = (e: ReactPointerEvent) => {
    if (e.button !== 0) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    // Where on the card the pointer took it: lifted, it pivots and grows about that point.
    const box = body.current?.getBoundingClientRect();
    if (box && box.width > 0 && box.height > 0) {
      grip.current = [
        Math.min(1, Math.max(0, (e.clientX - box.left) / box.width)),
        Math.min(1, Math.max(0, (e.clientY - box.top) / box.height)),
      ];
    }
    drag.current = {
      start: stagePoint(e),
      from: { x: x.get(), y: y.get() },
      moved: false,
    };
  };
  const onPointerMove = (e: ReactPointerEvent) => {
    const d = drag.current;
    if (!d) return;
    const at = stagePoint(e);
    const [dx, dy] = [at.x - d.start.x, at.y - d.start.y];
    if (!d.moved && Math.hypot(dx, dy) < DRAG_THRESHOLD) return;
    if (!state.draggable) {
      d.strayed = true;
      return;
    }
    if (!d.moved) {
      d.moved = true;
      x.stop();
      y.stop();
      setHeld({ dragging: true });
      layer.onHover(undefined);
      layer.onLift?.(object.slot);
      play("slide");
    }
    x.set(d.from.x + dx);
    y.set(d.from.y + dy);
  };
  const onPointerUp = (e: ReactPointerEvent) => {
    const d = drag.current;
    drag.current = undefined;
    if (!d) return;
    const at = stagePoint(e);
    if (d.strayed) return;
    if (!d.moved) {
      if (!layer.onClick(object.slot, at) && hand && !reduce) {
        animate(shake, SHAKE, { duration: 0.32, ease: "easeInOut" });
      }
      return;
    }
    layer.onLift?.(undefined);
    // A played card stays where it was dropped until the next view moves it on; any other
    // goes back to its place.
    const box = body.current?.getBoundingClientRect();
    const card = box && {
      x: at.x + (box.left + box.width / 2 - e.clientX) / stageScale,
      y: at.y + (box.top + box.height / 2 - e.clientY) / stageScale,
      width: box.width / stageScale,
    };
    const played = layer.onDrop(object.slot, {
      start: d.start,
      at,
      card,
      onto: under(e, e.currentTarget as HTMLElement),
    });
    setHeld(
      played
        ? { dragging: false, epoch, split: played === "split" }
        : undefined,
    );
  };

  const classes = [
    "card-el",
    `card-el-${place.look}`,
    `card-el-${object.controller}`,
    state.playable && "card-el-playable",
    state.draggable && "card-el-draggable",
    dragging && "card-el-dragging",
    rim && "card-el-rim",
  ]
    .filter(Boolean)
    .join(" ");
  const frame = { x, y, scale, rotate, skewX, opacity, originX: 0, originY: 0 };
  return (
    <>
      <motion.div
        ref={element}
        className={classes}
        data-slot={present || !temporary ? object.slot : undefined}
        data-ceasing-slot={!present && temporary ? object.slot : undefined}
        data-token-life={tokenLife}
        data-zone={object.zone}
        data-dissolving={!present && temporary && !reduce ? "" : undefined}
        data-card={place.back ? undefined : (object.name ?? undefined)}
        data-library-pile={parked ? place.librarySide : undefined}
        style={{
          ...frame,
          visibility: state.splitting ? "hidden" : undefined,
          // A library's cards, drawn only while they travel, never take the pointer.
          pointerEvents:
            !present ||
            place.look === "hidden" ||
            place.look === "handling" ||
            place.look === "scry-bottom"
              ? "none"
              : undefined,
          // A card travelling into the hand keeps its place in the fan's order all the way,
          // sliding in under the cards to its right, rather than landing over them and then
          // dropping behind.
          zIndex: dragging
            ? DRAGGING_Z
            : flying || emerging || (travelling && !hand && !change.tucking)
              ? FLYING_Z + departureLayer
              : lifted && !ghosted
                ? RAISED_Z
                : place.z,
        }}
        exit={
          temporary
            ? dissolve(reduce)
            : reduce
              ? undefined
              : { opacity: 0, transition: { duration: 0.3 } }
        }
        onPointerDown={untouchable ? undefined : onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={() => {
          if (drag.current?.moved) layer.onLift?.(undefined);
          drag.current = undefined;
          setHeld(undefined);
        }}
        onPointerEnter={() =>
          !drag.current && !untouchable && layer.onHover(object.slot)
        }
        onPointerLeave={() => !drag.current && layer.onHover(undefined)}
      >
        {concealed || rim ? (
          <div
            className="pile-card-rim"
            style={{ width, height: width / CARD_ASPECT }}
            aria-hidden
          />
        ) : (
          <motion.div
            className={hand ? "card-look" : undefined}
            style={{ ...raise, opacity: ghosted ? 0 : 1 }}
          >
            <motion.div style={{ x: shake, filter: flash }}>
              <div className="card-body" ref={body}>
                <motion.div
                  className="card-flip"
                  style={{ rotateY: turn, transformPerspective: 800 }}
                >
                  {shown.back ? (
                    <CardBack width={width} />
                  ) : (
                    <Look
                      object={shown.object}
                      place={place}
                      state={state}
                      ghost={!present}
                    />
                  )}
                </motion.div>
                <span className="card-shade" ref={shade} />
              </div>
              <span
                className="card-cast"
                ref={shadow}
                style={{ width: size[0], height: size[1] }}
              />
            </motion.div>
          </motion.div>
        )}
        {hand && (
          <div
            className="hand-hit"
            style={{
              width: place.width,
              height: place.width / CARD_ASPECT + (raised ? lift : 0),
              top: raised ? -lift : 0,
            }}
          />
        )}
      </motion.div>
      {ghosted && (
        <motion.div
          className={`${classes} card-ghost`}
          data-ghost-slot={object.slot}
          data-zone={object.zone}
          aria-hidden
          style={{ ...frame, zIndex: RAISED_Z }}
        >
          <motion.div style={raise}>
            <motion.div style={{ rotateY: turn, transformPerspective: 800 }}>
              {shown.back ? (
                <CardBack width={width} />
              ) : (
                <Look object={shown.object} place={place} state={state} ghost />
              )}
            </motion.div>
          </motion.div>
        </motion.div>
      )}
    </>
  );
});

/** The transform raising a hand card out of a fan that turns it `turn` degrees, or none when
 * `turn` is undefined: it turns back upright about the card's top center, rises `height`
 * straight up the screen and grows by `RAISE.scale`. */
function useRaise(turn: number | undefined, height: number, instant: boolean) {
  const x = useMotionValue(0);
  const y = useMotionValue(0);
  const rotate = useMotionValue(0);
  const scale = useMotionValue(1);
  const a = ((turn ?? 0) * Math.PI) / 180;
  // Straight up the screen, in the turned card's own axes.
  const lift = turn === undefined ? 0 : height;
  const how = { how: RAISING };
  useGlide(x, -lift * Math.sin(a), instant, how);
  useGlide(y, -lift * Math.cos(a), instant, how);
  useGlide(rotate, -(turn ?? 0), instant, how);
  useGlide(scale, turn === undefined ? 1 : RAISE.scale, instant, how);
  return { x, y, rotate, scale, originX: 0.5, originY: 0 };
}

/** What is under the pointer of event `e` besides the dragged element `self`: a card or a
 * life orb. */
function under(e: ReactPointerEvent, self: HTMLElement): Onto | undefined {
  for (const hit of document.elementsFromPoint(e.clientX, e.clientY)) {
    if (self.contains(hit)) continue;
    const card = hit.closest<HTMLElement>("[data-slot]");
    if (card?.dataset.slot) return { slot: Number(card.dataset.slot) };
    const life = hit.closest<HTMLElement>("[data-life]")?.dataset.life;
    if (life === "you" || life === "opponent") return { player: life };
  }
  return undefined;
}

/** Turn the same card as its visible surface changes. Use only the previously projected
 * face until the turn is edge-on, even if the newest view already knows more. */
function useCardTurn(
  object: ObjectView,
  back: boolean,
  reduce: boolean,
  handoff?: CardLayerProps["modeHandoff"],
) {
  const face = cardSurface(object, back);
  const [facing, setFacing] = useState(face);
  const previous = useRef({ object, back });
  const last = useRef<CardHandoff>(undefined);
  const from =
    handoff && handoff.from !== last.current && handoff.faceName !== undefined
      ? { ...object, name: handoff.faceName, facing: "up" as const }
      : undefined;
  const turning = (from ? cardSurface(from, false) : facing) !== face;
  const shown =
    !turning || reduce
      ? { object, back }
      : from
        ? { object: from, back: false }
        : previous.current;
  const stamp = handoff?.from ?? last.current;
  const turn = useMotionValue(0);
  useLayoutEffect(() => {
    previous.current = shown;
  }, [shown]);
  useLayoutEffect(() => {
    if (handoff) last.current = handoff.from;
    if (reduce || (from && !turning)) {
      turn.jump(0);
      setFacing(face);
      return;
    }
    if (from) setFacing(cardSurface(from, false));
    if (!turning && turn.get() === 0) return;
    let cancelled = false;
    const first = animate(turn, turning ? 90 : 0, {
      duration: turning ? 0.16 : 0.17,
      ease: turning ? "easeIn" : "easeOut",
    });
    let second: ReturnType<typeof animate> | undefined;
    if (turning) {
      void first.then(() => {
        if (cancelled) return;
        turn.jump(-90);
        setFacing(face);
        second = animate(turn, 0, { duration: 0.17, ease: "easeOut" });
      });
    }
    return () => {
      cancelled = true;
      first.stop();
      second?.stop();
    };
    // Updating facing midway through a turn must not restart it.
  }, [face, reduce, turn, stamp]);
  return { turn, shown };
}

function cardSurface(object: ObjectView, back: boolean) {
  if (back) return "back";
  if (object.facing === "down") return "down";
  return object.name === null ? "back" : `face:${object.name}`;
}

/** The card itself, as its placement draws it. */
function Look({
  object,
  place,
  state,
  ghost = false,
}: {
  object: ObjectView;
  place: Placement;
  state: CardState;
  /** A copy drawn over the card's element: the element keeps the id arrows find it by. */
  ghost?: boolean;
}) {
  const card = useFace(object);
  const under = place.look === "beneath" || place.look === "attached";
  // A face-down permanent shows its back to both players; its controller's hover still names
  // its card (CR 708.5). On the table it keeps a permanent's frame, its marks and its 2/2.
  const down = object.facing === "down";
  const field =
    under ||
    place.look === "field" ||
    place.look === "land" ||
    (down && place.look === "shown" && object.zone === "battlefield");
  if (down ? !field : object.name === null) {
    return <CardBack width={place.width} />;
  }
  const combat = field ? object.combat?.kind : undefined;
  // A card your hand fan holds because you can play it from where it lies (a graveyard, exile):
  // its own outline, apart from the hand's playable edge.
  const elsewhere = place.look === "hand" && object.zone !== "hand";
  const card_ = (
    <Card
      id={ghost ? undefined : cardId(object.slot)}
      card={down ? faceDownFace(object) : card}
      back={down}
      combat={
        combat === "attacking"
          ? "attacker"
          : combat === "blocking"
            ? "blocker"
            : undefined
      }
      variant={field ? "field" : "full"}
      marks={field ? marksOf(object) : undefined}
      count={place.count}
      width={place.width}
      tapped={field && object.tapped}
      highlight={state.highlight}
      // A gold or cyan glow says more than the playable edge or the link's ring, which would
      // hide it.
      className={
        [
          state.playable && !state.highlight && !elsewhere && "card-playable",
          elsewhere && !state.highlight && "card-elsewhere",
          state.paying && "card-paying",
          state.linked && !state.highlight && !state.paying && "card-linked",
          under && "card-under",
        ]
          .filter(Boolean)
          .join(" ") || undefined
      }
    />
  );
  if (place.look === "stack") {
    return (
      <div className="on-stack">
        {card_}
        <StackMarks
          controller={object.controller}
          x={state.x ?? 0}
          paid={state.paid}
        />
      </div>
    );
  }
  return field ? (
    <div className="on-table">
      {card_}
      {state.waterbending && (
        <span className="waterbend-tap" aria-label="Waterbend contribution">
          <ManaSymbol symbol="T" />
        </span>
      )}
    </div>
  ) : place.look === "hand" ? (
    <div className="hand-card">{card_}</div>
  ) : (
    card_
  );
}
