// Cards as physical things over the table, faked in the DOM without WebGL. A card has a height
// above the felt and a tilt, which a damped integrator advances only while something moves it:
// the pointer holding it, gravity after it is let go, or the hand that carries it from zone to
// zone while it glides. From those follow what a camera would see: the card grows as it nears
// the lens, leans into its motion, is shaded by how it faces the light, and casts a shadow that
// slides away from the light and softens with height.
//
// Units are stage pixels. A battlefield card is 128 px wide for its 63 mm, so about 2 px a
// millimeter, which sets gravity and a falling card's terminal speed. Only transforms, opacity
// and the shadow's mask change per frame, written straight to the elements, and a card at rest
// runs no frame at all, so a still board costs nothing.
import { useEffect, useRef, type RefObject } from "react";
import type { MotionValue } from "motion/react";
import { cameraDistance } from "./layout";

/** How far a card's shadow falls from beneath it on the table, per pixel of height: the light
 * hangs above the far left of the table, so shadows fall toward you and a little right. */
export const LIGHT = { x: 0.14, y: 0.5 };
/** The direction toward the light, for shading a tilted card (unit length). */
const TO_LIGHT = normalize([-LIGHT.x, -LIGHT.y, 1]);
/** How brightly the light falls on a card lying flat. */
const FLAT_LIT = TO_LIGHT[2];
/** The penumbra's width: a lamp, not a point, so a shadow blurs by the light's angular size
 * times the card's height, plus the felt's own softness. */
const PENUMBRA = { base: 2, perHeight: 0.18 };
/** A shadow's darkness where the card blocks the whole lamp: the felt is still lit by the room. */
const SHADOW_ALPHA = 0.58;
/** The height over which a cast shadow takes over from the resting contact shadow. */
const CONTACT_HEIGHT = 10;

/** 9.81 m/s² at 2 px/mm. */
const GRAVITY = 19620;
/** A card falling flat in air (1.8 g over 63 × 88 mm, a drag coefficient of 1.2) stops
 * speeding up at about 2.1 m/s. */
const TERMINAL = 4200;
/** How much of its speed a card keeps bouncing off the felt. */
const RESTITUTION = 0.1;

/** How high a card is held while dragged: as high as a raised hand card's growth says
 * (layout.ts' RAISE, at the camera's distance). */
export const HELD_HEIGHT = 80;
/** A card glided to a new place rises with its speed, as a hand lifts what it carries, up to
 * this height. */
const CARRY = { perSpeed: 0.03, max: 56 };
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
/** Velocity and acceleration are smoothed over about this long, in seconds, so that the
 * pointer's uneven events do not jolt the card. */
const SMOOTH = { velocity: 0.035, acceleration: 0.05 };
/** The longest step integrated at once, and the longest gap a frame is taken to stand for. */
const STEP = 1 / 240;
const MAX_GAP = 1 / 20;

/** How a card is being moved. */
export type Handling =
  | "held" // the pointer has it
  | "dropped" // let go where it was played: it falls to the felt
  | "free"; // glided by the layout, carried while it moves

/** A card's physical state. Heights are above the felt; tilts are the turn of the card's
 * normal toward +x and +y on the screen, in radians. */
interface Body {
  h: number;
  vh: number;
  tilt: [number, number];
  spin: [number, number];
  /** The point carried, its smoothed velocity and acceleration. */
  at: [number, number];
  v: [number, number];
  a: [number, number];
  /** The shadow's fade-in, so a card lifted from a raised hand does not pop one in. */
  presence: number;
  /** Where a card the layout glides comes to rest: its layout's height, which it follows as
   * a hand card rises or sinks in the fan, though never above the felt it was dropped on
   * until it eases back up. */
  base: number;
  /** The handling and layout height of the last step. */
  handling: Handling;
  rest: number;
}

/** What a card's frame says about it this moment. */
export interface Frame {
  handling: Handling;
  /** The carried point, in stage pixels. */
  x: number;
  y: number;
  /** The height its layout already draws it at: a raised hand card's. It rests there unless
   * dropped, and its look's growth already includes it. */
  rest: number;
  /** Half the card's extent across and down the screen, in stage pixels: how far its edge can
   * dip before it touches the felt. */
  reach: [number, number];
}

