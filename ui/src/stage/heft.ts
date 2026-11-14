// The heft of a card the pointer picks up: it rises off the felt, leans into the way it is
// moved and casts a shadow on the felt that slides away from the light and softens as it
// rises, and when it is let go it settles back flat. Arena's dragged card leans so
// (`_IBqPx0gcag 4:10.02` to `4:10.18`, filmed at 30 frames a second: tipped toward where it is
// pulled while it moves, flat again within a few frames of stopping).
//
// This is the one motion on the stage that runs on the browser's frames and not on the stage
// clock: the held card follows the pointer, not a script (pointer.ts), so nothing here is part
// of the stage's model or of any pose. A damped integrator (`Heft`) advances only while the
// card is held or has yet to settle, and the surface (dom.ts) writes what it gives as the
// `transform` of what is inside the card's node and of a shadow element under its face, and
// the shadow's `opacity`. Both have layers of their own, so a frame repaints nothing, and a
// card at rest runs no frame at all.
//
// Units are stage pixels and seconds. A battlefield card is 128 px wide for its 63 mm, so
// about 2 px a millimeter.

/** How far a card's shadow falls from beneath it on the table, per pixel of height: the light
 * hangs above the far left of the table, so shadows fall toward you and a little right. */
export const LIGHT = { x: 0.14, y: 0.5 };
/** How high a card is held while dragged: high enough that, nearer the camera
 * (`DISTANCE`), it is as large as a raised hand card (stage.css, `scale: 1.06`). */
export const HELD_HEIGHT = 80;
/** The camera's distance from the felt, which sets how much a card grows as it rises and how
 * deep its lean looks. */
const DISTANCE = 1400;
/** The shadow's darkness where the card blocks the whole light, and the height over which it
 * comes in: a card lying on the felt casts none. */
const SHADOW = { alpha: 0.55, emerging: 10 };
/** How much wider than the card its shadow spreads, per pixel of height: a lamp, not a point,
 * so a higher card's shadow is larger and softer. */
const PENUMBRA = 0.18;

/** The hand lifting or setting a card down: a critically damped spring. */
const LIFT = { omega: 2 * Math.PI * 4.5 };
/** The grip against the card's tilt: a stiff, lightly damped spring, so a card stopped short
 * rocks once and settles. */
const TILT = { omega: 2 * Math.PI * 3.5, zeta: 0.42 };
/** The acceleration that tilts a held card 45°: the inertia of the plate below the fingertip
 * against the grip's stiffness. */
const GRIP = 60000;
/** Air drag on a moving card tilts it as an acceleration would, per px/s of speed. */
const DRAG = 5;
/** No grip lets a card lean further than this, in radians. */
const MAX_TILT = (20 * Math.PI) / 180;
/** Velocity and acceleration are smoothed over about this long, so that the pointer's uneven
 * events do not jolt the card. */
const SMOOTH = { velocity: 0.035, acceleration: 0.05 };
/** The longest step integrated at once, and the longest gap a frame is taken to stand for. */
const STEP = 1 / 240;
export const MAX_GAP = 1 / 20;

type Vec2 = [number, number];

/** Where the card is this frame: the point carried, in stage pixels, whether the pointer still
 * holds it, and half its extent across and down the screen, which is how far its edge can dip
 * before it touches the felt. */
export interface Sample {
  x: number;
  y: number;
  held: boolean;
  reach: Vec2;
}

/** How the card looks this frame: its height above the felt and its tilt (the turn of its
 * face toward +x and +y on the screen, in radians). */
export interface Look {
  height: number;
  tilt: Vec2;
}

/** One card's height and tilt, from the moment it is picked up until it settles. */
export class Heft {
  #h = 0;
  #vh = 0;
  #tilt: Vec2 = [0, 0];
  #spin: Vec2 = [0, 0];
  #at: Vec2;
  #v: Vec2 = [0, 0];
  #a: Vec2 = [0, 0];
  #held = true;

  /** A card picked up at `x`, `y`, lying flat on the felt. */
  constructor(x: number, y: number) {
    this.#at = [x, y];
  }

