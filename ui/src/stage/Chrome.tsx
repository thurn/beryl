// The desktop's chrome around the stage's cards (plan K25): everything on the game screen that
// is neither motion nor a prompt, drawn by React from the components the old board draws it
// with. What shows the table (life, mana, the piles, the stack's peeks, the turn's step) is
// drawn from the stage's table, so that what a step changes shows as the step plays; what
// answers the player (undo, the settings) goes by the view last played out. Announcements show
// for as long as the step that makes them plays, on the stage's time (`Shown.notices`); the
// combat banner, which no step announces, keeps the old board's timer.
import { useEffect, useMemo, useState, useSyncExternalStore } from "react";
import { Button, Tip } from "../components/Controls";
import { LifeOrb, ManaPool, PlayerBadge, Thinking } from "../components/Player";
import { StepStops } from "../components/Steps";
import type { PlayerView, Side, StopsSetting } from "../generated/protocol";
import { incoming } from "../game/combat";
import { FullControlIcon } from "../game/Corner";
import { InspectAt, inspectorPlace } from "../game/Inspector";
import { Incoming, lifeId } from "../game/Life";
import type { LogEntry } from "../game/log";
import { GameLog, opponentActing } from "../game/Narration";
import { PileButton, ZoneViewer } from "../game/Piles";
import { itemId, StackPeek } from "../game/StackPeek";
import { useStageWidth } from "../game/Stage";
import { StepBanner, useBannerUp } from "../game/StepBanner";
import { combatBanner, STEP_NAMES } from "../game/stepLabels";
import { MANA_POOLS } from "../game/tableRows";
import { TurnAnnouncement } from "../game/TurnBanner";
import { knownLibraries, zonePiles } from "../game/zoneViews";
import { desktop } from "./desktop.ts";
import type { Name } from "./layout.ts";
import { announced, coinBanner, scryWords } from "./notices.ts";
import type { ScriptStage } from "./stage.ts";

const SIDES = ["opponent", "you"] as const;
const NO_ORDER: ReadonlyMap<number, number> = new Map();
const UNMARKED = new Map<number, never>();

/** Where each badge stands, at the table's left. */
const BADGES = { opponent: 62, you: 704 };

/** Where the pill of a player's scry stands: under the opponent's orb, over the player's. */
const SCRY_TOP = { opponent: 160, you: 580 };

/** What lies on the table under its cards: the badges, both life totals with the combat
 * damage on its way, the mana pools, the log, and what is announced. */
export function Hud({
  stage,
  view,
  log,
  waiting,
  logOpen,
  onLog,
}: {
  stage: ScriptStage;
  /** The view last played out. */
  view: PlayerView;
  log: LogEntry[];
  waiting: boolean;
  logOpen: boolean;
  onLog(): void;
}) {
  const { table, notices } = useSyncExternalStore(
    stage.subscribe,
    () => stage.shown,
  );
  // The orbs and the pills hold to the stage's middle.
  useStageWidth();
  const coin = announced(notices, "coin");
  const scry = announced(notices, "scry");
  const turn = announced(notices, "turn");
  const combat = combatBanner(view);
  const combatKey =
    coin || !combat ? undefined : `${combat.title}|${combat.detail}`;
  const combatUp = useBannerUp(combatKey);
  if (!table) return null;
  const banner = coin ? coinBanner(coin, table) : combat;
  const bannerUp = coin !== undefined || combatUp;
  const anchors = desktop.anchors(table);
  const early = table.turn.number <= 2;
  // The damage shows from the attack's declaration until it is dealt.
  const beforeDamage =
    table.turn.step === "declareAttackers" ||
    table.turn.step === "declareBlockers";
  return (
    <>
      {SIDES.map((side) => (
        <div key={side} className="hud" style={{ left: 18, top: BADGES[side] }}>
          <PlayerBadge
            seat={side}
            mulligans={early ? table.players[side].mulligans : 0}
          />
        </div>
      ))}
      {SIDES.map((side) => {
        const { corner } = anchors.life(side);
        const { life } = table.players[side];
        const damage = beforeDamage ? incoming(table, side) : undefined;
        return (
          <div
            key={side}
            className="hud life"
            data-life={side}
            style={{ left: corner.x, top: corner.y }}
          >
            <LifeOrb
              life={life}
              active={table.turn.active === side}
              seat={side}
              anchor={lifeId(side)}
            />
            {damage !== undefined && (
              <Incoming amount={damage} lethal={damage > 0 && damage >= life} />
            )}
          </div>
        );
      })}
      {scry && (
        <div
          className="hud library-choice-hud"
          role="status"
          style={{ top: SCRY_TOP[scry.notice.player] }}
        >
          <Button variant="secondary" onClick={onLog}>
            {scryWords(scry.notice)}
          </Button>
        </div>
      )}
      {opponentActing(view, waiting) && !bannerUp && (
        <div
          className="hud thinking-hud"
          style={scry?.notice.player === "opponent" ? { top: 200 } : undefined}
        >
          <Thinking />
        </div>
      )}
      {SIDES.map((side) => (
        <div key={side} className="hud" style={MANA_POOLS[side]}>
          <ManaPool pool={table.players[side].mana} />
        </div>
      ))}
      {banner && (
        <StepBanner
          key={`${banner.title}|${banner.detail}`}
          shown={bannerUp}
          coin={coin !== undefined}
          {...banner}
        />
      )}
      <GameLog log={log} folded open={logOpen} onToggle={onLog} />
      {turn && (
        <TurnAnnouncement
          key={table.turn.number}
          side={turn.notice.active}
          ms={stage.timings(turn.step.act).duration}
        />
      )}
    </>
  );
}

