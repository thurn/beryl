// The game's table drawn on the stage (plan K25), in place of game/Board.tsx behind `?stage`
// (switch.tsx) until it takes over: the cloth, one node an object or ability where the page's
// stage has it (playback.ts), and the chrome around them (Chrome.tsx, game/Corner.tsx). The
// stage plays whatever the session sent; this gives it somewhere to draw, lays the table out
// for what stands around it, and hears the pointer. The prompt picked on the table is the
// session's, drawn and answered through prompt.ts and PromptBar.tsx; the other shapes come
// with their own beads, and are answered over the table until then.
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
import { Controls, Hud } from "./Chrome.tsx";
import { desktopWith } from "./desktop.ts";
import { DESKTOP, Faces } from "./Faces.tsx";
import { useSurface } from "./mount.ts";
import { pageStage } from "./playback.ts";
import {
  PlayerTarget,
  PromptDock,
  usePrompt,
  usePromptKeys,
} from "./PromptBar.tsx";
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
  // The hand sinks under a screen over the table: away under the opening, which shows it.
  const hold = !dimmed ? "held" : view.turn.number === 0 ? "away" : "tucked";
  // The table is laid out for the desktop while this board shows it, and afresh at each
  // width (the layout reads the stage's as it places, game/layout.ts STAGE) and whenever what
  // stands around it changes.
  useLayoutEffect(
    () => stage.relayout(desktopWith({ hold, logOpen })),
    [stage, width, hold, logOpen],
  );
  const decision =
    view.status.kind === "deciding" ? view.status.decision : undefined;
  const undoable = !waiting && undo !== undefined && (decision?.undo ?? 0) > 0;
  const answers = { waiting, press: prompts.press.bind(prompts), cancel };
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
