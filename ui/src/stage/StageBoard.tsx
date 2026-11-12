// The game's table drawn on the stage (plan K25), in place of game/Board.tsx behind `?stage`
// (switch.tsx) until it takes over: the cloth, one node an object or ability where the page's
// stage has it (playback.ts), and the chrome around them (Chrome.tsx, game/Corner.tsx). The
// stage plays whatever the session sent; this gives it somewhere to draw, lays the table out
// for what stands around it, and hears the pointer. The prompt picked on the table is the
// session's, drawn and answered through prompt.ts and PromptBar.tsx, as are a cost paid with
// permanents, a declaration of attackers or blockers (Aim.tsx draws its line to the pointer),
// the cards a prompt holds up to be sorted, picked or asked about and the triggers it has put
// in order (Tray.tsx), and the ways to use a card, a catalog's entries and a division's
// shares (Choose.tsx). What the session holds no state of is answered over the table.
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ComponentProps,
} from "react";
import type { Board } from "../game/Board";
import { Corner, UndoButton } from "../game/Corner";
import { useStageWidth } from "../game/Stage";
import { Aim } from "./Aim.tsx";
import { Entries, Shares, Ways } from "./Choose.tsx";
import { Controls, Hud } from "./Chrome.tsx";
import type { ObjectView } from "../generated/protocol";
import { DESKTOP_REVEALED, desktopTray, desktopWith } from "./desktop.ts";
import { DESKTOP, Faces } from "./Faces.tsx";
import { useSurface } from "./mount.ts";
import { pageStage } from "./playback.ts";
import {
  PlayerTarget,
  PromptDock,
  usePaying,
  usePrompt,
  usePromptKeys,
} from "./PromptBar.tsx";
import { RevealedCards } from "./Revealed.tsx";
import { OrderRow, TrayUnder } from "./Tray.tsx";
import { landing } from "./tray.ts";
import "./stage.css";

export function StageBoard({
  view,
  log,
  waiting,
  onSkip,
  cancel,
  undo,
  stops,
  setStops,
  onLeave,
  onConcede,
  dimmed,
  inert,
}: ComponentProps<typeof Board>) {
  const { stage, prompts } = pageStage();
  const asked = usePrompt(prompts);
  const { layer, surface } = useSurface(stage);
  const width = useStageWidth();
  const [settings, setSettings] = useState(false);
  const [logOpen, setLogOpen] = useState(false);
  const toggleLog = useCallback(() => setLogOpen((open) => !open), []);
  // The Revealed Cards the player dismissed.
  const [cleared, setCleared] = useState<ReadonlySet<ObjectView>>();
  // The hand sinks under a screen over the table: away under the opening, which shows it.
  const hold = !dimmed ? "held" : view.turn.number === 0 ? "away" : "tucked";
  // The table is laid out for the desktop while this board shows it, and afresh at each
  // width (the layout reads the stage's as it places, game/layout.ts STAGE) and whenever what
  // stands around it changes.
  useLayoutEffect(
    () => stage.relayout(desktopWith({ hold, logOpen, cleared })),
    [stage, width, hold, logOpen, cleared],
  );
  // A card carried between a tray's piles lands where the desktop lays the tray out.
  useEffect(() => {
    prompts.lands = (tray, at) => landing(tray, desktopTray(), at);
    return () => {
      prompts.lands = () => undefined;
    };
  }, [prompts]);
  const decision =
    view.status.kind === "deciding" ? view.status.decision : undefined;
  const undoable = !waiting && undo !== undefined && (decision?.undo ?? 0) > 0;
  const { pay, suggest } = usePaying(asked);
  const press = prompts.press.bind(prompts);
  const number = prompts.number.bind(prompts);
  const answers = { waiting, press, cancel, suggest, number };
  // A prompt's keys answer it while nothing else takes them.
  usePromptKeys(asked, answers, !settings && !onSkip && !inert);
  // The keys that are not a prompt's: Escape closes the settings, the platform's undo takes
  // back an answer, and Space or Enter ends a replay as a press on the table does.
  const onKey = useRef<(e: KeyboardEvent) => void>(undefined);
  onKey.current = (e) => {
    if (e.key === "Escape") {
      if (settings) setSettings(false);
    } else if (e.key.toLowerCase() === "z" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      if (undoable) undo();
    } else if ((e.key === " " || e.key === "Enter") && onSkip) {
      // A control that has the focus keeps its own key.
      const control =
        e.target instanceof Element &&
        e.target.closest("button, input, select, textarea, a");
      if (control) return;
      e.preventDefault();
      onSkip();
    }
  };
  useEffect(() => {
    const heard = (e: KeyboardEvent) => onKey.current?.(e);
    window.addEventListener("keydown", heard);
    return () => window.removeEventListener("keydown", heard);
  }, []);
  return (
    <>
      <div
        className={dimmed ? "table table-dimmed" : "table"}
        data-opening={view.turn.number === 0 || undefined}
        data-pay={pay}
        onPointerDown={() => setSettings(false)}
        // Before the card under it hears the press, so that the card's click or drag is one
        // on the newest view.
        onPointerDownCapture={onSkip}
      >
        <div className="far-shade" />
        <Hud
          stage={stage}
          view={view}
          log={log}
          waiting={waiting}
          logOpen={logOpen}
          onLog={toggleLog}
          orb={(side) => (
            <PlayerTarget prompts={prompts} asked={asked} side={side} />
          )}
        />
        <div className="card-layer" ref={layer} />
        {surface && <Faces stage={stage} surface={surface} skin={DESKTOP} />}
        {surface && (
          // Keyed by the width its places are read at (game/layout.ts STAGE).
          <TrayUnder
            key={width}
            asked={asked}
            space={desktopTray()}
            layer={layer.current}
          />
        )}
        {surface && (
          <Aim asked={asked} layer={layer.current} surface={surface} />
        )}
        <OrderRow
          key={`order-${width}`}
          asked={asked}
          prompts={prompts}
          view={view}
          space={desktopTray()}
        />
        <Ways
          key={`ways-${width}`}
          asked={asked}
          prompts={prompts}
          view={view}
          space={desktopTray()}
        />
        <Entries
          key={`entries-${width}`}
          asked={asked}
          prompts={prompts}
          space={desktopTray()}
        />
        <Shares asked={asked} prompts={prompts} stage={stage} />
        <Controls
          stage={stage}
          prompts={prompts}
          stops={stops}
          setStops={setStops}
          inert={inert}
          dimmed={dimmed}
        >
          <PromptDock asked={asked} {...answers} />
        </Controls>
        {!dimmed && (
          <RevealedCards
            stage={stage}
            prompts={prompts}
            space={DESKTOP_REVEALED}
            onDismiss={(cards) => setCleared(new Set(cards))}
          />
        )}
        <Corner
          inert={inert}
          open={settings}
          setOpen={setSettings}
          stops={stops}
          setStops={setStops}
          onLeave={onLeave}
          onConcede={onConcede}
        />
      </div>
      {/* Undo stays accessible while a choice covers the table. */}
      {undoable && <UndoButton onUndo={undo} />}
    </>
  );
}
