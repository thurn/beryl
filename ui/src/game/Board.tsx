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
  type ReactNode,
} from "react";
import {
  animate,
  motion,
  useMotionValue,
  useReducedMotion,
} from "motion/react";
import { faceEntry, landFace } from "../cards";
import type { Highlight } from "../components/Card";
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
  type Act,
  type Intent,
} from "./decisions";
import {
  attackPlan,
  blockable,
  blockPlan,
  declared,
  emptyDraft,
  type Draft,
} from "./combat";
import { abilityParagraph, abilityText, BLANK, useAbilityFace } from "./faces";
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
import {
  combatBanner,
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
  zonePiles,
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
  /** Answers the open decision, and the decisions after it with `then`, as each arrives. */
  choose(index: CandidateIndex, then?: Intent[]): void;
  /** Takes back the spell or ability being cast, at a decision that may
   * (`DecisionView.cancellable`). */
  cancel?(): void;
  /** Full control: stop at every priority (decision D13). */
  setFullControl(on: boolean): void;
  onLeave(): void;
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
 * the pointer. */
interface Menu {
  slot: number;
  at: Point;
  acts: Act[];
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
  choose,
  cancel,
  setFullControl,
  onLeave,
  dimmed = false,
  inert = false,
}: BoardProps) {
  const [hovered, setHovered] = useState<number>();
  const [menu, setMenu] = useState<Menu>();
  const [casting, setCasting] = useState<Casting>();
  // The combat declaration being made, shown as if declared until it is confirmed.
  const [draft, setDraft] = useState<Draft>(emptyDraft);
  // The card being dragged.
  const [lifted, setLifted] = useState<number>();
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
  // The table as the declaration would leave it.
  const shown = useMemo(
    () => (drafting ? declared(view, draft) : view),
    [view, draft, drafting],
  );
  // A decision other than priority, the opening's and combat's asks in a prompt over the
  // turn controls, under the stack, which rises to clear it.
  const prompting =
    deciding &&
    decision !== undefined &&
    decision.kind !== "priority" &&
    !isCombat(decision.kind) &&
    !isOpening(decision.kind);
  const castingNow = deciding ? casting : undefined;
  // The cast under way, which the engine is asking about, can still be taken back.
  const cancellable = deciding && decision?.cancellable === true;
  // The prompt or the cast step sits over the turn controls; the stack rises to clear it.
  const asking = prompting || castingNow !== undefined;
  const placements = useMemo(
    () => layout(shown, asking, dimmed),
    [shown, asking, dimmed],
  );
  // The hovered card, unless it has since gone where it is not shown alone.
  const hoveredPlace =
    hovered === undefined ? undefined : placements.get(hovered);
  const inspected =
    hoveredPlace?.look === "hidden" || hoveredPlace?.look === "pile"
      ? undefined
      : hoveredPlace;
  const piles = useMemo(() => zonePiles(shown), [shown]);
  const viewed = piles.find(
    (p) => p.side === viewing?.side && p.zone === viewing.zone,
  );

  // A new view closes whatever was open on the old one, and starts any declaration afresh.
  useEffect(() => {
    setMenu(undefined);
    setCasting(undefined);
    setDraft(emptyDraft());
  }, [view]);

  const pick = (index: CandidateIndex, then?: Intent[]) => {
    setMenu(undefined);
    setCasting(undefined);
    choose(index, then);
  };
  /** Casts with `choice`: the cast, then its X and payment as each is asked. */
  const cast = (choice: CastChoice) => pick(choice.act.index, castPlan(choice));
  /** Plays an act of a card's: a cast with its least X and the default payment. */
  const play = (act: Act) =>
    act.option.kind === "cast" ? cast(firstChoice(act)) : pick(act.index);
  /** Opens the cast step for the card in `slot` with its casts. */
  const setUp = (slot: number, casts: Act[]) => {
    setMenu(undefined);
    setCasting({ slot, casts, choice: firstChoice(casts[0]!) });
  };
  /** Casts from a click: at once, as a drop does, unless the cast step has a choice to offer:
   * which cast, X, or which permanents pay. */
  const castOrSetUp = (slot: number, casts: Act[]) => {
    const choice = firstChoice(casts[0]!);
    if (asksCastStep(casts) || plansOf(choice).length > 1) setUp(slot, casts);
    else cast(choice);
  };
  /** A menu's pick: a cast goes to the cast step, with all of the card's casts. */
  const pickAct = (slot: number, act: Act) => {
    const casts = castActs(can.bySlot.get(slot) ?? []);
    if (act.option.kind === "cast") setUp(slot, casts);
    else pick(act.index);
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
      states.set(item.slot, { ...states.get(item.slot), x: item.x });
    }
  }
  // The top of the stack's source glows gold and its targets cyan, whoever has priority
  // (trigger_on_stack_with_target.png).
  const top = view.stack[view.stack.length - 1];
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
  const taps = new Set(paying ? tapsOf(paying) : []);
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
    const acts = deciding ? can.bySlot.get(slot) : undefined;
    if (!acts) {
      setMenu(undefined);
      return false;
    }
    const casts = castActs(acts);
    if (casts.length > 0 && casts.length === acts.length) {
      // A card that can only be cast is cast, or set up in the cast step.
      castOrSetUp(slot, casts);
    } else if (acts.length === 1) {
      // One use acts on the click: a land is played, an ability activated, a target picked.
      pick(acts[0]!.index);
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
    const acts = deciding ? (can.bySlot.get(slot) ?? []) : [];
    if (objects.get(slot)?.zone === "hand") {
      const plays = at.y < BATTLEFIELD_EDGE ? playActs(acts) : [];
      const casts = castActs(plays);
      if (casts.length === plays.length && asksCastStep(casts)) {
        // The cast asks for more (which cast, X): the cast step, with the card back in hand.
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
    if (inert || e.defaultPrevented) return;
    if (e.key === "Escape") {
      if (settings) setSettings(false);
      else if (cancellable && !menu && !castingNow && !viewed) cancel?.();
      return;
    }
    if ((e.key !== " " && e.key !== "Enter") || e.repeat || menu || viewed) {
      return;
    }
    const target = e.target instanceof Element ? e.target : null;
    if (target?.closest("button, input, select, textarea, a")) return;
    const { done, pass } = can;
    let primary: (() => void) | undefined;
    if (castingNow) primary = () => cast(castingNow.choice);
    else if (drafting) primary = declare;
    else if (declaring)
      primary = done === undefined ? undefined : () => pick(done);
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
  const sourceName =
    top &&
    (top.kind === "spell"
      ? (objects.get(top.slot)?.name ?? undefined)
      : top.name);
  const stop = decision?.halt
    ? haltButton(view)
    : decision?.passTo
      ? passButton(view, decision.passTo)
      : { label: "Pass", caption: "" };
  const arrows = useMemo(() => boardArrows(shown), [shown]);
  const banner = drafting
    ? {
        title: "Combat",
        detail: instruction(drafting, draft, walkers.length > 0, nameOf),
      }
    : (combatBanner(view) ?? coinFlipBanner(view));
  // A combat declaration is finished with the turn button, where priority is passed.
  const declaring =
    deciding && decision && isCombat(decision.kind) ? decision : undefined;

  return (
    <>
      <div
        className={dimmed ? "table table-dimmed" : "table"}
        onPointerDown={() => setSettings(false)}
        // A right-click takes back what was begun, as Escape does.
        onContextMenu={(e) => {
          e.preventDefault();
          if (menu || inert) return;
          if (castingNow) setCasting(undefined);
          else if (cancellable) cancel?.();
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
                <Tip side={side === "you" ? "above" : "below"}>
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
            activity={opponentActivity(view, waiting)}
          />
        </div>
        <div className="hud" style={{ left: 18, top: 704 }}>
          <PlayerBadge name="You" />
        </div>
        <Life
          side="opponent"
          life={opponent.life}
          active={active === "opponent"}
          style={{ left: 670, top: 48 }}
          target={deciding ? can.players.get("opponent") : undefined}
          onPick={pick}
        />
        <Life
          side="you"
          life={you.life}
          active={active === "you"}
          style={{ left: 670, top: 618 }}
          target={deciding ? can.players.get("you") : undefined}
          onPick={pick}
        />
        <div className="hud" style={POOLS.opponent}>
          <ManaPool pool={opponent.mana} />
        </div>
        <div className="hud" style={POOLS.you}>
          <ManaPool pool={you.mana} pulse={lost} />
        </div>

        {banner && <StepBanner {...banner} />}

        <GameLog log={log} folded={overLog(placements)} />
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
          onHover={setHovered}
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

        <div className="board-arrows">
          <Arrows arrows={arrows} follow avoid={AVOID} />
        </div>

        {/* Under a screen over the table, the turn's controls give way (scry.png). */}
        {!inert && (
          <div className="turn-controls">
            <PhaseTracker
              current={phaseOf(view.turn.step)}
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
              <Finish decision={declaring} done={can.done} onPick={pick} />
            ) : decision?.kind === "priority" || waiting ? (
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
            decision={decision}
            source={sourceName}
            nameOf={nameOf}
            objects={objects}
            onTable={(slot) => {
              const look = placements.get(slot)?.look;
              return look !== undefined && look !== "hidden" && look !== "pile";
            }}
            onPick={pick}
            onCancel={cancellable ? cancel : undefined}
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
            x={menu ? menu.at.x : inspected!.x}
            lit={menu?.pointed && litParagraph(menu.pointed, objects)}
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
            <Button variant="secondary" size="small" onClick={onLeave}>
              Leave game
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

/** The turn button at a halt of the session's (`DecisionView.halt`), where passing is all the
 * player can do: ending their own turn from its first main phase, or going on from the
 * opponent's end step. */
function haltButton(view: PlayerView): { label: string; caption: string } {
  return view.turn.active === "you"
    ? { label: "End Turn", caption: "Nothing to play" }
    : { label: "Continue", caption: "To Your Turn" };
}

/** A step announcement over the battlefield, such as "Combat" over "Blockers declared". */
function StepBanner({ title, detail }: { title: string; detail: string }) {
  return (
    <div className="step-banner" role="status">
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
    </div>
  );
}

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
  active,
  style,
  target,
  onPick,
}: {
  side: Side;
  life: number;
  active: boolean;
  style: CSSProperties;
  target?: CandidateIndex;
  onPick(index: CandidateIndex): void;
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
    </motion.div>
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

/** The lines over the table (decision D10): each blocked attacker's to its blockers and each
 * attacker's at a planeswalker to it (combat.png), and for the top of the stack, a gold line
 * from an ability's source and a cyan one to each target (trigger_on_stack_with_target.png).
 * An attacker at the defending player draws none: its chevron and its step out of its row say
 * it attacks, and a line from each attacker to the life orb crossed the whole table. */
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

/** The result of a coin flip, while the view it happened in shows. */
function coinFlipBanner(
  view: PlayerView,
): { title: string; detail: string } | undefined {
  const flip = view.events.find((e) => e.kind === "coinFlip");
  if (flip?.kind !== "coinFlip") return undefined;
  const who = flip.player === "you" ? "You" : "Your opponent";
  return {
    title: "Coin Flip",
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
 * cards picked on the table, where each glows with its side and state in view. */
function Prompt({
  decision,
  source,
  nameOf,
  objects,
  onTable,
  onPick,
  onCancel,
}: {
  decision: DecisionView;
  source?: string;
  nameOf(slot: number): string;
  objects: Map<number, ObjectView>;
  /** Whether the card in a slot is shown on the table, to be clicked there. */
  onTable(slot: number): boolean;
  onPick(index: CandidateIndex): void;
  /** Takes back the cast the decision is part of. */
  onCancel?(): void;
}) {
  // A payment plan reads as what it taps, never as a number; a trigger as its source, since
  // the title says what happens to it.
  const label = (c: DecisionView["candidates"][number]) => {
    if (c.option.kind === "trigger" && c.option.source !== null) {
      return nameOf(c.option.source);
    }
    const taps = c.payments[0]?.taps;
    if (c.option.kind !== "plan" || !taps) return c.label;
    return taps.length > 0 ? `Tap ${tapList(taps.map(nameOf))}` : "Tap nothing";
  };
  const buttons = decision.candidates
    .map((c, index) => ({ c, index, label: label(c) }))
    .filter(({ c }) => c.option.kind !== "object" || !onTable(c.option.slot));
  // Cards of one name off the table say whose they are.
  const named = (b: (typeof buttons)[number]) => {
    const o = b.c.option.kind === "object" && objects.get(b.c.option.slot);
    if (!o || buttons.filter((x) => x.label === b.label).length < 2) {
      return b.label;
    }
    return `${o.controller === "you" ? "Your" : "Their"} ${b.label}`;
  };
  return (
    <div className="prompt" role="dialog" aria-label={prompt(decision, source)}>
      <h2 className="prompt-title">{prompt(decision, source)}</h2>
      {buttons.length > 0 && (
        <div className="prompt-choices">
          {buttons.map((b) => (
            <Button
              key={b.index}
              size="small"
              variant="secondary"
              onClick={() => onPick(b.index)}
            >
              <RulesText text={named(b)} />
            </Button>
          ))}
        </div>
      )}
      {onCancel && (
        <div className="cast-buttons">
          <Button size="small" variant="secondary" onClick={onCancel}>
            Cancel
          </Button>
        </div>
      )}
    </div>
  );
}

/** The inspector for `object`, beside the table on the side away from `x`, with what is
 * attached to it. */
function InspectAt({
  view,
  object,
  x,
  lit,
}: {
  view: PlayerView;
  object?: ObjectView;
  x: number;
  lit?: number;
}) {
  return (
    <Inspect
      object={object}
      attached={view.objects.filter(
        (o) => o.zone === "battlefield" && o.attachedTo === object?.slot,
      )}
      left={x < 800 ? 1600 - 330 - 24 : 24}
      lit={lit}
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

/** What a card offers, listed where it was clicked or dropped; pointing at an option says
 * which. */
function ActionMenu({
  menu,
  title,
  onPick,
  onPoint,
  onClose,
}: {
  menu: Menu;
  title?: string;
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
        {menu.acts.map((a) => (
          <button
            key={a.index}
            type="button"
            className="action-menu-item"
            onClick={() => onPick(a)}
            onPointerEnter={() => onPoint(a)}
            onPointerLeave={() => onPoint(undefined)}
          >
            {menuLabel(a, title)}
          </button>
        ))}
      </div>
    </>
  );
}

/** A candidate's label in its card's menu, its symbols drawn: an activation by its cost
 * alone, not repeating the card's name. */
function menuLabel(act: Act, name?: string): ReactNode {
  const label = act.label;
  if (name && label.startsWith(`${name}: `)) {
    return <CostText text={label.slice(name.length + 2)} />;
  }
  return <RulesText text={label} />;
}
