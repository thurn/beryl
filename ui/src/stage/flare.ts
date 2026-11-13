// The light of what a card pays and of what it lands with (UI Conventions, Verbs, streak;
// Effects, settle and flash), as the stage's painter draws it over the table (light.ts,
// glow.ts): a ribbon of mana out of each land into the spell it pays for, the spell's flare
// as the mana lands, and the burst of light where a permanent comes to rest. Each is a
// function of stage time and of where the model has its ends, as all of the stage's light is.
//
// These follow Arena's own, filmed whole (`UrGalw_f35w 17:00.30` and `17:01.63`). Mana curls
// out of each land that made it as a ribbon of its color, the land glowing where it left, and
// the ribbons run together into the spell, which flares white with rays for about a third
// of a second. A creature that lands is whole for two frames, then whitens, bursts in a
// round bloom with thin rays about one and a half cards wide 0.1 s later, throws a soft ring
// to twice its width, and stands washed white until about 0.3 s, flecks drifting off it.
import type { Point } from "./layout.ts";
import type { Curve, Glint, Lit, Scene, Tone } from "./light.ts";
import type { Reach } from "./lines.ts";

/** A streak under way, as its light is drawn: along `curve` from `back` to `through` of the
 * way, its head `wide` stage pixels across at half its width, made of `look` (a color of
 * mana, or `damage`). */
export interface LitStreak {
  key: string;
  look: string;
  curve: Curve;
  back: number;
  through: number;
  wide: number;
}

/** An effect under way that is drawn as light: which, on what, and how long ago in stage
 * milliseconds it began, of the `span` it lasts. */
export interface LitFlare {
  name: string;
  reach: Reach;
  age: number;
  span: number;
}

/** The effects that are light: a permanent landing, and mana landing on what it pays for. */
export const FLARES: ReadonlySet<string> = new Set(["settle", "flash"]);

const TONE: Record<string, Tone> = {
  white: "white",
  blue: "blue",
  black: "black",
  red: "red",
  green: "green",
  damage: "ember",
};

const clamp = (x: number) => Math.min(1, Math.max(0, x));
const hash = (i: number, k: number) => {
  const s = Math.sin(i * 127.1 + k * 311.7) * 43758.5453;
  return s - Math.floor(s);
};
const seedOf = (key: string) => {
  let sum = 0;
  for (let i = 0; i < key.length; i++)
    sum = (sum * 31 + key.charCodeAt(i)) % 9973;
  return sum / 9973;
};
const at = ([a, c, b]: Curve, t: number): Point => ({
  x: (1 - t) * (1 - t) * a.x + 2 * (1 - t) * t * c.x + t * t * b.x,
  y: (1 - t) * (1 - t) * a.y + 2 * (1 - t) * t * c.y + t * t * b.y,
});
const UP: Point = { x: 0, y: 1 };

const glint = (
  kind: Glint["kind"],
  middle: Point,
  size: number,
  tone: Tone,
  gain: number,
  ring: number,
  seed: number,
): Glint => ({
  kind,
  at: middle,
  up: UP,
  size: { x: size, y: size },
  tone,
  warm: 0,
  gain,
  ring: clamp(ring),
  seed,
});

/** How a streak's light is drawn: its ribbon `ribbon` times as wide as its head, an orb where
 * it left `orb` times as wide, and `motes` flecks strung out behind its head. */
export const STREAK = { ribbon: 1.5, orb: 7, motes: 5 };

/** The light of one streak: the ribbon from its tail to its head, the glow of what it left,
 * which fades as the head goes, a hot point at its head and flecks behind it. */