/** Senses how the carried point moved over the `dt` seconds since the last frame. */
function sense(b: Body, f: Frame, dt: number) {
  const raw: Vec2 = [(f.x - b.at[0]) / dt, (f.y - b.at[1]) / dt];
  b.at = [f.x, f.y];
  const kv = 1 - Math.exp(-dt / SMOOTH.velocity);
  const ka = 1 - Math.exp(-dt / SMOOTH.acceleration);
  for (const i of [0, 1] as const) {
    const v = b.v[i] + (raw[i] - b.v[i]) * kv;
    b.a[i] += ((v - b.v[i]) / dt - b.a[i]) * ka;
    b.v[i] = v;
  }
}

/** One integration of `b` over `dt` seconds, toward what `f` says. */
function step(b: Body, f: Frame, dt: number) {
  if (f.handling !== b.handling) {
    b.base = Math.min(b.h, f.rest);
    b.handling = f.handling;
  }
  const base = Math.min(
    f.rest,
    Math.max(0, b.base + f.rest - b.rest) +
      (f.rest - b.base) * (1 - Math.exp(-dt / 0.3)),
  );
  // Glided, the card is carried up and down with its layout's height, not dragged after it.
  if (f.handling === "free") b.h += base - b.base;
  b.base = base;
  b.rest = f.rest;

  if (f.handling === "dropped") {
    // Let go: gravity, against air drag growing with the square of the speed.
    const drag = GRAVITY * (b.vh / TERMINAL) ** 2 * Math.sign(b.vh);
    b.vh += (-GRAVITY - drag) * dt;
    b.h += b.vh * dt;
    if (b.h <= 0) {
      b.h = 0;
      b.vh = Math.abs(b.vh) * RESTITUTION < 40 ? 0 : -b.vh * RESTITUTION;
    }
  } else {
    const speed = Math.hypot(...b.v);
    const target =
      f.handling === "held"
        ? Math.max(HELD_HEIGHT, f.rest)
        : b.base + Math.min(CARRY.max, speed * CARRY.perSpeed);
    const w = LIFT.omega;
    b.vh += (w * w * (target - b.h) - 2 * w * b.vh) * dt;
    b.h += b.vh * dt;
  }

  // Held above its center of mass, a card's plate lags what moves the grip: it leans into
  // its acceleration and into the air it pushes through, as a pendulum would, and the grip's
  // stiffness pulls it back flat.
  const { omega: w, zeta } = TILT;
  for (const i of [0, 1] as const) {
    const push = b.a[i] + DRAG * b.v[i];
    const goal = Math.atan(push / GRIP);
    b.spin[i] += (w * w * (goal - b.tilt[i]) - 2 * zeta * w * b.spin[i]) * dt;
    b.tilt[i] += b.spin[i] * dt;
    // The dipping edge cannot go through the felt.
    const most = Math.min(
      MAX_TILT,
      Math.asin(Math.min(1, Math.max(0, b.h) / f.reach[i])),
    );
    if (Math.abs(b.tilt[i]) > most) {
      b.tilt[i] = Math.sign(b.tilt[i]) * most;
      b.spin[i] = 0;
    }
  }
  // A card in the hand casts no shadow of its own on the felt, only one lifted over it.
  const shown = f.handling !== "free" || b.h - b.base > 2 ? 1 : 0;
  b.presence += (shown - b.presence) * (1 - Math.exp(-dt / 0.08));
}

/** Whether `b` has come to rest where `f` leaves it. */
function settled(b: Body, f: Frame) {
  return (
    f.handling !== "held" &&
    (f.handling === "dropped"
      ? b.h === 0
      : Math.abs(b.h - f.rest) < 0.05 &&
        Math.abs(b.base - f.rest) < 0.05 &&
        b.presence < 0.02) &&
    Math.abs(b.vh) < 1 &&
    Math.hypot(...b.tilt) < 1e-3 &&
    Math.hypot(...b.spin) < 1e-2 &&
    Math.hypot(...b.v) < 1 &&
    b.at[0] === f.x &&
    b.at[1] === f.y
  );
}

/** One frame loop for every moving card, running only while one is. */
const awake = new Set<(dt: number) => boolean>();
let pending = 0;
let last: number | undefined;
function tick(now: number) {
  const dt =
    last === undefined ? 1 / 60 : Math.min((now - last) / 1000, MAX_GAP);
  last = now;
  for (const advance of [...awake]) {
    if (!advance(dt)) awake.delete(advance);
  }
  pending = awake.size > 0 ? requestAnimationFrame(tick) : 0;
  if (!pending) last = undefined;
}
function wake(advance: (dt: number) => boolean) {
  awake.add(advance);
  if (!pending) pending = requestAnimationFrame(tick);
}

