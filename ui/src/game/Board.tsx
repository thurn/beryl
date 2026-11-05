// The battlefield screen of a game in progress, drawn from the latest PlayerView as
// docs/ui_mockups/battlefield.png lays it out: the opponent's hand as card backs, life orbs,
// land piles, permanents, libraries and graveyards, player badges, the phase tracker and the
// turn button, with the hand fanned at the near edge (decisions D10 to D13). Combat and the
// stack follow combat.png and trigger_on_stack_with_target.png: attack and block lines and the
// step banner; spells and abilities large at the right, a gold line from an ability's source
// and cyan lines to targets.
import {
  Fragment,
  useEffect,
  useMemo,
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
import { Card, type Highlight } from "../components/Card";
import {
  Button,
  PhaseTracker,
  PriorityPill,
  TurnButton,
} from "../components/Controls";
import {
  GraveyardEdges,
  Library,
  LifeOrb,
  PlayerBadge,
} from "../components/Player";
import { Arrows, StackItem, type ArrowSpec } from "../components/Stack";
import { cardEntry, useCardArt } from "../cards";
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
  interactions,
  isCombat,
  isOpening,
  playActs,
  prompt,
  type Act,
  type Intent,
} from "./decisions";
import { abilityText, useAbilityFace, useFace } from "./faces";
import {
  combatBanner,
  layout,
  passButton,
  phaseOf,
  pileLook,
  PILES,
  STACK_Z,
  stackPlace,
  type PileLook,
  type Placement,
} from "./layout";
import { Brand } from "./Stage";

export interface BoardProps {
  view: PlayerView;
  /** An answer was sent and the next view has not arrived: the opponent is acting. */
  waiting: boolean;
  /** Answers the open decision, and the decisions after it with `then`, as each arrives. */
  choose(index: CandidateIndex, then?: Intent[]): void;
  /** Full control: stop at every priority (decision D13). */
  setFullControl(on: boolean): void;
  onLeave(): void;
  /** Blurred and darkened behind a screen over it. */
  dimmed?: boolean;
  /** A screen over it answers the decision: the table only shows. */
  inert?: boolean;
}

/** Dropped above this line, a hand card is on the battlefield rather than back in the hand. */
const BATTLEFIELD_EDGE = 690;
/** Dragged forward at least this far, a creature attacks. */
const FORWARD = 60;

/** A menu of a card's candidates, open at a point on the stage. */
interface Menu {
  slot: number;
  at: Point;
  acts: Act[];
}

