// What the stage draws as light (UI Conventions, Verbs, point; "Combat marks"): an arrow at a
// target, the line from an ability to its source, a line of combat, the wedge an attacker
// wears and the crescent under a blocker. This is the model of it: everything lit at one stage
// time, as ribbons along curves, shapes of light and sparks, each a function of that time and
// of where the stage has the two ends alone, so a scene is the same however its time was
// reached and a test can step it. Nothing here reads the page or draws: glow.ts paints a
// scene, and the surface that has no painter draws the lines as paths instead (dom.ts).
//
// How each looks is Beryl's own. An arrow strikes from its spell to its target, lands with a
// flash and sparks, and light runs along it toward the target while it stands; as the step
// that ends it plays it drains into the target. Arena's appears whole in a frame and is cut
// (`I3Te0m1bBFw 31:35.05`, `31:36.65`): what Beryl keeps of that is when each begins and ends.
// An attacker's wedge pops out of its leading edge and settles, as Arena's is a frame wide and
// bright before it settles (`IEXcjBn3BXk 22:16.53`), and turns gold once it is blocked, as
// Arena's does (`IEXcjBn3BXk 22:22.3`).
import type { Arrow } from "../generated/protocol";
import type { Point } from "./layout.ts";
import { wayOf, type Reach } from "./lines.ts";

/** A curve from its first point round its second to its third. */
export type Curve = readonly [Point, Point, Point];

/** What a light says by its color (UI Conventions, exception 4): green at what its controller
 * controls and red at what they do not; ember an attack, gold a block; pale what only shows
 * where something came from. */
export type Tone =
  | "friendly"
  | "hostile"
  | "ember"
  | "gold"
  | "pale"
  | "white"
  | "blue"
  | "black"
  | "red"
  | "green";

type Rgb = readonly [number, number, number];

/** Each tone's color and the color of its hot core. */
export const TONES: Record<Tone, { glow: Rgb; core: Rgb }> = {
  friendly: { glow: [0.08, 0.92, 0.5], core: [0.86, 1, 0.9] },
  hostile: { glow: [1, 0.17, 0.1], core: [1, 0.86, 0.7] },
  ember: { glow: [1, 0.15, 0.04], core: [1, 0.8, 0.5] },
  gold: { glow: [1, 0.66, 0.1], core: [1, 0.97, 0.8] },
  pale: { glow: [0.86, 0.72, 0.46], core: [1, 0.95, 0.85] },
  // The colors of mana (flare.ts), and of the white light a card lands in.
  white: { glow: [1, 0.9, 0.62], core: [1, 1, 0.96] },
  blue: { glow: [0.25, 0.66, 0.96], core: [0.89, 0.96, 1] },
  black: { glow: [0.6, 0.36, 0.84], core: [0.94, 0.89, 1] },
  red: { glow: [0.95, 0.33, 0.17], core: [1, 0.91, 0.86] },
  green: { glow: [0.25, 0.75, 0.31], core: [0.91, 1, 0.9] },
};

/** How a ribbon is cut along its length: an arrow is a thread at its tail that swells to a
 * spear head; a beam narrows to a dart; a thread is one width with soft ends; a swell is a
 * hairline at its start that is widest a little past half way and fades out before its end. */
export type Cut = "arrow" | "beam" | "thread" | "swell";

/** A ribbon of light along `curve`, from `from` to `to` of the way along it. */
export interface Ribbon {
  curve: Curve;
  from: number;
  to: number;
  cut: Cut;
  /** Its half-width at its widest, in stage pixels. */
  wide: number;
  tone: Tone;
  gain: number;
  /** What sets its streaks apart from another ribbon's. */
  seed: number;
}

/** A shape of light at `at`, its own upward way `up` on the stage: a wedge with its base at
 * `at`, `size.x` to each side and `size.y` tall; a crescent of that half-size about `at`,
 * open toward `up`; a flash `size.x` across with a ring `ring` of the way out; a burst, a
 * bloom with thin rays out to `size.x` and such a ring, its rays set by `seed`; a beam, a
 * cone of light shot from `at` toward `up`, `size.x` to each side at its base and `size.y`
 * long, brightest at its base and along its middle. */
export interface Glint {
  kind: "wedge" | "crescent" | "flash" | "burst" | "beam";
  at: Point;
  up: Point;
  size: Point;
  tone: Tone;
  /** How far its color has turned to gold, from 0 to 1. */
  warm: number;
  gain: number;
  ring: number;
  seed?: number;
}

