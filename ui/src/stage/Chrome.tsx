// The desktop's chrome around the stage's cards: everything on the game screen that
// is neither motion nor a prompt, drawn by React. What shows the table (life, mana, the piles,
// the turn's step) is
// drawn from the stage's table, so that what a step changes shows as the step plays; what
// answers the player (undo, the settings) goes by the view last played out. Announcements show
// for as long as the step that makes them plays, on the stage's time (`Shown.notices`); the
// combat banner, which no step announces, keeps a timer of its own.
//
// What the pointer does that is no prompt's is answered here (prompt.ts `listen`): the
// inspector of the card it is on, placed clear of what the open prompt may have the player
// pick next; the viewer a click on a card tucked under its host opens; and why a click was
// refused (Refusal.tsx).
import {
  useEffect,
  useMemo,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from "react";
import { Button, Tip } from "../components/Controls";
import { LifeOrb, ManaPool, PlayerBadge, Thinking } from "../components/Player";
import { StepStops } from "../components/Steps";
import type { PlayerView, Side, StopsSetting } from "../generated/protocol";
import type { Box } from "../game/placement";
import { incoming } from "../game/combat";
import { FullControlIcon } from "../game/Corner";
import { InspectAt, inspectorPlace } from "../game/Inspector";
import { Incoming, LifeChange } from "../game/Life";
import type { LogEntry } from "../game/log";
import { GameLog, opponentActing } from "../game/Narration";
import { PileButton, ZoneViewer } from "../game/Piles";
import { useStageWidth } from "../game/Stage";
import { StepBanner, useBannerUp } from "../game/StepBanner";
import { combatBanner, STEP_NAMES } from "../game/stepLabels";
import { MANA_POOLS } from "../game/tableRows";
import { NoticeSlot, TurnAnnouncement } from "../game/TurnBanner";
import {
  exiledWith,
  held as heldBy,
  libraries,
  zonePiles,
} from "../game/zoneViews";
import { desktop } from "./desktop.ts";
import type { Name, Resting } from "./layout.ts";
import { CARD_ASPECT } from "../components/Card";
import {
  announced,
  coinBanner,
  endWords,
  lifeChange,
  scryWords,
} from "./notices.ts";
import { trayOf, type Asked, type Prompts } from "./prompt.ts";
import { noticePlace, TimingRefusal, useRefusal } from "./Refusal.tsx";
import type { ScriptStage } from "./stage.ts";

const SIDES = ["opponent", "you"] as const;

/** Where each badge stands, at the table's left. */
const BADGES = { opponent: 62, you: 704 };

/** Where the pill of a player's scry stands: under the opponent's orb, over the player's. */
const SCRY_TOP = { opponent: 160, you: 580 };

/** Where the pill naming the end of a library a player chose lies: under that library,
 * toward the table, centred on it. The graveyard lies beside it and the card going onto it
 * lands on it, so neither side of it is clear. */
function underLibrary({
  x,
  y,
  width,
}: {
  x: number;
  y: number;
  width: number;
}) {
  return {
    left: x + width / 2,
    top: y + width / CARD_ASPECT + 8,
    translate: "-50% 0",
  };
}

/** The attackers the open prompt has a blocker aimed at: blocked, once it is answered so. */
export function blockedBy({ prompt }: Asked): ReadonlySet<number> {
  const state = prompt?.body;
  if (state?.shape !== "declare" || state.arrow !== "block") return NONE;
  return new Set(
    state.pairs.flatMap(({ to }) => (to.kind === "object" ? [to.slot] : [])),
  );
}

const NONE: ReadonlySet<number> = new Set();

/** What lies on the table under its cards: the badges, both life totals with the combat
 * damage on its way, the mana pools, the log, and what is announced. */
export function Hud({
  stage,
  view,
  asked,
  lost,
  log,
  waiting,
  logOpen,
  onLog,
  orb,
}: {
  stage: ScriptStage;
  /** The open prompt: the blocks it has picked take their attackers' damage off what is on
   * its way. */
  asked: Asked;
  /** How many times the player passed on with mana unspent: each pulses their pool
   * (PromptBar.tsx `useLostMana`). */
  lost: number;
  /** What a prompt lays over a player's life total: where a click picks that player. */
  orb?: (side: Side) => ReactNode;
  /** The view last played out. */
  view: PlayerView;
  log: LogEntry[];
  waiting: boolean;
  logOpen: boolean;
  onLog(): void;
}) {
  const { table, notices, counts } = useSyncExternalStore(
    stage.subscribe,
    () => stage.shown,
  );
  // The orbs and the pills hold to the stage's middle.
  useStageWidth();
  const coin = announced(notices, "coin");
  const scry = announced(notices, "scry");
  const end = announced(notices, "topOrBottom");
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
  const blocked = blockedBy(asked);
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
        const damage = beforeDamage
          ? incoming(table, side, blocked)
          : undefined;
        const change = lifeChange(counts, side);
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
            />
            {damage !== undefined && (
              <Incoming amount={damage} lethal={damage > 0 && damage >= life} />
            )}
            {change !== undefined && <LifeChange by={change} />}
            {orb?.(side)}
          </div>
        );
      })}
      <NoticeSlot>
        {scry && (
          <div
            className="hud library-choice-hud stage-notice-scry"
            role="status"
            style={{ top: SCRY_TOP[scry.notice.player] }}
          >
            <Button variant="secondary" onClick={onLog}>
              {scryWords(scry.notice)}
            </Button>
          </div>
        )}
      </NoticeSlot>
      <NoticeSlot>
        {end && (
          <div
            className="hud library-end-hud"
            style={underLibrary(anchors.pile(end.notice.player, "library"))}
          >
            <div className="stage-notice-end" role="status">
              <Button variant="secondary" onClick={onLog}>
                {endWords(end.notice)}
              </Button>
            </div>
          </div>
        )}
      </NoticeSlot>
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
          <ManaPool
            pool={table.players[side].mana}
            pulse={side === "you" ? lost : 0}
          />
        </div>
      ))}
      <NoticeSlot>
        {banner && (
          <StepBanner
            key={`${banner.title}|${banner.detail}`}
            shown={bannerUp}
            coin={coin !== undefined}
            staged={coin !== undefined}
            {...banner}
          />
        )}
      </NoticeSlot>
      <GameLog log={log} folded open={logOpen} onToggle={onLog} />
      <NoticeSlot>
        {turn && (
          <TurnAnnouncement key={table.turn.number} side={turn.notice.active} />
        )}
      </NoticeSlot>
    </>
  );
}

