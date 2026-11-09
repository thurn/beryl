// The UI's sound: sound effects (`play`) and music (`setMusic`), on Web Audio, with the
// player's volumes and mute (`audioSettings`). The cues and their files are manifest.json's,
// which `just audio` transcodes into public/audio: variant n of a cue is
// `audio/<music|sfx>/<cue>-<n>.<m4a|wav>`.
//
// A page under automation (ipc.ts's `audioAvailable`) never fetches, decodes or outputs audio,
// but logs every cue it is asked for, on the page's clock, in `window.berylSounds` and
// `window.berylMusic`, for tests to assert on.
//
// The context is made at once, so the effects decode ahead, and music starts as soon as it
// runs. In the app that is at launch: Tauri's wry sets WKWebView's
// `mediaTypesRequiringUserActionForPlayback` to none, lifting the gesture rule. In a browser
// (`just dev` in Safari, the spec pages) the context stays suspended until the first pointer
// or key press resumes it.
import { note } from "../diagnostics";
import { audioAvailable } from "../ipc";
import manifest from "./manifest.json";
import loops from "./loops.json";

export type Sound = keyof typeof manifest.sounds;
export type Music = keyof typeof manifest.music;

interface Cue {
  readonly variants: readonly unknown[];
  /** Music: whether it repeats, at its exact length in loops.json. */
  readonly loop?: boolean;
  /** Decibels, added at playback: the files are all normalized to one peak or loudness. */
  readonly gain?: number;
  /** A sound's most copies at once: more requests while that many play are dropped. */
  readonly voices?: number;
}

const SOUNDS: Record<Sound, Cue> = manifest.sounds;
const MUSIC: Record<Music, Cue> = manifest.music;
const LOOPS: Partial<Record<Music, readonly number[]>> = loops;

/** A sound's most copies at once when its manifest entry gives none. */
const VOICES = 4;
/** How far each copy of a sound strays from the file, so repeats are not identical: up to
 * this fraction in pitch and this many decibels in level. */
const PITCH_JITTER = 0.03;
const GAIN_JITTER_DB = 1;
/** Seconds one music track takes to fade into the next. */
const CROSSFADE = 1.5;
/** How many times a game's track plays before the next in the rotation takes over. */
const PLAYS_PER_TRACK = 2;

const soundUrl = (sound: Sound, n: number) => `/audio/sfx/${sound}-${n}.wav`;
const musicUrl = (music: Music, n: number) => `/audio/music/${music}-${n}.m4a`;
const fromDb = (db: number) => 10 ** (db / 20);

// ---- Settings ----

export interface AudioSettings {
  /** Linear levels, 0 to 1. */
  music: number;
  sfx: number;
  muted: boolean;
}

const DEFAULTS: AudioSettings = { music: 0.35, sfx: 0.7, muted: false };
/** Where the settings persist, and which game track plays next. */
const SETTINGS_KEY = "beryl.audio";
const ROTATION_KEY = "beryl.audio.rotation";

const level = (v: unknown, fallback: number) =>
  typeof v === "number" && v >= 0 && v <= 1 ? v : fallback;

function readSettings(): AudioSettings {
  try {
    const saved: unknown = JSON.parse(
      localStorage.getItem(SETTINGS_KEY) ?? "{}",
    );
    if (typeof saved !== "object" || saved === null) return DEFAULTS;
    const { music, sfx, muted } = saved as Record<string, unknown>;
    return {
      music: level(music, DEFAULTS.music),
      sfx: level(sfx, DEFAULTS.sfx),
      muted: typeof muted === "boolean" ? muted : DEFAULTS.muted,
    };
  } catch {
    return DEFAULTS;
  }
}

let settings = readSettings();
const listeners = new Set<() => void>();

export const audioSettings = () => settings;

export function subscribeAudio(listener: () => void) {
  listeners.add(listener);
  return () => void listeners.delete(listener);
}

function settingsChanged() {
  applyLevels();
  for (const listener of listeners) listener();
}

export function setAudioSettings(change: Partial<AudioSettings>) {
  settings = { ...settings, ...change };
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
  } catch {
    // Storage may be blocked: the settings last until the page goes.
  }
  settingsChanged();
}

// Spec moments play in separate frames. Keep their gains and controls in sync with
// the parent page, other frames, and full-size moments in other tabs.
window.addEventListener("storage", (event) => {
  if (event.key !== SETTINGS_KEY && event.key !== null) return;
  settings = readSettings();
  settingsChanged();
});

// ---- The log ----

declare global {
  interface Window {
    berylSounds?: { sound: Sound; at: number }[];
    berylMusic?: { music: Music | null; at: number }[];
  }
}