/** What a card's element is drawn with: its motion values, as CardLayer animates them. */
export interface CardMotion {
  x: MotionValue<number>;
  y: MotionValue<number>;
  scale: MotionValue<number>;
  rotate: MotionValue<number>;
  /** The raise inside the element (CardLayer's useRaise). */
  raiseScale: MotionValue<number>;
  raiseRotate: MotionValue<number>;
}

export interface PhysicsOptions {
  motion: CardMotion;
  handling: Handling;
  /** The card's footprint in its element, before any raise: its width and height. */
  size: [number, number];
  /** Whether it can rise at all: a card no one sees cannot. */
  enabled: boolean;
  reduce: boolean;
  /** What tilts and grows: the card's look. */
  body: RefObject<HTMLElement | null>;
  /** Its shadow on the felt, and the shade over its face. */
  shadow: RefObject<HTMLElement | null>;
  shade: RefObject<HTMLElement | null>;
  /** Where the pointer took the card, as a fraction of its footprint; its center otherwise. */
  grip: RefObject<[number, number]>;
}

/** Runs a card's body while anything moves it, drawing its height, tilt, shade and shadow. */
export function useCardPhysics(o: PhysicsOptions) {
  const options = useRef(o);
  options.current = o;
  const body = useRef<Body | undefined>(undefined);
  const startRef = useRef<() => void>(undefined);

  useEffect(() => {
    const { x, y } = o.motion;
    let running = false;
    const advance = (dt: number) => {
      const opts = options.current;
      const frame = frameOf(opts);
      let b = body.current;
      if (!b) {
        b = body.current = {
          h: frame.rest,
          vh: 0,
          tilt: [0, 0],
          spin: [0, 0],
          at: [frame.x, frame.y],
          v: [0, 0],
          a: [0, 0],
          presence: 0,
          base: frame.rest,
          handling: frame.handling,
          rest: frame.rest,
        };
      }
      if (!opts.enabled) {
        running = false;
        body.current = undefined;
        clear(opts);
        return false;
      }
      if (opts.reduce) {
        // No motion: a held card still stands off the felt and a dropped one lies on it, both
        // without swaying; one the layout glides is simply as its layout draws it.
        if (frame.handling === "free") {
          running = false;
          body.current = undefined;
          clear(opts);
          return false;
        }
        b.h = frame.handling === "held" ? Math.max(HELD_HEIGHT, frame.rest) : 0;
        b.presence = 1;
        draw(opts, b, frame);
        running = frame.handling === "held";
        return running;
      }
      sense(b, frame, dt);
      for (let left = dt; left > 1e-6; left -= STEP) {
        step(b, frame, Math.min(STEP, left));
      }
      if (settled(b, frame)) {
        running = false;
        body.current = undefined;
        clear(opts);
        return false;
      }
      draw(opts, b, frame);
      return true;
    };
    const start = () => {
      if (running || !options.current.enabled) return;
      running = true;
      wake(advance);
    };
    // A hand card rising in the fan stays as its layout draws it: only moving it wakes it.
    const unsubscribe = [x, y].map((v) => v.on("change", start));
    start();
    startRef.current = start;
    return () => {
      unsubscribe.forEach((u) => u());
      awake.delete(advance);
      running = false;
    };
    // The motion values are the element's for its life; the rest is read each frame.
  }, []);
  // Picked up, let go or handed back to the layout: the card may move without its place doing.
  useEffect(() => {
    startRef.current?.();
  }, [o.handling, o.enabled]);
}

/** The frame `o` stands in now. */
function frameOf(o: PhysicsOptions): Frame {
  const { x, y, scale, raiseScale } = o.motion;
  const [width, height] = o.size;
  const s = scale.get() * raiseScale.get();
  const distance = cameraDistance(y.get());
  // The raise grows a hand card as nearing the camera by this much would.
  const rest = distance * (1 - 1 / raiseScale.get());
  return {
    handling: o.handling,
    // Where on the card it is carried only offsets the point, which moves alike.
    x: x.get(),
    y: y.get(),
    rest: rest < 0.5 ? 0 : rest,
    reach: [(width * s) / 2, (height * s) / 2],
  };
}

