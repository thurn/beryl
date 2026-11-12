// The phone's chrome over the stage's cards beyond its strips and dock (mobile/chrome.tsx):
// what the desktop's board shows around its table, composed for a phone from the same
// components (plan K29; UI Conventions, "The phone"). The menu holds the settings, the log and
// the bug report; a touch on a pile lists its cards, and a touch on a card inspects it; undo
// and full control's mark stand at the end of the middle band; announcements pass over the
// table for as long as their steps play.
import type { CSSProperties, ReactNode } from "react";
import { requestBugReport } from "../components/BugReport";
import { Button } from "../components/Controls";
import type {
  ObjectView,
  PlayerView,
  Side,
  StopsSetting,
} from "../generated/protocol";
import { FullControlIcon, Settings, UndoButton } from "../game/Corner";
import type { LogEntry } from "../game/log";
import { GameLog } from "../game/Narration";
import { Inspect, pileName } from "../game/Piles";
import { StepBanner, useBannerUp } from "../game/StepBanner";
import { combatBanner } from "../game/stepLabels";
import { TurnAnnouncement } from "../game/TurnBanner";
import { knownLibraries, zonePiles, type ZonePile } from "../game/zoneViews";
import { CardScan } from "../mobile/faces";
import {
  PILE,
  type Band,
  type BandName,
  type Viewport,
} from "../mobile/layout";
import type { Name, Resting } from "./layout.ts";
import { announced, coinBanner, scryWords } from "./notices.ts";
import type { ScriptStage } from "./stage.ts";

const NO_ORDER: ReadonlyMap<number, number> = new Map();

/** The least a control measures on a phone (phone.css). */
const TOUCH = 40;
/** A card listed in a pile's sheet. */
const LISTED = 76;
/** The inspector's width on a phone at most (game/Piles.tsx INSPECT_WIDTH), and its margin. */
const INSPECT = { width: 330, margin: 16, top: 56 };

type Bands = Record<BandName, Band>;

/** A pile the phone lists: a strip has a library and a graveyard. */
export type PhonePile = ZonePile & { zone: "library" | "graveyard" };

/** The piles a touch opens: a graveyard that holds anything, and a library of which a card is
 * known. */
export function phonePiles(table: PlayerView): PhonePile[] {
  return [...zonePiles(table, NO_ORDER), ...knownLibraries(table)].filter(
    (pile): pile is PhonePile => pile.zone !== "exile" && pile.cards.length > 0,
  );
}

/** `table` with only the cards that have landed in each library and graveyard, for the strips'
 * piles (mobile/chrome.tsx Piles): one still going from sight where it was (a `vanish` under
 * way) rests dressed as it was there, and its pile shows no edge for it until it lands. */
export function landed(
  table: PlayerView,
  resting: ReadonlyMap<Name, Resting>,
): PlayerView {
  const on = (zone: "library" | "graveyard", slots: number[]) =>
    slots.filter((slot) => (resting.get(slot)?.dress.zone ?? zone) === zone);
  const { you, opponent } = table.players;
  const piles = (player: typeof you) => ({
    ...player,
    library: on("library", player.library),
    graveyard: on("graveyard", player.graveyard),
  });
  return { ...table, players: { you: piles(you), opponent: piles(opponent) } };
}

/** A button over each pile that opens, on the card the stage draws there. */
export function PileButtons({
  bands,
  piles,
  onOpen,
}: {
  bands: Bands;
  piles: PhonePile[];
  onOpen(pile: PhonePile): void;
}) {
  return piles.map((pile) => {
    // As large as a finger: its strip's height, and half the piles' corner each.
    const strip = pile.side === "you" ? bands.yourStrip : bands.opponentStrip;
    return (
      <button
        key={`${pile.side}-${pile.zone}`}
        type="button"
        className="phone-pile-button"
        style={{
          left: pile.zone === "library" ? 0 : PILE.graveyard - 6,
          top: strip.top,
          width: TOUCH,
          height: strip.height,
        }}
        aria-label={pileName(pile)}
        data-pile={pile.zone}
        data-side={pile.side}
        onPointerDown={(e) => e.stopPropagation()}
        onClick={() => onOpen(pile)}
      />
    );
  });
}

