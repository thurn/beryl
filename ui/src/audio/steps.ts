// The sounds of what happens in a game, as each step of a script shows it on the stage (plan
// K25; rust/crates/beryl_session/src/script/mod.rs, "Sound"): a sound follows a step. It is
// picked from the step's verb and what the step carries (the zones and the reason of a card's
// move, the direction of a tap, what a count counts and whether it rose from the table's
// value), in a map the compiler holds whole, so a new verb does not build until it has one.
// It is asked for as its step begins, or as the step's motion ends where it is the sound of
// something arriving (a card placed as it lands), at that edge's stage time, and sounds then
// or a share of the step after (a strike's hit, as its lunge lands): nothing waits a fixed
// time. This is audio/events.ts for the stage, which takes its place at the cut-over.
import type {
  ObjectView,
  Side,
  Subject,
  Verb,
  Why,
  ZoneName,
} from "../generated/protocol";
import type { Edge, ScriptStage } from "../stage/stage.ts";
import type { Act, Step as ScriptStep, Table } from "../stage/table.ts";
import { STRIKE } from "../stage/tracks.ts";
import { ARRIVING, THEIRS_DB } from "./events";
import { play, type Sound } from "./sound";

/** A sound a step asks for, the edge of the step it is asked for at, and how far into the
 * step after that edge it sounds, as a share of the step's time (none: at the edge). */
export interface Cue {
  sound: Sound;
  edge: Edge["edge"];
  into?: number;
}

/** What a step's sounds are picked from besides its act: the table as the step found it, and
 * the steps of its beat before it. */
type Found = Pick<Edge, "table" | "earlier">;

type Cues = {
  [V in Verb]: (act: Extract<Act, { verb: V }>, found: Found) => Cue[];
};

const at = (edge: Cue["edge"], sound: Sound | null | false): Cue[] =>
  sound ? [{ sound, edge }] : [];

/** A card going from `from` to `to`, as it sets out. */
function moving(from: ZoneName, to: ZoneName, why: Why): Sound | null {
  // A spell sounds as it is cast, however it comes to the stack.
  if (to === "stack") return why === "cast" ? "cast" : null;
  if (from === "stack" && to !== "battlefield") {
    // A spell done resolving chimes; one taken off the stack otherwise (countered, bounced,
    // exiled) is struck.
    if (why === "resolve") return "resolve";
    if (why !== "cast" && why !== "play") return "shatter";
  }
  return from === to ? null : ARRIVING[to];
}

const recordOf = ({ view }: Table, slot: number) =>
  view.objects.find((o) => o.slot === slot);

/** How many counters of `kind` `object` has. */
const counted = (object: ObjectView | undefined, kind: string) =>
  object?.counters.find((c) => c.kind === kind)?.count ?? 0;

/** Whether `player` is dealt damage by a step of `steps`, whose sound says the life it
 * costs them. */
const hurt = (steps: readonly ScriptStep[], player: Side) =>
  steps.some(
    ({ act }) =>
      (act.verb === "strike" ||
        (act.verb === "streak" && act.stream.kind === "damage")) &&
      act.to.kind === "player" &&
      act.to.player === player,
  );

const sum = (pool: Record<string, number>) =>
  Object.values(pool).reduce((a, b) => a + b, 0);

/** A permanent newly declared an attacker, as its record says. */
const attacks = (object: ObjectView, table: Table) =>
  object.combat?.kind === "attacking" &&
  recordOf(table, object.slot)?.combat?.kind !== "attacking";