/** What one layer of light holds at stage time `now`. `dots` are sparks, six numbers each:
 * where, how wide, and its color. */
export interface Scene {
  now: number;
  ribbons: Ribbon[];
  glints: Glint[];
  dots: number[];
}

/** Everything lit at a stage time, in the two layers it is painted in. A line of combat and
 * the marks of combat lie on the table, `under` the cards and the step's banner: one passes
 * behind a creature that stands in its way, and is never read as that creature's. An arrow at
 * a target and the line to a source are `over` everything at rest and under what flies: an
 * arrow's head lies on the face of what it is aimed at. */
export interface Lit {
  under: Scene;
  over: Scene;
}

/** The layer a line is painted in. */
export const layerOf = (arrow: Arrow): keyof Lit =>
  arrow === "attack" || arrow === "block" ? "under" : "over";

/** A line the stage holds, as the light is drawn from it: the whole way it runs (lines.ts
 * `wayOf`), how its ends lie, how far it is drawn in and how far drained, and the stage time
 * from which it has been whole. */
export interface LitLine {
  key: string;
  arrow: Arrow;
  way: Curve;
  from: Reach;
  to: Reach;
  shown: number;
  left: number;
  whole: number;
}

/** A creature in combat as the light is drawn from it: how it lies, the stage time since
 * which it has fought, and for an attacker that is blocked, since when. */
export interface Fighter {
  reach: Reach;
  fights: "attacks" | "blocks";
  since: number;
  blocked?: number;
}

const clamp = (x: number, low = 0, high = 1) =>
  Math.min(high, Math.max(low, x));
const mix = (a: number, b: number, t: number) => a + (b - a) * t;
const smooth = (a: number, b: number, x: number) => {
  const t = clamp((x - a) / (b - a));
  return t * t * (3 - 2 * t);
};
const outCubic = (t: number) => 1 - (1 - clamp(t)) ** 3;
/** A number from 0 to 1 that is always the same for the same two. */
const hash = (i: number, k: number) => {
  const s = Math.sin(i * 127.1 + k * 311.7) * 43758.5453;
  return s - Math.floor(s);
};
/** A number from 0 to 1 for a line's name. */
const seedOf = (key: string) => {
  let sum = 0;
  for (let i = 0; i < key.length; i++)
    sum = (sum * 31 + key.charCodeAt(i)) % 9973;
  return sum / 9973;
};

/** The point `t` of the way along `curve`. */
export const along = ([a, c, b]: Curve, t: number): Point => {
  const [p, q] = [
    { x: mix(a.x, c.x, t), y: mix(a.y, c.y, t) },
    { x: mix(c.x, b.x, t), y: mix(c.y, b.y, t) },
  ];
  return { x: mix(p.x, q.x, t), y: mix(p.y, q.y, t) };
};

/** A ribbon's half-width where it has come `arc` of the `whole` stage pixels it is long. */
export function widthOf(cut: Cut, wide: number, arc: number, whole: number) {
  const [s, left] = [arc / (whole || 1), whole - arc];
  if (cut === "thread") {
    return wide * Math.sqrt(clamp(Math.min(arc, left) / 18));
  }
  if (cut === "swell") {
    const { peak, least, start, end } = SWELL;
    const bulge = Math.sin(Math.PI * s ** (Math.log(0.5) / Math.log(peak)));
    const ends = clamp(s / start) * clamp((1 - s) / end) ** 1.5;
    return wide * (least + (1 - least) * bulge) * ends;
  }
  if (cut === "beam") {
    // A dart 17 pixels long at a card's own size, wider than the beam it ends.
    const [head, neck] = [17 * wide, 19 * wide];
    const beam = mix(5.4, 2.3, s) * wide;
    if (left < head) return (7.6 * wide * left) / head;
    return left < neck
      ? mix(7.6 * wide, beam, (left - head) / (neck - head))
      : beam;
  }
  const tail = 0.13 + 0.87 * smooth(0, 0.92, s) ** 1.25;
  const flare = 1 + 0.5 * Math.exp(-(((left - 38) / 15) ** 2));
  return wide * tail * flare * clamp(left / 36) ** 0.72;
}

/** The width of a swell along its length: widest `peak` of the way along, `least` of that
 * at its ends' full width, coming in over the first `start` of the way and fading out over
 * the last `end` of it. Arena's line from an ability to its source is a hairline where it
 * leaves the source, widest where it hangs lowest and gone before it reaches the stack card
 * (`2VW6ZzmS8U4 32:16.6`, `0VsYGs88NXA 19:39.85`). */
