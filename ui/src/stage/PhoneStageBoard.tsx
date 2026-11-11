// The phone's game screen drawn on the stage (plan K25 and K29), in place of
// mobile/PhoneBoard.tsx behind `?stage` (switch.tsx): the phone's strips and dock as they are
// (mobile/chrome.tsx), one node an object or ability where the page's stage has it, laid out
// for the phone (phone.ts) and drawn in the phone's faces (mobile/faces.tsx), and the rest of
// the chrome the desktop's board has, composed for a phone (PhoneChrome.tsx). The same scripts
// play as on the desktop. What the chrome shows of the table (life, mana, the piles, the
// phase) is the stage's table, so that what a step changes shows as the step plays; the dock
// and undo answer the view last played out. A touch on a card inspects it, as the stage tells
// it (pointer.ts); the prompt shapes add what a touch chooses.
import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useState,
  useSyncExternalStore,
  type ComponentProps,
} from "react";
import { ManaPool, Thinking } from "../components/Player";
import type { StopsSetting } from "../generated/protocol";
import { incoming } from "../game/combat";
import { interactions } from "../game/decisions";
import type { LogEntry } from "../game/log";
import { opponentActing } from "../game/Narration";
import {
  MenuButton,
  Middle,
  Orb,
  PhoneDock,
  Piles,
  useViewport,
} from "../mobile/chrome";
import { PHONE } from "../mobile/faces";
import { phoneLayout } from "../mobile/layout";
import type { PhoneBoard } from "../mobile/PhoneBoard";
import "../mobile/phone.css";
import { Faces } from "./Faces.tsx";
import { lifeChange } from "./notices.ts";
import { useSurface } from "./mount.ts";
import { phone } from "./phone.ts";
import {
  Inspected,
  LogSheet,
  Menu,
  MiddleEnd,
  Notices,
  PileButtons,
  phonePiles,
  PileSheet,
  type PhonePile,
} from "./PhoneChrome.tsx";
import { pageStage } from "./playback.ts";
import "./stage.css";

/** What the phone's board on the stage takes: the old phone board's, and what the chrome it
 * adds answers with. */
export type PhoneStageProps = ComponentProps<typeof PhoneBoard> & {
  log: LogEntry[];
  /** Takes back the player's latest answer, at a decision that may. */
  undo?: () => void;
  stops: StopsSetting;
  setStops(stops: StopsSetting): void;
  /** Concedes the game; none once it is over. */
  onConcede?: () => void;
};

const SIDES = ["opponent", "you"] as const;

export function PhoneStageBoard({
  view,
  waiting,
  choose,
  passOn,
  onLeave,
  log,
  undo,
  stops,
  setStops,
  onConcede,
}: PhoneStageProps) {
  const { stage, feed } = pageStage();
  const { ref, viewport } = useViewport();
  const { layer, surface } = useSurface(stage);
  const { table, counts } = useSyncExternalStore(stage.subscribe, () => stage.shown);
  // The table is laid out for a phone this size while this board shows it.
  useLayoutEffect(() => {
    if (viewport) stage.relayout(phone(viewport));
  }, [stage, viewport]);
  const bands = useMemo(
    () => viewport && table && phoneLayout(table, viewport).bands,
    [table, viewport],
  );
  const can = useMemo(() => interactions(view), [view]);
  const objects = useMemo(
    () => new Map(view.objects.map((o) => [o.slot, o])),
    [view],
  );
  const piles = useMemo(() => (table ? phonePiles(table) : []), [table]);
  const [sheet, setSheet] = useState<
    "menu" | "log" | Pick<PhonePile, "side" | "zone">
  >();
  // The slot of the card a touch inspects.
  const [inspecting, setInspecting] = useState<number>();
  // A touch on a card the player can read inspects it, and a second one puts it away.
  useEffect(
    () =>
      stage.pointer.listen({
        click(name) {
          const touched = stage.shown.resting.get(name);
          const readable =
            typeof name === "number" &&
            touched?.object?.name != null &&
            !touched.dress.back &&
            touched.dress.look !== "hidden";
          setInspecting((slot) =>
            readable && slot !== name ? name : undefined,
          );
        },
      }),
    [stage],
  );
  const inspected =
    inspecting === undefined
      ? undefined
      : table?.objects.find((o) => o.slot === inspecting);
  const pile =
    typeof sheet === "object"
      ? piles.find((p) => p.side === sheet.side && p.zone === sheet.zone)
      : undefined;
  const decision =
    view.status.kind === "deciding" ? view.status.decision : undefined;
  const undoable = !waiting && undo !== undefined && (decision?.undo ?? 0) > 0;
  // The damage shows from the attack's declaration until it is dealt.
  const beforeDamage =
    table?.turn.step === "declareAttackers" ||
    table?.turn.step === "declareBlockers";
  return (
    <div
      className="phone-board"
      ref={ref}
      onPointerDown={(e) => {
        feed.skip();
        // A touch beside every card puts the inspected one away.
        if (!(e.target as Element).closest("[data-stage]"))
          setInspecting(undefined);
      }}
    >
      {bands && table && (
        <>
          {SIDES.map((side) => (
            <div
              key={side}
              className={`phone-strip phone-strip-${side}`}
              style={side === "you" ? bands.yourStrip : bands.opponentStrip}
            >
              <Piles side={side} view={table} tops={false} />
              <div className="phone-pool">
                <ManaPool pool={table.players[side].mana} />
              </div>
              <Orb
                side={side}
                view={table}
                incoming={beforeDamage ? incoming(table, side) : undefined}
                change={lifeChange(counts, side)}
              />
              {side === "you" ? (
                <PhoneDock
                  view={view}
                  waiting={waiting}
                  can={can}
                  objects={objects}
                  choose={choose}
                  passOn={passOn}
                />
              ) : (
                <>
                  {opponentActing(view, waiting) && (
                    <div className="phone-thinking">
                      <Thinking />
                    </div>
                  )}
                  <MenuButton
                    onLeave={onLeave}
                    onOpen={() => setSheet("menu")}
                  />
                </>
              )}
            </div>
          ))}
          <Middle band={bands.middle} view={table} />
          <MiddleEnd
            band={bands.middle}
            fullControl={stops.fullControl}
            onUndo={undoable ? undo : undefined}
          />
        </>
      )}
      <div className="card-layer phone-cards" ref={layer} />
      {surface && <Faces stage={stage} surface={surface} skin={PHONE} />}
      {bands && table && (
        <>
          <PileButtons
            bands={bands}
            piles={piles}
            onOpen={({ side, zone }) => setSheet({ side, zone })}
          />
          <Notices stage={stage} view={view} table={table} bands={bands} />
        </>
      )}
      {sheet === "menu" && (
        <Menu
          stops={stops}
          setStops={setStops}
          onLeave={onLeave}
          onConcede={onConcede}
          onLog={() => setSheet("log")}
          onClose={() => setSheet(undefined)}
        />
      )}
      {sheet === "log" && (
        <LogSheet log={log} onClose={() => setSheet(undefined)} />
      )}
      {pile && (
        <PileSheet
          pile={pile}
          onInspect={setInspecting}
          onClose={() => {
            setSheet(undefined);
            setInspecting(undefined);
          }}
        />
      )}
      {inspected && table && viewport && (
        <Inspected object={inspected} table={table} viewport={viewport} />
      )}
    </div>
  );
}