type Log = Required<Pick<Window, "berylSounds" | "berylMusic">>;
const logged: Log | undefined = audioAvailable()
  ? undefined
  : Object.assign(window, { berylSounds: [], berylMusic: [] } as Log);

// ---- The engine ----

interface Engine {
  context: AudioContext;
  music: GainNode;
  sfx: GainNode;
  /** The sounds decoded so far, by URL. */
  decoded: Map<string, AudioBuffer>;
  /** How many copies of each sound are playing. */
  playing: Map<Sound, number>;
}

let engine: Engine | undefined;

async function decode(context: AudioContext, url: string) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  return context.decodeAudioData(await response.arrayBuffer());
}

function applyLevels() {
  if (!engine) return;
  const now = engine.context.currentTime;
  const off = settings.muted ? 0 : 1;
  engine.music.gain.setTargetAtTime(settings.music * off, now, 0.03);
  engine.sfx.gain.setTargetAtTime(settings.sfx * off, now, 0.03);
}

/** Makes the engine and decodes every sound; once per page, and only where audio plays. */
export function installAudio() {
  installListeners();
  if (engine || !audioAvailable() || typeof AudioContext === "undefined") {
    return;
  }
  const context = new AudioContext();
  const music = context.createGain();
  const sfx = context.createGain();
  music.connect(context.destination);
  sfx.connect(context.destination);
  engine = {
    context,
    music,
    sfx,
    decoded: new Map(),
    playing: new Map(),
  };
  applyLevels();
  // A new context reports suspended and turns running a moment later where nothing blocks
  // it, so the music waits for the change rather than for a gesture.
  context.addEventListener("statechange", () => {
    if (context.state === "running" && !track && wanted !== undefined) {
      startMusic(wanted);
    }
  });
  context.resume().catch(() => undefined); // Locked: the first gesture resumes it.
  for (const sound of Object.keys(SOUNDS) as Sound[]) {
    SOUNDS[sound].variants.forEach((_, n) => {
      const url = soundUrl(sound, n);
      decode(context, url).then(
        (buffer) => engine?.decoded.set(url, buffer),
        (e: unknown) => note("warn", "audio", `${url}: ${String(e)}`),
      );
    });
  }
}

/** Whether a gesture's resume is under way: a sound asked for meanwhile (the click of the
 * press that unlocks) plays as the context starts. */
let resuming = false;

/** Resumes a context still locked at a user gesture; its `statechange` starts the music. */
function unlock() {
  if (!engine || engine.context.state === "running" || resuming) return;
  resuming = true;
  engine.context.resume().then(
    () => (resuming = false),
    () => (resuming = false), // A later gesture tries again.
  );
}

const pick = (cue: Cue) => Math.floor(Math.random() * cue.variants.length);

/** Plays a sound effect: a variant at random, slightly varied, unless that sound already
 * plays at its most copies or the player muted effects. `gain` is in decibels; `delay` in
 * milliseconds; `at` stamps the log entry (the beat it sounds, game/playback.ts), now by
 * default. */
export function play(
  sound: Sound,
  {
    gain = 0,
    delay = 0,
    at = performance.now(),
  }: { gain?: number; delay?: number; at?: number } = {},
) {
  logged?.berylSounds.push({ sound, at });
  if (!engine || settings.muted || settings.sfx === 0) return;
  // Sounds asked for while the context is locked would all burst out when it unlocks.
  if (engine.context.state !== "running" && !resuming) return;
  const cue = SOUNDS[sound];
  const playing = engine.playing.get(sound) ?? 0;
  const buffer = engine.decoded.get(soundUrl(sound, pick(cue)));
  if (!buffer || playing >= (cue.voices ?? VOICES)) return;
  const { context, sfx } = engine;
  const source = context.createBufferSource();
  source.buffer = buffer;
  source.playbackRate.value = 1 + (Math.random() * 2 - 1) * PITCH_JITTER;
  const level = context.createGain();
  const jitter = (Math.random() * 2 - 1) * GAIN_JITTER_DB;
  level.gain.value = fromDb((cue.gain ?? 0) + gain + jitter);
  source.connect(level).connect(sfx);
  engine.playing.set(sound, playing + 1);
  source.onended = () => {
    engine?.playing.set(sound, (engine.playing.get(sound) ?? 1) - 1);
    level.disconnect();
  };
  source.start(context.currentTime + delay / 1000);
}

// ---- Music ----

/** The music the screen asks for; none for silence. */
let wanted: Music | undefined;
/** The track playing, fading in or holding; none in silence. */
let track:
  { music: Music; level: GainNode; source?: AudioBufferSourceNode } | undefined;
