// The bug report: the bug button at the top right (the board's own on the game screen), or
// ⌘B / Ctrl+B, freezes the page (freeze.ts)
// and snapshots the window as it was that instant, so an animation mid-flight or a hover is
// caught. The player drags out the region that shows the bug over the frozen frame (a click
// takes the whole window), says what went wrong, and submits: the app writes a report folder
// with the region, the window, the game's replay, its latest views and both logs
// (beryl_session::bugs). "AI play" marks a questionable play of the AI's, whose report carries
// the agent's latest searches (beryl_session::ai). The button beside the X, or Space or Enter, takes the whole window
// instead, even pressed straight after ⌘B, before the snapshot is in.
// The X or Escape closes it at any point, and the page plays on.
import { useOpenSound } from "../audio/SoundControls";
import { useCallback, useEffect, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import {
  canCaptureWindow,
  captureWindow,
  reportBug,
  snapshotUrl,
} from "../ipc";
import type { PageInfo, Region } from "../ipc";
import { diagnostics, note } from "../diagnostics";
import { freeze, thaw } from "../freeze";
import { Button } from "./Controls";

type Stage =
  | { kind: "idle" }
  /** Freezing and snapshotting: nothing of the reporter shows, or it would be in the frame. */
  | { kind: "capturing" }
  | { kind: "select"; frame?: Shot }
  | {
      kind: "describe";
      frame?: Shot;
      region: Region;
      page: PageInfo;
      saving: boolean;
      error?: string;
    }
  | { kind: "saved"; frame?: Shot; region: Region };

/** The app's snapshot of the frozen window: its number, and where the app serves it. */
interface Shot {
  id: number;
  url: string;
  pageWidth: number;
}

/** A drag shorter than this, in CSS pixels, is a click: the whole window. */
const CLICK = 6;
/** How long "Saved" shows before the page plays on. */
const SAVED_MS = 900;

const whole = (): Region => ({
  x: 0,
  y: 0,
  width: window.innerWidth,
  height: window.innerHeight,
});

/** The rectangle from `a` to `b`. */
const span = (
  a: { x: number; y: number },
  b: { x: number; y: number },
): Region => ({
  x: Math.min(a.x, b.x),
  y: Math.min(a.y, b.y),
  width: Math.abs(a.x - b.x),
  height: Math.abs(a.y - b.y),
});

/** The event a bug button elsewhere (the game board's, in the gear's column) sends to open the
 * report. */
const REQUEST = "beryl:report-bug";

/** Opens the bug report, as its own button or ⌘B does. */
export const requestBugReport = () => window.dispatchEvent(new Event(REQUEST));

export function BugReporter({ route }: { route: string }) {
  const [stage, setStage] = useState<Stage>({ kind: "idle" });
  const open = stage.kind !== "idle";
  // While it is open, a bug button elsewhere hides (screens.css), out of the frame.
  useEffect(() => {
    if (open) document.documentElement.dataset.bugReport = "";
    else delete document.documentElement.dataset.bugReport;
  }, [open]);

  // What the keys and the snapshot act on, without waiting for a render.
  const stageRef = useRef(stage);
  stageRef.current = stage;
  /** Space or Enter came while the snapshot was being taken: take the whole window. */
  const wantWhole = useRef(false);
  /** Counts openings, so a snapshot that lands after its report was closed is dropped. */
  const opening = useRef(0);
  // The keyboard can resize or zoom the WebView after the player selects a region.
  const frozenPage = useRef<PageInfo>(null);

  /** Goes on to describe `region` of `frame`. */
  const choose = useCallback((frame: Shot | undefined, region: Region) => {
    const page = frozenPage.current;
    if (page)
      setStage({ kind: "describe", frame, region, page, saving: false });
  }, []);

  const start = useCallback(async () => {
    // Stop the motion first: the frame is the instant the player asked.
    freeze();
    const mine = ++opening.current;
    const page = {
      url: window.location.href,
      width: window.innerWidth,
      height: window.innerHeight,
      scale: window.devicePixelRatio,
    };
    frozenPage.current = page;
    wantWhole.current = false;
    setStage({ kind: "capturing" });
    let frame: Shot | undefined;
    if (canCaptureWindow()) {
      try {
        // The pixels stay in the app, which serves them: nothing large crosses IPC.
        const id = await captureWindow();
        frame = { id, url: snapshotUrl(id), pageWidth: page.width };
        // Decoded before it shows, so the overlay never flashes empty.
        const image = new Image();
        image.src = frame.url;
        await image.decode();
      } catch (e) {
        note("warn", "bug", `no window snapshot: ${String(e)}`);
      }
    }
    if (mine !== opening.current) return;
    if (wantWhole.current) choose(frame, whole());
    else setStage({ kind: "select", frame });
  }, [choose]);

  const close = useCallback(() => {
    opening.current += 1;
    setStage({ kind: "idle" });
    thaw();
  }, []);

  // ⌘B or Ctrl+B anywhere opens it; Escape closes it; while it captures or selects, Space or
  // Enter takes the whole window. It hears keys first (the capture phase), and while it
  // captures or selects, nothing else on the page does: the board's Space would pass.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const now = stageRef.current;
      const swallow = () => {
        e.preventDefault();
        e.stopPropagation();
      };
      if (
        (e.metaKey || e.ctrlKey) &&
        !e.altKey &&
        !e.shiftKey &&
        e.key.toLowerCase() === "b"
      ) {
        swallow();
        if (now.kind === "idle") void start();
      } else if (e.key === "Escape" && now.kind !== "idle") {
        swallow();
        close();
      } else if (now.kind === "describe" || now.kind === "saved") {
        // ⌘Enter submits from anywhere, focus in the text or not (a checkbox takes none),
        // and no key aimed past the overlay reaches the page: the board's Enter would pass.
        const inside =
          e.target instanceof Element && e.target.closest("[data-unfrozen]");
        if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
          swallow();
          document
            .querySelector<HTMLFormElement>(".bug-panel")
            ?.requestSubmit();
        } else if (!inside) {
          swallow();
        }
      } else if (now.kind === "capturing" || now.kind === "select") {
        swallow();
        const plain = !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey;
        if ((e.key === " " || e.key === "Enter") && plain && !e.repeat) {
          if (now.kind === "select") choose(now.frame, whole());
          else wantWhole.current = true;
        }
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [start, close, choose]);
  useEffect(() => {
    const onRequest = () => {
      if (stageRef.current.kind === "idle") void start();
    };
    window.addEventListener(REQUEST, onRequest);
    return () => window.removeEventListener(REQUEST, onRequest);
  }, [start]);

  const submit = async (description: string, ai: boolean) => {
    if (stage.kind !== "describe" || stage.saving) return;
    setStage({ ...stage, saving: true, error: undefined });
    const { log, updates } = diagnostics();
    try {
      await reportBug({
        description,
        ai,
        snapshot: stage.frame?.id ?? null,
        region: stage.region,
        page: stage.page,
        uiLog: log,
        updates,
      });
      setStage({ kind: "saved", frame: stage.frame, region: stage.region });
      window.setTimeout(close, SAVED_MS);
    } catch (e) {
      const error =
        e instanceof Error ? e.message : `Not saved: ${JSON.stringify(e)}`;
      setStage({ ...stage, saving: false, error });
    }
  };

  return (
    <>
      {/* The game board has its own, in the gear's column. */}
      {route !== "/game" && (
        <button
          type="button"
          className="icon-button bug-button"
          aria-label="Report bug"
          title="Report bug (⌘B)"
          onClick={() => void start()}
          hidden={open}
        >
          <BugIcon />
        </button>
      )}
      {stage.kind === "select" && (
        <Selector
          frame={stage.frame}
          onChoose={(region) => choose(stage.frame, region)}
          onClose={close}
        />
      )}
      {(stage.kind === "describe" || stage.kind === "saved") && (
        <div
          className="bug-overlay"
          data-unfrozen
          // What is typed here is the report's: the board's keys (⌘Z undoes) never hear it.
          onKeyDown={(e) => e.stopPropagation()}
        >
          <Frame frame={stage.frame} />
          <div
            className="bug-region bug-region-dim"
            style={box(stage.region)}
          />
          {stage.kind === "describe" ? (
            <Describe
              frame={stage.frame}
              region={stage.region}
              saving={stage.saving}
              error={stage.error}
              onSubmit={submit}
            />
          ) : (
            <div className="bug-panel bug-saved" role="status">
              Saved
            </div>
          )}
          <Tools onClose={close} />
        </div>
      )}
    </>
  );
}

