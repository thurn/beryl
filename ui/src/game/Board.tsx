// The battlefield screen of a game in progress, drawn from the latest PlayerView as
// docs/ui_mockups/battlefield.png lays it out: the opponent's hand as card backs, life orbs,
// land piles, permanents, libraries and graveyards, player badges, the phase tracker and the
// turn button, with the hand fanned at the near edge (decisions D10 to D13). Combat and the
// stack follow combat.png and trigger_on_stack_with_target.png: attack and block lines and the
// step banner; spells and abilities large at the right, a gold line from an ability's source
// and cyan lines to targets.
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import {
  animate,
  AnimatePresence,
  motion,
  useMotionValue,
  useReducedMotion,
} from "motion/react";
import { cardEntry, faceEntry, landFace } from "../cards";
import { Card, type Highlight } from "../components/Card";
import {
  Button,
  PhaseTracker,
  PriorityPill,
  Tip,
  TurnButton,
} from "../components/Controls";
import { CostText, RulesText } from "../components/Mana";
import {
  FaceUpEdges,
  floating,
  Library,
  LifeOrb,
  ManaPool,
  PlayerBadge,
  Coin,
} from "../components/Player";
import {
  Arrows,
  StackItem,
  StackMarks,
  type ArrowSpec,
} from "../components/Stack";
import type {
  CandidateIndex,
  DecisionView,
  Kind,
  ObjectView,
  PlayerView,
  RefusalView,
  Side,
  StackItem as StackItemView,
} from "../generated/protocol";
import {
  CardLayer,
  cardId,
  type CardState,
  type Drop,
  type Point,
} from "./CardLayer";
import {
  follow,
  interactions,
  isCombat,
  isOpening,
  playActs,
  prompt,
  slotOf,
  triggerPlan,
  type Act,
  type Intent,
} from "./decisions";
import {
  attackPlan,
  blockable,
  blockPlan,
  declared,
  emptyDraft,
  incoming,
  type Draft,
} from "./combat";
import {
  abilityParagraph,
  abilityText,
  BLANK,
  useAbilityFace,
  useFace,
  withoutReminder,
} from "./faces";
import {
  asksCastStep,
  castActs,
  castPlan,
  firstChoice,
  plansOf,
  tapList,
  tapsOf,
  type CastChoice,
} from "./casting";
import { CastStep } from "./CastStep";
import { tablePick } from "./tablePick";
import {
  combatBanner,
  footprint,
  layout,
  linked,
  overLog,
  passButton,
  phaseOf,
  pileLook,
  PILES,
  STACK_Z,
  stackPlace,
  STEP_NAMES,
  STAGE,
  zonePiles,
  type Box,
  type Placement,
  type ZonePile,
} from "./layout";
import type { LogEntry } from "./log";
import {
  actingLabel,
  Announcements,
  GameLog,
  opponentActivity,
} from "./Narration";
import {
  Inspect,
  INSPECT_WIDTH,
  inspectorBox,
  type InspectPlace,
  Lying,
  pileMark,
  PileButton,
  pileTip,
  ZoneViewer,
} from "./Piles";
import { Brand } from "./Stage";

export interface BoardProps {
  view: PlayerView;
  /** The view was shown as the game played on: the spells that came and went in it are
   * announced. */
  live: boolean;
  /** What has happened in the game, oldest first. */
  log: LogEntry[];
  /** An answer was sent and the next view has not arrived, or the game plays on without the
   * player: the opponent is acting. */
  waiting: boolean;
  /** While views wait to be shown, shows them all at once: a press on the table or Space
   * ends the replay. A card pressed then is let go on the newest view, so its click or drag
   * acts there. */
  onSkip?: () => void;
  /** Answers the open decision, and the decisions after it with `then`, as each arrives. */
  choose(index: CandidateIndex, then?: Intent[]): void;
  /** Takes back the spell or ability being cast, at a decision that may
   * (`DecisionView.cancellable`). */
  cancel?(): void;
  /** Full control: stop at every priority (decision D13). */
  setFullControl(on: boolean): void;
  onLeave(): void;
  /** Concedes the game; none once it is over. */
  onConcede?: () => void;
  /** Blurred and darkened behind a screen over it. */
  dimmed?: boolean;
  /** A screen over it answers the decision: the table only shows. */
  inert?: boolean;
}

/** Where each player's unspent mana shows: right of their life orb's blades, level with its
 * oval. */
const POOLS: Record<Side, CSSProperties> = {
  opponent: { left: 936, top: 92 },
  you: { left: 936, top: 659 },
};

/** Dropped above this line, a hand card is on the battlefield rather than back in the hand. */
const BATTLEFIELD_EDGE = 690;
/** Dragged forward at least this far, a creature attacks. */
const FORWARD = 60;

/** A menu of a card's candidates, open at a point on the stage; `pointed`, the one under
 * the pointer. A planeswalker's lists its loyalty abilities it cannot activate too, as
 * `unaffordable` paragraphs of its rules text. */
interface Menu {
  slot: number;
  at: Point;
  acts: Act[];
  unaffordable?: number[];
  pointed?: Act;
}

/** A card's cast being set up in the cast step. */
interface Casting {
  slot: number;
  casts: Act[];
  choice: CastChoice;
}