/** The node the pointer is on, as the stage tells it (pointer.ts). The one listener the stage's
 * pointer has is this board's: the prompt shapes add what a click and a drop do here. */
function useHovered(stage: ScriptStage): Name | undefined {
  const [hovered, setHovered] = useState<Name>();
  useEffect(() => {
    setHovered(stage.pointer.hovered);
    return stage.pointer.listen({ hover: setHovered });
  }, [stage]);
  return hovered;
}

/** What lies over the table's cards: a button on each pile and the viewer it opens, the
 * strips the stack's lower items peek out by, each player's step stops with full control's
 * mark, and the inspector of the card the pointer is on. */
export function Controls({
  stage,
  stops,
  setStops,
  inert,
  dimmed,
}: {
  stage: ScriptStage;
  stops: StopsSetting;
  setStops(stops: StopsSetting): void;
  /** A screen over the table answers the decision: the turn's controls give way. */
  inert?: boolean;
  dimmed?: boolean;
}) {
  const { table, resting } = useSyncExternalStore(
    stage.subscribe,
    () => stage.shown,
  );
  const width = useStageWidth();
  const [viewing, setViewing] = useState<{ side: Side; zone: string }>();
  const hovered = useHovered(stage);
  const piles = useMemo(
    () =>
      table ? [...zonePiles(table, NO_ORDER), ...knownLibraries(table)] : [],
    // The piles hold to the stage's edges.
    [table, width],
  );
  if (!table) return null;
  const viewed = piles.find(
    (p) => p.side === viewing?.side && p.zone === viewing.zone,
  );
  // The card the pointer is on, where it is shown alone: not one under the top of a pile.
  const pointed =
    typeof hovered === "number" ? resting.get(hovered) : undefined;
  const inspected =
    pointed?.object &&
    pointed.dress.look !== "hidden" &&
    pointed.dress.look !== "pile" &&
    pointed.dress.look !== "handling"
      ? pointed
      : undefined;
  return (
    <>
      {/* Over the cards on them: a pile is opened, never pointed at card by card. */}
      {piles.map(
        (pile) =>
          (pile.cards.length > 0 || pile.zone === "library") && (
            <PileButton
              key={`${pile.side}-${pile.zone}`}
              pile={pile}
              onOpen={() => setViewing({ side: pile.side, zone: pile.zone })}
            />
          ),
      )}
      {/* Where each item beneath the top peeks out, which its arrows leave from. */}
      {table.stack.map(
        (item, i) =>
          i < table.stack.length - 1 && (
            <StackPeek
              key={i}
              index={i}
              count={table.stack.length}
              within={itemId(item, i)}
              nextItem={itemId(table.stack[i + 1]!, i + 1)}
            />
          ),
      )}
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
          {SIDES.map((side) => {
            const key = side === "you" ? "yours" : "theirs";
            return (
              <StepStops
                key={side}
                side={side}
                current={
                  table.turn.active === side ? table.turn.step : undefined
                }
                now={`Turn ${table.turn.number} · ${STEP_NAMES[table.turn.step]}`}
                whose={side === "you" ? "Your" : "Opponent's"}
                stops={stops[key]}
                onStops={(steps) => setStops({ ...stops, [key]: steps })}
              />
            );
          })}
        </div>
      )}
      {inspected && !dimmed && !viewed && (
        <InspectAt
          view={table}
          object={inspected.object}
          place={inspectorPlace(inspected.pose.x, table.stack.length > 0, [])}
        />
      )}
      {viewed && !dimmed && (
        <ZoneViewer
          key={`${viewed.side}:${viewed.zone}`}
          pile={viewed}
          title={
            viewed.zone === "library"
              ? `Known cards · ${viewed.side === "you" ? "Your" : "Opponent’s"} library`
              : undefined
          }
          states={UNMARKED}
          offers={() => false}
          onPick={() => {}}
          onClose={() => setViewing(undefined)}
        />
      )}
    </>
  );
}