export const SWELL = { peak: 0.6, least: 0.25, start: 0.06, end: 0.2 };

/** How large the light on something is: 1 on a card as wide as the design's (110 stage
 * pixels), by the shorter side of what lies there, turned or not. */
const sized = ({ half }: Reach) =>
  clamp(Math.min(half.x, half.y) / 55, 0.7, 1.4);

/** Sparks thrown from `at`, `age` ms ago: each is where it is by its age alone. */
function burst(
  dots: number[],
  at: Point,
  age: number,
  tone: Tone,
  count: number,
  seed: number,
  reach: number,
) {
  if (!(age >= 0 && age <= 700)) return;
  const { glow, core } = TONES[tone];
  for (let i = 0; i < count; i++) {
    const fade = 1 - age / (320 + 340 * hash(i, seed));
    if (fade <= 0) continue;
    const angle = hash(i, seed + 1) * 2 * Math.PI;
    const far =
      (26 + 62 * hash(i, seed + 2)) * reach * (1 - Math.exp(-age / 170));
    const [r, g, b] = i % 3 ? core : glow;
    const lit = fade ** 1.4 * 1.3;
    dots.push(
      at.x + Math.cos(angle) * far,
      at.y + Math.sin(angle) * far + age * age * 0.00006 * reach,
      (5 + 7 * hash(i, seed + 3)) * reach,
      r * lit,
      g * lit,
      b * lit,
    );
  }
}

/** Motes that ride an arrow toward its head, and embers that lift off where it lands: where
 * each is, is the stage time's alone. */
function motes(
  dots: number[],
  curve: Curve,
  now: number,
  tone: Tone,
  wide: number,
  gain: number,
  seed: number,
) {
  const { glow, core } = TONES[tone];
  const k = wide / 7.2;
  for (let i = 0; i < 7; i++) {
    const turn = now * 0.00062 * (0.8 + 0.5 * hash(i, 5)) + hash(i, 9) + seed;
    const u = turn - Math.floor(turn);
    const [p, q] = [along(curve, u), along(curve, Math.min(u + 0.01, 1))];
    const [x, y] = [q.x - p.x, q.y - p.y];
    const long = Math.hypot(x, y) || 1;
    const off =
      (hash(i, 13) - 0.5) * 2.2 * widthOf("arrow", wide, u * 600, 600);
    const lit = Math.sin(Math.PI * u) * 0.95 * gain;
    dots.push(
      p.x + (y / long) * off,
      p.y - (x / long) * off,
      (4 + 4 * hash(i, 17)) * k,
      core[0] * lit,
      core[1] * lit,
      core[2] * lit,
    );
  }
  const tip = curve[2];
  for (let i = 0; i < 5; i++) {
    const cycle = 900 + 500 * hash(i, 21);
    const turn = now / cycle + hash(i, 23) + seed;
    const [a, lap] = [turn - Math.floor(turn), Math.floor(turn)];
    const angle = hash(i, 29 + (lap % 97)) * 2 * Math.PI;
    const lit = (1 - a) * 0.9 * gain;
    dots.push(
      tip.x + Math.cos(angle) * 30 * a * k,
      tip.y + (Math.sin(angle) * 30 - 14) * a * k,
      (3 + 4 * hash(i, 31)) * k,
      glow[0] * lit,
      glow[1] * lit,
      glow[2] * lit,
    );
  }
}

const UP: Point = { x: 0, y: 1 };
const flash = (
  at: Point,
  size: number,
  tone: Tone,
  gain: number,
  ring: number,
): Glint => ({
  kind: "flash",
  at,
  up: UP,
  size: { x: size, y: size },
  tone,
  warm: 0,
  gain,
  ring: clamp(ring),
});