export function Board({
  view,
  live,
  log,
  waiting,
  onSkip,
  choose,
  cancel,
  setFullControl,
  onLeave,
  onConcede,
  dimmed = false,
  inert = false,
}: BoardProps) {
  const [hovered, setHovered] = useState<number>();
  // The hand card a click found nothing to do with, which the inspector says why of while
  // the pointer stays on it.
  const [refused, setRefused] = useState<number>();
  const [menu, setMenu] = useState<Menu>();
  const [casting, setCasting] = useState<Casting>();
  // The spell cast last and the permanents its payment taps.
  const [paid, setPaid] = useState<{ slot: number; taps: number[] }>();
  // The combat declaration being made, shown as if declared until it is confirmed.
  const [draft, setDraft] = useState<Draft>(emptyDraft);
  // The cards picked on the table for the view's choice of them, until it is confirmed.
  const [picks, setPicks] = useState<{
    view: PlayerView;
    picked: ReadonlySet<number>;
  }>();
  // The card being dragged.
  const [lifted, setLifted] = useState<number>();
  // The card off the table the pointer is on in the prompt, shown whole.
  const [pointedOff, setPointedOff] = useState<number>();
  const [settings, setSettings] = useState(false);
  /** How many times the player passed with mana unspent: each pulses their pool. */
  const [lost, setLost] = useState(0);
  // The graveyard or exile pile open in the zone viewer, which stays open as views go by.
  const [viewing, setViewing] = useState<Pick<ZonePile, "side" | "zone">>();
  const can = useMemo(() => interactions(view), [view]);
  // Under a screen over the table (the opening hand, a choice, the game's end), the table only
  // shows.
  const deciding =
    view.status.kind === "deciding" && !waiting && !dimmed && !inert;
  const decision =
    view.status.kind === "deciding" ? view.status.decision : undefined;
  const objects = useMemo(
    () => new Map(view.objects.map((o) => [o.slot, o])),
    [view],
  );
  // Declaring attackers or blockers: the creatures are picked on the table, in any order.
  const drafting =
    deciding && (decision?.kind === "attacker" || decision?.kind === "blocker")
      ? decision.kind
      : undefined;
  // A choice of cards made on the table (an untap's lands, a discard from the hand): the
  // eligible ones lit, those picked to begin with picked until the player changes them.
  const picking = useMemo(
    () => (deciding ? tablePick(view, can) : undefined),
    [deciding, view, can],
  );
  const picked =
    picks?.view === view ? picks.picked : new Set(picking?.initial ?? []);
  // The table as the declaration would leave it.
  const shown = useMemo(
    () => (drafting ? declared(view, draft) : view),
    [view, draft, drafting],
  );
  // Until combat damage is dealt, what each player's unblocked attackers will deal them.
  const beforeDamage =
    view.turn.step === "declareAttackers" ||
    view.turn.step === "declareBlockers";
  const combatDamage = (side: Side) =>
    beforeDamage ? incoming(shown, side) : undefined;
  // A decision other than priority, the opening's and combat's asks in a prompt over the
  // turn controls, under the stack, which rises to clear it.
  const prompting =
    deciding &&
    decision !== undefined &&
    decision.kind !== "priority" &&
    !isCombat(decision.kind) &&
    !isOpening(decision.kind);
  const castingNow = deciding ? casting : undefined;
  // The cast under way, which the engine is asking about, can still be taken back: on the
  // table, or from it while a screen asking about the cast is put aside to see the table
  // (inert, but not dimmed).
  const cancellable = !waiting && !dimmed && decision?.cancellable === true;
  // The prompt or the cast step sits over the turn controls; the stack rises to clear it.
  const asking = prompting || castingNow !== undefined;
  // The log folds away while the rows reach into its column; opened then, the rows give it room.
  const [logOpen, setLogOpen] = useState(false);
  const resting = useMemo(
    () => layout(shown, asking, dimmed),
    [shown, asking, dimmed],
  );
  const logFolded = overLog(resting);
  const placements = useMemo(
    () =>
      logFolded && logOpen ? layout(shown, asking, dimmed, true) : resting,
    [resting, logFolded, logOpen, shown, asking, dimmed],
  );
  // The hovered card, unless it has since gone where it is not shown alone.
  const hoveredPlace =
    hovered === undefined ? undefined : placements.get(hovered);
  const inspected =
    hoveredPlace?.look === "hidden" || hoveredPlace?.look === "pile"
      ? undefined
      : hoveredPlace;
  // Why the refused card under the pointer can't be played, as the engine says.
  const refusal =
    deciding && refused === hovered
      ? decision?.refused.find((r) => r.slot === refused)?.reason
      : undefined;
  // A hovered card that a new view moves may leave the pointer, which then sends no event: once
  // it has glided to its new place, it stays inspected only if the pointer is still on it.
  const pointer = useRef<{ x: number; y: number }>(undefined);
  const hoveredAt = useRef<Placement>(undefined);
  const recheck = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => {
    const was = hoveredAt.current;
    hoveredAt.current = hoveredPlace;
    if (
      !was ||
      !hoveredPlace ||
      was.slot !== hoveredPlace.slot ||
      (was.zone === hoveredPlace.zone &&
        Math.hypot(hoveredPlace.x - was.x, hoveredPlace.y - was.y) <= 4)
    ) {
      return;
    }
    const slot = hoveredPlace.slot;
    clearTimeout(recheck.current);
    recheck.current = setTimeout(() => {
      const at = pointer.current;
      const under =
        at &&
        document
          .elementFromPoint(at.x, at.y)
          ?.closest<HTMLElement>("[data-slot]")?.dataset.slot;
      if (under !== String(slot)) {
        setHovered((h) => (h === slot ? undefined : h));
      }
    }, GLIDE_MS);
  }, [hoveredPlace]);
  useEffect(() => () => clearTimeout(recheck.current), []);
  const piles = useMemo(() => zonePiles(shown), [shown]);
  const viewed = piles.find(
    (p) => p.side === viewing?.side && p.zone === viewing.zone,
  );

  // A new view closes whatever was open on the old one, and starts any declaration afresh.
  useEffect(() => {
    setMenu(undefined);
    setCasting(undefined);
    setDraft(emptyDraft());
    setPointedOff(undefined);
  }, [view]);

  const pick = (index: CandidateIndex, then?: Intent[]) => {
    setMenu(undefined);
    setCasting(undefined);
    choose(index, then);
  };
  /** Casts with `choice`: the cast, then its X and payment as each is asked. */
  const cast = (choice: CastChoice) => {
    const slot = slotOf(choice.act.option);
    if (slot !== undefined) setPaid({ slot, taps: tapsOf(choice) });
    pick(choice.act.index, castPlan(choice));
  };
  /** Plays an act of a card's: a cast with its least X and the default payment. */
  const play = (act: Act) =>
    act.option.kind === "cast" ? cast(firstChoice(act)) : pick(act.index);
  /** Opens the cast step for the card in `slot` with its casts. */
  const setUp = (slot: number, casts: Act[]) => {
    setMenu(undefined);
    setCasting({ slot, casts, choice: firstChoice(casts[0]!) });
  };
  /** Whether casting from `casts` stops in the cast step: to choose which cast or X or, under
   * full control (Arena's), which permanents pay. Otherwise the client pays, as Arena's
   * auto-tapper does, the way the session ranks first. */
  const stepsIn = (casts: Act[]) =>
    asksCastStep(casts) ||
    (view.stops === "exact" && plansOf(firstChoice(casts[0]!)).length > 1);
  /** Casts from a click: at once, as a drop does, unless the cast step has a choice to
   * offer. */
  const castOrSetUp = (slot: number, casts: Act[]) => {
    if (stepsIn(casts)) setUp(slot, casts);
    else cast(firstChoice(casts[0]!));
  };
  /** A menu's pick: a cast, with all of the card's casts, is made as a click makes it. */
  const pickAct = (slot: number, act: Act) => {
    const casts = castActs(can.bySlot.get(slot) ?? []);
    if (act.option.kind === "cast") castOrSetUp(slot, casts);
    else pick(act.index, act.then);
  };

  const states = new Map<number, CardState>();
  if (deciding) {
    for (const [slot, acts] of can.bySlot) {
      const o = objects.get(slot);
      const single = can.picking && acts.length === 1;
      states.set(slot, {
        highlight: single ? "target" : undefined,
        playable: !single,
        draggable:
          (o?.zone === "hand" &&
            o.owner === "you" &&
            playActs(acts).length > 0) ||
          acts.some(
            (a) => a.option.kind === "attack" || a.option.kind === "block",
          ),
      });
    }
  }
  for (const item of view.stack) {
    if (item.kind === "spell") {
      states.set(item.slot, {
        ...states.get(item.slot),
        x: item.x,
        paid: item.paid,
      });
    }
  }
  // The top of the stack's source glows gold and its targets cyan, whoever has priority
  // (trigger_on_stack_with_target.png).
  const top = view.stack[view.stack.length - 1];
  // The spell being cast, on top of the stack from the start of its casting (CR 601.2a), can be
  // dragged back to the hand while it can still be taken back.
  const unpaid =
    cancellable &&
    top?.kind === "spell" &&
    objects.get(top.slot)?.controller === "you"
      ? top.slot
      : undefined;
  if (unpaid !== undefined) {
    states.set(unpaid, { ...states.get(unpaid), draggable: true });
  }
  const marks: [number | null | undefined, Highlight][] = [
    [top?.kind === "ability" ? top.source : undefined, "source"],
  ];
  for (const t of top?.targets ?? []) {
    if (t.kind === "object") marks.push([t.slot, "target"]);
  }
  for (const [slot, highlight] of marks) {
    if (slot == null || states.get(slot)?.highlight) continue;
    states.set(slot, { ...states.get(slot), highlight });
  }
  if (can.source !== undefined) {
    states.set(can.source, {
      ...states.get(can.source),
      highlight: "source",
    });
  }
  // What paying "unless pays" would tap, lit as it is asked.
  const unlessTaps =
    deciding && decision?.ask.kind === "unlessPays"
      ? decision.candidates.find((c) => c.option.kind === "yes")?.payments[0]
          ?.taps
      : undefined;
  // The permanents the payment taps glow as it is set up, or while the card is dragged, its
  // first cast's (the one a drop makes when it asks nothing more). In the cast step, the ones
  // only another payment taps can be clicked to pay that way.
  const liftedCast =
    deciding && lifted !== undefined
      ? castActs(can.bySlot.get(lifted) ?? [])[0]
      : undefined;
  const paying = castingNow
    ? castingNow.choice
    : liftedCast && firstChoice(liftedCast);
  // The spell cast last keeps its payment lit while it is on top of the stack: as its
  // targets are chosen, before the payment is made (CR 601.2c to 601.2h), and once its
  // permanents are tapped.
  const taps = new Set(
    paying
      ? tapsOf(paying)
      : (unlessTaps ??
          (paid && top?.kind === "spell" && top.slot === paid.slot
            ? paid.taps
            : [])),
  );
  const others = castingNow
    ? plansOf(castingNow.choice).flatMap((p) =>
        p.taps.filter((slot) => !taps.has(slot)),
      )
    : [];
  for (const slot of others) {
    states.set(slot, { ...states.get(slot), playable: true });
  }
  for (const slot of taps) {
    states.set(slot, { ...states.get(slot), paying: true, playable: false });
  }
  // The pointer over a permanent or what is attached to it shows the other.
  for (const slot of linked(shown, hovered)) {
    states.set(slot, { ...states.get(slot), linked: true });
  }

  // A declaration's creatures: those still to pick glow cyan, the one waiting for its attacker
  // or planeswalker gold; once picked, their combat marks say what they do.
  const declarable = (slot: number) =>
    (can.bySlot.get(slot) ?? []).find(
      (a) => a.option.kind === "attack" || a.option.kind === "block",
    );
  const walkers = view.battlefield.filter((slot) => {
    const o = objects.get(slot);
    return o?.controller === "opponent" && o.types.includes("planeswalker");
  });
  // The attackers the chosen blocker may block.
  const blockTargets =
    drafting === "blocker" && draft.selected !== undefined
      ? blockable(declarable(draft.selected)?.blocks ?? [], objects)
      : new Set<number>();
  // The creature picked last waits for what it attacks or blocks.
  const aiming = drafting === "blocker" || walkers.length > 0;
  if (drafting) {
    for (const [slot, state] of states) {
      if (!declarable(slot)) continue;
      const picked = draft.attacks.has(slot) || draft.blocks.has(slot);
      states.set(slot, {
        ...state,
        highlight:
          slot === draft.selected && aiming
            ? "source"
            : picked || (drafting === "blocker" && draft.selected !== undefined)
              ? undefined
              : "target",
        playable: false,
      });
    }
    const aims =
      drafting === "blocker"
        ? [...blockTargets]
        : draft.selected !== undefined
          ? walkers
          : [];
    for (const slot of aims) {
      states.set(slot, { ...states.get(slot), highlight: "target" });
    }
  }

  // The cards of a choice on the table: those still to pick glow cyan while more may be,
  // those picked gold.
  if (picking) {
    const room = picked.size < picking.max;
    for (const slot of picking.eligible) {
      states.set(slot, {
        ...states.get(slot),
        highlight: picked.has(slot) ? "source" : room ? "target" : undefined,
        playable: false,
      });
    }
  }
  // The cards glowing as targets, which the inspector keeps clear of.
  const targets = [...states].flatMap(([slot, state]) => {
    const place = placements.get(slot);
    return state.highlight === "target" && place ? [footprint(place)] : [];
  });

  /** Confirms the cards picked on the table: the picks that say them, as the engine asks
   * them. */
  const confirmPicks = () => {
    if (!decision || !picking?.ready(picked)) return;
    const { index, rest } = follow(decision, picking.plan(picked));
    if (index !== undefined) pick(index, rest);
  };

  /** A click on the card in `slot`: what it offers, or nothing (false). */
  const onClick = (slot: number, at: Point): boolean => {
    if (castingNow) {
      // A permanent another payment taps pays that way.
      const plan = plansOf(castingNow.choice).find((p) =>
        p.taps.includes(slot),
      );
      if (plan && !taps.has(slot)) {
        setCasting({
          ...castingNow,
          choice: { ...castingNow.choice, plan: plan.plan ?? undefined },
        });
        return true;
      }
      if (slot === castingNow.slot) return true;
      setCasting(undefined);
    }
    if (drafting) {
      setDraft(drafted(draft, slot));
      return true;
    }
    if (picking) {
      if (!picking.eligible.has(slot)) return false;
      const next = new Set(picked);
      if (!next.delete(slot) && next.size < picking.max) next.add(slot);
      setPicks({ view, picked: next });
      return true;
    }
    const acts = deciding ? can.bySlot.get(slot) : undefined;
    if (!acts) {
      setMenu(undefined);
      setRefused(slot);
      return false;
    }
    const casts = castActs(acts);
    const walker = loyaltyMenu(objects.get(slot), acts);
    if (walker) {
      // A loyalty ability, which cannot be taken back once its cost is paid, is chosen from
      // all of them, even when only one can be activated.
      setMenu({ slot, at, acts, unaffordable: walker });
    } else if (casts.length > 0 && casts.length === acts.length) {
      // A card that can only be cast is cast, or set up in the cast step.
      castOrSetUp(slot, casts);
    } else if (acts.length === 1) {
      // One use acts on the click: a land is played, an ability activated, a target picked.
      pick(acts[0]!.index, acts[0]!.then);
    } else {
      // One entry for all of a card's casts, which the cast step tells apart.
      const listed = acts.filter(
        (a) => a.option.kind !== "cast" || a === casts[0],
      );
      setMenu({ slot, at, acts: listed });
    }
    return true;
  };
  const onDrop = (slot: number, { start, at, onto }: Drop) => {
    // The card being cast let go in the hand goes back: the cast step closes, or the spell not
    // yet paid for is taken back. The cast step's card let go on the battlefield is cast as set.
    const inHand = at.y >= BATTLEFIELD_EDGE;
    if (castingNow?.slot === slot) {
      if (inHand) {
        setCasting(undefined);
        return false;
      }
      cast(castingNow.choice);
      return true;
    }
    if (slot === unpaid) {
      if (inHand) cancel?.();
      return inHand;
    }
    const acts = deciding ? (can.bySlot.get(slot) ?? []) : [];
    if (objects.get(slot)?.zone === "hand") {
      const plays = inHand ? [] : playActs(acts);
      const casts = castActs(plays);
      if (casts.length > 0 && casts.length === plays.length && stepsIn(casts)) {
        // The cast asks for more (which cast, X, its payment): the cast step, with the card
        // back in hand.
        setUp(slot, casts);
        return false;
      }
      if (plays.length === 1) {
        play(plays[0]!);
        return true;
      }
      if (plays.length > 1) {
        const listed = plays.filter(
          (a) => a.option.kind !== "cast" || a === casts[0],
        );
        setMenu({ slot, at, acts: listed });
      }
      return false;
    }
    // A creature dragged in a declaration joins it, as clicking would, and goes to its new
    // place: a blocker onto the attacker it blocks (CR 509.1a); an attacker forward, or onto
    // what it attacks, the defending player or one of their planeswalkers (CR 508.1b).
    const over = onto && "slot" in onto ? onto.slot : undefined;
    if (drafting === "blocker" && declarable(slot) && over !== undefined) {
      setDraft(drafted({ ...draft, selected: slot }, over));
    } else if (drafting === "attacker" && declarable(slot)) {
      const walker = over !== undefined && walkers.includes(over) ? over : null;
      const atPlayer = onto && "player" in onto && onto.player === "opponent";
      if (walker !== null || atPlayer || start.y - at.y >= FORWARD) {
        const attacks = new Map(draft.attacks).set(slot, walker);
        setDraft({ ...draft, attacks, selected: slot });
      }
    }
    return false;
  };

  /** The declaration with the card in `slot` clicked: a creature to attack with joins it or
   * leaves it, and a planeswalker clicked next is what it attacks; a creature to block with is
   * chosen, or taken back to choose again, and the attacker clicked next is what it blocks. */
  const drafted = (d: Draft, slot: number): Draft => {
    const { selected } = d;
    if (drafting === "attacker") {
      const attacks = new Map(d.attacks);
      if (attacks.delete(slot)) return { ...d, attacks, selected: undefined };
      if (declarable(slot)) {
        return { ...d, attacks: attacks.set(slot, null), selected: slot };
      }
      if (selected !== undefined && walkers.includes(slot)) {
        return { ...d, attacks: attacks.set(selected, slot) };
      }
      return { ...d, selected: undefined };
    }
    const blocks = new Map(d.blocks);
    if (declarable(slot)) {
      blocks.delete(slot);
      return { ...d, blocks, selected: slot === selected ? undefined : slot };
    }
    const may =
      selected !== undefined &&
      blockable(declarable(selected)?.blocks ?? [], objects).has(slot);
    if (may) blocks.set(selected, slot);
    return { ...d, blocks, selected: undefined };
  };

  /** Confirms the declaration: the picks that make it, as the engine asks them. */
  const declare = () => {
    if (!decision) return;
    const plan =
      drafting === "attacker"
        ? attackPlan(draft.attacks, objects)
        : blockPlan(draft.blocks, objects);
    const { index, rest } = follow(decision, plan);
    if (index !== undefined) pick(index, rest);
  };
  /** Every creature that may attack, attacking the defending player but for those aimed at a
   * planeswalker already. */
  const attackWithAll = () => {
    const attacks = new Map(draft.attacks);
    for (const slot of can.bySlot.keys()) {
      if (declarable(slot) && !attacks.has(slot)) attacks.set(slot, null);
    }
    setDraft({ ...draft, attacks, selected: undefined });
  };

  // The keyboard (Arena's): Space or Enter does the primary action, passing or confirming, and
  // Escape takes back what was begun. A menu, the cast step and a pile's viewer close on
  // Escape themselves; a focused button takes its own Space and Enter.
  const onKey = useRef<(e: KeyboardEvent) => void>(undefined);
  onKey.current = (e) => {
    if (e.defaultPrevented) return;
    if (e.key === "Escape") {
      if (settings && !inert) setSettings(false);
      else if (cancellable && !menu && !castingNow && !viewed) cancel?.();
      return;
    }
    if (inert) return;
    if ((e.key !== " " && e.key !== "Enter") || e.repeat || menu || viewed) {
      return;
    }
    const target = e.target instanceof Element ? e.target : null;
    if (target?.closest("button, input, select, textarea, a")) return;
    if (onSkip) {
      e.preventDefault();
      onSkip();
      return;
    }
    const { pass } = can;
    let primary: (() => void) | undefined;
    if (castingNow) primary = () => cast(castingNow.choice);
    else if (drafting) primary = declare;
    else if (picking) primary = confirmPicks;
    else if (declaring)
      primary = finish === undefined ? undefined : () => pick(finish);
    else if (deciding && decision?.kind === "priority" && pass !== undefined) {
      primary = () => pick(pass);
    }
    if (!primary) return;
    e.preventDefault();
    primary();
  };
  useEffect(() => {
    const listener = (e: KeyboardEvent) => onKey.current?.(e);
    window.addEventListener("keydown", listener);
    return () => window.removeEventListener("keydown", listener);
  }, []);

  const nameOf = (slot: number) => objects.get(slot)?.name ?? "a card";
  const { you, opponent } = view.players;
  const active = view.turn.active;
  // The mulligans taken show until each player has had a turn, while hand sizes still tell.
  const early = view.turn.number <= 2;
  const sourceName =
    top &&
    (top.kind === "spell"
      ? (objects.get(top.slot)?.name ?? undefined)
      : top.name);
  const stop = decision?.halt
    ? HALT_BUTTON
    : decision?.passTo
      ? passButton(view, decision.passTo)
      : { label: "Pass", caption: "" };
  const arrows = useMemo(() => boardArrows(shown), [shown]);
  const attacks = useMemo(() => attackLines(shown), [shown]);
  const announcement = combatBanner(view) ?? coinFlipBanner(view);
  // While a declaration is made, the banner says what to click; any other is an announcement,
  // which shows for a moment.
  const banner = drafting
    ? {
        title: "Combat",
        detail: instruction(drafting, draft, walkers.length > 0, nameOf),
        lasting: true,
      }
    : announcement && { ...announcement, lasting: false };
  // A combat declaration is finished with the turn button, where priority is passed.
  const declaring =
    deciding && decision && isCombat(decision.kind) ? decision : undefined;
  // What finishes it: what a creature put onto the battlefield attacking (Adeline's Human,
  // shown attacking the player as it is asked) attacks defaults to the player (CR 508.4).
  const finish =
    declaring?.kind === "attackTarget"
      ? can.players.get("opponent")?.index
      : can.done;

  return (
    <>
      <div
        className={dimmed ? "table table-dimmed" : "table"}
        onPointerDown={() => setSettings(false)}
        // Before the card under it hears the press, so that the card's click or drag is one on
        // the newest view.
        onPointerDownCapture={onSkip}
        // A right-click takes back what was begun, as Escape does.
        onContextMenu={(e) => {
          e.preventDefault();
          if (menu) return;
          if (castingNow) setCasting(undefined);
          else if (cancellable) cancel?.();
        }}
        onPointerMove={(e) => {
          pointer.current = { x: e.clientX, y: e.clientY };
        }}
      >
        <div className="far-shade" />
        {(["opponent", "you"] as const).map((side) => {
          const { library } = view.players[side];
          const pile = pileLook(side, PILES[side].library, library.length);
          return (
            <Lying
              key={side}
              look={pile}
              tip={
                <Tip side={side === "you" ? "over" : "below"}>
                  {pileTip("library", library.length)}
                </Tip>
              }
            >
              <Library
                count={library.length}
                width={pile.width}
                depth={pile.depth}
              />
            </Lying>
          );
        })}
        {piles.map(
          ({ side, zone, cards, look }) =>
            cards.length > 1 && (
              <Lying key={`${side}-${zone}`} look={look}>
                <FaceUpEdges
                  count={cards.length}
                  width={look.width}
                  depth={look.depth}
                />
              </Lying>
            ),
        )}

        <div className="hud" style={{ left: 18, top: 62 }}>
          <PlayerBadge
            name="Opponent"
            seat="opponent"
            activity={opponentActivity(view, waiting)}
            mulligans={early ? opponent.mulligans : 0}
          />
        </div>
        <div className="hud" style={{ left: 18, top: 704 }}>
          <PlayerBadge name="You" mulligans={early ? you.mulligans : 0} />
        </div>
        <Life
          side="opponent"
          life={opponent.life}
          incoming={combatDamage("opponent")}
          active={active === "opponent"}
          first={view.turn.starting === "opponent"}
          style={{ left: 670, top: 48 }}
          target={deciding ? can.players.get("opponent") : undefined}
          onPick={(act) => pick(act.index, act.then)}
        />
        <Life
          side="you"
          life={you.life}
          incoming={combatDamage("you")}
          active={active === "you"}
          first={view.turn.starting === "you"}
          style={{ left: 670, top: 618 }}
          target={deciding ? can.players.get("you") : undefined}
          onPick={(act) => pick(act.index, act.then)}
        />
        <div className="hud" style={POOLS.opponent}>
          <ManaPool pool={opponent.mana} />
        </div>
        <div className="hud" style={POOLS.you}>
          <ManaPool pool={you.mana} pulse={lost} />
        </div>

        {banner && (
          <StepBanner
            key={
              banner.lasting ? "lasting" : `${banner.title}|${banner.detail}`
            }
            {...banner}
          />
        )}

        <GameLog
          log={log}
          folded={logFolded}
          open={logOpen}
          onToggle={() => setLogOpen((o) => !o)}
        />
        {live && <Announcements view={view} />}

        {view.stack.map(
          (item, i) =>
            item.kind === "ability" && (
              <AbilityOnStack
                key={`${i}-${item.name}`}
                index={i}
                count={view.stack.length}
                lifted={asking}
                item={item}
                source={
                  item.source === null ? undefined : objects.get(item.source)
                }
              />
            ),
        )}

        <CardLayer
          epoch={view}
          objects={shown.objects}
          placements={placements}
          hovered={hovered}
          states={states}
          onClick={onClick}
          onDrop={onDrop}
          onHover={(slot) => {
            setHovered(slot);
            setRefused((r) => (r === slot ? r : undefined));
          }}
          onLift={setLifted}
        />

        {/* Over the cards on them: a pile is opened, never pointed at card by card. */}
        {piles.map(
          (pile) =>
            pile.cards.length > 0 && (
              <PileButton
                key={`${pile.side}-${pile.zone}`}
                pile={pile}
                mark={pileMark(pile, states)}
                onOpen={() => {
                  setMenu(undefined);
                  setViewing({ side: pile.side, zone: pile.zone });
                }}
              />
            ),
        )}

        <div className="board-arrows board-arrows-under">
          <Arrows arrows={attacks} follow />
        </div>
        <div className="board-arrows">
          <Arrows arrows={arrows} follow avoid={AVOID} />
        </div>

        {/* Under a screen over the table, the turn's controls give way (scry.png). */}
        {!inert && (
          <div className="turn-controls">
            {view.stops === "exact" && (
              <span className="full-control has-tip">
                <FullControlIcon />
                <Tip side="above">Full control</Tip>
              </span>
            )}
            <PhaseTracker
              current={phaseOf(view.turn.step)}
              step={view.turn.step}
              compact
              now={`Turn ${view.turn.number} · ${STEP_NAMES[view.turn.step]}`}
            />
            {drafting ? (
              <Declare
                kind={drafting}
                draft={draft}
                everyone={
                  drafting === "attacker" &&
                  [...can.bySlot.keys()].some(
                    (slot) => declarable(slot) && !draft.attacks.has(slot),
                  )
                }
                onDeclare={declare}
                onAll={attackWithAll}
                onClear={() => setDraft(emptyDraft())}
              />
            ) : declaring ? (
              <Finish decision={declaring} done={finish} onPick={pick} />
            ) : decision?.kind === "priority" || waiting ? (
              <div className="turn-action">
                {/* In the cast step, its Cast is the one primary action. */}
                <TurnButton
                  label={waiting ? "Pass" : stop.label}
                  disabled={
                    waiting ||
                    can.pass === undefined ||
                    castingNow !== undefined
                  }
                  onClick={() => {
                    if (can.pass === undefined || castingNow) return;
                    // Mana left unspent empties as the step ends (CR 500.4): passing on to a
                    // step, rather than to the top of the stack resolving, pulses the pool
                    // rather than asking.
                    const resolves = decision?.passTo?.kind === "resolve";
                    if (floating(you.mana) && !resolves) setLost((n) => n + 1);
                    pick(can.pass);
                  }}
                />
                {/* While you wait, the priority pill takes the caption's place under the
                button, leaving the tracker showing the phase. */}
                {waiting ? (
                  <PriorityPill>{actingLabel(view)}</PriorityPill>
                ) : (
                  <span className="turn-caption">{stop.caption}</span>
                )}
              </div>
            ) : null}
          </div>
        )}

        {prompting && decision && (
          <Prompt
            key={decision.id.seq}
            decision={decision}
            skip={can.skip}
            source={sourceName}
            nameOf={nameOf}
            objects={objects}
            onTable={(slot) => {
              const look = placements.get(slot)?.look;
              return look !== undefined && look !== "hidden" && look !== "pile";
            }}
            onPick={pick}
            onPlan={(plan) => {
              const { index, rest } = follow(decision, plan);
              if (index !== undefined) pick(index, rest);
            }}
            onCancel={cancellable ? cancel : undefined}
            onPoint={setPointedOff}
            confirm={
              picking && {
                label: picking.label(picked),
                disabled: !picking.ready(picked),
                onClick: confirmPicks,
              }
            }
            hovered={hovered}
          />
        )}

        {castingNow && !prompting && (
          <CastStep
            name={objects.get(castingNow.slot)?.name ?? "a card"}
            casts={castingNow.casts}
            choice={castingNow.choice}
            nameOf={nameOf}
            onChoose={(choice) => setCasting({ ...castingNow, choice })}
            onCast={() => cast(castingNow.choice)}
            onCancel={() => setCasting(undefined)}
          />
        )}

        {menu && (
          <ActionMenu
            menu={menu}
            title={objects.get(menu.slot)?.name ?? undefined}
            objects={objects}
            onPick={(act) => pickAct(menu.slot, act)}
            onPoint={(act) => setMenu({ ...menu, pointed: act })}
            onClose={() => setMenu(undefined)}
          />
        )}

        {/* The card the pointer is on or, while its menu points at an ability, the card that
            ability's paragraph lit. */}
        {(menu?.pointed ?? (inspected && !menu)) && !castingNow && !dimmed && (
          <InspectAt
            view={view}
            object={pointedObject(
              objects.get(menu?.slot ?? inspected!.slot),
              menu?.pointed,
            )}
            place={inspectorPlace(
              menu ? menu.at.x : inspected!.x,
              view.stack.length > 0 || asking,
              targets,
            )}
            lit={menu?.pointed && litParagraph(menu.pointed, objects)}
            refusal={menu ? undefined : refusal}
          />
        )}

        {pointedOff !== undefined && prompting && (
          <InspectAt
            view={view}
            object={objects.get(pointedOff)}
            place={inspectorPlace(STAGE.width, true, targets)}
          />
        )}

        {viewed && !dimmed && (
          <ZoneViewer
            pile={viewed}
            states={states}
            offers={(slot) => deciding && can.bySlot.has(slot)}
            onPick={(slot) => {
              setViewing(undefined);
              onClick(slot, { x: viewed.look.x, y: viewed.look.y });
            }}
            onClose={() => setViewing(undefined)}
          />
        )}

        {/* Like the turn's controls, the gear gives way to a screen over the table. */}
        {!inert && (
          <button
            type="button"
            className="gear-button"
            aria-label="Settings"
            onPointerDown={(e) => e.stopPropagation()}
            onClick={() => setSettings(!settings)}
          >
            <Gear />
          </button>
        )}
        {settings && !inert && (
          <div className="settings" onPointerDown={(e) => e.stopPropagation()}>
            <label className="settings-toggle">
              <input
                type="checkbox"
                checked={view.stops === "exact"}
                disabled={!deciding}
                onChange={(e) => setFullControl(e.target.checked)}
              />
              <span>
                Full control
                <small>Stop at every priority</small>
              </span>
            </label>
            {onConcede && (
              <ConcedeButton
                onConcede={() => {
                  setSettings(false);
                  onConcede();
                }}
              />
            )}
            <Button variant="secondary" size="small" onClick={onLeave}>
              Leave Game
            </Button>
          </div>
        )}
      </div>
      {/* Outside the table, so it stays crisp over the dimmed table and any screen on it
          (scry.png). */}
      <Brand />
    </>
  );
}