/** Draws body `b` of the card `o` describes. */
function draw(o: PhysicsOptions, b: Body, f: Frame) {
  const { scale, rotate, raiseScale, raiseRotate, y } = o.motion;
  const [width, height] = o.size;
  const [gx, gy] = o.grip.current;
  const raise = raiseScale.get();
  const s = scale.get() * raise;
  const distance = cameraDistance(y.get());
  // Nearer the camera than its layout draws it, the card grows about the grip, along the line
  // of sight through it, so that it stays under the pointer.
  const lift = distance / (distance - Math.min(b.h - f.rest, distance / 2));
  const [tx, ty] = b.tilt;
  const rotateX = -ty;
  const rotateY = tx;
  const bodyEl = o.body.current;
  if (bodyEl) {
    const origin = `${gx * width}px ${gy * height}px`;
    bodyEl.style.transformOrigin = origin;
    bodyEl.style.transform =
      `perspective(${distance / s}px) scale(${lift}) ` +
      `rotateX(${rotateX}rad) rotateY(${rotateY}rad)`;
    bodyEl.style.setProperty(
      "--contact",
      String(Math.max(0, 1 - b.h / CONTACT_HEIGHT)),
    );
  }

  // Lambert's law: the face is lit by how squarely it faces the lamp, against how it is lit
  // lying flat.
  const normal: Vec3 = [
    Math.sin(rotateY),
    -Math.cos(rotateY) * Math.sin(rotateX),
    Math.cos(rotateY) * Math.cos(rotateX),
  ];
  const lit = dot(normal, TO_LIGHT) / FLAT_LIT - 1;
  const shadeEl = o.shade.current;
  if (shadeEl) {
    shadeEl.style.background = lit > 0 ? "#fff" : "#000";
    shadeEl.style.opacity = String(Math.min(0.32, Math.abs(lit) * 1.1));
  }

  // The shadow lies on the felt, in the element's turned and scaled frame: under the card as
  // seen, pushed away from the light by its height, as large as the card itself (only the
  // card nears the camera), blurred by the lamp's size and narrowed by the card's tilt.
  const shadowEl = o.shadow.current;
  if (!shadowEl) return;
  const turn = ((rotate.get() + raiseRotate.get()) * Math.PI) / 180;
  const [sx, sy] = [LIGHT.x * b.h, LIGHT.y * b.h];
  const local: Vec2 = [
    (sx * Math.cos(turn) + sy * Math.sin(turn)) / s,
    (-sx * Math.sin(turn) + sy * Math.cos(turn)) / s,
  ];
  const grip: Vec2 = [gx * width, gy * height];
  const center: Vec2 = [
    grip[0] + (width / 2 - grip[0]) * lift + local[0],
    grip[1] + (height / 2 - grip[1]) * lift + local[1],
  ];
  const penumbra = (PENUMBRA.base + PENUMBRA.perHeight * b.h) / s;
  const across = (width * Math.cos(tx)) / raise + penumbra;
  const down = (height * Math.cos(ty)) / raise + penumbra;
  shadowEl.style.transform =
    `translate(${center[0] - width / 2}px, ${center[1] - height / 2}px) ` +
    `scale(${across / width}, ${down / height})`;
  shadowEl.style.setProperty("--pen-x", `${(penumbra / across) * 100}%`);
  shadowEl.style.setProperty("--pen-y", `${(penumbra / down) * 100}%`);
  const emerging = Math.min(1, b.h / CONTACT_HEIGHT);
  shadowEl.style.opacity = String(SHADOW_ALPHA * emerging * b.presence);
}

/** Puts the card back as its layout draws it. */
function clear(o: PhysicsOptions) {
  const bodyEl = o.body.current;
  if (bodyEl) {
    bodyEl.style.transform = "";
    bodyEl.style.removeProperty("--contact");
  }
  if (o.shade.current) o.shade.current.style.opacity = "0";
  if (o.shadow.current) o.shadow.current.style.opacity = "0";
  o.grip.current = [0.5, 0.5];
}

type Vec2 = [number, number];

type Vec3 = [number, number, number];

function normalize([x, y, z]: Vec3): Vec3 {
  const length = Math.hypot(x, y, z);
  return [x / length, y / length, z / length];
}

const dot = (a: Vec3, b: Vec3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
