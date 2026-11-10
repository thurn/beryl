// The battlefield screen of a game in progress, drawn from the latest PlayerView as
// docs/ui_mockups/battlefield.png lays it out: the opponent's hand as card backs, life orbs,
// land piles, permanents, libraries and graveyards, player badges, the phase tracker and the
// turn button, with the hand fanned at the near edge (decisions D10 to D13). Combat and the
// stack follow combat.png and trigger_on_stack_with_target.png: attack and block lines and the
// step banner; spells and abilities large at the right, red or green lines to targets and, while one is
// pointed at, a gold line from an ability's source.
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type RefObject,
} from "react";
import {
  animate,
  AnimatePresence,
  motion,
  useIsPresent,
  useMotionValue,
  useReducedMotion,
} from "motion/react";
import { useWaterbend } from "./waterbend";
import { cardEntry, faceEntry, landFace } from "../cards";
import { BugIcon, requestBugReport } from "../components/BugReport";
import { CARD_ASPECT, Card, type Highlight } from "../components/Card";
import {
  Button,
  Dock,
  DockButtons,
  Tip,
  TurnButton,
  FastForwardIcon,
} from "../components/Controls";
import {
  CostText,
  MANA_SYMBOLS,
  ManaSymbol,
  RulesText,
} from "../components/Mana";
import {
  Coin,
  floating,
  LifeOrb,
  ManaPool,
  PlayerBadge,
  Thinking,
} from "../components/Player";
import { StepStops } from "../components/Steps";
import {
  Arrows,
  StackItem,
  StackMarks,
  type ArrowSpec,
} from "../components/Stack";
import type {
  Candidate,
  CandidateIndex,
  DecisionView,
  Kind,
  ObjectView,
  PlayerView,
  RefusalView,
  Side,
  StackItem as StackItemView,
  StopsSetting,
} from "../generated/protocol";
import { announcedItem, sameItem } from "./beats";
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
  moveVerb,
  prompt,
  slotOf,
  tapsOnly,
  triggerPlan,
  withResolving,
  type Act,
  type Intent,
} from "./decisions";
import {
  aimAttacks,
  attackPlan,
  blockable,
  blockPlan,
  declared,
  emptyDraft,
  incoming,
  pendingAttackers,
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
import { CastCaption, CastStep } from "./CastStep";
import { ModeChooser, type Mode, type Origin } from "./ModeChooser";
import { eventsHold } from "./playback";
import { Risen } from "./Risen";
import { useBorrowedCard, type CardHandoff } from "./choiceFlight";
import { useStageWidth } from "./Stage";
import { FloatPrompt, PromptRow } from "./FloatPrompt";
import { useStandIns } from "./standIns";
import { tablePick } from "./tablePick";
import {
  combatBanner,
  emblemObjects,
  emblemSlot,
  exiledWith,
  held,
  handlingSlots,
  footprint,
  fromMiddle,
  isEmblem,
  FULL_WIDTH,
  HALT_BUTTON,
  layout,
  linked,
  MANA_POOLS,
  overLog,
  passButton,
  sameHandling,
  STACK,
  STACK_Z,
  stackPlace,
  STEP_NAMES,
  STAGE,
  zonePiles,
  knownLibraries,
  type Box,
  type Layout,
  type Placement,
  type Spotlight,
  type ZonePile,
} from "./layout";
import {
  abilityChoices,
  playChoices,
  type AbilityChoice,
  type PlayChoice,
} from "./AbilityChooser";
import type { LogEntry } from "./log";
import { GameLog, opponentActing } from "./Narration";
import {
  Inspect,
  INSPECT_WIDTH,
  inspectorBox,
  type InspectPlace,
  Lying,
  pileMark,
  PileButton,
  ZoneViewer,
} from "./Piles";
import { ManaFlow, manaMotes } from "./ManaFlow";
import { TurnBanner } from "./TurnBanner";
import { SoundPanel, useOpenSound } from "../audio/SoundControls";
import { play as playSound } from "../audio/sound";

export interface BoardProps {
  view: PlayerView;
  /** The view was shown as the game played on: a new turn in it is announced. */
  live: boolean;
  /** Cards shown apart from where they lie (beats.ts): on their way to a pile, or just drawn
   * by the opponent. */
  spotlight?: Spotlight;
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
  /** Takes back the player's latest answer, at a decision that may (`DecisionView.undo`). */
  undo?(): void;
  /** Passes on to the player's next stop, at a priority that may (`DecisionView.passOn`). */
  passOn?(): void;
  /** The player's stops (decision D13), as last chosen: the view's until a change arrives. */
  stops: StopsSetting;
  /** Changes the player's stops: full control, or a phase stop (stops.ts). */
  setStops(stops: StopsSetting): void;
  onLeave(): void;
  /** Concedes the game; none once it is over. */
  onConcede?: () => void;
  /** Blurred and darkened behind a screen over it. */
  dimmed?: boolean;
  /** A screen over it answers the decision: the table only shows. */
  inert?: boolean;
  /** Cards in an open choice tray, excluded from the table pile counts. */
  traySlots?: number[];
}

/** Dropped above this line, a hand card is on the battlefield rather than back in the hand. */
const BATTLEFIELD_EDGE = 690;
/** Dragged forward at least this far, a creature attacks. */
const FORWARD = 60;

/** A menu of a card's candidates, open at a point on the stage; `pointed`, the one under
 * the pointer. A permanent's abilities are chosen in the ability chooser instead. */
interface Menu {
  slot: number;
  at: Point;
  acts: Act[];
  waterbend?: boolean;
  pointed?: Act;
}

/** A card's cast being set up in the cast step. */
interface Casting {
  slot: number;
  casts: Act[];
  choice: CastChoice;
}

const NO_SPOTLIGHT: Spotlight = {};

export function Board({
  view: given,
  live,
  spotlight = NO_SPOTLIGHT,
  log,
  waiting,
  onSkip,
  choose,
  cancel,
  undo,
  passOn,
  stops,
  setStops,
  onLeave,
  onConcede,
  dimmed = false,
  inert = false,
  traySlots,
}: BoardProps) {
  // The table shows what is resolving on the stack while its resolution asks a question.
  const view = useMemo(() => withResolving(given), [given]);
  // Placed afresh when the window changes shape, as the stage then does (layout.ts STAGE).
  const stageWidth = useStageWidth();
  // The view shown before this one, whose stack says what just resolved.
  const views = useRef<{ last?: PlayerView; before?: PlayerView }>({});
  if (views.current.last !== view) {
    views.current = { last: view, before: views.current.last };
  }
  const [hovered, setHovered] = useState<number>();
  // The ability on the stack under the pointer, by its place and name (`data-stack-key`): one
  // that leaves under the pointer sends no event, and its key then names nothing.
  const [pointedItem, setPointedItem] = useState<string>();
  // The hand card a click found nothing to do with, which the inspector says why of while
  // the pointer stays on it.
  const [refused, setRefused] = useState<number>();
  const [timingNotice, setTimingNotice] = useState<{
    view: PlayerView;
    source: number;
    id: number;
  }>();
  const noticeId = useRef(0);
  useEffect(() => {
    if (!timingNotice) return;
    const timer = setTimeout(() => setTimingNotice(undefined), 1800);
    return () => clearTimeout(timer);
  }, [timingNotice]);
  const [menu, setMenu] = useState<Menu>();
  // The card whose uses are being chosen among (its casts, its activated abilities), and
  // where they sprang from.
  const [chooser, setChooser] = useState<{
    id: number;
    view: PlayerView;
    object: ObjectView;
    modes: Mode[];
    from?: Origin;
    split?: boolean;
  }>();
  const chooserId = useRef(0);
  const [pickedMode, setPickedMode] = useState<{
    chooser: NonNullable<typeof chooser>;
    key: string;
    action: Candidate;
    from: CardHandoff;
    faceName?: string | null;
  }>();
  const [returnedMode, setReturnedMode] = useState<{
    id: number;
    key: string;
    slot: number;
    from: CardHandoff;
    faceName?: string | null;
  }>();
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
  const [stackPick, setStackPick] = useState<{ view: PlayerView; act: Act }>();
  // The card being dragged.
  const [lifted, setLifted] = useState<number>();
  // The card off the table the pointer is on in the prompt, shown whole.
  const [pointedOff, setPointedOff] = useState<number>();
  const [settings, setSettings] = useState(false);
  /** How many times the player passed with mana unspent: each pulses their pool. */
  const [lost, setLost] = useState(0);
  // The graveyard or exile pile open in the zone viewer, which stays open as views go by; or
  // the cards exiled with the permanent in `linked`.
  const [viewing, setViewing] = useState<
    Pick<ZonePile, "side" | "zone"> & { linked?: number }
  >();
  const can = useMemo(() => interactions(view), [view]);
  // The hand card the player handles is the one that moves, of identical ones.
  const standIns = useStandIns(view);
  // Under a screen over the table (the opening hand, a choice, the game's end), the table only
  // shows.
  const deciding =
    view.status.kind === "deciding" && !waiting && !dimmed && !inert;
  const decision =
    view.status.kind === "deciding" ? view.status.decision : undefined;
  // Emblems are drawn as cards (layout's emblemObjects), shown whole when pointed at.
  const emblems = useMemo(() => emblemObjects(view), [view]);
  const objects = useMemo(
    () => new Map([...view.objects, ...emblems].map((o) => [o.slot, o])),
    [view, emblems],
  );
  const heldGroup = useRef<{ view: PlayerView; slots: number[] }>(undefined);
  const previousGroup = heldGroup.current;
  const handledSlots =
    previousGroup &&
    (previousGroup.view === view ||
      (view.handling?.zone === "library" &&
        sameHandling(previousGroup.view, view)))
      ? previousGroup.slots
      : handlingSlots(view, standIns.order);
  heldGroup.current = { view, slots: handledSlots };
  const waterbend = useWaterbend(decision, objects);
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
  // A stack spell may be preselected, but it becomes a target only on confirmation.
  const stackTargets =
    deciding && decision?.kind === "target"
      ? decision.candidates.flatMap(({ option, ...candidate }, index) => {
          const object = option.kind === "object" && objects.get(option.slot);
          return object && object.zone === "stack"
            ? [{ act: { index, option, ...candidate }, object }]
            : [];
        })
      : [];
  const opposingSpells = stackTargets.filter(
    ({ object }) => object.controller === "opponent",
  );
  const defaultSpell =
    stackTargets.length > 1 &&
    stackTargets.length === decision?.candidates.length &&
    opposingSpells.length === 1
      ? opposingSpells[0]?.act
      : undefined;
  const selectedSpell =
    stackTargets.length > 0 && stackPick?.view === view
      ? stackPick.act
      : defaultSpell;
  const sourceSpell =
    decision?.source == null ? undefined : objects.get(decision.source);
  const counteringSpell =
    sourceSpell?.zone === "stack" &&
    sourceSpell.name &&
    /^Counter target /m.test(faceEntry(sourceSpell.name)?.oracle_text ?? "");
  const selfCounter = (act: Act) =>
    counteringSpell &&
    stackTargets.some(
      ({ act: target, object }) =>
        target.index === act.index && object.controller === "you",
    );
  const stackConfirmation = selectedSpell && (() => pick(selectedSpell.index));
  // The table as the declaration would leave it.
  const shown = useMemo(() => {
    const declaration =
      decision?.kind === "attacker" || decision?.kind === "blocker";
    const declaredView = declaration ? declared(view, draft) : view;
    if (!waterbend.active) return declaredView;
    // Preview the selected physical cards without committing the payment.
    return {
      ...declaredView,
      objects: declaredView.objects.map((o) =>
        waterbend.selected.includes(o.slot) ? { ...o, tapped: true } : o,
      ),
    };
  }, [view, draft, decision, waterbend.active, waterbend.selected]);
  // Every card the table draws: the objects shown, and the emblems.
  const drawn = useMemo(() => [...shown.objects, ...emblems], [shown, emblems]);
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
  const pickedAction = pickedMode?.action;
  const chosenAbility =
    pickedAction?.kind === "activate"
      ? view.stack.findIndex((item, i) => {
          const prior = pickedMode!.chooser.view.stack[i];
          return (
            item.kind === "ability" &&
            !item.triggered &&
            item.controller === "you" &&
            item.source === pickedAction.slot &&
            item.ability === pickedAction.ability &&
            item.granted === pickedAction.granted &&
            (!prior || stackKey(i, item) !== stackKey(i, prior))
          );
        })
      : -1;
  const choosingCard =
    pickedAction?.kind === "playLand" || pickedAction?.kind === "cast";
  const acted = choosingCard ? objects.get(pickedAction.slot) : undefined;
  const sourceSlot = pickedMode?.chooser.object.slot;
  const chosenCard =
    choosingCard && castingNow?.slot === sourceSlot
      ? sourceSlot
      : acted && acted.zone !== pickedMode?.chooser.object.zone
        ? acted.slot
        : undefined;
  const handedMode =
    returnedMode ??
    (pickedMode && (chosenAbility >= 0 || chosenCard !== undefined)
      ? { id: pickedMode.chooser.id, key: pickedMode.key }
      : undefined);
  const handedRef = useRef<typeof handedMode>(undefined);
  if (handedMode) handedRef.current = handedMode;
  const pendingMode = waiting && !handedMode ? pickedMode : undefined;
  const activeChooser =
    deciding && chooser?.view === view ? chooser : undefined;
  const shownChooser = activeChooser ?? pendingMode?.chooser;
  const modeHandoff =
    returnedMode ??
    (chosenCard !== undefined && pickedMode
      ? {
          slot: chosenCard,
          from: pickedMode.from,
          faceName: pickedMode.faceName,
        }
      : undefined);
  useEffect(() => {
    // Hold the selected frame while the engine accepts the action. A rejected action
    // releases it to the same exit as Cancel.
    if (pickedMode && (!waiting || handedMode)) setPickedMode(undefined);
  }, [pickedMode, waiting, handedMode?.id]);
  useEffect(() => {
    if (returnedMode) setReturnedMode(undefined);
  }, [returnedMode]);
  // The cast under way, which the engine is asking about, can still be taken back: on the
  // table, or from it while a screen asking about the cast is put aside to see the table
  // (inert, but not dimmed).
  /** What the priority pill says while you wait, if anything. */
  const cancellable = !waiting && !dimmed && decision?.cancellable === true;
  // An answer can be taken back, one per press.
  const undoable = !waiting && undo !== undefined && (decision?.undo ?? 0) > 0;
  // The prompt or the cast step sits over the turn controls; the stack rises to clear it.
  const asking = prompting || castingNow !== undefined;
  // The log stays folded to its button until opened; opened while the rows reach into its
  // column, the rows give it room.
  const [logOpen, setLogOpen] = useState(false);
  const toggleLog = useCallback(() => setLogOpen((o) => !o), []);
  const libraryChoice = useMemo(
    () =>
      [...log]
        .reverse()
        .find((entry) => entry.side === "opponent" && entry.kind !== "turn")
        ?.libraryChoice,
    [log],
  );
  // The hand sinks under a screen over the table, and rises whole for a choice of its cards,
  // once what the view shows has been seen (the card a loot draws arriving in it).
  const [seen, setSeen] = useState<PlayerView>();
  useEffect(() => {
    if (!live || !picking?.fromHand || view.events.length === 0) {
      setSeen(view);
      return;
    }
    const wait = setTimeout(
      () => setSeen(view),
      Math.min(eventsHold(view), RAISE_WAIT),
    );
    return () => clearTimeout(wait);
  }, [view, live, picking]);
  // Before the first turn the screen over the table is the opening, which shows the hand.
  const hold = dimmed
    ? view.turn.number === 0
      ? "away"
      : "tucked"
    : picking?.fromHand && seen === view
      ? "raised"
      : "held";
  const piles = useMemo(
    () => zonePiles(shown, standIns.exileOrder),
    [shown, stageWidth, standIns.exileOrder],
  );
  const libraries = useMemo(() => knownLibraries(shown), [shown, stageWidth]);
  const pile = [...piles, ...libraries].find(
    (p) => p.side === viewing?.side && p.zone === viewing.zone,
  );
  const viewed =
    pile && viewing?.linked !== undefined
      ? { ...pile, cards: exiledWith(shown, viewing.linked) }
      : pile;
  const away = useMemo(() => {
    const slots = new Set(traySlots);
    if (!dimmed) for (const card of viewed?.cards ?? []) slots.add(card.slot);
    return slots;
  }, [traySlots, dimmed, viewed]);

  const reduceMotion = useReducedMotion();
  const visibleSpotlight = useMemo(
    () =>
      reduceMotion && spotlight.shuffle
        ? { ...spotlight, shuffle: undefined }
        : spotlight,
    [reduceMotion, spotlight],
  );
  const resting = useMemo(
    () =>
      layout(
        shown,
        hold,
        false,
        visibleSpotlight,
        standIns.order,
        away,
        piles,
        handledSlots,
      ),
    [
      shown,
      hold,
      visibleSpotlight,
      standIns.order,
      away,
      piles,
      handledSlots,
      stageWidth,
    ],
  );
  const crowdsLog = overLog(resting);
  const laid = useMemo(
    () =>
      crowdsLog && logOpen
        ? layout(
            shown,
            hold,
            true,
            visibleSpotlight,
            standIns.order,
            away,
            piles,
            handledSlots,
          )
        : resting,
    [
      resting,
      crowdsLog,
      logOpen,
      shown,
      hold,
      visibleSpotlight,
      standIns.order,
      away,
      piles,
      handledSlots,
    ],
  );
  // The table as the view before this one laid it: where a source sacrificed for its mana was.
  const tables = useRef<{ view?: PlayerView; laid?: Layout; before?: Layout }>(
    {},
  );
  if (tables.current.view !== view) {
    tables.current = { view, laid, before: tables.current.laid };
  } else {
    tables.current.laid = laid;
  }
  const previousTable = tables.current.before;
  const sourcePlaces = useRef(new Map<number, Placement>());
  // Mana flowing from its sources into what it paid for, as the game plays on.
  const motes = useMemo(
    () =>
      live ? manaMotes(views.current.before, view, laid, previousTable) : [],
    // Once per view: the table settling under it (the log opening) sends none again.
    [view, live],
  );
  // In the raised hand, the cards picked stand higher than the rest.
  const placements = useMemo(() => {
    if (hold !== "raised" || picked.size === 0) return laid;
    const out = new Map(laid);
    for (const slot of picked) {
      const p = out.get(slot);
      if (p?.look === "hand") out.set(slot, { ...p, y: p.y - PICKED_RISE });
    }
    return out;
  }, [laid, hold, picked]);
  useEffect(() => {
    for (const [slot, place] of placements) {
      if (place.opacity > 0) sourcePlaces.current.set(slot, place);
    }
  }, [placements]);
  // The hovered card, unless it has since gone where it is not shown alone.
  const hoveredPlace =
    hovered === undefined ? undefined : placements.get(hovered);
  // The raised hand's cards are large enough to read where they stand.
  const inspected =
    hoveredPlace?.look === "hidden" ||
    hoveredPlace?.look === "pile" ||
    hoveredPlace?.look === "handling" ||
    (hold === "raised" && hoveredPlace?.look === "hand")
      ? undefined
      : hoveredPlace;
  // Why the refused card under the pointer can't be played, as the engine says.
  const refusal =
    deciding && refused === hovered
      ? decision?.refused.find((r) => r.slot === refused)?.reason
      : undefined;
  const noticeSource =
    deciding && timingNotice?.view === view
      ? placements.get(timingNotice.source)
      : undefined;
  const noticeBox = noticeSource && footprint(noticeSource);
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
  // A new view closes whatever was open on the old one, and starts any declaration afresh.
  useEffect(() => {
    setMenu(undefined);
    setChooser(undefined);
    setCasting(undefined);
    setDraft(emptyDraft());
    setPointedOff(undefined);
    setPointedItem((key) =>
      view.stack.some((item, index) => stackKey(index, item) === key)
        ? key
        : undefined,
    );
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
  /** Where the card in `slot` lies, for the mode chooser to spring from: at `at`, let go
   * there, or else its place. */
  const originOf = (slot: number, at?: Point): Origin | undefined => {
    const p = placements.get(slot);
    if (!p) return at && { ...at, width: FULL_WIDTH };
    const width = p.width * p.scale;
    if (at) return { ...at, width };
    return { x: p.x + width / 2, y: p.y + width / CARD_ASPECT / 2, width };
  };
  /** Opens the mode chooser on a permanent's activated abilities. */
  const chooseAbility = (
    slot: number,
    choices: AbilityChoice[],
    at?: Point,
  ) => {
    const o = objects.get(slot);
    if (!o) return;
    const label = o.types.includes("planeswalker")
      ? "Loyalty Ability"
      : "Activated Ability";
    setMenu(undefined);
    setChooser({
      id: ++chooserId.current,
      view,
      object: o,
      from: originOf(slot, at),
      modes: choices.map((c) => ({
        key: `ability-${c.at}`,
        name: c.text,
        action: c.act?.option,
        look: { kind: "ability", label, text: c.text },
        onPick: c.act && (() => pickAct(slot, c.act!)),
      })),
    });
  };
  /** Opens the mode chooser on a card's uses off the battlefield: its casts, its land play,
   * its abilities activated from where it lies. */
  const choosePlay = (
    slot: number,
    choices: PlayChoice[],
    from = originOf(slot),
    split = false,
  ) => {
    const o = objects.get(slot);
    if (!o) return;
    const name = o.name ?? "";
    // The printed cast (or the land play for a cycling land) is the primary use.
    const primary =
      choices.find(
        ({ act: { option } }) =>
          option.kind === "cast" && option.costs.length === 0,
      ) ?? choices.find(({ act }) => act.option.kind === "playLand");
    const ordered = primary
      ? [...choices.filter((choice) => choice !== primary), primary]
      : choices;
    // A cast or a land played shows the face it is by: a modal card's back, a land's face.
    const faceOf = (act: Act) => {
      if (act.option.kind === "playLand") return pointedObject(o, act);
      const face =
        act.option.kind === "cast" && act.option.face > 0
          ? cardEntry(name)?.faces[act.option.face]?.name
          : undefined;
      return face ? { ...o, name: face } : o;
    };
    setMenu(undefined);
    setChooser({
      id: ++chooserId.current,
      view,
      object: o,
      from,
      split,
      modes: ordered.map(({ act, text }) => {
        if (act.option.kind === "cast") {
          return {
            key: `act-${act.index}`,
            name: act.label,
            primary: act === primary?.act,
            action: act.option,
            look: { kind: "card", object: faceOf(act) },
            caption: <CastCaption act={act} name={name} />,
            onPick: () => {
              if (stepsIn([act])) setUp(slot, [act]);
              else cast(firstChoice(act));
            },
          };
        }
        return {
          key: `act-${act.index}`,
          name: act.label,
          primary: act === primary?.act,
          action: act.option,
          look:
            act.option.kind === "playLand"
              ? { kind: "card", object: faceOf(act) }
              : {
                  kind: "ability",
                  label: "Activated Ability",
                  text: text ?? act.label,
                },
          caption: act.option.kind === "playLand" ? "Play" : undefined,
          onPick: () => pick(act.index, act.then),
        };
      }),
    });
  };
  /** Whether casting from `casts` stops in the cast step: to choose which cast or X or, under
   * full control (Arena's), which permanents pay. Otherwise the client pays, as Arena's
   * auto-tapper does, the way the session ranks first. */
  const stepsIn = (casts: Act[]) =>
    asksCastStep(casts) ||
    (view.stops.fullControl && plansOf(firstChoice(casts[0]!)).length > 1);
  /** Casts from a click: at once, as a drop does, unless there is a choice to offer: which
   * cast, in the mode chooser, or its X or payment, in the cast step. */
  const castOrSetUp = (slot: number, casts: Act[], at?: Point) => {
    if (casts.length > 1) {
      choosePlay(
        slot,
        casts.map((act) => ({ act })),
        originOf(slot, at),
      );
    } else if (stepsIn(casts)) setUp(slot, casts);
    else cast(firstChoice(casts[0]!));
  };
  const editWaterbend = (slot: number, act?: Act) => {
    const option = act?.option;
    if (
      !waterbend.toggle(
        slot,
        option?.kind === "mana" ? option.color : undefined,
      )
    )
      return false;
    setMenu(undefined);
    playSound("select");
    return true;
  };

  /** A menu's pick: a cast, with all of the card's casts, is made as a click makes it. */
  const pickAct = (slot: number, act: Act) => {
    if (deciding && waterbend.active) {
      editWaterbend(slot, act);
      return;
    }
    const casts = castActs(can.bySlot.get(slot) ?? []);
    if (act.option.kind === "cast") castOrSetUp(slot, casts);
    else pick(act.index, act.then);
  };

  const states = new Map<number, CardState>();
  if (deciding) {
    for (const [slot, acts] of can.bySlot) {
      const single = can.picking && acts.length === 1;
      states.set(slot, {
        highlight: single ? "target" : undefined,
        // Every untapped source can tap for mana at priority, which no glow need say.
        playable: !single && !tapsOnly(acts),
        // A card in your hand fan, or one it holds because you can play it from elsewhere,
        // and a creature to declare. A permanent's abilities are only ever clicked: a drag
        // that activates reads as moving the card.
        draggable:
          (placements.get(slot)?.look === "hand" &&
            playActs(acts).length > 0) ||
          acts.some(
            (a) => a.option.kind === "attack" || a.option.kind === "block",
          ),
      });
    }
  }
  if (selectedSpell?.option.kind === "object") {
    const slot = selectedSpell.option.slot;
    states.set(slot, { ...states.get(slot), highlight: "source" });
  }
  const splitting = activeChooser?.split
    ? activeChooser.object.slot
    : pendingMode && choosingCard
      ? pendingMode.chooser.object.slot
      : undefined;
  if (splitting !== undefined) {
    states.set(splitting, { ...states.get(splitting), splitting: true });
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
    [
      top?.kind === "ability" && decision?.ask.kind !== "attackTarget"
        ? top.source
        : undefined,
      "source",
    ],
  ];
  for (const t of top?.targets ?? []) {
    if (t.kind === "object") marks.push([t.slot, "target"]);
  }
  for (const [slot, highlight] of marks) {
    if (slot == null || states.get(slot)?.highlight) continue;
    states.set(slot, { ...states.get(slot), highlight });
  }
  if (can.source !== undefined && decision?.ask.kind !== "attackTarget") {
    states.set(can.source, {
      ...states.get(can.source),
      highlight: "source",
    });
  }
  if (decision?.ask.kind === "attackTarget") {
    for (const slot of decision.ask.attackers) {
      states.set(slot, { ...states.get(slot), highlight: "source" });
    }
  }
  // The card a "you may" move asks about, where the table shows it: risen above its place,
  // as above the graveyard it lies on top of.
  const risen = useMemo(() => {
    if (!prompting || decision?.ask.kind !== "mayMove") return undefined;
    const slot = decision.ask.card;
    const p = placements.get(slot);
    if (!p || p.look === "hidden" || p.opacity === 0) return undefined;
    const width = p.width * p.scale;
    return { slot, at: { x: p.x + width / 2, y: p.y } };
  }, [prompting, decision, placements]);
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
    waterbend.active
      ? waterbend.taps
      : paying
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
  for (const slot of waterbend.benders) {
    states.set(slot, { ...states.get(slot), waterbending: true });
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
  const aimingAttack =
    drafting === "attacker" &&
    walkers.length > 0 &&
    draft.attackGroup.length > 0;
  if (drafting) {
    for (const [slot, state] of states) {
      if (!declarable(slot)) continue;
      const picked = draft.attacks.has(slot) || draft.blocks.has(slot);
      states.set(slot, {
        ...state,
        highlight:
          (drafting === "blocker" && slot === draft.selected) ||
          (aimingAttack && draft.attackGroup.includes(slot))
            ? "source"
            : picked || (drafting === "blocker" && draft.selected !== undefined)
              ? undefined
              : "target",
        playable: false,
      });
    }
    const aims =
      drafting === "blocker" ? [...blockTargets] : aimingAttack ? walkers : [];
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

  /** Opens the viewer on the cards exiled with the permanent in `slot`. */
  const viewExiled = (slot: number) => {
    const o = objects.get(slot);
    setMenu(undefined);
    if (o) setViewing({ side: o.owner, zone: "exile", linked: slot });
  };

  /** A click on the card in `slot`: what it offers, or nothing (false). */
  const onClick = (slot: number, at: Point): boolean => {
    if (isEmblem(slot)) return false;
    if (deciding && waterbend.active) {
      const acts = can.bySlot.get(slot) ?? [];
      if (waterbend.selected.includes(slot) || acts.length <= 1)
        return editWaterbend(slot, acts[0]);
      setMenu({
        slot,
        at,
        waterbend: true,
        acts: acts.map((act) =>
          act.option.kind === "object" ? { ...act, label: "Waterbend" } : act,
        ),
      });
      return true;
    }
    standIns.touch(slot, deciding ? can.bySlot.get(slot) : undefined);
    if (castingNow) {
      // A permanent another payment taps pays that way.
      const plan = plansOf(castingNow.choice).find((p) =>
        p.taps.includes(slot),
      );
      if (plan && !taps.has(slot)) {
        playSound("select");
        setCasting({
          ...castingNow,
          choice: { ...castingNow.choice, plan: plan.plan ?? undefined },
        });
        return true;
      }
      if (slot === castingNow.slot) return true;
      setCasting(undefined);
    }
    // A pile of identical permanents is taken from the top, whichever of its cards is
    // clicked, as the engine takes the lowest slot of interchangeable ones, which lies on top.
    const pile = placements.get(slot)?.pile ?? [slot];
    if (drafting) {
      playSound("select");
      setDraft(drafted(draft, pile[0]!));
      return true;
    }
    if (picking) {
      if (!picking.eligible.has(slot)) {
        playSound("no");
        return false;
      }
      playSound("select");
      // Picked, its pile gives back its deepest pick; otherwise its topmost card not yet
      // picked is picked, so a pile's picks are always its top cards.
      const mates = pile.filter((s) => picking.eligible.has(s));
      const next = new Set(picked);
      if (picked.has(slot)) {
        next.delete([...mates].reverse().find((s) => picked.has(s))!);
      } else if (next.size < picking.max) {
        next.add(mates.find((s) => !picked.has(s))!);
      } else if (picking.max === 1) {
        // One to pick: another card takes the pick's place.
        next.clear();
        next.add(mates.find((s) => !picked.has(s))!);
      }
      setPicks({ view, picked: next });
      return true;
    }
    const acts = deciding ? can.bySlot.get(slot) : undefined;
    // A card peeking out from behind the permanent holding it in exile opens them all.
    const clicked = objects.get(slot);
    const host = clicked && held(clicked, new Set(view.battlefield));
    if (!acts && host !== undefined) {
      viewExiled(host);
      return true;
    }
    if (!acts) {
      playSound("no");
      setMenu(undefined);
      setRefused(slot);
      const reason = deciding
        ? decision?.refused.find((r) => r.slot === slot)?.reason
        : undefined;
      const source =
        reason?.kind === "timing" && reason.by !== null
          ? objects.get(reason.by)
          : undefined;
      setTimingNotice(
        source?.name &&
          source.zone === "battlefield" &&
          source.controller === "opponent"
          ? { view, source: source.slot, id: ++noticeId.current }
          : undefined,
      );
      return false;
    }
    const casts = castActs(acts);
    const choices = abilityChoices(clicked, acts);
    const plays = playChoices(clicked, acts);
    if (choices) {
      // A permanent's abilities are chosen from all of them, even when only one can be
      // activated: activating is always the card, then the ability.
      chooseAbility(slot, choices);
    } else if (plays) {
      // A card's uses off the battlefield, chosen as its frames.
      choosePlay(slot, plays);
    } else if (casts.length > 0 && casts.length === acts.length) {
      // A card that can only be cast is cast, or set up in the cast step.
      castOrSetUp(slot, casts);
    } else if (acts.length === 1) {
      // One use acts on the click: a land is played, an ability activated, a target picked.
      const act = acts[0]!;
      if (
        stackTargets.some((target) => target.act.index === act.index) &&
        (selectedSpell || selfCounter(act))
      ) {
        setStackPick({ view, act });
      } else {
        pick(act.index, act.then);
      }
    } else {
      // One entry for all of a card's casts, which the cast step tells apart.
      const listed = acts.filter(
        (a) => a.option.kind !== "cast" || a === casts[0],
      );
      setMenu({ slot, at, acts: listed });
    }
    return true;
  };
  const onDrop = (slot: number, { start, at, card, onto }: Drop) => {
    standIns.touch(slot, deciding ? can.bySlot.get(slot) : undefined);
    // The card being cast let go in the hand goes back: the cast step closes, or the spell not
    // yet paid for is taken back. The cast step's card let go on the battlefield is cast as set.
    const inHand = at.y >= BATTLEFIELD_EDGE;
    if (castingNow?.slot === slot) {
      if (inHand) {
        playSound("back");
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
    if (placements.get(slot)?.look === "hand") {
      const plays = inHand ? [] : playActs(acts);
      const casts = castActs(plays);
      const choices =
        plays.length > 0
          ? (playChoices(objects.get(slot), acts) ??
            (plays.length > 1 ? plays.map((act) => ({ act })) : undefined))
          : undefined;
      if (choices) {
        // Offer the same uses as a click, including cycling a land instead of playing it.
        choosePlay(slot, choices, card ?? originOf(slot, at), true);
        return "split";
      }
      if (casts.length === 1 && stepsIn(casts)) {
        // The cast asks for more (X, its payment): the cast step, with the card back in hand.
        setUp(slot, casts);
        return false;
      }
      if (plays.length === 1) {
        play(plays[0]!);
        return true;
      }
      // Let go over the table with nothing to play it by: refused, back to the hand.
      if (!inHand) playSound("no");
      return false;
    }
    // A creature dragged in a declaration joins it, as clicking would, and goes to its new
    // place: a blocker onto the attacker it blocks (CR 509.1a); an attacker forward, or onto
    // what it attacks, the defending player or one of their planeswalkers (CR 508.1b).
    const over = onto && "slot" in onto ? onto.slot : undefined;
    if (drafting === "blocker" && declarable(slot) && over !== undefined) {
      playSound("select");
      setDraft(drafted({ ...draft, selected: slot }, over));
    } else if (drafting === "attacker" && declarable(slot)) {
      const walker = over !== undefined && walkers.includes(over) ? over : null;
      const atPlayer = onto && "player" in onto && onto.player === "opponent";
      if (walker !== null || atPlayer || start.y - at.y >= FORWARD) {
        playSound("select");
        setDraft(aimAttacks({ ...draft, attackGroup: [slot] }, walker));
      }
    }
    return false;
  };

  /** Clicking an attacker adds it to the group choosing a destination. An assigned attacker
   * can be focused on its own. Clicking the focused creature again takes it out of combat.
   * A blocker is paired with the attacker clicked next. */
  const drafted = (d: Draft, slot: number): Draft => {
    const { selected } = d;
    if (drafting === "attacker") {
      const attacks = new Map(d.attacks);
      if (d.attackGroup.length && walkers.includes(slot))
        return aimAttacks(d, slot);
      if (!declarable(slot)) return d;
      if (attacks.has(slot)) {
        if (
          walkers.length &&
          (!d.attackGroup.includes(slot) ||
            (d.aimed && d.attackGroup.length > 1))
        ) {
          return { ...d, attackGroup: [slot], aimed: true };
        }
        attacks.delete(slot);
        return {
          ...d,
          attacks,
          attackGroup: d.attackGroup.filter((s) => s !== slot),
        };
      }
      return {
        ...d,
        attacks: attacks.set(slot, null),
        attackGroup: d.aimed ? [slot] : [...d.attackGroup, slot],
        aimed: false,
      };
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
    setDraft({
      ...draft,
      attacks,
      attackGroup: [...attacks.keys()],
      aimed: false,
    });
  };

  const attackAllFirst =
    drafting === "attacker" &&
    draft.attacks.size === 0 &&
    [...can.bySlot.keys()].some(declarable);

  // The keyboard (Arena's): Space or Enter does the primary action, passing or confirming,
  // Shift with either passes on to the next stop, Escape takes back what was begun and
  // Cmd-Z (Ctrl-Z) the latest answer. A menu, the cast step and a pile's viewer close on
  // Escape themselves; a focused button takes its own Space and Enter.
  const onKey = useRef<(e: KeyboardEvent) => void>(undefined);
  onKey.current = (e) => {
    if (e.defaultPrevented) return;
    if (e.key === "Escape") {
      if (settings && !inert) setSettings(false);
      else if (cancellable && !menu && !chooser && !castingNow && !viewed)
        cancel?.();
      return;
    }
    if (
      e.key.toLowerCase() === "z" &&
      (e.metaKey || e.ctrlKey) &&
      !e.shiftKey &&
      !e.repeat
    ) {
      if (undoable) {
        e.preventDefault();
        undo?.();
      }
      return;
    }
    if (inert) return;
    if (
      (e.key !== " " && e.key !== "Enter") ||
      e.repeat ||
      menu ||
      activeChooser ||
      viewed
    ) {
      return;
    }
    const target = e.target instanceof Element ? e.target : null;
    if (target?.closest("button, input, select, textarea, a")) return;
    if (e.shiftKey && !onSkip) {
      if (passesOn) {
        e.preventDefault();
        passOnNow();
      }
      return;
    }
    if (onSkip) {
      e.preventDefault();
      onSkip();
      return;
    }
    const { pass } = can;
    let primary: (() => void) | undefined;
    if (castingNow) primary = () => cast(castingNow.choice);
    else if (drafting) primary = attackAllFirst ? attackWithAll : declare;
    else if (picking) primary = confirmPicks;
    else if (stackConfirmation) primary = stackConfirmation;
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
  // Passing on to the next stop, beside the turn button where passing would stop again in
  // this step: as the top of the stack resolves, or combat damage.
  const passesOn =
    !waiting &&
    !castingNow &&
    !chooser &&
    passOn !== undefined &&
    decision?.kind === "priority" &&
    decision.passOn !== null &&
    can.pass !== undefined;
  const passOnNow = () => {
    if (!passesOn) return;
    // It passes on out of this step, whose mana empties (CR 500.4).
    if (floating(you.mana)) setLost((n) => n + 1);
    passOn();
  };
  // The source link appears when the player points at the ability on the stack.
  const sourced = shown.stack
    .flatMap((item, i) =>
      item.kind === "ability" &&
      !item.triggered &&
      item.source !== null &&
      pointedItem === stackKey(i, item)
        ? [i]
        : [],
    )
    .join(",");
  const arrows = useMemo(
    () =>
      boardArrows(
        shown,
        new Set(sourced === "" ? [] : sourced.split(",").map(Number)),
        live,
      ),
    [shown, sourced, live],
  );
  const flip = coinFlipBanner(view);
  const banner = flip ?? combatBanner(view);
  const bannerKey = banner && `${banner.title}|${banner.detail}`;
  const bannerUp = useBannerUp(bannerKey);
  // A combat declaration is finished with the turn button, where priority is passed.
  const declaring =
    deciding && decision && isCombat(decision.kind) ? decision : undefined;
  // Attack destinations are chosen on the player or planeswalker itself.
  const finish = can.done;

  return (
    <>
      <div
        className={dimmed ? "table table-dimmed" : "table"}
        data-opening={view.turn.number === 0 || undefined}
        onPointerDown={() => setSettings(false)}
        // Before the card under it hears the press, so that the card's click or drag is one on
        // the newest view.
        onPointerDownCapture={onSkip}
        // A right-click takes back what was begun, as Escape does.
        onContextMenu={(e) => {
          e.preventDefault();
          if (menu || chooser) return;
          if (castingNow) setCasting(undefined);
          else if (cancellable) cancel?.();
        }}
        onPointerMove={(e) => {
          pointer.current = { x: e.clientX, y: e.clientY };
        }}
      >
        <div className="far-shade" />
        {/* The arrange tray borrows the pile as a packet. This anchor supplies its flight box. */}
        {libraries.map(({ side, look }) => (
          <Lying key={side} look={look}>
            <div
              data-table-library={side}
              style={{
                width: look.width + Math.abs(look.depth.x),
                height: look.width / CARD_ASPECT + look.depth.y,
              }}
            />
          </Lying>
        ))}

        <div className="hud" style={{ left: 18, top: 62 }}>
          <PlayerBadge
            seat="opponent"
            mulligans={early ? opponent.mulligans : 0}
          />
        </div>
        <div className="hud" style={{ left: 18, top: 704 }}>
          <PlayerBadge mulligans={early ? you.mulligans : 0} />
        </div>
        <Life
          side="opponent"
          life={opponent.life}
          incoming={combatDamage("opponent")}
          active={active === "opponent"}
          style={{ left: fromMiddle(670), top: 48 }}
          target={deciding ? can.players.get("opponent") : undefined}
          onPick={(act) => pick(act.index, act.then)}
          onAttack={
            aimingAttack ? () => setDraft(aimAttacks(draft, null)) : undefined
          }
        />
        {libraryChoice !== undefined && (
          <div className="hud library-choice-hud">
            <Button variant="secondary" onClick={toggleLog}>
              {libraryChoice}
            </Button>
          </div>
        )}
        {opponentActing(view, waiting) && !bannerUp && (
          <div
            className="hud thinking-hud"
            style={libraryChoice === undefined ? undefined : { top: 200 }}
          >
            <Thinking />
          </div>
        )}
        <Life
          side="you"
          life={you.life}
          incoming={combatDamage("you")}
          active={active === "you"}
          style={{ left: fromMiddle(670), top: 618 }}
          target={deciding ? can.players.get("you") : undefined}
          onPick={(act) => pick(act.index, act.then)}
        />
        <div className="hud" style={MANA_POOLS.opponent}>
          <ManaPool pool={opponent.mana} />
        </div>
        <div className="hud" style={MANA_POOLS.you}>
          <ManaPool pool={you.mana} pulse={lost} />
        </div>

        {banner && (
          <StepBanner
            key={bannerKey}
            shown={bannerUp}
            coin={flip !== undefined}
            {...banner}
          />
        )}

        <GameLog log={log} folded open={logOpen} onToggle={toggleLog} />
        <TurnBanner view={view} live={live} />

        {/* Stack abilities keep their frame through their exit. */}
        <AnimatePresence>
          {view.stack.map(
            (item, i) =>
              item.kind === "ability" && (
                <AbilityOnStack
                  key={stackKey(i, item)}
                  index={i}
                  count={view.stack.length}
                  item={item}
                  source={
                    item.source === null ? undefined : objects.get(item.source)
                  }
                  from={
                    live
                      ? abilityPlacement(
                          view,
                          item,
                          placements,
                          previousTable,
                          sourcePlaces.current,
                        )
                      : undefined
                  }
                  chosen={i === chosenAbility ? pickedMode?.from : undefined}
                  onPoint={(on) => {
                    const key = stackKey(i, item);
                    setPointedItem((k) =>
                      on ? key : k === key ? undefined : k,
                    );
                  }}
                />
              ),
          )}
        </AnimatePresence>

        <CardLayer
          epoch={view}
          events={view.events}
          objects={drawn}
          keys={standIns.keys}
          placements={placements}
          origins={tokenOrigins(
            views.current.before,
            view,
            previousTable,
            placements,
          )}
          modeHandoff={modeHandoff}
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

        <ManaFlow motes={motes} />

        <AnimatePresence>
          {timingNotice && noticeBox && (
            <motion.div
              key={timingNotice.id}
              className="timing-refusal"
              role="status"
              style={{
                left: (noticeBox.left + noticeBox.right) / 2,
                top: noticeBox.bottom + 6,
                x: "-50%",
              }}
              initial={reduceMotion ? false : { opacity: 0, y: -5 }}
              animate={{ opacity: 1, y: 0 }}
              exit={reduceMotion ? undefined : { opacity: 0 }}
              transition={{ duration: 0.16 }}
            >
              Sorcery speed only
            </motion.div>
          )}
        </AnimatePresence>

        {/* Over the cards on them: a pile is opened, never pointed at card by card. */}
        {[...piles, ...libraries].map(
          (pile) =>
            (pile.cards.length > 0 || pile.zone === "library") && (
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

        {/* Where each item beneath the top peeks out, which its arrows leave from. */}
        {view.stack.map(
          (item, i) =>
            i < view.stack.length - 1 && (
              <StackPeek
                key={stackKey(i, item)}
                index={i}
                count={view.stack.length}
                within={itemId(item, i)}
                nextItem={itemId(view.stack[i + 1]!, i + 1)}
              />
            ),
        )}

        <div className="board-arrows">
          <Arrows arrows={arrows} follow avoid={AVOID} />
        </div>

        {/* Under a screen over the table, the turn's controls give way (scry.png). */}
        {!inert && (
          <div className="turn-controls">
            {stops.fullControl && (
              <span className="full-control has-tip">
                <FullControlIcon />
                <Tip side="above">Full control</Tip>
              </span>
            )}
            {/* As on Arena, each player's step stops about their life orb; the current step
                lights in the active player's. */}
            {(["opponent", "you"] as const).map((side) => {
              const key = side === "you" ? "yours" : "theirs";
              return (
                <StepStops
                  key={side}
                  side={side}
                  current={
                    view.turn.active === side ? view.turn.step : undefined
                  }
                  now={`Turn ${view.turn.number} · ${STEP_NAMES[view.turn.step]}`}
                  whose={side === "you" ? "Your" : "Opponent's"}
                  stops={stops[key]}
                  onStops={(steps) => setStops({ ...stops, [key]: steps })}
                />
              );
            })}
            {drafting ? (
              <Declare
                kind={drafting}
                draft={draft}
                attackAllFirst={attackAllFirst}
                hasWalkers={walkers.length > 0}
                onDeclare={declare}
                onAll={attackWithAll}
                onClear={() => setDraft(emptyDraft())}
              />
            ) : declaring ? (
              <Finish
                decision={declaring}
                done={finish}
                onPick={pick}
                objects={objects}
              />
            ) : ((decision?.kind === "priority" && !chooser) || waiting) &&
              !castingNow ? (
              // In the cast step or the mode chooser, its buttons take the dock.
              <Dock>
                <div className="turn-action">
                  <TurnButton
                    label={waiting ? "Pass" : stop.label}
                    disabled={waiting || can.pass === undefined}
                    onClick={() => {
                      if (can.pass === undefined) return;
                      // Mana left unspent empties as the step ends (CR 500.4): passing on to a
                      // step, rather than to the top of the stack resolving, pulses the pool
                      // rather than asking.
                      const resolves = decision?.passTo?.kind === "resolve";
                      if (floating(you.mana) && !resolves)
                        setLost((n) => n + 1);
                      pick(can.pass);
                    }}
                  />
                  {passesOn && decision.passOn && (
                    <button
                      type="button"
                      className="pass-on-button has-tip"
                      aria-label={passButton(view, decision.passOn).caption}
                      onPointerDown={(e) => e.stopPropagation()}
                      onClick={passOnNow}
                    >
                      <span className="pass-on-face button-primary">
                        <FastForwardIcon />
                      </span>
                      <Tip side="above">
                        {passButton(view, decision.passOn).caption}
                        <kbd>⇧ Space</kbd>
                      </Tip>
                    </button>
                  )}
                  {!waiting && stop.caption && (
                    <span className="turn-caption">{stop.caption}</span>
                  )}
                </div>
              </Dock>
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
              // A card peeking out from behind a permanent is chosen in the prompt, whole.
              const look = placements.get(slot)?.look;
              return (
                look !== undefined &&
                look !== "hidden" &&
                look !== "pile" &&
                look !== "beneath"
              );
            }}
            onPick={pick}
            onPlan={(plan) => {
              const { index, rest } = follow(decision, plan);
              if (index !== undefined) pick(index, rest);
            }}
            onCancel={cancellable ? cancel : undefined}
            onPoint={setPointedOff}
            targetLabel={
              selectedSpell &&
              (selfCounter(selectedSpell)
                ? `Counter your own spell, ${selectedSpell.label}?`
                : `Target ${selectedSpell.label}`)
            }
            confirm={
              stackConfirmation
                ? {
                    label: "Confirm",
                    disabled: false,
                    onClick: stackConfirmation,
                  }
                : waterbend.active
                  ? {
                      label: "Confirm",
                      disabled: !waterbend.ready,
                      onClick: () => {
                        const { index, rest } = follow(
                          decision,
                          waterbend.plan(),
                        );
                        if (index !== undefined) pick(index, rest);
                      },
                    }
                  : picking && {
                      label: picking.label(picked),
                      disabled: !picking.ready(picked),
                      onClick: confirmPicks,
                    }
            }
            hovered={hovered}
            risen={risen?.slot}
          />
        )}

        {risen && objects.get(risen.slot) && (
          <Risen object={objects.get(risen.slot)!} at={risen.at} />
        )}

        {castingNow && !prompting && (
          <CastStep
            name={objects.get(castingNow.slot)?.name ?? "a card"}
            casts={castingNow.casts}
            choice={castingNow.choice}
            onChoose={(choice) => setCasting({ ...castingNow, choice })}
            onCast={cast}
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

        <AnimatePresence custom={handedRef.current}>
          {shownChooser && (
            <ModeChooser
              key={shownChooser.id}
              id={shownChooser.id}
              object={shownChooser.object}
              modes={shownChooser.modes}
              from={shownChooser.from}
              split={shownChooser.split}
              selected={activeChooser ? undefined : pendingMode?.key}
              onPick={(mode, from) => {
                if (!mode.action) return;
                setChooser(undefined);
                setPickedMode({
                  chooser: shownChooser,
                  key: mode.key,
                  action: mode.action,
                  from,
                  faceName:
                    mode.look.kind === "card"
                      ? (mode.look.object ?? shownChooser.object).name
                      : undefined,
                });
                mode.onPick?.();
              }}
              onClose={(returned) => {
                setChooser(undefined);
                if (returned)
                  setReturnedMode({
                    id: shownChooser.id,
                    slot: shownChooser.object.slot,
                    ...returned,
                  });
              }}
            />
          )}
        </AnimatePresence>

        {/* The card the pointer is on or, while its menu points at an ability, the card that
            ability's paragraph lit. */}
        {(menu?.pointed ?? (inspected && !menu)) &&
          !chooser &&
          !castingNow &&
          !dimmed && (
            <InspectAt
              view={view}
              object={pointedObject(
                objects.get(menu?.slot ?? inspected!.slot),
                menu?.pointed,
              )}
              place={inspectorPlace(
                menu ? menu.at.x : inspected!.x,
                view.stack.length > 0 || asking,
                noticeBox
                  ? [
                      ...targets,
                      {
                        left: (noticeBox.left + noticeBox.right) / 2 - 84,
                        right: (noticeBox.left + noticeBox.right) / 2 + 84,
                        top: noticeBox.top,
                        bottom: noticeBox.bottom + 42,
                      },
                    ]
                  : targets,
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
            key={`${viewed.side}:${viewed.zone}:${viewing?.linked ?? ""}`}
            pile={viewed}
            title={
              viewed.zone === "library"
                ? `Known cards · ${viewed.side === "you" ? "Your" : "Opponent’s"} library`
                : viewing?.linked === undefined
                  ? undefined
                  : (objects.get(viewing.linked)?.name ?? undefined)
            }
            states={states}
            offers={(slot) => deciding && can.bySlot.has(slot)}
            onPick={(slot) => {
              setViewing(undefined);
              onClick(slot, { x: viewed.look.x, y: viewed.look.y });
            }}
            onClose={() => setViewing(undefined)}
          />
        )}

        {/* The gear's column at the top right: the gear, the bug report, and undo while there
            is something to take back. The bug report stays over any screen. */}
        <button
          type="button"
          className="icon-button bug-button bug-button-game"
          aria-label="Report bug"
          title={`Report bug (${MOD_KEY}B)`}
          onPointerDown={(e) => e.stopPropagation()}
          onClick={requestBugReport}
        >
          <BugIcon />
        </button>
        {/* Like the turn's controls, the gear gives way to a screen over the table. */}
        {!inert && (
          <button
            type="button"
            className="icon-button gear-button"
            aria-label="Settings"
            onPointerDown={(e) => e.stopPropagation()}
            onClick={() => setSettings(!settings)}
          >
            <Gear />
          </button>
        )}
        {settings && !inert && (
          <div className="settings" onPointerDown={(e) => e.stopPropagation()}>
            <SoundPanel />
            <label className="settings-toggle">
              <input
                type="checkbox"
                checked={stops.fullControl}
                onChange={(e) =>
                  setStops({ ...stops, fullControl: e.target.checked })
                }
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
      {/* Undo stays accessible while a choice covers the table. */}
      {undoable && <UndoButton onUndo={undo} />}
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

/** How long a card takes to glide to a new place (CardLayer's spring), near enough, in ms. */
const GLIDE_MS = 700;

/** How long an announcing banner shows, in ms. */
const BANNER_MS = 2500;

/** Whether the banner of this key is still showing: for BANNER_MS after it appears, then not,
 * until it goes and comes again. The spinner under the opponent's orb hides meanwhile, since the
 * banner lies where it does. */
function useBannerUp(key: string | undefined): boolean {
  const [gone, setGone] = useState<string>();
  useEffect(() => {
    if (key === undefined) {
      setGone(undefined);
      return;
    }
    const timer = setTimeout(() => setGone(key), BANNER_MS);
    return () => clearTimeout(timer);
  }, [key]);
  return key !== undefined && gone !== key;
}

/** A step announcement over the battlefield, such as "Combat" over "Blockers declared", between
 * the opponent's life orb and their permanents: it fades away once no longer `shown`. */
function StepBanner({
  title,
  detail,
  shown,
  coin,
}: {
  title: string;
  detail: string;
  shown: boolean;
  coin: boolean;
}) {
  const reduce = useReducedMotion() ?? false;
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
          {coin ? (
            <Coin className="step-banner-coin" first={false} size={40} />
          ) : (
            <>
              <i className="step-banner-rule step-banner-rule-left" />
              <i className="step-banner-diamond" />
            </>
          )}
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
    ? betweenOrbsAndStack()
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

/** Right of the life orbs (x 670 to 930 on the base stage, held to its middle), left of the
 * stack (STACK.x, held to its right edge), and below the opponent's orb and above yours (y 159
 * and 618), with room for the artist's line. */
const betweenOrbsAndStack = (): InspectPlace => ({
  left: fromMiddle(940),
  top: 172,
  width: STACK.x - 54 - fromMiddle(940),
});

/** Under the gear and the bug report: takes back the player's latest answer, one per press, across the
 * opponent's turns. */
function UndoButton({ onUndo }: { onUndo: () => void }) {
  return (
    <button
      type="button"
      className="icon-button undo-button has-tip"
      aria-label="Undo"
      onPointerDown={(e) => e.stopPropagation()}
      onClick={onUndo}
    >
      <svg className="undo-icon" viewBox="0 0 24 24" aria-hidden="true">
        <path
          className="undo-arrow"
          d="M8.5 4.5 4 9l4.5 4.5M4 9h10a5.5 5.5 0 0 1 0 11h-4"
        />
      </svg>
      <Tip side="left">
        Undo<kbd>{MOD_KEY}Z</kbd>
      </Tip>
    </button>
  );
}

/** The platform's command modifier, as its keyboard shortcuts name it. */
const MOD_KEY = /Mac|iPhone|iPad/.test(navigator.platform) ? "⌘" : "Ctrl ";

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
  style,
  target,
  onPick,
  onAttack,
}: {
  side: Side;
  life: number;
  /** In combat, before its damage: what the unblocked attackers will deal this player. */
  incoming?: number;
  active: boolean;
  style: CSSProperties;
  target?: Act;
  onPick(act: Act): void;
  onAttack?(): void;
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
  const targetable = target !== undefined || onAttack !== undefined;
  return (
    <motion.div
      className={targetable ? "hud life life-aimed" : "hud life"}
      data-life={side}
      style={{ ...style, scale: pulse }}
    >
      {/* As a target, the orb's whole face is clicked, not just its number; the stops on
          either side keep theirs. */}
      {targetable && (
        <span
          className="life-hit life-target"
          role="button"
          aria-label={
            onAttack
              ? "Attack opponent"
              : side === "you"
                ? "Target yourself"
                : "Target opponent"
          }
          onClick={onAttack ?? (() => target && onPick(target))}
        />
      )}
      <LifeOrb
        life={shown}
        active={active}
        seat={side === "opponent" ? "opponent" : "you"}
        anchor={lifeId(side)}
      />
      {incoming !== undefined && (
        <Incoming amount={incoming} lethal={incoming > 0 && incoming >= life} />
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
  item,
  source,
  from,
  chosen,
  onPoint,
}: {
  index: number;
  count: number;
  item: Extract<StackItemView, { kind: "ability" }>;
  source?: ObjectView;
  /** Where its source is drawn: it springs out of it onto the stack. */
  from?: Placement;
  chosen?: CardHandoff;
  /** The pointer coming onto it, or leaving: its source's arrow shows meanwhile. */
  onPoint(on: boolean): void;
}) {
  const face = useAbilityFace(item.name, source, BLANK);
  const present = useIsPresent();
  const place = stackPlace(index, count);
  const reduce = useReducedMotion() ?? false;
  // Keep the departure fixed if the next view rearranges the source during flight.
  const [liftFrom, setLiftFrom] = useState(() =>
    !reduce && !chosen ? from : undefined,
  );
  const origin = liftFrom && abilityLift(liftFrom, item.triggered);
  const text = abilityText(face.text ?? "", item.triggered, item.nth);
  // A trigger starts inside its source. With no source drawn, it fades in at the stack.
  const start = chosen
    ? {
        opacity: 1,
        x: chosen.x,
        y: chosen.y,
        scale: chosen.width / place.width,
        rotate: 0,
      }
    : origin && item.triggered
      ? {
          opacity: 0,
          x: origin.start.x,
          y: origin.start.y,
          scale: origin.start.width / place.width,
          rotate: 0,
        }
      : from
        ? {
            opacity: 1,
            x: from.x,
            y: from.y,
            scale: (from.width * from.scale) / place.width,
            rotate: from.rotate,
          }
        : { opacity: 0, x: place.x, y: place.y, scale: 0.8 };
  return (
    <motion.div
      className="hud stack-ability"
      title={present && index < count - 1 ? text : undefined}
      // Which item it is, by place and the ability's own name, for a motion audit: two
      // items sharing a source's face (an emblem's and its planeswalker's) stay apart.
      data-stack-key={`${index}:${item.name}`}
      style={{
        left: 0,
        top: 0,
        zIndex: present ? STACK_Z + index : STACK_Z - 1,
        transformOrigin: "0 0",
        pointerEvents: present ? undefined : "none",
      }}
      initial={reduce ? false : start}
      animate={{
        opacity: 1,
        scale: origin
          ? [null, Math.min(1, origin.lift.width / place.width), 1]
          : 1,
        x: origin ? [null, origin.lift.x, place.x] : place.x,
        y: origin ? [null, origin.lift.y, place.y] : place.y,
        rotate: 0,
      }}
      transition={
        origin && item.triggered
          ? TRIGGER_FLIGHT
          : origin
            ? SOURCE_FLIGHT
            : ABILITY_FLIGHT
      }
      onAnimationComplete={() => setLiftFrom(undefined)}
      onPointerMove={() => onPoint(true)}
      onPointerLeave={() => onPoint(false)}
      exit={{ opacity: 0, transition: { duration: reduce ? 0.09 : 0.25 } }}
    >
      <StackItem
        id={present ? stackId(index) : undefined}
        source={face}
        label={item.triggered ? "Triggered Ability" : "Activated Ability"}
        text={text}
        width={place.width}
        showArt
      />
      <StackMarks controller={item.controller} x={item.x} />
    </motion.div>
  );
}

/** Let a trigger emerge at the center of the visible card before it crosses the board. */
function abilityLift(from: Placement, triggered: boolean) {
  const width = from.width * from.scale;
  if (!triggered)
    return {
      start: { x: from.x, y: from.y, width },
      lift: { x: from.x, y: from.y - 54, width: width * 1.35 },
    };
  const { left, right, top, bottom } = footprint(from);
  const centerX = (left + right) / 2;
  const centerY = (top + bottom) / 2;
  const startWidth = Math.min(right - left, (bottom - top) * CARD_ASPECT) * 0.6;
  const liftWidth = width * 1.16;
  return {
    start: {
      x: centerX - startWidth / 2,
      y: centerY - startWidth / CARD_ASPECT / 2,
      width: startWidth,
    },
    lift: {
      x: centerX - liftWidth / 2,
      y: centerY - liftWidth / CARD_ASPECT / 2 - 38,
      width: liftWidth,
    },
  };
}

/** An ability's flight from its source onto the stack: long enough for the eye to follow it
 * across the table, easing into its place. */
const ABILITY_FLIGHT = { duration: 0.5, ease: [0.2, 0.7, 0.3, 1] } as const;

/** Lift and grow right away over the source before crossing the table. */
const SOURCE_LEG = {
  duration: 0.64,
  times: [0, 0.18, 1],
  ease: [
    [0.15, 0.55, 0.35, 1],
    [0.3, 0, 0.25, 1],
  ],
} as const;
const SOURCE_FLIGHT = {
  ...ABILITY_FLIGHT,
  x: SOURCE_LEG,
  y: SOURCE_LEG,
  scale: SOURCE_LEG,
};

/** Reveal the trigger as it lifts, without a visible static frame over its source. */
const TRIGGER_FLIGHT = {
  ...SOURCE_FLIGHT,
  opacity: { duration: 0.1, ease: "linear" },
} as const;

/** A stack item's identity during its flight and exit. Targets can change while it is there. */
const stackKey = (index: number, item: StackItemView) =>
  item.kind === "spell"
    ? `${index}:${item.slot}`
    : `${index}:${item.name}:${item.source}:${item.ability}:${item.granted}:${item.controller}`;

/** The slot of what an ability on the stack comes from: its source, or for an emblem's ability,
 * which the view gives no source ([CR 114.4](https://yawgatog.com/resources/magic-rules/#R1144):
 * "Abilities of emblems function in the command zone"), the card the table draws for the
 * emblem of that name its controller has. */
function abilityOrigin(
  view: PlayerView,
  item: Extract<StackItemView, { kind: "ability" }>,
): number {
  if (item.source !== null) return item.source;
  const index = view.players[item.controller].emblems.indexOf(item.name);
  return index < 0 ? Number.NaN : emblemSlot(item.controller, index);
}

/** Fly from the visible source before this beat moved or tapped it. A permanent that
 * just entered has no battlefield position in the preceding view. */
function abilityPlacement(
  view: PlayerView,
  item: Extract<StackItemView, { kind: "ability" }>,
  placements: Layout,
  previous: Layout | undefined,
  last: Layout,
): Placement | undefined {
  const slot = abilityOrigin(view, item);
  const from = previous?.get(slot);
  const current = placements.get(slot);
  // Skipping a queued landing beat can leave a hand placement as the previous one.
  if (
    item.triggered &&
    current?.zone === "battlefield" &&
    from?.zone !== "battlefield"
  )
    return current;
  return from?.opacity &&
    (from.zone === "battlefield" ||
      from.zone === "hand" ||
      item.source === null)
    ? from
    : current?.opacity
      ? current
      : from?.opacity
        ? from
        : last.get(slot);
}

/** The element id of the ability on the stack at `index`. */
const stackId = (index: number) => `stack-${index}`;

/** The element id of the stack item at `index`: its card for a spell. */
const itemId = (item: StackItemView, index: number) =>
  item.kind === "spell" ? cardId(item.slot) : stackId(index);

/** The element id of the strip of the stack item at `index` that peeks out from under the
 * next. */
const peekId = (index: number) => `stack-peek-${index}`;

/** The strip of the stack item at `index` of `count` that the next one leaves uncovered: its
 * name bar (STACK), which is all of it the eye can tie an arrow to. Unseen, part of the item's
 * element `within` for the arrows. */
function StackPeek({
  index,
  count,
  within,
  nextItem,
}: {
  index: number;
  count: number;
  within: string;
  nextItem: string;
}) {
  const place = stackPlace(index, count);
  const next = stackPlace(index + 1, count);
  return (
    <div
      id={peekId(index)}
      className="hud stack-peek"
      data-within={within}
      data-stack-next={nextItem}
      style={{
        left: place.x,
        top: place.y,
        width: place.width,
        height: next.y - place.y,
      }}
    />
  );
}

/** What the stack's arrows keep off: every card on the screen, the stack's items, the life
 * orbs and the game log. */
const AVOID = ".card-el .card-slot, .stack-item, .life-orb, .game-log";

/** The element id of the oval of a player's life orb. */
const lifeId = (side: Side) => `${side}-life-orb`;

/** Where each token `view`'s events made, and each emblem new since `before`, comes from: the
 * spell or permanent whose ability made it, resolving at the top of the stack `before`. */
function tokenOrigins(
  before: PlayerView | undefined,
  view: PlayerView,
  previous: Layout | undefined,
  current: Layout,
): Map<number, Box> | undefined {
  if (!before) return undefined;
  // The tokens made, and the emblems: those past the ones each player had.
  const made = [
    ...view.events.flatMap((e) => (e.kind === "tokensCreated" ? e.tokens : [])),
    ...(["opponent", "you"] as const).flatMap((side) =>
      view.players[side].emblems
        .slice(before.players[side].emblems.length)
        .map((_, i) =>
          emblemSlot(side, before.players[side].emblems.length + i),
        ),
    ),
  ];
  if (made.length === 0) return undefined;
  const top = before.stack.at(-1);
  if (!top) {
    // If entry and creation arrive in one beat without a resolving item, the entering
    // permanent is the only visible source available. Ambiguous batches grow in place.
    const arrivals = view.events.filter(
      (e) =>
        e.kind === "zoneChange" &&
        e.to === "battlefield" &&
        e.from !== "nowhere",
    );
    const arrival = arrivals.length === 1 ? arrivals[0] : undefined;
    const source =
      arrival?.kind === "zoneChange" ? current.get(arrival.slot) : undefined;
    return source?.opacity
      ? new Map(made.map((slot) => [slot, footprint(source)]))
      : undefined;
  }
  const from = top.kind === "spell" ? previous?.get(top.slot) : undefined;
  if (from?.opacity)
    return new Map(made.map((slot) => [slot, footprint(from)]));
  if (top.kind === "spell") return undefined;
  // An ability makes its tokens from the resolving indicator, even while its source
  // permanent remains on the battlefield.
  const { x, y, width } = stackPlace(
    before.stack.length - 1,
    before.stack.length,
  );
  const box = {
    left: x,
    top: y,
    right: x + width,
    bottom: y + width / CARD_ASPECT,
  };
  return new Map(made.map((slot) => [slot, box]));
}

/** Combat links and stack arrows. Thin straight lines join blockers to attackers and
 * attackers to their chosen destinations. Stack targets are green for the controller
 * and their objects, red for their opponent, consistently from either seat. Abilities at the places `sourced` names draw a gold arrow from their source. */
function boardArrows(
  view: PlayerView,
  sourced: Set<number>,
  live: boolean,
): ArrowSpec[] {
  const objects = new Map(view.objects.map((o) => [o.slot, o]));
  const out: ArrowSpec[] = [];
  const walkerControllers = new Set(
    view.objects
      .filter(
        (o) => o.zone === "battlefield" && o.types.includes("planeswalker"),
      )
      .map((o) => o.controller),
  );
  const pending = new Set(pendingAttackers(view));
  for (const slot of view.battlefield) {
    const o = objects.get(slot);
    if (!o) continue;
    const combat = o.combat;
    if (pending.has(slot)) continue;
    if (combat?.kind === "blocking" && combat.attacker !== null) {
      out.push({
        from: cardId(slot),
        to: cardId(combat.attacker),
        tone: "gold",
        direct: true,
      });
    }
    // When the defender has a planeswalker, show every attack destination, including the
    // life orb. With only one destination the chevron suffices. Blocked attacks link to blockers.
    if (
      combat?.kind !== "attacking" ||
      combat.blocked ||
      combat.targetDeparted ||
      (combat.planeswalker === null &&
        !walkerControllers.has(o.controller === "you" ? "opponent" : "you"))
    ) {
      continue;
    }
    out.push({
      from: cardId(slot),
      to:
        combat.planeswalker === null
          ? lifeId(o.controller === "you" ? "opponent" : "you")
          : cardId(combat.planeswalker),
      tone: "red",
      direct: true,
      fromChevron: true,
    });
  }
  // Each item's targets, the top's first so that it routes clearest: what a spell or ability
  // aims at is what a player most needs to read, beneath the top as on it. An ability's
  // source only while the ability is pointed at: its glow says it otherwise.
  // One beneath the top ties its arrows to the strip of it that shows.
  const top = view.stack.length - 1;
  const spells = new Map(
    view.stack.flatMap((item, i) =>
      item.kind === "spell" ? [[item.slot, i] as const] : [],
    ),
  );
  for (let i = top; i >= 0; i--) {
    const item = view.stack[i]!;
    const from = i === top ? itemId(item, i) : peekId(i);
    const controller =
      item.kind === "ability"
        ? item.controller
        : objects.get(item.slot)?.controller;
    if (item.kind === "spell" && objects.get(item.slot)?.zone !== "stack") {
      continue;
    }
    const announced = view.events.findIndex((e) => {
      const other = announcedItem(e);
      return other !== undefined && sameItem(item, other);
    });
    if (item.kind === "ability" && item.source !== null && sourced.has(i)) {
      out.push({ from: cardId(item.source), to: from, tone: "gold" });
    }
    for (const target of item.targets) {
      if (target.kind === "player") {
        out.push({
          from,
          to: lifeId(target.player),
          tone: target.player === controller ? "green" : "red",
        });
        continue;
      }
      const object = objects.get(target.slot);
      const index = spells.get(target.slot);
      // Two stacked items already show which spell is being answered.
      if (index !== undefined && top === 1) continue;
      // A skipped beat can contain both an announcement and its target leaving. The slot
      // is the same, but what the stack item was aimed at has left.
      const departed =
        !object ||
        object.zone === "hand" ||
        object.zone === "library" ||
        view.events.some(
          (e, at) =>
            at > announced &&
            e.kind === "zoneChange" &&
            e.slot === target.slot &&
            e.from !== e.to,
        );
      if (departed && !live) continue;
      out.push({
        from,
        to:
          index !== undefined && index < top
            ? peekId(index)
            : cardId(target.slot),
        tone: object?.controller === controller ? "green" : "red",
        pulse: live
          ? {
              key: `${stackKey(i, item)}:${target.slot}`,
              zone: object?.zone,
              departed,
            }
          : undefined,
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
  attackAllFirst,
  hasWalkers,
  onDeclare,
  onAll,
  onClear,
}: {
  kind: "attacker" | "blocker";
  draft: Draft;
  /** There are eligible attackers and none has been selected. */
  attackAllFirst: boolean;
  hasWalkers: boolean;
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
    <Dock>
      {(attackAllFirst || n > 0) && (
        <DockButtons>
          {attackAllFirst && (
            <Button variant="secondary" onClick={onDeclare}>
              No Attacks
            </Button>
          )}
          {n > 0 && (
            <Button variant="secondary" onClick={onClear}>
              Clear
            </Button>
          )}
        </DockButtons>
      )}
      <div className="turn-action">
        <TurnButton
          label={attackAllFirst ? "Attack With All" : label}
          onClick={attackAllFirst ? onAll : onDeclare}
        />
        <span className="turn-caption">
          {attack && hasWalkers && !draft.aimed
            ? n === 0
              ? "Choose attackers, then a target"
              : "Choose attack target"
            : caption}
        </span>
      </div>
    </Dock>
  );
}

/** The turn button at the rest of combat's decisions, which the engine asks only where a
 * declaration sent from the table could not say it all: what an attacker attacks, which
 * attacker a blocker blocks, how damage is divided. It waits for the choice on the table. */
function Finish({
  decision,
  done,
  onPick,
  objects,
}: {
  decision: DecisionView;
  objects: Map<number, ObjectView>;
  done?: CandidateIndex;
  onPick(index: CandidateIndex): void;
}) {
  if (decision.kind === "attackTarget") {
    const attackers =
      decision.ask.kind === "attackTarget" ? decision.ask.attackers : [];
    const attacker =
      attackers.length === 1 ? objects.get(attackers[0]!) : undefined;
    const name = attacker?.token
      ? attacker.name?.replace(/ Token$/, "")
      : attacker?.name;
    return (
      <Dock
        title={
          name ? `Choose attack target for ${name}` : "Choose attack target"
        }
      />
    );
  }
  const labels: Partial<Record<Kind, [string, string]>> = {
    blockTarget: ["Block", "Choose an attacker to block"],
    damageDivision: ["Assign Damage", "Choose blockers to destroy"],
  };
  const [label, caption] = labels[decision.kind] ?? ["Done", prompt(decision)];
  return (
    <Dock>
      <div className="turn-action">
        <TurnButton
          label={label}
          disabled={done === undefined}
          onClick={() => done !== undefined && onPick(done)}
        />
        <span className="turn-caption">{caption}</span>
      </div>
    </Dock>
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
  targetLabel,
  confirm,
  hovered,
  risen,
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
  targetLabel?: string;
  /** Confirms the cards picked on the table, which leaves the prompt no candidate to offer. */
  confirm?: { label: string; disabled: boolean; onClick(): void } | false;
  /** The card the pointer is on, which a crew meter previews. */
  hovered?: number;
  /** The card the question is about, shown risen above its place on the table. */
  risen?: number;
}) {
  const { ask } = decision;
  // A payment plan reads as what it taps, never as a number; a payment asked of a spell's
  // controller by what it pays.
  const label = (c: DecisionView["candidates"][number]) => {
    if (ask.kind === "unlessPays") {
      return c.option.kind === "yes" ? `Pay ${ask.cost}` : "Don't Pay";
    }
    // A "you may" move by what it does ("Exile"), or declining.
    if (ask.kind === "mayMove") {
      return c.option.kind === "yes" ? moveVerb(ask.to) : "Decline";
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
        .filter(({ c }) =>
          // Players are picked on their life orbs, which light for them.
          c.option.kind === "player"
            ? false
            : c.option.kind !== "object" || !onTable(c.option.slot),
        );
  // Up to one target asks for the target, which the board takes with its count; the prompt
  // offers none.
  const listed =
    skip === undefined
      ? offered
      : [{ c: decision.candidates[skip]!, index: skip, label: "No Target" }];
  const faces: { object: ObjectView; index?: CandidateIndex }[] =
    listed.flatMap(({ c, index }) => {
      const o = c.option.kind === "object" && objects.get(c.option.slot);
      return o ? [{ object: o, index }] : [];
    });
  // The card a "you may" would move, shown as the question's subject: risen above its place
  // on the table (`Risen`), or else here.
  const subject = ask.kind === "mayMove" ? objects.get(ask.card) : undefined;
  if (subject && subject.slot !== risen) faces.unshift({ object: subject });
  const ordering = decision.kind === "triggerOrder";
  // Its answers in the dock, a "you may" move's first.
  const buttons = ordering
    ? []
    : listed
        .filter(({ c }) => c.option.kind !== "object")
        .sort(
          (a, b) =>
            Number(b.c.option.kind === "yes") -
            Number(a.c.option.kind === "yes"),
        );
  const [order, setOrder] = useState<CandidateIndex[]>([]);
  const title =
    targetLabel ??
    (ordering
      ? order.length === 0
        ? "Resolve first"
        : "Resolve next"
      : prompt(
          skip === undefined ? decision : { ...decision, kind: "target" },
          source,
          subject?.zone,
        ));
  const body = (
    <>
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
                  key={index ?? `subject-${object.slot}`}
                  object={object}
                  width={promptCardWidth(faces.length)}
                  onPick={index === undefined ? undefined : () => onPick(index)}
                  onPoint={(on) => onPoint(on ? object.slot : undefined)}
                />
              ))}
            </div>
          )}
        </>
      )}
    </>
  );
  return (
    <FloatPrompt
      label={title}
      body={(ask.kind === "crew" || ordering || faces.length > 0) && body}
      title={
        <>
          <RulesText
            text={
              decision.kind === "target" && !targetLabel
                ? prompt(decision)
                : title
            }
          />
          {ask.kind === "waterbend" && (
            <span className="prompt-cost">
              <CostText text={ask.cost} />
            </span>
          )}
        </>
      }
    >
      {(buttons.length > 0 || onCancel || confirm) && (
        <PromptRow>
          {buttons.map((b, i) => (
            <Button
              key={b.index}
              variant={
                i === 0 && !confirm && skip === undefined
                  ? "primary"
                  : "secondary"
              }
              onClick={() => onPick(b.index)}
            >
              <RulesText text={b.label} />
            </Button>
          ))}
          {confirm && (
            <Button disabled={confirm.disabled} onClick={confirm.onClick}>
              {confirm.label}
            </Button>
          )}
          {onCancel && (
            <Button variant="secondary" onClick={onCancel}>
              Cancel
            </Button>
          )}
        </PromptRow>
      )}
    </FloatPrompt>
  );
}

/** A candidate card off the table, as a graveyard's target, by its face. */
function PromptCard({
  object,
  width,
  onPick,
  onPoint,
}: {
  object: ObjectView;
  width: number;
  /** Picks the card; none for a card the question is about, shown but not picked. */
  onPick?(): void;
  onPoint(on: boolean): void;
}) {
  const face = useFace(object);
  const ref = useRef<HTMLElement>(null);
  useBorrowedCard(ref, object.slot);
  const hover = {
    onPointerEnter: () => onPoint(true),
    onPointerLeave: () => onPoint(false),
  };
  const card = (
    <Card card={face} width={width} highlight={onPick ? "target" : undefined} />
  );
  return onPick ? (
    <button
      ref={ref as RefObject<HTMLButtonElement>}
      type="button"
      className="prompt-card"
      data-option={object.slot}
      data-prompt-slot={object.slot}
      aria-label={object.name ?? "A card"}
      onClick={onPick}
      {...hover}
    >
      {card}
    </button>
  ) : (
    <span
      ref={ref as RefObject<HTMLSpanElement>}
      className="prompt-card prompt-subject"
      data-prompt-slot={object.slot}
      aria-label={object.name ?? "A card"}
      {...hover}
    >
      {card}
    </span>
  );
}

/** The longest the hand waits for its view's events to be seen before it rises, in ms. */
const RAISE_WAIT = 1200;

/** How much higher a card picked from the raised hand stands than the rest. */
const PICKED_RISE = 44;

/** The width of each of `count` faces in the prompt's one row: whole cards, large enough to
 * read, a row of five clear of the stack at the right. */
const promptCardWidth = (count: number) =>
  count <= 3 ? 210 : count === 4 ? 180 : 160;

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
  const sourceOf = (c: DecisionView["candidates"][number]) =>
    c.option.kind === "trigger" ? c.option.source : undefined;
  const twin = (c: DecisionView["candidates"][number]) =>
    decision.candidates.some((d) => d !== c && sourceOf(d) === sourceOf(c));
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
        // A trigger says what it does only to tell it from another of its source's.
        const at = twin(c)
          ? abilityParagraph(text, true, c.nth ?? 0)
          : undefined;
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
 * its paragraph, a cast's the card's mana cost). */
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
    if (o.kind === "mana") {
      const effect =
        menu.waterbend && name
          ? act.label.replace(` ${name} for `, " for ")
          : `Add {${MANA_SYMBOLS[o.color]}}`;
      return { act, effect, labelled: true };
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
  return entries;
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
  useOpenSound();
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  // Kept on the stage: opened near an edge, it opens away from it.
  const left = Math.min(menu.at.x, STAGE.width - 330);
  const bottom = menu.at.y > STAGE.height / 2;
  const place = bottom
    ? { left, bottom: STAGE.height - menu.at.y + 8 }
    : { left, top: menu.at.y + 8 };
  if (tapsOnly(menu.acts)) {
    // A source of several colors: the mana it can add, one symbol each.
    return (
      <>
        <div className="menu-scrim" onPointerDown={onClose} />
        <div
          className="mana-picker"
          role="group"
          aria-label={title}
          style={place}
        >
          {menu.acts.map((act) =>
            act.option.kind === "mana" ? (
              <button
                key={act.index}
                type="button"
                className="mana-picker-item"
                aria-label={act.label}
                onClick={() => onPick(act)}
              >
                <ManaSymbol symbol={MANA_SYMBOLS[act.option.color]} />
              </button>
            ) : null,
          )}
        </div>
      </>
    );
  }
  return (
    <>
      <div className="menu-scrim" onPointerDown={onClose} />
      <div
        className="action-menu"
        role="group"
        aria-label={title}
        style={place}
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