/** Something over the whole board that a touch beside it closes. */
function Sheet({
  label,
  className,
  onClose,
  children,
}: {
  label: string;
  className: string;
  onClose(): void;
  children: ReactNode;
}) {
  return (
    <div
      className="phone-scrim"
      onPointerDown={(e) => e.stopPropagation()}
      onClick={onClose}
    >
      <div
        className={className}
        role="dialog"
        aria-label={label}
        onClick={(e) => e.stopPropagation()}
      >
        {children}
      </div>
    </div>
  );
}

/** The menu under its button: the log and the bug report, then the settings the desktop's
 * gear holds. */
export function Menu({
  stops,
  setStops,
  onLeave,
  onConcede,
  onLog,
  onClose,
}: {
  stops: StopsSetting;
  setStops(stops: StopsSetting): void;
  onLeave(): void;
  onConcede?: () => void;
  onLog(): void;
  onClose(): void;
}) {
  return (
    <Sheet label="Menu" className="settings phone-menu-sheet" onClose={onClose}>
      <div className="phone-menu-row">
        <Button variant="secondary" size="small" onClick={onLog}>
          Log
        </Button>
        <Button
          variant="secondary"
          size="small"
          onClick={() => {
            onClose();
            requestBugReport();
          }}
        >
          Report Bug
        </Button>
      </div>
      <Settings
        stops={stops}
        setStops={setStops}
        onLeave={onLeave}
        onConcede={
          onConcede &&
          (() => {
            onClose();
            onConcede();
          })
        }
      />
    </Sheet>
  );
}

const never = () => {};

/** The game log, whole. */
export function LogSheet({
  log,
  onClose,
}: {
  log: LogEntry[];
  onClose(): void;
}) {
  return (
    <Sheet label="Game log" className="phone-sheet" onClose={onClose}>
      <GameLog log={log} folded={false} open onToggle={never} />
    </Sheet>
  );
}

/** A pile's cards, the top one first; a touch on one inspects it. */
export function PileSheet({
  pile,
  onInspect,
  onClose,
}: {
  pile: PhonePile;
  onInspect(slot: number | undefined): void;
  onClose(): void;
}) {
  const name = pileName(pile);
  return (
    <Sheet label={name} className="phone-sheet" onClose={onClose}>
      <h2 className="phone-sheet-title" onClick={() => onInspect(undefined)}>
        {name}
      </h2>
      <div className="phone-sheet-cards" onClick={() => onInspect(undefined)}>
        {[...pile.cards].reverse().map((object) => (
          <button
            key={object.slot}
            type="button"
            className="phone-sheet-card"
            aria-label={object.name ?? "A card"}
            onClick={(e) => {
              e.stopPropagation();
              onInspect(object.slot);
            }}
          >
            <CardScan object={object} width={LISTED} field={false} />
          </button>
        ))}
      </div>
    </Sheet>
  );
}

/** The card a touch inspects, whole, over the middle of the board. */
export function Inspected({
  object,
  table,
  viewport,
}: {
  object: ObjectView;
  table: PlayerView;
  viewport: Viewport;
}) {
  const width = Math.min(INSPECT.width, viewport.width - 2 * INSPECT.margin);
  return (
    <Inspect
      object={object}
      attached={table.objects.filter(
        (o) => o.zone === "battlefield" && o.attachedTo === object.slot,
      )}
      place={{ left: (viewport.width - width) / 2, top: INSPECT.top, width }}
    />
  );
}

/** At the end of the middle band, across from the phase: undo while there is an answer to take
 * back, and full control's mark while it is on. */
