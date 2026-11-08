// Freezing the page for a bug report (components/BugReport.tsx): every motion stops where it
// is, so the frame the player reports is the one they saw, an animation caught mid-flight or a
// hover held, and it all plays on from there when they are done.
//
// Motion has three clocks, and freezing stops each:
// - requestAnimationFrame, which Motion's frame loop, the physics and the arrows run on: while
//   frozen, callbacks wait, and once thawed they run on a clock that skipped the frozen time;
// - performance.now, which Motion reads between frames: it stands still while frozen and then
//   runs on behind real time by the time spent frozen;
// - the document's timeline, which CSS animations and transitions and Motion's accelerated
//   animations run on: each is paused, including any that start while frozen, and resumed.
//
// The clocks are wrapped when this module loads, before Motion takes its own references to
// them, so main.tsx imports it first. Game views that arrive while frozen wait too (ipc.ts),
// and so does the playback's pacing (game/playback.ts).

const realFrame = window.requestAnimationFrame.bind(window);
const realCancel = window.cancelAnimationFrame.bind(window);
const realNow = performance.now.bind(performance);

/** Milliseconds spent frozen before the current freeze. */
let skipped = 0;
/** When the current freeze began, on the real clock; none while thawed. */
let frozenAt: number | undefined;
/** Callbacks requested while frozen, by the ids handed out for them. */
const waiting = new Map<number, FrameRequestCallback>();
let nextId = -1;
const paused = new Set<Animation>();
let sweep: number | undefined;
const listeners = new Set<() => void>();
const thawed: (() => void)[] = [];

/** The page's clock: real time less the time spent frozen. */
const now = () => (frozenAt ?? realNow()) - skipped;

performance.now = now;
window.requestAnimationFrame = (callback) => {
  if (frozenAt !== undefined) {
    const id = nextId--;
    waiting.set(id, callback);
    return id;
  }
  // A frame asked for before the freeze that comes after it waits too.
  return realFrame((t) => {
    if (frozenAt === undefined) callback(t - skipped);
    else waiting.set(nextId--, callback);
  });
};
window.cancelAnimationFrame = (id) => {
  if (!waiting.delete(id)) realCancel(id);
};

/** Pauses the page's running animations, but not the reporter's own. */
const pauseAnimations = () => {
  for (const a of document.getAnimations()) {
    const target = (a.effect as KeyframeEffect | null)?.target;
    if (a.playState === "running" && !target?.closest("[data-unfrozen]")) {
      a.pause();
      paused.add(a);
    }
  }
};

/** Stops every motion on the page where it is. */
export function freeze() {
  if (frozenAt !== undefined) return;
  frozenAt = realNow();
  document.documentElement.classList.add("page-frozen");
  pauseAnimations();
  // Animations that start while frozen (a view that was already being drawn) stop too.
  sweep = window.setInterval(pauseAnimations, 50);
  for (const l of listeners) l();
}

/** Plays every motion on from where it stopped. */
export function thaw() {
  if (frozenAt === undefined) return;
  skipped += realNow() - frozenAt;
  frozenAt = undefined;
  document.documentElement.classList.remove("page-frozen");
  window.clearInterval(sweep);
  for (const a of paused) {
    if (a.playState === "paused") a.play();
  }
  paused.clear();
  const callbacks = [...waiting.values()];
  waiting.clear();
  realFrame((t) => {
    for (const callback of callbacks) callback(t - skipped);
  });
  for (const run of thawed.splice(0)) run();
  for (const l of listeners) l();
}

export const isFrozen = () => frozenAt !== undefined;

/** Runs `f` now, or once thawed if frozen. */
export function whenThawed(f: () => void) {
  if (frozenAt === undefined) f();
  else thawed.push(f);
}

/** For useSyncExternalStore: calls `onChange` as the page freezes and thaws. */
export function subscribeFrozen(onChange: () => void) {
  listeners.add(onChange);
  return () => void listeners.delete(onChange);
}