/** The light of one line at stage time `now`. */
function lineLight(scene: Scene, line: LitLine, now: number) {
  const { arrow, way, from, to, shown, left, whole } = line;
  const seed = seedOf(line.key);
  const k = Math.min(sized(from), sized(to));
  // How fresh its landing is: 1 as it lands, gone in a few tenths of a second.
  const hit = now >= whole ? Math.exp(-(now - whole) / 150) : 0;
  if (arrow === "source") {
    // It grows out of the source, which is where it ends.
    const back: Curve = [way[2], way[1], way[0]];
    // Arena's is a hairline at the source, about 8 pixels across a 1080 table where it
    // hangs lowest, with a white core, and fades out short of the stack card.
    scene.ribbons.push({
      curve: back,
      from: left,
      to: shown,
      cut: "swell",
      wide: 2.8 * k,
      tone: "pale",
      gain: 1.15,
      seed,
    });
    return;
  }
  if (arrow === "attack" || arrow === "block") {
    const tone = arrow === "attack" ? "ember" : "gold";
    // A blocker's line meets the tip of the wedge its attacker wears, where that faces it.
    const faces = Math.sign(way[0].y - way[2].y) === to.toward;
    const tip =
      arrow === "block" && faces
        ? { x: way[2].x, y: way[2].y + to.toward * to.ahead }
        : way[2];
    const curve: Curve = [
      way[0],
      { x: (way[0].x + tip.x) / 2, y: (way[0].y + tip.y) / 2 },
      tip,
    ];
    scene.ribbons.push({
      curve,
      from: left,
      to: shown,
      cut: "beam",
      wide: (arrow === "attack" ? 0.55 : 1) * k,
      tone,
      gain: (arrow === "attack" ? 0.75 : 1) + 0.9 * hit + 0.4 * left,
      seed,
    });
    scene.glints.push(flash(tip, 34 * k, tone, hit * 1.1, (now - whole) / 260));
    burst(scene.dots, tip, now - whole, tone, 8, seed * 89, 0.6 * k);
    return;
  }
  const tone = arrow === "friendly-target" ? "friendly" : "hostile";
  const wide = 7.2 * k;
  scene.ribbons.push({
    curve: way,
    from: left,
    to: shown,
    cut: "arrow",
    wide: wide * (1 + 0.25 * hit),
    tone,
    gain: 1.05 + 0.95 * hit + 0.5 * left,
    seed,
  });
  if (now < whole || left >= 1) return;
  const tip = way[2];
  motes(scene.dots, way, now, tone, wide, 1 - left, seed);
  scene.glints.push(
    flash(tip, 50 * k, tone, hit * 0.9, (now - whole) / 300),
    // Where it rests on its target stays lit, and flickers.
    flash(
      tip,
      22 * k,
      tone,
      (0.42 + 0.1 * Math.sin(now * 0.0053)) * (1 - left),
      1,
    ),
  );
  burst(scene.dots, tip, now - whole, tone, 16, seed * 97, k);
}

/** The light of one creature in combat at stage time `now`. */
function fighterLight(scene: Scene, fighter: Fighter, now: number) {
  const { reach, fights, since, blocked } = fighter;
  if (reach.toward === 0) return;
  const k = sized(reach);
  const age = now - since;
  const edge = {
    x: reach.middle.x,
    y: reach.middle.y + reach.toward * reach.half.y,
  };
  const up = { x: 0, y: reach.toward };
  if (fights === "blocks") {
    // Arena's is a dome of gold light over the blocker's edge that faces its attacker: a
    // card's width and a little more across, about a fifth of a width tall, its chord about
    // a twelfth of a width clear of the edge, a bright rim along its arc and light that fades
    // back toward the blocker (`IEXcjBn3BXk 22:23.0`). It is there almost whole as the
    // blocker is declared (`22:21.10`).
    const open = mix(0.85, 1, outCubic(age / 120));
    scene.glints.push({
      kind: "crescent",
      at: { x: edge.x, y: edge.y + reach.toward * 0.16 * reach.half.x },
      up,
      size: {
        x: reach.half.x * 1.1 * open,
        y: reach.half.x * 0.36 * open,
      },
      tone: "gold",
      warm: 1,
      gain: clamp(age / 40) * (0.6 + 0.95 * Math.exp(-age / 210)),
      ring: 0,
    });
    return;
  }
  // A layout whose attackers wear no mark has no room ahead of them for one.
  if (reach.ahead <= 0) return;
  const met = blocked === undefined ? Infinity : now - blocked;
  const breath = 0.93 + 0.07 * Math.sin(now * 0.0037 + reach.middle.x);
  // Arena's chevron is as wide as its card, against its edge. It is there whole in the frame
  // the attack is declared, a little wide and bright, and has settled two or three frames
  // later (`IEXcjBn3BXk 22:16.53` to `22:16.63`).
  const pop = Math.exp(-age / 60);
  scene.glints.push({
    kind: "wedge",
    at: edge,
    up,
    size: {
      x: reach.half.x * (1 + 0.1 * pop),
      y: reach.ahead * (1 + 0.15 * pop),
    },
    tone: "ember",
    warm: blocked === undefined ? 0 : clamp(met / 160),
    gain: (1 + 1.3 * pop + 0.8 * Math.exp(-met / 150)) * breath,
    ring: 0,
  });
  const tip = { x: edge.x, y: edge.y + reach.toward * reach.ahead };
  burst(scene.dots, tip, age - 60, "ember", 7, reach.middle.x, 0.7 * k);
}