/** The node the pointer is on, as the stage tells it (pointer.ts) through what draws its
 * prompts, which hears the pointer first (prompt.ts). */
export function useHovered(
  stage: ScriptStage,
  prompts: Prompts,
): Name | undefined {
  const [hovered, setHovered] = useState<Name>();
  useEffect(() => {
    setHovered(stage.pointer.hovered);
    return prompts.listen({ hover: setHovered });
  }, [stage, prompts]);
  return hovered;
}

/** The room the card `resting` takes on the stage, leaned as it lies. */
function boxOf(resting: Resting): Box {
  const middle = desktop.middle(resting, resting.pose);
  const half = desktop.half(resting, resting.pose);
  const lean = Math.abs(Math.tan((resting.pose.skew * Math.PI) / 180)) * half.y;
  return {
    left: middle.x - half.x - lean,
    right: middle.x + half.x + lean,
    top: middle.y - half.y,
    bottom: middle.y + half.y,
  };
}

/** What the inspector, and a raised hand card, keep clear of while `asked` is open: every
 * permanent the prompt marks, which the player may be about to pick, and the cards it holds
 * up. None at a priority, whose marks say only what may be used. */
export function keptClear(
  { prompt }: Asked,
  resting: ReadonlyMap<Name, Resting>,
): Box[] {
  if (!prompt || prompt.purpose === "priority") return [];
  const marked = prompt.marks.flatMap(({ on }) => {
    const lies = on.kind === "object" ? resting.get(on.slot) : undefined;
    return lies?.dress.zone === "battlefield" ? [lies] : [];
  });
  const held = (trayOf(prompt)?.piles ?? []).flatMap(({ cards }) =>
    cards.flatMap((slot) => resting.get(slot) ?? []),
  );
  return [...marked, ...held].map(boxOf);
}