/** Concede, asked once in place: the first click turns it into the confirmation, which the
 * settings closing puts back. */
function ConcedeButton({ onConcede }: { onConcede(): void }) {
  const [confirming, setConfirming] = useState(false);
  return (
    <Button
      variant={confirming ? "primary" : "secondary"}
      size="small"
      onClick={confirming ? onConcede : () => setConfirming(true)}
    >
      {confirming ? "Confirm Concede" : "Concede"}
    </Button>
  );
}

/** The turn button at a halt of the session's (`DecisionView.halt`), where passing is all the
 * player can do: ending their own turn from its first main phase. */
const HALT_BUTTON = { label: "End Turn", caption: "Nothing to play" };

/** How long a card takes to glide to a new place (CardLayer's spring), near enough, in ms. */
const GLIDE_MS = 700;

/** How long an announcing banner shows, in ms. */
const BANNER_MS = 2500;

/** A step announcement over the battlefield, such as "Combat" over "Blockers declared", between
 * the opponent's life orb and their permanents: it fades away after a moment unless `lasting`
 * (a declaration's instruction). */
function StepBanner({
  title,
  detail,
  lasting = false,
}: {
  title: string;
  detail: string;
  lasting?: boolean;
}) {
  const reduce = useReducedMotion() ?? false;
  const [shown, setShown] = useState(true);
  useEffect(() => {
    if (lasting) return;
    const timer = setTimeout(() => setShown(false), BANNER_MS);
    return () => clearTimeout(timer);
  }, [lasting]);
  return (
    <AnimatePresence>
      {shown && (
        <motion.div
          className="step-banner"
          role="status"
          exit={
            reduce ? undefined : { opacity: 0, transition: { duration: 0.4 } }
          }
        >
          <i className="step-banner-rule step-banner-rule-left" />
          <i className="step-banner-diamond" />
          <span className="step-banner-title">{title}</span>
          <i className="step-banner-dot" />
          <i className="step-banner-rule step-banner-rule-right" />
          <i className="step-banner-underline" />
          <span className="step-banner-detail">{detail}</span>
          <svg className="step-banner-notch" viewBox="0 0 140 12" aria-hidden>
            <path d="M0 1.5 H60.5 L70 10.5 L79.5 1.5 H140" />
          </svg>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

/** Full control's mark beside the turn button: a stop sign, for stopping at every priority. */
function FullControlIcon() {
  return (
    <svg viewBox="0 0 24 24" role="img" aria-label="Full control">
      <path d="M8 1.5 H16 L22.5 8 V16 L16 22.5 H8 L1.5 16 V8 Z" />
      <path className="full-control-bars" d="M9.5 7.5 V16.5 M14.5 7.5 V16.5" />
    </svg>
  );
}

/** Where the inspector stands: beside the table on the side away from the card at `x`, never over a
 * life orb, which lies between the two sides. While the stack or a prompt holds the table's
 * right (`taken`), it is shown smaller in the column between the life orbs and the stack
 * instead. It never covers a card glowing as a target (`targets`, their boxes), which the
 * player may be about to pick: where its side would, it takes the other, and where both would,
 * it shrinks toward a side's outer edge until it clears them. Failing that, it covers as little
 * of them as it can. */
function inspectorPlace(
  x: number,
  taken: boolean,
  targets: Box[],
): InspectPlace {
  const right: InspectPlace = taken
    ? BETWEEN_ORBS_AND_STACK
    : { left: STAGE.width - INSPECT_WIDTH - 24 };
  const sides = x >= STAGE.width / 2 ? [LEFT, right] : [right, LEFT];
  const covered = (place: InspectPlace) => {
    const box = inspectorBox(place);
    return targets.reduce((sum, t) => {
      const w = Math.min(box.right, t.right) - Math.max(box.left, t.left);
      const h = Math.min(box.bottom, t.bottom) - Math.max(box.top, t.top);
      return sum + (w > 0 && h > 0 ? w * h : 0);
    }, 0);
  };
  for (const side of sides) {
    const full = side.width ?? INSPECT_WIDTH;
    for (let width = full; width >= INSPECT_MIN_WIDTH; width -= 10) {
      // The left side keeps its left edge; the others their right one, by the stack or the
      // stage's edge.
      const place = {
        ...side,
        left: side === LEFT ? side.left : side.left + full - width,
        width,
      };
      if (covered(place) === 0) return place;
    }
  }
  return sides.reduce((a, b) => (covered(b) < covered(a) ? b : a));
}

/** The table's left, under the badges. */
const LEFT: InspectPlace = { left: 24 };
/** How narrow the inspector gets to clear a target: a scan's rules text still reads. */
const INSPECT_MIN_WIDTH = 230;

/** Right of the life orbs (x 670 to 930), left of the stack (STACK.x), and below the
 * opponent's orb and above yours (y 159 and 618), with room for the artist's line. */
const BETWEEN_ORBS_AND_STACK = { left: 940, top: 172, width: 258 };

/** The settings gear at the top right. */
function Gear() {
  return (
    <svg className="gear" viewBox="0 0 24 24" aria-label="Settings" role="img">
      <circle className="gear-hole" cx="12" cy="12" r="4.9" />
      <path d="M19.67 9.71 L22.34 9.69 L22.34 14.31 L19.67 14.29 L19.04 15.80 L20.95 17.68 L17.68 20.95 L15.80 19.04 L14.29 19.67 L14.31 22.34 L9.69 22.34 L9.71 19.67 L8.20 19.04 L6.32 20.95 L3.05 17.68 L4.96 15.80 L4.33 14.29 L1.66 14.31 L1.66 9.69 L4.33 9.71 L4.96 8.20 L3.05 6.32 L6.32 3.05 L8.20 4.96 L9.71 4.33 L9.69 1.66 L14.31 1.66 L14.29 4.33 L15.80 4.96 L17.68 3.05 L20.95 6.32 L19.04 8.20 Z M16.9 12 A4.9 4.9 0 1 0 7.1 12 A4.9 4.9 0 1 0 16.9 12 Z" />
    </svg>
  );
}

/** A life orb that counts to a new total and pulses as it changes; it glows cyan as a target
 * and is then clicked to pick that player. It flares while its player is the active player,
 * whoever holds priority: the priority pill says who that is. */
function Life({
  side,
  life,
  incoming,
  active,
  first,
  style,
  target,
  onPick,
}: {
  side: Side;
  life: number;
  /** In combat, before its damage: what the unblocked attackers will deal this player. */
  incoming?: number;
  active: boolean;
  /** Their player takes, or took, the first turn: a coin by the orb says so. */
  first: boolean;
  style: CSSProperties;
  target?: Act;
  onPick(act: Act): void;
}) {
  const reduce = useReducedMotion() ?? false;
  const value = useMotionValue(life);
  const pulse = useMotionValue(1);
  const [shown, setShown] = useState(life);
  useEffect(() => {
    if (value.get() === life) return;
    const count = animate(value, life, {
      duration: reduce ? 0 : 0.7,
      ease: "easeOut",
      onUpdate: (v) => setShown(Math.round(v)),
    });
    const beat = reduce
      ? undefined
      : animate(pulse, [1.14, 1], { duration: 0.5, ease: "easeOut" });
    return () => {
      count.stop();
      beat?.stop();
    };
  }, [life, value, pulse, reduce]);
  const targetable = target !== undefined;
  return (
    <motion.div
      className={targetable ? "hud life life-target" : "hud life"}
      data-life={side}
      style={{ ...style, scale: pulse }}
      onClick={() => targetable && onPick(target)}
    >
      <LifeOrb
        life={shown}
        active={active}
        seat={side === "opponent" ? "opponent" : "you"}
        anchor={lifeId(side)}
      />
      {incoming !== undefined && (
        <Incoming amount={incoming} lethal={incoming > 0 && incoming >= life} />
      )}
      {first && (
        <span className="life-first has-tip" data-first={side}>
          <Coin size={24} />
          <Tip side={side === "you" ? "above" : "below"}>Played first</Tip>
        </span>
      )}
    </motion.div>
  );
}

/** The combat damage a player is about to take, beside their orb: a number, red when it is
 * lethal. */
function Incoming({ amount, lethal }: { amount: number; lethal: boolean }) {
  return (
    <span
      className={
        lethal ? "life-incoming life-incoming-lethal" : "life-incoming"
      }
      role="status"
      aria-label={`${amount} combat damage${lethal ? ", lethal" : ""}`}
      data-incoming={amount}
      data-lethal={lethal || undefined}
    >
      {amount > 0 ? `\u2212${amount}` : "0"}
    </span>
  );
}

/** An activated or triggered ability on the stack, in the stack's cascade: its source's name
 * and art over what kind of ability it is and the paragraph of rules text that says it, as
 * trigger_on_stack_with_target.png draws a trigger. */
function AbilityOnStack({
  index,
  count,
  lifted,
  item,
  source,
}: {
  index: number;
  count: number;
  lifted: boolean;
  item: Extract<StackItemView, { kind: "ability" }>;
  source?: ObjectView;
}) {
  const face = useAbilityFace(item.name, source, BLANK);
  const place = stackPlace(index, count, lifted);
  return (
    <motion.div
      className="hud stack-ability"
      style={{ zIndex: STACK_Z + index }}
      initial={{ opacity: 0, scale: 0.8, x: place.x, y: place.y }}
      animate={{ opacity: 1, scale: 1, x: place.x, y: place.y }}
    >
      <StackItem
        id={stackId(index)}
        source={face}
        label={item.triggered ? "Triggered Ability" : "Activated Ability"}
        text={abilityText(face.text ?? "", item.triggered, item.nth)}
        width={place.width}
        showArt
      />
      <StackMarks controller={item.controller} x={item.x} />
    </motion.div>
  );
}

/** The element id of the stack item at `index`: its card for a spell. */
const stackId = (index: number) => `stack-${index}`;

/** What the stack's arrows keep off: every card on the screen, the stack's items, the life
 * orbs and the game log. */
const AVOID = ".card-el .card-slot, .stack-item, .life-orb, .game-log";

/** The element id of the oval of a player's life orb. */
const lifeId = (side: Side) => `${side}-life-orb`;

/** The lines under the table's cards: each unblocked attacker's at a player, a hairline out of
 * its chevron's apex into that player's life orb, so the chevron does not seem to point at
 * whatever stands across from it. Under the cards, they do not cross the block lines and cards
 * between, only leave the attacker and land on the orb. */
function attackLines(view: PlayerView): ArrowSpec[] {
  const objects = new Map(view.objects.map((o) => [o.slot, o]));
  return view.battlefield.flatMap((slot): ArrowSpec[] => {
    const combat = objects.get(slot)?.combat;
    if (combat?.kind !== "attacking") return [];
    if (combat.blocked || combat.planeswalker !== null) return [];
    const up = objects.get(slot)?.controller === "you";
    return [
      {
        from: `${cardId(slot)}-mark`,
        to: lifeId(up ? "opponent" : "you"),
        tone: "gold",
        fromSide: up ? "top" : "bottom",
        toSide: "facing",
        bend: 0,
        start: "none",
        fine: true,
      },
    ];
  });
}

/** The lines over the table (decision D10): each blocked attacker's to its blockers and each
 * attacker's at a planeswalker to it (combat.png), and for the top of the stack, a gold line
 * from an ability's source and a cyan one to each target (trigger_on_stack_with_target.png).
 * An attacker at a player draws its line under the cards (`attackLines`). */
function boardArrows(view: PlayerView): ArrowSpec[] {
  const objects = new Map(view.objects.map((o) => [o.slot, o]));
  const out: ArrowSpec[] = [];
  for (const slot of view.battlefield) {
    const o = objects.get(slot);
    if (o?.combat?.kind !== "attacking") continue;
    // The opponent's attackers come down the table toward you; yours go up it.
    const up = o.controller === "you";
    const blockers = view.battlefield.filter((b) => {
      const c = objects.get(b)?.combat;
      return c?.kind === "blocking" && c.attacker === slot;
    });
    // A block is a straight link over both cards, from the attacker's middle to the blocker's,
    // a dot on each: a pair reads as one even where the rows could not set them face to face,
    // where a line between their edges would hide among the combat marks.
    for (const b of blockers) {
      out.push({
        from: cardId(slot),
        to: cardId(b),
        tone: "gold",
        bend: 0,
        fromSide: "center",
        toSide: "center",
        start: "dot",
        end: "dot",
      });
    }
    const walker = o.combat.planeswalker;
    if (o.combat.blocked || walker === null) continue;
    // Out of its chevron's apex and into the planeswalker it attacks, wherever that stands.
    out.push({
      from: `${cardId(slot)}-mark`,
      to: cardId(walker),
      tone: "gold",
      fromSide: up ? "top" : "bottom",
      toSide: "facing",
      start: "none",
      fine: true,
    });
  }
  // The top of the stack's lines are routed round the cards on the table (AVOID): the source's
  // first, then each target's, kept apart from it.
  const top = view.stack.length - 1;
  const item = view.stack[top];
  if (item) {
    const from = item.kind === "spell" ? cardId(item.slot) : stackId(top);
    if (item.kind === "ability" && item.source !== null) {
      out.push({ from: cardId(item.source), to: from, tone: "gold" });
    }
    for (const target of item.targets) {
      out.push({
        from,
        to:
          target.kind === "player"
            ? lifeId(target.player)
            : cardId(target.slot),
        tone: "cyan",
      });
    }
  }
  return out;
}

/** The result of a coin flip, while the view it happened in shows: the card that flipped it,
 * and who won or lost. */
function coinFlipBanner(
  view: PlayerView,
): { title: string; detail: string } | undefined {
  const flip = view.events.find((e) => e.kind === "coinFlip");
  if (flip?.kind !== "coinFlip") return undefined;
  const source = view.objects.find((o) => o.slot === flip.source)?.name;
  const who = flip.player === "you" ? "You" : "Opponent";
  return {
    title: source ?? "Coin flip",
    detail: `${who} ${flip.won ? "won" : "lost"} the flip`,
  };
}

/** The turn button while attackers or blockers are picked on the table: it confirms the
 * declaration, with shortcuts above it to attack with everything and to start over. */
function Declare({
  kind,
  draft,
  everyone,
  onDeclare,
  onAll,
  onClear,
}: {
  kind: "attacker" | "blocker";
  draft: Draft;
  /** Some creature that may attack is not attacking yet. */
  everyone: boolean;
  onDeclare(): void;
  onAll(): void;
  onClear(): void;
}) {
  const attack = kind === "attacker";
  const n = attack ? draft.attacks.size : draft.blocks.size;
  const label = attack
    ? n > 0
      ? "Attack"
      : "No Attacks"
    : n > 0
      ? "Block"
      : "No Blocks";
  const caption =
    n === 0
      ? attack
        ? "Declare attackers"
        : "Declare blockers"
      : `${n} ${attack ? "attacking" : "blocking"}`;
  return (
    <>
      <div className="combat-actions">
        {everyone && (
          <Button variant="secondary" size="small" onClick={onAll}>
            Attack with All
          </Button>
        )}
        {n > 0 && (
          <Button variant="secondary" size="small" onClick={onClear}>
            Clear
          </Button>
        )}
      </div>
      <div className="turn-action">
        <TurnButton label={label} onClick={onDeclare} />
        <span className="turn-caption">{caption}</span>
      </div>
    </>
  );
}

/** What to do next in a declaration, for the step banner. */
function instruction(
  kind: "attacker" | "blocker",
  draft: Draft,
  walkers: boolean,
  nameOf: (slot: number) => string,
): string {
  const selected = draft.selected;
  if (kind === "attacker") {
    return walkers && selected !== undefined
      ? `Click a planeswalker for ${nameOf(selected)} to attack`
      : "Click creatures to attack with them";
  }
  return selected === undefined
    ? "Click a creature to block with"
    : `Click the attacker ${nameOf(selected)} blocks`;
}

/** The turn button at the rest of combat's decisions, which the engine asks only where a
 * declaration sent from the table could not say it all: what an attacker attacks, which
 * attacker a blocker blocks, how damage is divided. It waits for the choice on the table. */
function Finish({
  decision,
  done,
  onPick,
}: {
  decision: DecisionView;
  done?: CandidateIndex;
  onPick(index: CandidateIndex): void;
}) {
  const labels: Partial<Record<Kind, [string, string]>> = {
    attackTarget: ["Attack", "Choose what it attacks"],
    blockTarget: ["Block", "Choose an attacker to block"],
    damageDivision: ["Assign Damage", "Choose blockers to destroy"],
  };
  const [label, caption] = labels[decision.kind] ?? ["Done", prompt(decision)];
  return (
    <div className="turn-action">
      <TurnButton
        label={label}
        disabled={done === undefined}
        onClick={() => done !== undefined && onPick(done)}
      />
      <span className="turn-caption">{caption}</span>
    </div>
  );
}

/** A decision other than priority: what it asks, with a button for each candidate but the
 * cards picked on the table, where each glows with its side and state in view; a card off the
 * table, as one in a graveyard, shows its face. */
function Prompt({
  decision,
  skip,
  source,
  nameOf,
  objects,
  onTable,
  onPick,
  onPlan,
  onCancel,
  onPoint,
  confirm,
  hovered,
}: {
  decision: DecisionView;
  /** Choosing no target, where the target is picked on the board with its count. */
  skip?: CandidateIndex;
  source?: string;
  nameOf(slot: number): string;
  objects: Map<number, ObjectView>;
  /** Whether the card in a slot is shown on the table, to be clicked there. */
  onTable(slot: number): boolean;
  onPick(index: CandidateIndex): void;
  /** Answers with picks that span decisions, as a trigger order does. */
  onPlan(plan: Intent[]): void;
  /** Takes back the cast the decision is part of. */
  onCancel?(): void;
  /** The card off the table the pointer is on, if any, to show whole. */
  onPoint(slot: number | undefined): void;
  /** Confirms the cards picked on the table, which leaves the prompt no candidate to offer. */
  confirm?: { label: string; disabled: boolean; onClick(): void } | false;
  /** The card the pointer is on, which a crew meter previews. */
  hovered?: number;
}) {
  const { ask } = decision;
  // A payment plan reads as what it taps, never as a number; a payment asked of a spell's
  // controller by what it pays.
  const label = (c: DecisionView["candidates"][number]) => {
    if (ask.kind === "unlessPays") {
      return c.option.kind === "yes" ? `Pay ${ask.cost}` : "Don't Pay";
    }
    const taps = c.payments[0]?.taps;
    if (c.option.kind !== "plan" || !taps) return c.label;
    return taps.length > 0 ? `Tap ${tapList(taps.map(nameOf))}` : "Tap nothing";
  };
  // Cards picked on the table are not offered again; those off it show their faces.
  const offered = confirm
    ? []
    : decision.candidates
        .map((c, index) => ({ c, index, label: label(c) }))
        .filter(
          ({ c }) => c.option.kind !== "object" || !onTable(c.option.slot),
        );
  // Up to one target asks for the target, which the board takes with its count; the prompt
  // offers none.
  const listed =
    skip === undefined
      ? offered
      : [{ c: decision.candidates[skip]!, index: skip, label: "No Target" }];
  const faces = listed.flatMap(({ c, index }) => {
    const o = c.option.kind === "object" && objects.get(c.option.slot);
    return o ? [{ object: o, index }] : [];
  });
  const buttons = listed.filter(({ c }) => c.option.kind !== "object");
  const ordering = decision.kind === "triggerOrder";
  const [order, setOrder] = useState<CandidateIndex[]>([]);
  const title = ordering
    ? order.length === 0
      ? "Resolve first"
      : "Resolve next"
    : prompt(
        skip === undefined ? decision : { ...decision, kind: "target" },
        source,
      );
  return (
    <div className="prompt" role="dialog" aria-label={title}>
      <h2 className="prompt-title">
        {title}
        {ask.kind === "waterbend" && (
          <span className="prompt-cost">
            <CostText text={ask.cost} />
          </span>
        )}
      </h2>
      {ask.kind === "crew" && (
        <CrewMeter
          ask={ask}
          adding={
            hovered !== undefined &&
            decision.candidates.some(
              (c) => c.option.kind === "object" && c.option.slot === hovered,
            )
              ? (objects.get(hovered)?.pt?.power ?? 0)
              : 0
          }
        />
      )}
      {ordering ? (
        <TriggerOrder
          decision={decision}
          nameOf={nameOf}
          order={order}
          onOrder={setOrder}
          onPlan={onPlan}
        />
      ) : (
        <>
          {faces.length > 0 && (
            <div className="prompt-cards">
              {faces.map(({ object, index }) => (
                <PromptCard
                  key={index}
                  object={object}
                  onPick={() => onPick(index)}
                  onPoint={(on) => onPoint(on ? object.slot : undefined)}
                />
              ))}
            </div>
          )}
          {buttons.length > 0 && (
            <div className="prompt-choices">
              {buttons.map((b) => (
                <Button
                  key={b.index}
                  size="small"
                  variant="secondary"
                  onClick={() => onPick(b.index)}
                >
                  <RulesText text={b.label} />
                </Button>
              ))}
            </div>
          )}
        </>
      )}
      {(onCancel || confirm) && (
        <div className="cast-buttons">
          {onCancel && (
            <Button size="small" variant="secondary" onClick={onCancel}>
              Cancel
            </Button>
          )}
          {confirm && (
            <Button
              size="small"
              disabled={confirm.disabled}
              onClick={confirm.onClick}
            >
              {confirm.label}
            </Button>
          )}
        </div>
      )}
    </div>
  );
}

/** A candidate card off the table, as a graveyard's target, by its face. */
function PromptCard({
  object,
  onPick,
  onPoint,
}: {
  object: ObjectView;
  onPick(): void;
  onPoint(on: boolean): void;
}) {
  const face = useFace(object);
  return (
    <button
      type="button"
      className="prompt-card"
      data-option={object.slot}
      aria-label={object.name ?? "A card"}
      onClick={onPick}
      onPointerEnter={() => onPoint(true)}
      onPointerLeave={() => onPoint(false)}
    >
      <Card
        card={face}
        variant="field"
        width={PROMPT_CARD_WIDTH}
        highlight="target"
      />
    </button>
  );
}

/** Faces in the prompt's one row, as the battlefield draws cards: low enough that the prompt
 * stays under the lifted stack (layout's STACK_LIFT). */
const PROMPT_CARD_WIDTH = 80;

/** The order of simultaneous triggers ([CR 603.3b](https://yawgatog.com/resources/magic-rules/#R6033b):
 * "If multiple players have triggered abilities that have triggered since the last time a
 * player received priority, each player, in APNAP order, puts triggered abilities they control
 * on the stack in any order they choose"), chosen as they will resolve: each trigger by its
 * source and the paragraph that says it, numbered as it is picked, 1 resolving first. Once all
 * but one are numbered, the picks put them on the stack from the last to resolve up, the first
 * going on unasked; each pick after the first waits through the targets of the one before. */
function TriggerOrder({
  decision,
  nameOf,
  order,
  onOrder,
  onPlan,
}: {
  decision: DecisionView;
  nameOf(slot: number): string;
  order: CandidateIndex[];
  onOrder(order: CandidateIndex[]): void;
  onPlan(plan: Intent[]): void;
}) {
  const triggers = decision.candidates.map((c, index) => ({ c, index }));
  const choose = (index: CandidateIndex) => {
    const next = [...order, index];
    if (next.length < triggers.length - 1) onOrder(next);
    else onPlan(triggerPlan(decision, next));
  };
  return (
    <ol className="prompt-triggers">
      {triggers.map(({ c, index }) => {
        const source = c.option.kind === "trigger" ? c.option.source : null;
        const name = source === null ? c.label : nameOf(source);
        const text = faceEntry(name)?.oracle_text ?? "";
        const at = abilityParagraph(text, true, c.nth ?? 0);
        const n = order.indexOf(index);
        return (
          <li key={index}>
            <button
              type="button"
              className="prompt-trigger"
              aria-pressed={n >= 0}
              disabled={n >= 0}
              onClick={() => choose(index)}
            >
              <span className="prompt-trigger-order">
                {n >= 0 ? n + 1 : ""}
              </span>
              <span className="prompt-trigger-body">
                <span className="prompt-trigger-name">{name}</span>
                {at !== undefined && (
                  <span className="prompt-trigger-text">
                    <RulesText text={withoutReminder(text.split("\n")[at]!)} />
                  </span>
                )}
              </span>
            </button>
          </li>
        );
      })}
    </ol>
  );
}

/** A crew cost as pips, one per point of power: those the creatures tapped so far crew lit,
 * those the creature pointed at would crew half lit. */
function CrewMeter({
  ask,
  adding,
}: {
  ask: { crew: number; left: number };
  adding: number;
}) {
  const crewed = ask.crew - ask.left;
  return (
    <div
      className="crew-meter"
      role="meter"
      aria-label="Crew"
      aria-valuemin={0}
      aria-valuemax={ask.crew}
      aria-valuenow={crewed}
    >
      {Array.from({ length: ask.crew }, (_, i) => (
        <i
          key={i}
          className={
            i < crewed
              ? "crew-pip crew-pip-on"
              : i < crewed + adding
                ? "crew-pip crew-pip-next"
                : "crew-pip"
          }
        />
      ))}
    </div>
  );
}

/** The inspector for `object`, where `place` puts it (inspectorPlace), with what is attached
 * to it. */
function InspectAt({
  view,
  object,
  place,
  lit,
  refusal,
}: {
  view: PlayerView;
  object?: ObjectView;
  place: InspectPlace;
  lit?: number;
  refusal?: RefusalView;
}) {
  const by =
    refusal?.kind === "timing" && refusal.by !== null
      ? view.objects.find((o) => o.slot === refusal.by)?.name
      : undefined;
  return (
    <Inspect
      object={object}
      attached={view.objects.filter(
        (o) => o.zone === "battlefield" && o.attachedTo === object?.slot,
      )}
      place={place}
      lit={lit}
      refusal={refusal && { reason: refusal, by: by ?? undefined }}
    />
  );
}

/** A card as an option pointed at shows it: playing a land, its land face. */
function pointedObject(
  object: ObjectView | undefined,
  act: Act | undefined,
): ObjectView | undefined {
  const land =
    act?.option.kind === "playLand" && object?.name
      ? landFace(object.name)
      : undefined;
  return land && object ? { ...object, name: land.name } : object;
}

/** The paragraph of its card's rules text an activation's ability is, if the card says it. */
function litParagraph(
  act: Act,
  objects: Map<number, ObjectView>,
): number | undefined {
  const o = act.option;
  if (o.kind !== "activate" || act.nth === null) return undefined;
  const name = objects.get(o.slot)?.name;
  const text = name ? faceEntry(name)?.oracle_text : undefined;
  return text ? abilityParagraph(text, false, act.nth) : undefined;
}

/** A loyalty ability's paragraph: its loyalty cost, then a colon
 * ([CR 606.4](https://yawgatog.com/resources/magic-rules/#R6064): "The cost to activate a
 * loyalty ability of a permanent is to put on or remove from that permanent a certain number
 * of loyalty counters, as shown by the loyalty symbol in the ability's cost"). */
const LOYALTY = /^[+\u2212-]?(\d+|X): /;

/** For a planeswalker whose activations are all a click offers, the paragraphs of its rules
 * text that say the loyalty abilities it cannot activate now, for its menu to show disabled;
 * otherwise none, and the click does what it does for any card. */
function loyaltyMenu(
  o: ObjectView | undefined,
  acts: Act[],
): number[] | undefined {
  if (
    !o?.name ||
    o.zone !== "battlefield" ||
    !o.types.includes("planeswalker") ||
    !acts.every((a) => a.option.kind === "activate")
  ) {
    return undefined;
  }
  const text = faceEntry(o.name)?.oracle_text;
  if (!text) return undefined;
  const offered = new Set(
    acts.map((a) => a.nth !== null && abilityParagraph(text, false, a.nth)),
  );
  return text
    .split("\n")
    .flatMap((p, i) => (LOYALTY.test(p) && !offered.has(i) ? [i] : []));
}

/** One line of a card's menu: what it costs, in symbols, and what it does, from its
 * paragraph or else (`labelled`) the candidate's label; a loyalty ability the planeswalker
 * cannot activate has no candidate. `at` is its paragraph in the card's rules text. */
interface MenuEntry {
  act?: Act;
  at?: number;
  cost?: string;
  effect: string;
  labelled?: boolean;
}

/** What an ability whose cost is `cost` does, where only a keyword's reminder text says it:
 * cycling's "({3}, Discard this card: Draw a card.)". */
function reminded(paragraphs: string[], cost: string): string | undefined {
  for (const p of paragraphs) {
    for (const [, said] of p.matchAll(/\(([^)]*)\)/g)) {
      if (said!.startsWith(`${cost}: `)) return said!.slice(cost.length + 2);
    }
  }
  return undefined;
}

/** The lines of a card's menu: each candidate, by its cost and effect (an activation's from
 * its paragraph, a cast's the card's mana cost), and a planeswalker's unaffordable loyalty
 * abilities among them, in the order its rules text lists them. */
function menuEntries(
  menu: Menu,
  objects: Map<number, ObjectView>,
): MenuEntry[] {
  const name = objects.get(menu.slot)?.name ?? undefined;
  const text = name ? faceEntry(name)?.oracle_text : undefined;
  const paragraphs = text?.split("\n") ?? [];
  const entries: MenuEntry[] = menu.acts.map((act) => {
    const o = act.option;
    if (o.kind === "activate") {
      const at = litParagraph(act, objects);
      const said = at === undefined ? undefined : paragraphs[at];
      const cost =
        name && act.label.startsWith(`${name}: `)
          ? act.label.slice(name.length + 2)
          : undefined;
      if (said === undefined) {
        return cost === undefined
          ? { act, effect: act.label, labelled: true }
          : { act, cost, effect: reminded(paragraphs, cost) ?? "" };
      }
      return {
        act,
        at,
        cost,
        effect: withoutReminder(said.slice(said.indexOf(": ") + 2)),
      };
    }
    if (o.kind === "cast") {
      const cost = name ? cardEntry(name)?.faces[o.face]?.mana_cost : undefined;
      return {
        act,
        cost: cost || undefined,
        effect: act.label,
        labelled: true,
      };
    }
    return { act, effect: act.label, labelled: true };
  });
  for (const at of menu.unaffordable ?? []) {
    const p = paragraphs[at]!;
    const colon = p.indexOf(": ");
    entries.push({
      at,
      cost: p.slice(0, colon),
      effect: withoutReminder(p.slice(colon + 2)),
    });
  }
  return menu.unaffordable
    ? entries.sort((a, b) => (a.at ?? -1) - (b.at ?? -1))
    : entries;
}

/** What a card offers, listed where it was clicked or dropped, each by what it costs and
 * does; pointing at an option shows the card with its paragraph lit. */
function ActionMenu({
  menu,
  title,
  objects,
  onPick,
  onPoint,
  onClose,
}: {
  menu: Menu;
  title?: string;
  objects: Map<number, ObjectView>;
  onPick(act: Act): void;
  onPoint(act: Act | undefined): void;
  onClose(): void;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  // Kept on the stage: opened near an edge, it opens away from it.
  const left = Math.min(menu.at.x, 1600 - 330);
  const bottom = menu.at.y > 450;
  return (
    <>
      <div className="menu-scrim" onPointerDown={onClose} />
      <div
        className="action-menu"
        role="group"
        aria-label={title}
        style={
          bottom
            ? { left, bottom: 900 - menu.at.y + 8 }
            : { left, top: menu.at.y + 8 }
        }
      >
        {title && <div className="action-menu-title">{title}</div>}
        {menuEntries(menu, objects).map(
          ({ act, at, cost, effect, labelled }) => (
            <button
              key={act ? `act-${act.index}` : `text-${at}`}
              type="button"
              className="action-menu-item"
              disabled={!act}
              onClick={() => act && onPick(act)}
              onPointerEnter={() => act && onPoint(act)}
              onPointerLeave={() => act && onPoint(undefined)}
            >
              {cost !== undefined && (
                <span className="action-menu-cost">
                  <CostText text={cost} />
                </span>
              )}
              {effect && (
                <span
                  className={
                    labelled
                      ? "action-menu-effect action-menu-label"
                      : "action-menu-effect"
                  }
                >
                  <RulesText text={effect} />
                </span>
              )}
            </button>
          ),
        )}
      </div>
    </>
  );
}
