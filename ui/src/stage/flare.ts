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
  /** Where what it is on was `TRAIL.lag` ago, for a trail. */
  was?: Reach;
}

/** The effects that are light: a permanent landing, mana landing on what it pays for, a
 * triggered ability coming to rest on the stack, and a blow landing. */
export const FLARES: ReadonlySet<string> = new Set([
  "settle",
  "flash",
  "beam",
  "impact",
  "trail",
]);

/** How the trail behind a dashing creature is drawn (Arena's, `IEXcjBn3BXk 35:35.30`, a
 * streak of white light behind the card, widest at it): back to where the card was `lag`
 * stage ms ago, `wide` of its longer half across at the card. */
export const TRAIL = { lag: 70, wide: 0.55 };

/** How a blow's burst plays (Arena's, filmed whole at 30 fps, `IEXcjBn3BXk 22:25.37` on a
 * blocker and `35:35.37` on a player): white-hot and about one and a half of the struck
 * card's widths across in the frame it lands, with rays, spreading a little and gone in about
 * a third of a second, a ring leaving it as it fades. `wide` is its width at once and
 * `spread` how much wider it grows, in the struck thing's half widths. */
const IMPACT = { wide: 1.5, spread: 0.5, fade: 1.6 };

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

/** How the beam of a triggered ability coming to rest plays (Arena's, filmed whole at 30 fps:
 * `2VW6ZzmS8U4 32:15.43`, `5UHzpVvUkfI 19:48.97`, `IEXcjBn3BXk 20:39.83`). A cone of white
 * and yellow light shoots from the card's side toward the middle of the table, out of a point
 * `high` of its half-height above its middle. It is `long` of the card's half-width at its
 * longest, which it reaches by `grown` of its time, and `wide` of its half-height to each side
 * at its base until `thick`; by `thin` it has narrowed to `streak` of that, a horizontal
 * streak that fades out. A bloom lies where it leaves, brightest at once, and `sparks` gold
 * sparks hang along the way behind it, drifting, and fade with it. */
export const BEAM = {
  high: 0.3,
  long: 3.6,
  grown: 0.25,
  wide: 0.55,
  thick: 0.3,
  thin: 0.6,
  streak: 0.2,
  sparks: 16,
};

/** The light of a triggered ability's beam, `share` of the way through its time, `age` ms
 * since it began. */
function beamLight(scene: Scene, reach: Reach, share: number, age: number) {
  const { high, long, grown, wide, thick, thin, streak, sparks } = BEAM;
  const { middle, half } = reach;
  // Toward the middle of the table: from the stack at its right edge, to the left.
  const at = { x: middle.x - half.x, y: middle.y - high * half.y };
  const grow = 1 - (1 - clamp(share / grown)) ** 3;
  const length = half.x * long * (0.25 + 0.75 * grow + 0.15 * share);
  const narrow = clamp((share - thick) / (thin - thick));
  const across = half.y * wide * (1 - (1 - streak) * narrow);
  const gain =
    1.8 * clamp(share / 0.1) * (1 - clamp((share - 0.3) / 0.7)) ** 1.5;
  scene.glints.push({
    kind: "beam",
    at,
    up: { x: -1, y: 0 },
    size: { x: across, y: length },
    tone: "white",
    warm: 0.35,
    gain,
    ring: 0,
    seed: hash(Math.round(at.x), Math.round(at.y)),
  });
  scene.glints.push(
    glint("flash", at, half.y * 1.1, "white", 1.6 * Math.exp(-age / 90), 0, 0),
  );
  const seed = seedOf(`${Math.round(middle.x)}:${Math.round(middle.y)}`);
  const [r, g, b] = [1.15, 0.78, 0.32];
  for (let i = 0; i < sparks; i++) {
    const fade = (1 - share) ** 1.2 * (0.5 + 0.5 * hash(i, seed * 7 + 1));
    const x = at.x - length * 1.3 * hash(i, seed * 7 + 2) - age * 0.02;
    const y = at.y + (hash(i, seed * 7 + 3) - 0.5) * half.y * 1.2 - age * 0.03;
    const size = half.x * (0.05 + 0.05 * hash(i, seed * 7 + 4));
    scene.dots.push(x, y, size, r * fade, g * fade, b * fade);
  }
}

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
function flareLight(scene: Scene, flare: LitFlare) {
  const { name, reach, age, span } = flare;
  const share = clamp(age / (span || 1));
  const half = Math.max(reach.half.x, reach.half.y);
  const seed = hash(Math.round(reach.middle.x), Math.round(reach.middle.y));
  if (name === "beam") {
    beamLight(scene, reach, share, age);
    return;
  }
  if (name === "trail") {
    const { was } = flare;
    if (!was) return;
    const [a, b] = [reach.middle, was.middle];
    if (Math.hypot(a.x - b.x, a.y - b.y) < half * 0.5) return;
    // From the card back along its way, narrowing to nothing where it was.
    scene.ribbons.push({
      curve: [a, { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }, b],
      from: 0,
      to: 1,
      cut: "beam",
      wide: half * TRAIL.wide,
      tone: "white",
      gain: 1.3 * (1 - share) ** 0.8,
      seed,
    });
    return;
  }
  if (name === "impact") {
    const { wide, spread, fade } = IMPACT;
    scene.glints.push(
      glint(
        "burst",
        reach.middle,
        half * (wide + spread * share),
        "white",
        2 * (1 - share) ** fade,
        share,
        seed,
      ),
    );
    return;
  }
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