  get look(): Look {
    return { height: this.#h, tilt: [...this.#tilt] };
  }

  /** Advances the card `dt` seconds to where `s` has it; returns whether it is still moving. */
  advance(s: Sample, dt: number): boolean {
    dt = Math.min(dt, MAX_GAP);
    if (dt <= 0) return true;
    this.#held = s.held;
    this.#sense(s, dt);
    for (let left = dt; left > 1e-6; left -= STEP) {
      this.#step(s, Math.min(STEP, left));
    }
    if (this.#settled(s)) {
      this.#h = 0;
      this.#tilt = [0, 0];
      return false;
    }
    return true;
  }

  /** Smooths how the carried point moved over the `dt` seconds since the last frame. */
  #sense(s: Sample, dt: number) {
    const raw: Vec2 = [(s.x - this.#at[0]) / dt, (s.y - this.#at[1]) / dt];
    this.#at = [s.x, s.y];
    const kv = 1 - Math.exp(-dt / SMOOTH.velocity);
    const ka = 1 - Math.exp(-dt / SMOOTH.acceleration);
    for (const i of [0, 1] as const) {
      const v = this.#v[i] + (raw[i] - this.#v[i]) * kv;
      this.#a[i] += ((v - this.#v[i]) / dt - this.#a[i]) * ka;
      this.#v[i] = v;
    }
  }

  #step(s: Sample, dt: number) {
    const target = this.#held ? HELD_HEIGHT : 0;
    const w = LIFT.omega;
    this.#vh += (w * w * (target - this.#h) - 2 * w * this.#vh) * dt;
    this.#h = Math.max(0, this.#h + this.#vh * dt);
    // Held above its center of mass, a card's plate lags what moves the grip: it leans into
    // its acceleration and into the air it pushes through, as a pendulum would, and the
    // grip's stiffness pulls it back flat.
    const { omega, zeta } = TILT;
    for (const i of [0, 1] as const) {
      const goal = Math.atan((this.#a[i] + DRAG * this.#v[i]) / GRIP);
      this.#spin[i] +=
        (omega * omega * (goal - this.#tilt[i]) -
          2 * zeta * omega * this.#spin[i]) *
        dt;
      this.#tilt[i] += this.#spin[i] * dt;
      // The dipping edge cannot go through the felt.
      const most = Math.min(
        MAX_TILT,
        Math.asin(Math.min(1, this.#h / Math.max(1, s.reach[i]))),
      );
      if (Math.abs(this.#tilt[i]) > most) {
        this.#tilt[i] = Math.sign(this.#tilt[i]) * most;
        this.#spin[i] = 0;
      }
    }
  }

  /** Whether the card, let go, has come to rest on the felt. */
  #settled(s: Sample) {
    return (
      !this.#held &&
      this.#h < 0.05 &&
      Math.abs(this.#vh) < 1 &&
      Math.hypot(...this.#tilt) < 1e-3 &&
      Math.hypot(...this.#spin) < 1e-2 &&
      Math.hypot(...this.#v) < 1 &&
      this.#at[0] === s.x &&
      this.#at[1] === s.y
    );
  }
}

/** What the surface writes for a card that looks as `look` says: the `transform` of what is
 * inside its node, about `grip`, and the `transform` and `opacity` of its shadow. Lengths are
 * in the node's own frame, which is `frame` wide and `scale` times that on the stage, with
 * the card upright (a held card's pose is: pointer.ts `heldPose`). */
export function drawn(
  look: Look,
  grip: Vec2,
  frame: number,
  scale: number,
): { card: string; shadow: string; opacity: number } {
  const height = frame * (88 / 63);
  const h = look.height;
  const [tx, ty] = look.tilt;
  // Nearer the camera, the card grows about the grip, so it stays under the pointer.
  const lift = DISTANCE / (DISTANCE - Math.min(h, DISTANCE / 2));
  const round = (n: number) => Math.round(n * 1e4) / 1e4;
  const card =
    `perspective(${round(DISTANCE / scale)}px) ` +
    `translate(${round(grip[0])}px, ${round(grip[1])}px) scale(${round(lift)}) ` +
    `rotateX(${round(-ty)}rad) rotateY(${round(tx)}rad) ` +
    `translate(${round(-grip[0])}px, ${round(-grip[1])}px)`;
  // The shadow lies on the felt: under the card as seen, pushed away from the light by its
  // height, as large as the card itself (only the card nears the camera), spread by the
  // light's size and narrowed by the card's tilt.
  const center: Vec2 = [
    grip[0] + (frame / 2 - grip[0]) * lift + (LIGHT.x * h) / scale,
    grip[1] + (height / 2 - grip[1]) * lift + (LIGHT.y * h) / scale,
  ];
  const spread = (PENUMBRA * h) / scale;
  const across = (frame * Math.cos(tx) + spread) / frame;
  const down = (height * Math.cos(ty) + spread) / height;
  const shadow =
    `translate(${round(center[0] - frame / 2)}px, ${round(center[1] - height / 2)}px) ` +
    `scale(${round(across)}, ${round(down)})`;
  const opacity = SHADOW.alpha * Math.min(1, h / SHADOW.emerging);
  return { card, shadow, opacity: round(opacity) };
}