export function Board({
  view,
  waiting,
  choose,
  setFullControl,
  onLeave,
  dimmed = false,
  inert = false,
}: BoardProps) {
  const [hovered, setHovered] = useState<number>();
  const [menu, setMenu] = useState<Menu>();
  const [settings, setSettings] = useState(false);
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
  const raised =
    hovered !== undefined && objects.get(hovered)?.zone === "hand"
      ? hovered
      : undefined;
  // A decision other than priority, the opening's and combat's asks in a prompt over the
  // turn controls, under the stack, which rises to clear it.
  const prompting =
    deciding &&
    decision !== undefined &&
    decision.kind !== "priority" &&
    !isCombat(decision.kind) &&
    !isOpening(decision.kind);
  const placements = useMemo(
    () => layout(view, raised, prompting, dimmed),
    [view, raised, prompting, dimmed],
  );

  // A new view closes whatever was open on the old one.
  useEffect(() => setMenu(undefined), [view]);

  const pick = (index: CandidateIndex, then?: Intent[]) => {
    setMenu(undefined);
    choose(index, then);
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

  const onClick = (slot: number, at: Point) => {
    const acts = deciding ? can.bySlot.get(slot) : undefined;
    if (!acts) {
      setMenu(undefined);
      return;
    }
    if (can.picking && acts.length === 1) {
      pick(acts[0]!.index);
    } else {
      setMenu({ slot, at, acts });
    }
  };
  const onDrop = (slot: number, { start, at, onto }: Drop) => {
    const acts = deciding ? (can.bySlot.get(slot) ?? []) : [];
    if (objects.get(slot)?.zone === "hand") {
      const plays = at.y < BATTLEFIELD_EDGE ? playActs(acts) : [];
      if (plays.length === 1) {
        pick(plays[0]!.index);
        return true;
      }
      if (plays.length > 1) {
        setMenu({ slot, at, acts: plays });
      }
      return false;
    }
    const declare = acts.find(
      (a) => a.option.kind === "attack" || a.option.kind === "block",
    );
    const over = onto && "slot" in onto ? objects.get(onto.slot) : undefined;
    if (declare?.option.kind === "block") {
      // A blocker goes onto the attacker it blocks (CR 509.1a).
      if (over?.combat?.kind !== "attacking") return false;
      pick(declare.index, [
        {
          kind: "blockTarget",
          wants: (c) => c.kind === "object" && c.slot === over.slot,
        },
      ]);
      return true;
    }
    if (declare?.option.kind !== "attack") return false;
    // An attacker goes forward, or onto what it attacks: the defending player or one of their
    // planeswalkers (CR 508.1b), which the next decision asks when there is a choice.
    const walker =
      over?.controller === "opponent" && over.types.includes("planeswalker")
        ? over.slot
        : undefined;
    const atPlayer = onto && "player" in onto && onto.player === "opponent";
    if (walker === undefined && !atPlayer && start.y - at.y < FORWARD) {
      return false;
    }
    pick(
      declare.index,
      walker !== undefined || atPlayer
        ? [
            {
              kind: "attackTarget",
              wants: (c) =>
                c.kind === "defender" && c.planeswalker === (walker ?? null),
            },
          ]
        : [],
    );
    return true;
  };

  const { you, opponent } = view.players;
  const active = view.turn.active;
  const sourceName =
    top &&
    (top.kind === "spell"
      ? (objects.get(top.slot)?.name ?? undefined)
      : top.name);
  const stop = decision?.passTo
    ? passButton(view, decision.passTo)
    : { label: "Pass", caption: "" };
  const arrows = useMemo(() => boardArrows(view), [view]);
  const banner = combatBanner(view) ?? coinFlipBanner(view);
  // A combat declaration is finished with the turn button, where priority is passed.
  const declaring =
    deciding && decision && isCombat(decision.kind) ? decision : undefined;

  return (
    <>
      <div
        className={dimmed ? "table table-dimmed" : "table"}
        onPointerDown={() => setSettings(false)}
      >
        <div className="far-shade" />
        {(["opponent", "you"] as const).map((side) => {
          const { library, graveyard } = view.players[side];
          const libraryPile = pileLook(PILES[side].library, library.length);
          const graveyardPile = pileLook(
            PILES[side].graveyard,
            graveyard.length,
          );
          return (
            <Fragment key={side}>
              <Lying look={libraryPile}>
                <Library
                  count={library.length}
                  width={libraryPile.width}
                  depth={libraryPile.depth}
                />
              </Lying>
              {graveyard.length > 1 && (
                <Lying look={graveyardPile}>
                  <GraveyardEdges
                    count={graveyard.length}
                    width={graveyardPile.width}
                    depth={graveyardPile.depth}
                  />
                </Lying>
              )}
            </Fragment>
          );
        })}

        <div className="hud" style={{ left: 18, top: 62 }}>
          <PlayerBadge name="Opponent" />
        </div>
        <div className="hud" style={{ left: 18, top: 814 }}>
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

        {banner && <StepBanner {...banner} />}

        {view.stack.map(
          (item, i) =>
            item.kind === "ability" && (
              <AbilityOnStack
                key={`${i}-${item.name}`}
                index={i}
                count={view.stack.length}
                lifted={prompting}
                item={item}
                source={
                  item.source === null ? undefined : objects.get(item.source)
                }
              />
            ),
        )}

        <CardLayer
          epoch={view}
          objects={view.objects}
          placements={placements}
          states={states}
          onClick={onClick}
          onDrop={onDrop}
          onHover={setHovered}
        />

        <div className="board-arrows">
          <Arrows arrows={arrows} follow />
        </div>

        {/* Under a screen over the table, the turn's controls give way (scry.png). */}
        {!inert && (
          <div className="turn-controls">
            <PhaseTracker current={phaseOf(view.turn.step)} compact />
            {declaring ? (
              <Finish
                view={view}
                decision={declaring}
                done={can.done}
                onPick={pick}
              />
            ) : decision?.kind === "priority" || waiting ? (
              <div className="turn-action">
                <TurnButton
                  label={waiting ? "Pass" : stop.label}
                  disabled={waiting || can.pass === undefined}
                  onClick={() => can.pass !== undefined && pick(can.pass)}
                />
                {/* While you wait, the priority pill takes the caption's place under the
                button, leaving the tracker showing the phase. */}
                {waiting ? (
                  <PriorityPill>Opponent has priority</PriorityPill>
                ) : (
                  <span className="turn-caption">{stop.caption}</span>
                )}
              </div>
            ) : null}
          </div>
        )}

        {prompting && decision && (
          <Prompt decision={decision} source={sourceName} onPick={pick} />
        )}

        {menu && (
          <ActionMenu
            menu={menu}
            title={objects.get(menu.slot)?.name ?? undefined}
            onPick={pick}
            onClose={() => setMenu(undefined)}
          />
        )}

        {hovered !== undefined && !menu && !dimmed && (
          <Inspect
            object={objects.get(hovered)}
            place={placements.get(hovered)}
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

/** A pile lying on the table as `look` says: its top card's top-left corner at the look's
 * corner, the edges beneath on whichever side they show. */
function Lying({ look, children }: { look: PileLook; children: ReactNode }) {
  return (
    <div
      className="hud"
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
    </motion.div>
  );
}

/** The element id of the stack item at `index`: its card for a spell. */
const stackId = (index: number) => `stack-${index}`;

/** The element id of the oval of a player's life orb. */
const lifeId = (side: Side) => `${side}-life-orb`;

/** The lines over the table (decision D10): each attacker's to what it attacks or to its
 * blockers (combat.png), and for the top of the stack, a gold line from an ability's source
 * and a cyan one to each target (trigger_on_stack_with_target.png). */
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
    for (const b of blockers) {
      out.push({
        from: cardId(slot),
        to: `${cardId(b)}-mark`,
        tone: "gold",
        bend: 0,
        fromSide: up ? "top" : "bottom",
        toSide: { x: 0.5, y: up ? 0.85 : 0.15 },
        start: "none",
        end: "none",
        fine: true,
      });
    }
    if (o.combat.blocked) continue;
    // Out of its chevron's apex and into the oval of the life orb or the planeswalker it
    // attacks, wherever that stands.
    const walker = o.combat.planeswalker;
    out.push({
      from: `${cardId(slot)}-mark`,
      to: walker === null ? lifeId(up ? "opponent" : "you") : cardId(walker),
      tone: "gold",
      fromSide: up ? "top" : "bottom",
      toSide: "facing",
      start: "none",
      fine: true,
    });
  }
  // The top of the stack's lines leave and meet each end on the side facing the other, bowing
  // away from the middle of the table.
  const top = view.stack.length - 1;
  const item = view.stack[top];
  if (item) {
    const from = item.kind === "spell" ? cardId(item.slot) : stackId(top);
    if (item.kind === "ability" && item.source !== null) {
      out.push({
        from: cardId(item.source),
        to: from,
        tone: "gold",
        fromSide: "facing",
        toSide: "facing",
      });
    }
    for (const target of item.targets) {
      out.push({
        from,
        to:
          target.kind === "player"
            ? lifeId(target.player)
            : cardId(target.slot),
        tone: "cyan",
        fromSide: "facing",
        toSide: "facing",
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

/** The turn button during a combat declaration: it finishes the declaration ("Attack", "No
 * Blocks"), or, while a creature's attacker or defender is being chosen, waits for that. */
function Finish({
  view,
  decision,
  done,
  onPick,
}: {
  view: PlayerView;
  decision: DecisionView;
  done?: CandidateIndex;
  onPick(index: CandidateIndex): void;
}) {
  const yours = (kind: "attacking" | "blocking") =>
    view.objects.some((o) => o.controller === "you" && o.combat?.kind === kind);
  const labels: Partial<Record<Kind, [string, string]>> = {
    attacker: [
      yours("attacking") ? "Attack" : "No Attacks",
      "Declare attackers",
    ],
    attackTarget: ["Attack", "Choose what it attacks"],
    blocker: [yours("blocking") ? "Block" : "No Blocks", "Declare blockers"],
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

/** An object with nothing on it, for an ability whose source has left its zone. */
const BLANK: ObjectView = {
  slot: 0,
  zone: "stack",
  owner: "you",
  controller: "you",
  name: null,
  token: false,
  tapped: false,
  summoningSick: false,
  damage: 0,
  counters: [],
  types: [],
  pt: null,
  attachedTo: null,
  exiledBy: null,
  combat: null,
};

/** A decision other than priority: what it asks, with every candidate as a button, so each
 * pick on the board has a click fallback here too. */
function Prompt({
  decision,
  source,
  onPick,
}: {
  decision: DecisionView;
  source?: string;
  onPick(index: CandidateIndex): void;
}) {
  return (
    <div className="prompt" role="dialog" aria-label={prompt(decision, source)}>
      <h2 className="prompt-title">{prompt(decision, source)}</h2>
      <div className="prompt-choices">
        {decision.candidates.map((c, i) => (
          <Button
            key={i}
            size="small"
            variant="secondary"
            onClick={() => onPick(i)}
          >
            {c.label}
          </Button>
        ))}
      </div>
    </div>
  );
}

/** What a card offers, listed where it was clicked or dropped. */
function ActionMenu({
  menu,
  title,
  onPick,
  onClose,
}: {
  menu: Menu;
  title?: string;
  onPick(index: CandidateIndex): void;
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
        role="menu"
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
            role="menuitem"
            className="action-menu-item"
            onClick={() => onPick(a.index)}
          >
            {menuLabel(a, title)}
          </button>
        ))}
      </div>
    </>
  );
}

/** A candidate's label in its card's menu, without repeating the card's name. */
function menuLabel(act: Act, name?: string): ReactNode {
  const label = act.label;
  if (name && label.startsWith(`${name}: `)) {
    return label.slice(name.length + 2);
  }
  return label;
}

/** The hovered card's full Scryfall scan, which carries the artist credit, beside the table on
 * the side away from the card; outside the app, its frame stands in for the scan. */
function Inspect({
  object,
  place,
}: {
  object?: ObjectView;
  place?: Placement;
}) {
  const name = object?.name;
  const scan = useCardArt(name ?? "", undefined, "normal");
  const face = useFace(object ?? BLANK);
  if (!object || !name || !place || place.look === "hidden") return null;
  const entry = cardEntry(name);
  const left = place.x < 800 ? 1600 - 330 - 24 : 24;
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