const box = (r: Region) => ({
  left: r.x,
  top: r.y,
  width: r.width,
  height: r.height,
});

/** The frozen frame, over the live page; none where the app cannot snapshot (a browser),
 * where the frozen page itself shows through. */
function Frame({ frame }: { frame?: Shot }) {
  return frame ? <img className="bug-frame" src={frame.url} alt="" /> : null;
}

function Selector({
  frame,
  onChoose,
  onClose,
}: {
  frame?: Shot;
  onChoose: (region: Region) => void;
  onClose: () => void;
}) {
  // The report opens over the frozen frame.
  useOpenSound();
  const from = useRef<{ x: number; y: number } | undefined>(undefined);
  const [drag, setDrag] = useState<Region>();
  const down = (e: ReactPointerEvent) => {
    if (e.button !== 0) return;
    (e.target as Element).setPointerCapture(e.pointerId);
    from.current = { x: e.clientX, y: e.clientY };
    setDrag(span(from.current, from.current));
  };
  const move = (e: ReactPointerEvent) => {
    if (from.current)
      setDrag(span(from.current, { x: e.clientX, y: e.clientY }));
  };
  const up = (e: ReactPointerEvent) => {
    if (!from.current) return;
    const region = span(from.current, { x: e.clientX, y: e.clientY });
    from.current = undefined;
    onChoose(region.width < CLICK || region.height < CLICK ? whole() : region);
  };
  return (
    <div className="bug-overlay bug-selecting" data-unfrozen>
      <Frame frame={frame} />
      <div
        className="bug-surface"
        onPointerDown={down}
        onPointerMove={move}
        onPointerUp={up}
      >
        {drag ? (
          <div className="bug-region" style={box(drag)} />
        ) : (
          <div className="bug-shade" />
        )}
      </div>
      {!drag && <div className="bug-hint">Drag to select</div>}
      <Tools onClose={onClose} onWhole={() => onChoose(whole())} />
    </div>
  );
}