/** Bumped by every change of track, so a track still decoding when another is asked for
 * never starts. */
let generation = 0;
let nextTrack: number | undefined;

/** Which variant of `music` plays next: each in turn, the game's rotation carrying over
 * from one launch to the next. */
function rotate(music: Music): number {
  const count = MUSIC[music].variants.length;
  if (count === 1) return 0;
  let next = 0;
  try {
    const saved: unknown = JSON.parse(
      localStorage.getItem(ROTATION_KEY) ?? "{}",
    );
    const at =
      typeof saved === "object" && saved !== null
        ? (saved as Record<string, unknown>)[music]
        : undefined;
    next = typeof at === "number" ? at % count : 0;
    localStorage.setItem(
      ROTATION_KEY,
      JSON.stringify({ ...(saved as object), [music]: (next + 1) % count }),
    );
  } catch {
    next = Math.floor(Math.random() * count);
  }
  return next;
}

function fadeOut() {
  window.clearTimeout(nextTrack);
  if (!engine || !track) return;
  const old = track;
  track = undefined;
  const now = engine.context.currentTime;
  old.level.gain.cancelScheduledValues(now);
  old.level.gain.setValueAtTime(old.level.gain.value, now);
  old.level.gain.linearRampToValueAtTime(0, now + CROSSFADE);
  old.source?.stop(now + CROSSFADE);
  window.setTimeout(() => old.level.disconnect(), CROSSFADE * 1000 + 100);
}

/** Crossfades to a variant of `music`: a loop repeats at its exact length, the decoder's
 * trailing padding cut, and a rotation moves on after PLAYS_PER_TRACK plays. */
function startMusic(music: Music) {
  if (!engine) return;
  fadeOut();
  const ticket = ++generation;
  const { context } = engine;
  const cue = MUSIC[music];
  const n = rotate(music);
  const level = context.createGain();
  level.gain.value = 0;
  level.connect(engine.music);
  track = { music, level };
  const url = musicUrl(music, n);
  decode(context, url).then(
    (buffer) => {
      if (ticket !== generation || !track) {
        level.disconnect();
        return;
      }
      const source = context.createBufferSource();
      source.buffer = buffer;
      const length = LOOPS[music]?.[n];
      if (cue.loop && length !== undefined) {
        source.loop = true;
        source.loopEnd = length;
      }
      source.connect(level);
      const now = context.currentTime;
      // Loops fade in; a sting starts at once.
      level.gain.setValueAtTime(0, now);
      level.gain.linearRampToValueAtTime(
        1,
        now + (cue.loop ? CROSSFADE : 0.02),
      );
      source.start(now);
      track.source = source;
      if (cue.loop && length !== undefined && cue.variants.length > 1) {
        const after = length * PLAYS_PER_TRACK - CROSSFADE;
        nextTrack = window.setTimeout(() => {
          if (ticket === generation) startMusic(music);
        }, after * 1000);
      }
    },
    (e: unknown) => note("warn", "audio", `${url}: ${String(e)}`),
  );
}

/** Asks for `music`, or silence: a change crossfades, the same music plays on. Before the
 * context runs it waits for it. */
export function setMusic(music: Music | undefined) {
  if (music === wanted) return;
  wanted = music;
  logged?.berylMusic.push({ music: music ?? null, at: performance.now() });
  if (!engine || engine.context.state !== "running") return;
  if (music === undefined) fadeOut();
  else startMusic(music);
}

// ---- Clicks ----

let listening = false;
let lastTick = 0;
/** The fewest milliseconds between a slider's ticks as it is dragged. */
const TICK_GAP = 60;

/** Every button clicks, every checkbox and radio toggles and every slider ticks, through one
 * listener on the document, so a control added later sounds without asking. The first
 * pointer or key press unlocks the audio. */
function installListeners() {
  if (listening) return;
  listening = true;
  document.addEventListener("pointerdown", unlock, true);
  document.addEventListener("keydown", unlock, true);
  document.addEventListener(
    "click",
    (e) => {
      if (e.target instanceof Element && e.target.closest("button")) {
        play("click");
      }
    },
    true,
  );
  document.addEventListener(
    "input",
    (e) => {
      const input = e.target;
      if (!(input instanceof HTMLInputElement)) return;
      if (input.type === "checkbox" || input.type === "radio") {
        play("toggle");
      } else if (input.type === "range") {
        const now = performance.now();
        if (now - lastTick >= TICK_GAP) {
          lastTick = now;
          play("tick");
        }
      }
    },
    true,
  );
}
