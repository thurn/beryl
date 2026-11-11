// The sounds of what happens in a game, as each beat shows it (game/playback.ts): every event
// kind maps to a sound, or to an explicit none, in a map the compiler holds whole, so a new
// kind of event does not build until it has one.
import type {
  GameEvent,
  PlayerView,
  Side,
  ZoneName,
} from "../generated/protocol";
import { play, type Sound } from "./sound";

/** A card arriving in each zone. */
export const ARRIVING: Record<ZoneName, Sound | null> = {
  battlefield: "place",
  graveyard: "shove",
  exile: "exile",
  hand: "slide",
  library: "fan",
  // A spell sounds by its `cast` event, which comes with its move to the stack.
  stack: null,
  // A token ceasing to exist, once its move to a graveyard has sounded.
  nowhere: null,
};

type Sounds = {
  [K in GameEvent["kind"]]: (
    event: Extract<GameEvent, { kind: K }>,
  ) => Sound | null;
};

const EVENTS: Sounds = {
  zoneChange: (e) => {
    if (e.from === "stack" && e.to !== "battlefield") {
      // A spell done resolving chimes; one taken off the stack otherwise (countered, bounced,
      // exiled) is struck, as CardLayer shakes it.
      if (e.reason === "resolve") return "resolve";
      if (e.reason !== "cast" && e.reason !== "play") return "shatter";
    }
    return ARRIVING[e.to];
  },
  damage: () => "hit",
  draw: () => "slide",
  scry: () => null,
  libraryShuffled: () => "fan",
  lifeGain: () => "gain",
  lifeLoss: () => "drain",
  countersAdded: () => "chip",
  countersRemoved: () => "chips",
  tap: () => "tap",
  untap: () => "untap",
  mana: () => "mana",
  resolutionPayment: () => null,
  abilityFinished: () => null,
  tokensCreated: () => "place",
  cast: () => "cast",
  ability: () => "ability",
  attack: () => "attack",
  // A new turn rings as its banner shows (TurnBanner.tsx); any other step ticks.
  stepBegins: (e) => (e.step === "untap" ? "turn" : "step"),
  gainControl: () => "control",
  coinFlip: () => "coin",
  // How the game ended is the result's music sting (App.tsx).
  lose: () => null,
  win: () => null,
};

const soundOf = <K extends GameEvent["kind"]>(
  e: Extract<GameEvent, { kind: K }>,
) =>
  (EVENTS[e.kind] as (e: Extract<GameEvent, { kind: K }>) => Sound | null)(e);

/** How much quieter the opponent's doings are than the player's own, in decibels. */
export const THEIRS_DB = -4;
/** How long after its beat a sound waits, in ms: a card is placed as it lands, after its
 * glide (Board's GLIDE_MS, near enough). */
const DELAY: Partial<Record<Sound, number>> = { place: 450 };

/** Who did what `e` records, as far as it says. */
function sideOf(e: GameEvent, view: PlayerView): Side | undefined {
  if (e.kind === "stepBegins") return e.active;
  if (e.kind === "ability" && "controller" in e.item) return e.item.controller;
  if ("player" in e) return e.player;
  if ("controller" in e) return e.controller;
  const slot =
    "slot" in e ? e.slot : e.kind === "damage" ? e.source : undefined;
  return view.objects.find((o) => o.slot === slot)?.controller;
}

/** Whether `e` is life lost to damage `view` deals that player, which its hit already says. */
function hurt(e: GameEvent, view: PlayerView): boolean {
  return (
    e.kind === "lifeLoss" &&
    view.events.some(
      (d) =>
        d.kind === "damage" &&
        d.to.kind === "player" &&
        d.to.player === e.player,
    )
  );
}

/** The sounds `view`'s events make, each once however many events make it: louder if any of
 * them is the player's. */
export function beatSounds(view: PlayerView): Map<Sound, Side | undefined> {
  const sounds = new Map<Sound, Side | undefined>();
  if (view.undone) return sounds;
  for (const e of view.events) {
    const sound = soundOf(e);
    if (sound === null || hurt(e, view)) continue;
    const side = sideOf(e, view);
    if (sounds.get(sound) !== "you") sounds.set(sound, side);
  }
  return sounds;
}

/** Plays the sounds of a beat shown at `at` on the page's clock. */
export function soundBeat(view: PlayerView, at: number) {
  for (const [sound, side] of beatSounds(view)) {
    play(sound, {
      at,
      gain: side === "opponent" ? THEIRS_DB : 0,
      delay: DELAY[sound],
    });
  }
}

declare global {
  interface Window {
    /** For tests: the sounds a view shown live must have asked for. */
    berylBeatSounds?: (view: PlayerView) => Sound[];
  }
}
if (navigator.webdriver) {
  window.berylBeatSounds = (view) => [...beatSounds(view).keys()];
}