function Describe({
  frame,
  region,
  saving,
  error,
  onSubmit,
}: {
  frame?: Shot;
  region: Region;
  saving: boolean;
  error?: string;
  /** With the description, and whether it reports a play of the AI's. */
  onSubmit: (description: string, ai: boolean) => void;
}) {
  const [text, setText] = useState("");
  const [ai, setAi] = useState(false);
  const ready = text.trim().length > 0 && !saving;
  return (
    <form
      className="bug-panel"
      role="dialog"
      aria-label="Report bug"
      onSubmit={(e) => {
        e.preventDefault();
        if (ready) onSubmit(text, ai);
      }}
    >
      <h2 className="bug-title">Report bug</h2>
      {frame && <Thumb frame={frame} region={region} />}
      <textarea
        className="bug-text"
        autoFocus
        rows={5}
        placeholder="What went wrong?"
        value={text}
        onChange={(e) => setText(e.target.value)}
      />
      {error && <p className="bug-error">{error}</p>}
      <div className="bug-actions">
        {/* A questionable play of the AI's, for incident analysis: the report carries the
            agent's latest searches. */}
        <label className="bug-ai">
          <input
            type="checkbox"
            checked={ai}
            onChange={(e) => setAi(e.target.checked)}
          />
          AI play
        </label>
        <Button type="submit" size="small" disabled={!ready}>
          {saving ? "Saving…" : "Submit"}
        </Button>
      </div>
    </form>
  );
}

/** The region, cut from the frozen frame by the page's own drawing of it: no pixels made. */
function Thumb({ frame, region }: { frame: Shot; region: Region }) {
  const k = Math.min(1, 480 / region.width, 150 / region.height);
  return (
    <div
      className="bug-crop"
      style={{
        width: region.width * k,
        height: region.height * k,
        backgroundImage: `url("${frame.url}")`,
        backgroundSize: `${frame.pageWidth * k}px auto`,
        backgroundPosition: `${-region.x * k}px ${-region.y * k}px`,
      }}
    />
  );
}

/** The overlay's buttons at the top right, on a dark pill so they read over any frame: the
 * whole window, while selecting, and the X. */
function Tools({
  onClose,
  onWhole,
}: {
  onClose: () => void;
  onWhole?: () => void;
}) {
  return (
    <div className="bug-tools">
      {onWhole && (
        <button
          type="button"
          className="bug-tool"
          aria-label="Whole window"
          title="Whole window (Space)"
          onClick={onWhole}
        >
          <svg viewBox="0 0 24 24" role="img" aria-hidden className="bug-whole">
            <path d="M4 9V4h5M15 4h5v5M20 15v5h-5M9 20H4v-5" />
          </svg>
        </button>
      )}
      <button
        type="button"
        className="bug-tool"
        aria-label="Close"
        title="Close (Esc)"
        onClick={onClose}
      >
        <svg viewBox="0 0 24 24" role="img" aria-hidden className="bug-x">
          <path d="M5.6 3.5 12 9.9l6.4-6.4 2.1 2.1-6.4 6.4 6.4 6.4-2.1 2.1-6.4-6.4-6.4 6.4-2.1-2.1 6.4-6.4-6.4-6.4z" />
        </svg>
      </button>
    </div>
  );
}

export function BugIcon() {
  return (
    <svg className="bug-icon" viewBox="0 0 24 24" role="img" aria-hidden>
      <path d="M9 5.5 7.5 3.5M15 5.5l1.5-2" />
      <path d="M12 7.5c-2.5 0-4.5 2.2-4.5 5.5S9.5 20 12 20s4.5-3.7 4.5-7-2-5.5-4.5-5.5Z" />
      <path d="M9.2 7.4a2.8 2.8 0 0 1 5.6 0" />
      <path d="M12 10v10M7.5 12H4M16.5 12H20M7.8 16.5 5 18.5M16.2 16.5l2.8 2M8 9.5 5.5 7.5M16 9.5l2.5-2" />
    </svg>
  );
}