function streakLight(scene: Scene, streak: LitStreak) {
  const { curve, back, through, wide, look } = streak;
  const tone = TONE[look] ?? "pale";
  const seed = seedOf(streak.key);
  scene.ribbons.push({
    curve,
    from: back,
    to: through,
    cut: "arrow",
    wide: wide * STREAK.ribbon,
    tone,
    gain: 1.25,
    seed,
  });
  scene.glints.push(
    glint(
      "flash",
      curve[0],
      wide * STREAK.orb,
      tone,
      1.1 * (1 - through) ** 0.7,
      through,
      seed,
    ),
  );
  const head = at(curve, through);
  scene.dots.push(head.x, head.y, wide * 5, 1.3, 1.3, 1.3);
  for (let i = 0; i < STREAK.motes; i++) {
    const behind = through - (through - back) * hash(i, seed * 97 + 1);
    const p = at(curve, behind);
    const lit = 0.9 * (1 - (through - behind) / (through - back + 1e-6));
    scene.dots.push(
      p.x + (hash(i, seed * 97 + 2) - 0.5) * wide * 4,
      p.y + (hash(i, seed * 97 + 3) - 0.5) * wide * 4,
      wide * (1.2 + 1.6 * hash(i, seed * 97 + 4)),
      lit,
      lit,
      lit,
    );
  }
}

/** How the burst of a permanent landing plays, as shares of its time (Arena's, of about
 * 0.47 s from the frame the card comes to rest): nothing until `from`, brightest at `peak`,
 * its bloom gone by `bloomed`, its ring thrown from `ringFrom` to `ringTo`, and `flecks`
 * flecks thrown as it is brightest. Its bloom is `wide` of the card's half-width across at
 * its widest. */
export const SETTLE = {
  from: 0.15,
  peak: 0.36,
  bloomed: 0.66,
  ringFrom: 0.32,
  ringTo: 0.66,
  wide: 2.4,
  flecks: 12,
};

/** Flecks thrown from `middle` `age` ms ago, to `far` stage pixels: each is where it is by
 * its age alone, and drifts up the stage as it fades. */
function flecks(
  dots: number[],
  middle: Point,
  age: number,
  count: number,
  far: number,
  seed: number,
) {
  if (age < 0) return;
  for (let i = 0; i < count; i++) {
    const fade = 1 - age / (260 + 240 * hash(i, seed));
    if (fade <= 0) continue;
    const angle = hash(i, seed + 1) * 2 * Math.PI;
    const out =
      far * (0.5 + 0.7 * hash(i, seed + 2)) * (1 - Math.exp(-age / 150));
    const lit = fade ** 1.3 * 1.2;
    dots.push(
      middle.x + Math.cos(angle) * out,
      middle.y + Math.sin(angle) * out - age * 0.02,
      far * (0.05 + 0.06 * hash(i, seed + 3)),
      lit,
      lit,
      lit * 0.92,
    );
  }
}

/** The light of one effect: a burst with rays about what it is on. A permanent landing
 * swells to its brightest, throws a ring and flecks, and is gone; mana landing on a spell is
 * brightest at once and spreads as it fades. */
function flareLight(scene: Scene, { name, reach, age, span }: LitFlare) {
  const share = clamp(age / (span || 1));
  const half = Math.max(reach.half.x, reach.half.y);
  const seed = hash(Math.round(reach.middle.x), Math.round(reach.middle.y));
  if (name === "settle") {
    const { from, peak, bloomed, ringFrom, ringTo, wide } = SETTLE;
    const risen = clamp((share - from) / (peak - from));
    const gain =
      share < peak ? risen : clamp(1 - (share - peak) / (bloomed - peak));
    scene.glints.push(
      glint(
        "burst",
        reach.middle,
        half * wide * (0.55 + 0.45 * risen),
        "white",
        1.9 * gain,
        (share - ringFrom) / (ringTo - ringFrom),
        seed,
      ),
    );
    flecks(
      scene.dots,
      reach.middle,
      age - span * peak,
      SETTLE.flecks,
      half * 1.9,
      seed * 53,
    );
    return;
  }
  scene.glints.push(
    glint(
      "burst",
      reach.middle,
      half * (2.6 + 1.4 * share),
      "white",
      1.7 * (1 - share) ** 1.4,
      0,
      seed,
    ),
  );
}

/** Adds to `lit`, over everything at rest, the light of the `streaks` and `flares` under
 * way. */
export function flaresOf(
  lit: Lit,
  streaks: readonly LitStreak[],
  flares: readonly LitFlare[],
): Lit {
  for (const flare of flares) flareLight(lit.over, flare);
  for (const streak of streaks) streakLight(lit.over, streak);
  return lit;
}
