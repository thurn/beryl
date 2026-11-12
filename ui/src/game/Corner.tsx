// The table's corner controls: the gear's column at
// the top right (the bug report, the gear and its settings, with concede and leaving in them),
// undo under it, and full control's mark by the turn button (UI Conventions, "Chrome").
import { useState } from "react";
import { SoundPanel } from "../audio/SoundControls";
import { BugIcon, requestBugReport } from "../components/BugReport";
import { Button, Tip } from "../components/Controls";
import type { StopsSetting } from "../generated/protocol";

/** The gear's column at the top right: the bug report, which stays over any screen, and the
 * gear with the settings it opens. Undo, under them, is its own (`UndoButton`). */
export function Corner({
  open,
  setOpen,
  stops,
  setStops,
  onLeave,
  onConcede,
}: {
  /** The settings are open: the table's, since a press anywhere on it closes them. */
  open: boolean;
  setOpen(open: boolean): void;
  stops: StopsSetting;
  setStops(stops: StopsSetting): void;
  onLeave(): void;
  /** Concedes the game; none once it is over. */
  onConcede?: () => void;
}) {
  return (
    <>
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
      <button
        type="button"
        className="icon-button gear-button"
        aria-label="Settings"
        onPointerDown={(e) => e.stopPropagation()}
        onClick={() => setOpen(!open)}
      >
        <Gear />
      </button>
      {open && (
        <div className="settings" onPointerDown={(e) => e.stopPropagation()}>
          <Settings
            stops={stops}
            setStops={setStops}
            onLeave={onLeave}
            onConcede={
              onConcede &&
              (() => {
                setOpen(false);
                onConcede();
              })
            }
          />
        </div>
      )}
    </>
  );
}

/** What the settings hold, in the gear's panel and in the phone's menu: the sound, full
 * control, conceding and leaving. */
export function Settings({
  stops,
  setStops,
  onLeave,
  onConcede,
}: {
  stops: StopsSetting;
  setStops(stops: StopsSetting): void;
  onLeave(): void;
  onConcede?: () => void;
}) {
  return (
    <>
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
      {onConcede && <ConcedeButton onConcede={onConcede} />}
      <Button variant="secondary" size="small" onClick={onLeave}>
        Leave Game
      </Button>
    </>
  );
}

/** Concede, asked once in place: the first click turns it into the confirmation, which the
 * settings closing puts back. */
export function ConcedeButton({ onConcede }: { onConcede(): void }) {
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

/** Full control's mark beside the turn button: a stop sign, for stopping at every priority. */
export function FullControlIcon() {
  return (
    <svg viewBox="0 0 24 24" role="img" aria-label="Full control">
      <path d="M8 1.5 H16 L22.5 8 V16 L16 22.5 H8 L1.5 16 V8 Z" />
      <path className="full-control-bars" d="M9.5 7.5 V16.5 M14.5 7.5 V16.5" />
    </svg>
  );
}

/** Under the gear and the bug report: takes back the player's latest answer, one per press, across the
 * opponent's turns. */
export function UndoButton({ onUndo }: { onUndo: () => void }) {
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
export const MOD_KEY = /Mac|iPhone|iPad/.test(navigator.platform)
  ? "⌘"
  : "Ctrl ";

/** The settings gear at the top right. */
export function Gear() {
  return (
    <svg className="gear" viewBox="0 0 24 24" aria-label="Settings" role="img">
      <circle className="gear-hole" cx="12" cy="12" r="4.9" />
      <path d="M19.67 9.71 L22.34 9.69 L22.34 14.31 L19.67 14.29 L19.04 15.80 L20.95 17.68 L17.68 20.95 L15.80 19.04 L14.29 19.67 L14.31 22.34 L9.69 22.34 L9.71 19.67 L8.20 19.04 L6.32 20.95 L3.05 17.68 L4.96 15.80 L4.33 14.29 L1.66 14.31 L1.66 9.69 L4.33 9.71 L4.96 8.20 L3.05 6.32 L6.32 3.05 L8.20 4.96 L9.71 4.33 L9.69 1.66 L14.31 1.66 L14.29 4.33 L15.80 4.96 L17.68 3.05 L20.95 6.32 L19.04 8.20 Z M16.9 12 A4.9 4.9 0 1 0 7.1 12 A4.9 4.9 0 1 0 16.9 12 Z" />
    </svg>
  );
}