export function MiddleEnd({
  band,
  fullControl,
  onUndo,
}: {
  band: Band;
  fullControl: boolean;
  onUndo?: () => void;
}) {
  return (
    <div className="phone-middle-end" style={band}>
      {fullControl && (
        <span className="full-control">
          <FullControlIcon />
        </span>
      )}
      {onUndo && <UndoButton onUndo={onUndo} />}
    </div>
  );
}

/** How wide a scry's pill is at most, and how far it stands off the row it lies beside. */
const PILL = { width: 156, gap: 8 };

/** What is announced, over the table: the turn across the middle band, a combat step or a coin's
 * flip under the opponent's strip, and what a scry did beside its player's lands. The combat
 * banner, which no step announces, keeps the desktop's timer. */
export function Notices({
  stage,
  view,
  table,
  bands,
  width,
}: {
  stage: ScriptStage;
  /** The view last played out. */
  view: PlayerView;
  table: PlayerView;
  bands: Bands;
  /** The stage's width, in its own pixels. */
  width: number;
}) {
  const { notices, resting } = stage.shown;
  const coin = announced(notices, "coin");
  const scry = announced(notices, "scry");
  const turn = announced(notices, "turn");
  const combat = combatBanner(view);
  const combatUp = useBannerUp(
    coin || !combat ? undefined : `${combat.title}|${combat.detail}`,
  );
  const banner = coin ? coinBanner(coin, table) : combat;
  const middle = bands.middle.top + bands.middle.height / 2;
  /** Where a pill lies beside `side`'s lands: past the end of their row, on whichever side
   * of it has the room, so that it covers no permanent while play goes on under it. None
   * where the row leaves no room, when it lies at the strip's edge as before. */
  const beside = (side: Side): CSSProperties | undefined => {
    const band = side === "you" ? bands.yourLands : bands.opponentLands;
    let [left, right] = [Infinity, -Infinity];
    for (const { pose, dress, object } of resting.values()) {
      const within = pose.y >= band.top - 1 && pose.y < band.top + band.height;
      if (dress.zone !== "battlefield" || !within) continue;
      left = Math.min(left, pose.x);
      // A tapped card lies on its long side.
      right = Math.max(right, pose.x + pose.width * (object?.tapped ? 1.4 : 1));
    }
    const top = band.top + band.height / 2;
    const row = { top, translate: "0 -50%" };
    if (right === -Infinity)
      return { ...row, left: "50%", translate: "-50% -50%" };
    if (right + PILL.gap + PILL.width <= width) {
      return { ...row, left: right + PILL.gap };
    }
    if (left - PILL.gap - PILL.width >= 0) {
      return { ...row, left: left - PILL.gap, translate: "-100% -50%" };
    }
    return undefined;
  };
  const edge = (side: Side) =>
    side === "you"
      ? bands.yourStrip.top - 30
      : bands.opponentStrip.top + bands.opponentStrip.height + 4;
  return (
    <>
      {banner && (
        <div
          className="phone-announce phone-announce-step"
          // A coin is tossed mid-table, just above where the turn is announced; a combat
          // step's banner lies under the opponent's strip.
          style={{ top: coin ? middle - 80 : edge("opponent") }}
        >
          <StepBanner
            key={`${banner.title}|${banner.detail}`}
            shown={coin !== undefined || combatUp}
            coin={coin !== undefined}
            staged={coin !== undefined}
            {...banner}
          />
        </div>
      )}
      {scry && (
        <div
          className="phone-pill stage-notice-scry"
          role="status"
          style={
            beside(scry.notice.player) ?? { top: edge(scry.notice.player) }
          }
        >
          {scryWords(scry.notice)}
        </div>
      )}
      {turn && (
        <div className="phone-announce" style={{ top: middle }}>
          <TurnAnnouncement
            key={table.turn.number}
            side={turn.notice.active}
            staged
          />
        </div>
      )}
    </>
  );
}
