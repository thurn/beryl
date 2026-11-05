// The battlefield screen of a game in progress, drawn from the latest PlayerView as
// docs/ui_mockups/battlefield.png lays it out: the opponent's hand as card backs, life orbs,
// land piles, permanents, libraries and graveyards, player badges, the phase tracker and the
// turn button, with the hand fanned at the near edge (decisions D10 to D13).
import {
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
import { Card, type CardFace } from "../components/Card";
import {
  Button,
  FastForwardIcon,
  PhaseTracker,
  PriorityPill,
} from "../components/Controls";
import { Library, LifeOrb, PlayerBadge } from "../components/Player";
import { StackItem } from "../components/Stack";
import { cardEntry, useCardArt } from "../cards";
import type {
  CandidateIndex,
  DecisionView,
  ObjectView,
  PlayerView,
  Side,
} from "../generated/protocol";
import { Brand, Gear, Lying } from "../screens/Table";
import { CardLayer, type CardState, type Point } from "./CardLayer";
import {
  interactions,
  isOpening,
  playActs,
  prompt,
  type Act,
} from "./decisions";
import { frameFace, unknownFace } from "./faces";
import {
  layout,
  nextStop,
  phaseOf,
  PILES,
  stackPlace,
  type Placement,
} from "./layout";

export interface BoardProps {
  view: PlayerView;
  /** An answer was sent and the next view has not arrived: the opponent is acting. */
  waiting: boolean;
  choose(index: CandidateIndex): void;
  /** Full control: stop at every priority (decision D13). */
  setFullControl(on: boolean): void;
  onLeave(): void;
  /** Blurred and darkened behind a screen over it. */
  dimmed?: boolean;
}

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
}: BoardProps) {
  const [hovered, setHovered] = useState<number>();
  const [menu, setMenu] = useState<Menu>();
  const [settings, setSettings] = useState(false);
  const can = useMemo(() => interactions(view), [view]);
  // Under a screen over the table (the opening hand, the game's end), the table only shows.
  const deciding = view.status.kind === "deciding" && !waiting && !dimmed;
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
  const placements = useMemo(() => layout(view, raised), [view, raised]);

  // A new view closes whatever was open on the old one.
  useEffect(() => setMenu(undefined), [view]);

  const pick = (index: CandidateIndex) => {
    setMenu(undefined);
    choose(index);
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
          o?.zone === "hand" && o.owner === "you" && playActs(acts).length > 0,
      });
    }
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
  const onDrop = (slot: number, at: Point) => {
    const acts = deciding ? playActs(can.bySlot.get(slot) ?? []) : [];
    if (acts.length === 1) {
      pick(acts[0]!.index);
      return true;
    }
    if (acts.length > 1) {
      setMenu({ slot, at, acts });
    }
    return false;
  };

  const { you, opponent } = view.players;
  const active = view.turn.active;
  const top = view.stack[view.stack.length - 1];
  const sourceName =
    top &&
    (top.kind === "spell"
      ? (objects.get(top.slot)?.name ?? undefined)
      : top.name);
  const stop = nextStop(view);

  return (
    <div
      className={dimmed ? "table table-dimmed" : "table"}
      onPointerDown={() => setSettings(false)}
    >
      <Lying
        x={PILES.opponent.library.x}
        y={PILES.opponent.library.y}
        width={122}
        squash={0.85}
      >
        <Library
          count={opponent.library.length}
          width={PILES.opponent.library.width}
          depth={{ x: -29, y: 37, flare: 32 }}
        />
      </Lying>
      <Lying x={PILES.you.library.x} y={PILES.you.library.y} width={116}>
        <Library
          count={you.library.length}
          width={PILES.you.library.width}
          depth={{ x: 6, y: 12, twist: 7 }}
        />
      </Lying>

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

      {view.stack.map(
        (item, i) =>
          item.kind === "ability" && (
            <AbilityOnStack
              key={`${i}-${item.name}`}
              index={i}
              name={item.name}
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

      <div
        className={
          waiting || !decision || decision.kind !== "priority"
            ? "turn-controls turn-controls-wait"
            : "turn-controls"
        }
      >
        <PhaseTracker current={phaseOf(view.turn.step)} compact />
        {waiting && <PriorityPill>Opponent has priority</PriorityPill>}
        {decision?.kind === "priority" || waiting ? (
          <div className="turn-action">
            <Button
              icon={<FastForwardIcon />}
              disabled={waiting || can.pass === undefined}
              className={stop.label.length > 8 ? "button-long" : undefined}
              onClick={() => can.pass !== undefined && pick(can.pass)}
            >
              {waiting ? "Pass" : stop.label}
            </Button>
            {!waiting && <span className="turn-caption">{stop.caption}</span>}
          </div>
        ) : null}
      </div>

      {deciding &&
        decision &&
        decision.kind !== "priority" &&
        !isOpening(decision.kind) && (
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

      <button
        type="button"
        className="gear-button"
        aria-label="Settings"
        onPointerDown={(e) => e.stopPropagation()}
        onClick={() => setSettings(!settings)}
      >
        <Gear />
      </button>
      {settings && (
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
      <Brand />
    </div>
  );
}

/** A life orb that counts to a new total and pulses as it changes; it glows cyan as a target
 * and is then clicked to pick that player. */
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
      id={`${side}-life`}
      className={targetable ? "hud life life-target" : "hud life"}
      data-life={side}
      style={{ ...style, scale: pulse }}
      onClick={() => targetable && onPick(target)}
    >
      <LifeOrb
        life={shown}
        active={active}
        seat={side === "opponent" ? "opponent" : "you"}
      />
    </motion.div>
  );
}

/** An activated or triggered ability on the stack, in the stack's cascade. */
function AbilityOnStack({
  index,
  name,
  source,
}: {
  index: number;
  name: string;
  source?: ObjectView;
}) {
  const art = useCardArt(name);
  const face: CardFace = art
    ? frameFace(source ?? BLANK, art)
    : unknownFace(name);
  const place = stackPlace(index);
  return (
    <motion.div
      className="hud stack-ability"
      style={{ left: place.x, top: place.y, zIndex: 400 + index }}
      initial={{ opacity: 0, scale: 0.8 }}
      animate={{ opacity: 1, scale: 1 }}
    >
      <StackItem
        source={face}
        label="Ability"
        text={face.text ?? ""}
        width={place.width}
        showArt
      />
    </motion.div>
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
  const art = useCardArt(name ?? "");
  if (!object || !name || !place || place.look === "hidden") return null;
  const entry = cardEntry(name);
  const left = place.x < 800 ? 1600 - 330 - 24 : 24;
  return (
    <div className="inspect" style={{ left }} aria-label={`${name}, inspected`}>
      {scan?.src ? (
        <img className="inspect-scan" src={scan.src} alt={name} />
      ) : (
        <Card
          card={art ? frameFace(object, art) : unknownFace(name)}
          width={330}
        />
      )}
      {entry && <p className="inspect-artist">Illustrated by {entry.artist}</p>}
    </div>
  );
}