const CUES: Cues = {
  // A card is placed as it lands; any other move sounds as it sets out. A permanent that
  // stays on the battlefield and changes sides is taken.
  fly: ({ from, why, object }, { table }) => {
    const taken =
      from === object.zone &&
      recordOf(table, object.slot)?.controller !== object.controller;
    const sound = taken ? "control" : moving(from, object.zone, why);
    return at(sound === "place" ? "end" : "start", sound);
  },
  // No flight: a token is placed as it appears.
  appear: ({ from, why, object }) =>
    at("start", moving(from, object.zone, why)),
  // A card is then at its new place; an ability done, or a token ceasing to exist, has none.
  vanish: ({ from, why, object }) =>
    at("start", object ? moving(from, object.zone, why) : null),
  emerge: () => at("start", "ability"),
  flip: () => [],
  // An attacker taps as it is declared: its record says both.
  tap: ({ object }, { table }) => [
    ...at("start", object.tapped ? "tap" : "untap"),
    ...at("start", attacks(object, table) && "attack"),
  ],
  // A creature hits as its lunge lands, a share of the way through its strike (tracks.ts
  // STRIKE.hit), where Arena's screen flashes: one step, so the sound waits within it.
  strike: () => [{ sound: "hit", edge: "start", into: STRIKE.hit }],
  // Mana sounds as it leaves its source; damage hits as its streak lands.
  streak: ({ stream }) =>
    stream.kind === "mana" ? at("start", "mana") : at("end", "hit"),
  count: ({ tally }, { table, earlier }) => {
    switch (tally.kind) {
      case "life": {
        const was = table.view.players[tally.player].life;
        if (tally.life > was) return at("start", "gain");
        // Life lost to damage is the hit's to say.
        const drained = tally.life < was && !hurt(earlier, tally.player);
        return at("start", drained && "drain");
      }
      // Mana that stays in a pool; what leaves one streaks.
      case "pool": {
        const was = sum(table.view.players[tally.player].mana);
        return at("start", sum(tally.mana) > was && "mana");
      }
      case "counters": {
        const now = counted(tally.object, tally.counter);
        const was = counted(recordOf(table, tally.object.slot), tally.counter);
        return at("start", now > was ? "chip" : now < was && "chips");
      }
      case "stats":
        return [];
    }
  },
  set: ({ fact }, { table }) => {
    switch (fact.kind) {
      // A step ticks as it begins. A new turn rings as its banner shows (`announce`).
      case "turn": {
        const { number, step } = table.view.turn;
        return at(
          "start",
          fact.number === number && fact.step !== step && "step",
        );
      }
      case "object":
        return at("start", attacks(fact.object, table) && "attack");
      // A library is shuffled.
      case "library":
        return at("start", "fan");
      case "item":
        return [];
    }
  },
  showcase: () => [],
  dwell: () => [],
  announce: ({ notice }) => {
    switch (notice.kind) {
      case "turn":
        return at("start", "turn");
      case "coin":
        return at("start", "coin");
      case "scry":
        return [];
    }
  },
  point: () => [],
  mark: () => [],
  lift: () => [],
};

/** The sounds the step `step` asks for, given what it found: each at the step's start or its
 * end. */
export function stepCues(step: ScriptStep, found: Found): Cue[] {
  const act = step.act;
  const cues = CUES[act.verb] as (act: Act, found: Found) => Cue[];
  return cues(act, found);
}

/** Who controls what `subject` names on `table`. */
function controllerOf(table: Table, subject: Subject): Side | undefined {
  if (subject.kind === "player") return subject.player;
  if (subject.kind === "object") {
    return recordOf(table, subject.slot)?.controller;
  }
  let ability = 0;
  const item = table.view.stack.find(
    (item) =>
      item.kind === "ability" && table.abilities[ability++] === subject.id,
  );
  return item?.kind === "ability" ? item.controller : undefined;
}

/** Whose doing `step` is: its cause's controller, or else whose the record it carries is. */
function sideOf({ act, cause }: ScriptStep, table: Table): Side | undefined {
  if (act.verb === "emerge" && act.item.kind === "ability") {
    return act.item.controller;
  }
  if (cause) return controllerOf(table, cause);
  if ("object" in act && act.object) return act.object.controller;
  if (act.verb === "count" && "player" in act.tally) return act.tally.player;
  if (act.verb === "count" && "object" in act.tally) {
    return act.tally.object.controller;
  }
  if (act.verb === "set" && act.fact.kind === "turn") return act.fact.active;
  if (act.verb === "announce") {
    return "active" in act.notice ? act.notice.active : act.notice.player;
  }
  return undefined;
}

/** Plays the sounds of `stage`'s steps from now on, each as the edge of its step it belongs
 * to comes, or its share of the step after, stamped with that edge's stage time; returns how
 * to stop. A sound asked for more than once at one stage time plays once, as the first of
 * those steps asks. */
export function soundSteps(stage: Pick<ScriptStage, "follow">) {
  let time = NaN;
  const sounded = new Set<Sound>();
  return stage.follow((edge) => {
    if (edge.at !== time) sounded.clear();
    time = edge.at;
    for (const cue of stepCues(edge.step, edge)) {
      if (cue.edge !== edge.edge || sounded.has(cue.sound)) continue;
      sounded.add(cue.sound);
      const theirs = sideOf(edge.step, edge.table) === "opponent";
      const delay = (cue.into ?? 0) * (edge.span.to - edge.span.from);
      play(cue.sound, { at: edge.at, delay, gain: theirs ? THEIRS_DB : 0 });
    }
  });
}

declare global {
  interface Window {
    /** For tests: the sounds a step the stage plays must have asked for, as `stage.follow`
     * tells it, each with the edge of the step it sounds at. */
    berylStepSounds?: (edge: Edge) => Cue[];
  }
}
if (navigator.webdriver) {
  window.berylStepSounds = (edge) => stepCues(edge.step, edge);
}