/** A layer with nothing lit. */
export const dark = (now: number): Scene => ({
  now,
  ribbons: [],
  glints: [],
  dots: [],
});

/** Everything lit at stage time `now`: the stage's lines and its creatures in combat. */
export function lightOf(
  now: number,
  lines: readonly LitLine[],
  fighters: readonly Fighter[],
): Lit {
  const lit: Lit = { under: dark(now), over: dark(now) };
  // The marks of combat lie under the lines that meet them.
  for (const fighter of fighters) fighterLight(lit.under, fighter, now);
  for (const line of lines) lineLight(lit[layerOf(line.arrow)], line, now);
  return lit;
}

/** The arrow a choice of targets pulls toward the pointer (Aim.tsx), over what rests, as it
 * is at stage time `now`: out of `from`, what asks, to the pointer `at`, thin and pale while
 * the pointer is on nothing that may be chosen. On what may be (`lock`, since the stage time
 * the pointer came onto it) it is the arrow the choice will draw, along that arrow's own way
 * and in its color, which it swings onto in a few hundredths of a second and lands on with a
 * flash. Nothing while the pointer is on what asks. The pointer itself is never trailed:
 * only the swing onto a target takes time. */
export function sought(
  now: number,
  from: Reach,
  at: Point,
  lock?: { to: Reach; friendly: boolean; since: number },
): Scene {
  const scene = dark(now);
  const [x, y] = [at.x - from.middle.x, at.y - from.middle.y];
  if (Math.abs(x) <= from.half.x && Math.abs(y) <= from.half.y) return scene;
  const pointer: Reach = {
    middle: at,
    half: { x: 1, y: 1 },
    round: true,
    toward: 0,
    ahead: 0,
  };
  const way = wayOf("hostile-target", from, pointer);
  // Its tip is the pointer's own place, not the edge of a point's reach.
  const loose: [Point, Point, Point] | null = way && [way[0], way[1], at];
  const k = sized(from);
  if (!lock) {
    if (!loose) return scene;
    scene.ribbons.push({
      curve: loose,
      from: 0,
      to: 1,
      cut: "arrow",
      wide: 3.6 * k,
      tone: "pale",
      gain: 0.7,
      seed: 0.5,
    });
    return scene;
  }
  const arrow = lock.friendly ? "friendly-target" : "hostile-target";
  const held = wayOf(arrow, from, lock.to);
  if (!held) return scene;
  const age = Math.max(0, now - lock.since);
  const [swung, snap] = [1 - Math.exp(-age / 30), Math.exp(-age / 150)];
  const curve = (loose ?? held).map((p, i) => ({
    x: mix(p.x, held[i]!.x, swung),
    y: mix(p.y, held[i]!.y, swung),
  })) as [Point, Point, Point];
  const tone = lock.friendly ? "friendly" : "hostile";
  const wide = 7.2 * Math.min(k, sized(lock.to));
  scene.ribbons.push({
    curve,
    from: 0,
    to: 1,
    cut: "arrow",
    wide: wide * mix(0.5, 1, swung) * (1 + 0.25 * snap),
    tone,
    gain: 1.05 + 0.9 * snap,
    seed: 0.5,
  });
  motes(scene.dots, curve, now, tone, wide, swung, 0.5);
  scene.glints.push(
    flash(curve[2], (50 * wide) / 7.2, tone, snap * 0.9, age / 300),
    flash(curve[2], (22 * wide) / 7.2, tone, 0.42 * swung, 1),
  );
  burst(scene.dots, curve[2], age, tone, 10, 61, wide / 7.2);
  return scene;
}

/** The trace a declaration pulls toward the pointer (Aim.tsx): a beam from each of `from`
 * to `at` in the color of the line the aim will draw, thin until the click that aims it
 * draws the stage's own. */
export const aimed = (
  from: readonly Point[],
  at: Point,
  arrow: Arrow,
): Ribbon[] =>
  from.map((start) => ({
    curve: [start, { x: (start.x + at.x) / 2, y: (start.y + at.y) / 2 }, at],
    from: 0,
    to: 1,
    cut: "beam",
    wide: 0.6,
    tone: arrow === "attack" ? "ember" : arrow === "block" ? "gold" : "pale",
    gain: 0.85,
    seed: 0.5,
  }));