/** What lies over the table's cards: a button on each pile and the viewer it opens, the
 * strips the stack's lower items peek out by, each player's step stops with full control's
 * mark and the dock beside them (`children`), and the inspector of the card the pointer is
 * on. */
export function Controls({
  stage,
  prompts,
  asked,
  hovered,
  stops,
  setStops,
  dimmed,
  children,
}: {
  stage: ScriptStage;
  prompts: Prompts;
  asked: Asked;
  /** The node the pointer is on (`useHovered`). */
  hovered: Name | undefined;
  /** The dock: the open prompt's buttons (PromptBar.tsx). */
  children?: ReactNode;
  stops: StopsSetting;
  setStops(stops: StopsSetting): void;
  dimmed?: boolean;
}) {
  const { table, resting } = useSyncExternalStore(
    stage.subscribe,
    () => stage.shown,
  );
  const width = useStageWidth();
  // The pile whose viewer is open, or the permanent `linked` whose exiled cards it shows.
  const [viewing, setViewing] = useState<
    { side: Side; zone: string } | { linked: number }
  >();
  const { refused, notice } = useRefusal(prompts, asked, resting);
  // A card peeking out from behind the permanent that holds it in exile opens them all.
  useEffect(
    () =>
      prompts.listen({
        click(name) {
          const { table, resting } = stage.shown;
          const clicked = resting.get(name);
          if (!table || !clicked?.object) return;
          if (clicked.dress.look !== "beneath") return;
          const host = heldBy(clicked.object, new Set(table.battlefield));
          if (host !== undefined) setViewing({ linked: host });
        },
      }),
    [stage, prompts],
  );
  const piles = useMemo(
    () => (table ? [...zonePiles(table), ...libraries(table)] : []),
    // The piles hold to the stage's edges.
    [table, width],
  );
  if (!table) return null;
  const host = viewing && "linked" in viewing ? viewing.linked : undefined;
  const viewed =
    host !== undefined
      ? { cards: exiledWith(table, host) }
      : piles.find(
          (p) =>
            viewing &&
            "side" in viewing &&
            p.side === viewing.side &&
            p.zone === viewing.zone,
        );
  const viewedKey =
    host !== undefined
      ? `linked:${host}`
      : viewed && "zone" in viewed
        ? `${viewed.side}:${viewed.zone}`
        : "";
  const viewedTitle =
    host === undefined
      ? undefined
      : (resting.get(host)?.object?.name ?? undefined);
  const keeps = keptClear(asked, resting);
  // The card the pointer is on, where it is shown alone: not one under the top of a pile, nor
  // one of your hand, which rises large enough to read in place (UI Conventions, "Hover
  // inspection"), unless a click on it was refused, whose reason the inspector gives.
  const pointed = hovered === undefined ? undefined : resting.get(hovered);
  const inspected =
    pointed?.object &&
    pointed.dress.look !== "hidden" &&
    pointed.dress.look !== "pile" &&
    pointed.dress.look !== "handling" &&
    (pointed.dress.look !== "hand" || refused?.slot === pointed.object.slot)
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
              current={table.turn.active === side ? table.turn.step : undefined}
              now={`Turn ${table.turn.ownNumber} · ${STEP_NAMES[table.turn.step]}`}
              whose={side === "you" ? "Your" : "Opponent's"}
              stops={stops[key]}
              onStops={(steps) => setStops({ ...stops, [key]: steps })}
            />
          );
        })}
        {children}
      </div>
      {notice && <TimingRefusal key={notice.id} source={notice.source} />}
      {inspected && !dimmed && !viewed && (
        <InspectAt
          view={table}
          object={inspected.object}
          place={inspectorPlace(
            inspected.pose.x,
            // The stack, or a prompt's words and buttons, hold the table's right.
            table.stack.length > 0 || keeps.length > 0,
            notice ? [...keeps, noticePlace(notice.source).box] : keeps,
          )}
          refusal={
            refused?.slot === inspected.object.slot ? refused.reason : undefined
          }
        />
      )}
      {viewed && !dimmed && (
        <ZoneViewer
          key={viewedKey}
          pile={viewed}
          title={viewedTitle}
          onClose={() => setViewing(undefined)}
        />
      )}
    </>
  );
}
