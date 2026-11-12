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
// and the stage's clock stops with the frames it is stepped on (stage/clock.ts).

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
// Keep one public id across scheduling and freezing. An effect can cancel its frame
// after the browser delivered it into the frozen queue; changing that id would revive
// a disposed animation loop on thaw.
const scheduled = new Map<number, number>();
window.requestAnimationFrame = (callback) => {
  const id = nextId--;
  if (frozenAt !== undefined) {
    waiting.set(id, callback);
    return id;
  }
  scheduled.set(
    id,
    realFrame((t) => {
      scheduled.delete(id);
      if (frozenAt === undefined) callback(t - skipped);
      else waiting.set(id, callback);
    }),
  );
  return id;
};
window.cancelAnimationFrame = (id) => {
  waiting.delete(id);
  const handle = scheduled.get(id);
  if (handle !== undefined) {
    scheduled.delete(id);
    realCancel(handle);
  }
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
  const callbacks = [...waiting.keys()];
  realFrame((t) => {
    if (frozenAt !== undefined) return;
    for (const id of callbacks) {
      const callback = waiting.get(id);
      waiting.delete(id);
      callback?.(t - skipped);
    }
  });
  for (const run of thawed.splice(0)) run();
  for (const l of listeners) l();
}

/** Runs `f` now, or once thawed if frozen. */
export function whenThawed(f: () => void) {
  if (frozenAt === undefined) f();
  else thawed.push(f);
}
