// How each step is played, from the design language's data (data/ui-conventions.json, `verbs`):
// a verb's duration, delay and stagger in stage milliseconds and its curve, Beryl's own (UI
// Conventions, "Motion is Beryl's own"), first set from Arena's medians (data/arena-motion.json,
// plan K27). Where a verb has several cases (a draw, a discard), a step of a case is played with
// that case's own (`cases`), and with the verb's where the case has none. A verb with a number
// not taken has none, and one with no duration snaps. The effects a step begins are the data's
// too (`effects`, and each verb's list of those its steps begin): `effectsOf`.
import { easeIn, easeInOut, easeOut } from "motion";
import conventions from "../../../data/ui-conventions.json" with { type: "json" };
import type { PlayerView } from "../generated/protocol";
import type { Act } from "./table.ts";
import type { Effect, Layer, On } from "./effects.ts";
import { SNAP, type Timing } from "./stage.ts";
import { namedBy } from "./spans.ts";
import { CLOSED, meets, STRIKE } from "./tracks.ts";

/** The curves the data names, as progress from 0 to 1 (CSS's of the same names); `none` is a
 * verb in which nothing travels, which changes evenly. */
const CURVES: Record<string, Timing["ease"]> = {
  linear: SNAP.ease,
  none: SNAP.ease,
  "ease-in": easeIn,
  "ease-out": easeOut,
  "ease-in-out": easeInOut,
};

interface Values {
  duration: number | null;
  delay: number | null;
  stagger: number | null;
  curve: string | null;
}

/** The timing `values` give, each number or curve not taken being `over`'s. */
const timing = (values: Values, over?: Timing): Timing => ({
  duration: values.duration ?? over?.duration ?? 0,
  delay: values.delay ?? over?.delay ?? 0,
  stagger: values.stagger ?? over?.stagger ?? 0,
  ease: values.curve
    ? (CURVES[values.curve] ?? SNAP.ease)
    : (over?.ease ?? SNAP.ease),
});

const verbs = new Map(
  conventions.verbs.map((verb): [string, Timing] => [
    verb.name,
    timing(verb.values),
  ]),
);

/** Each verb's cases by Arena's names for them, each with its own timing. */
const cases = new Map(
  conventions.verbs.map((verb): [string, Map<string, Timing>] => {
    const own: Record<string, Values | undefined> =
      ("cases" in verb && verb.cases) || {};
    const whole = verbs.get(verb.name);
    return [
      verb.name,
      new Map(
        Object.entries(own).flatMap(([name, values]) =>
          values ? [[name, timing(values, whole)] as const] : [],
        ),
      ),
    ];
  }),
);

/** Arena's name for the case the step `act` is of (data/arena-motion.json's `case`; `by
 * motion` reads a capture's steps the same, tools' motion.py `case_of`): none for a case
 * Arena's instances do not have, which is played with its verb's values. `found` is the table
 * the step finds, which says whether a life total rises or falls. */
export function caseOf(act: Act, found?: PlayerView): string | undefined {
  switch (act.verb) {
    case "tap":
      return act.object.tapped ? "tap" : "untap";
    case "strike":
      return act.to.kind === "player"
        ? "unblocked attacker"
        : "blocked attacker";
    case "count":
      switch (act.tally.kind) {
        case "life": {
          const was = found?.players[act.tally.player].life;
          if (was === undefined || was === act.tally.life) return undefined;
          return act.tally.life > was ? "life gain" : "life loss";
        }
        case "counters":
          return act.tally.counter === "loyalty" ? "loyalty" : "counter";
        default:
          return undefined;
      }
    case "fly":
      switch (`${act.from}>${act.object.zone}`) {
        case "hand>stack":
          return act.object.controller === "you"
            ? "own spell cast"
            : "opponent spell cast";
        case "stack>battlefield":
          return "permanent resolves";
        case "hand>battlefield":
          return "land played";
        case "hand>graveyard":
          return "discard";
        case "library>hand":
          return "draw";
        default:
          return undefined;
      }
    // The stage's own, of the cards a prompt holds up (table.ts `Hold`).
    case "lift":
      return "tray" in act ? (act.sorted ? "sort" : "tray") : undefined;
    case "appear":
      if (act.from === "nowhere") return "token";
      return act.why === "draw" ? "opening hand" : undefined;
    case "emerge":
      return act.item.kind === "ability" && act.item.triggered
        ? "triggered ability"
        : "activated ability";
    case "vanish":
      if (act.subject.kind === "ability") return "ability finished";
      // Where it goes says more than why: a spell an effect returns is bounced.
      if (act.object?.zone === "hand") return "bounced";
      if (act.object?.zone === "exile") return "exiled";
      switch (act.why) {
        case "destroy":
        case "stateBased":
          return "destroyed";
        case "counter":
          return "countered";
        case "resolve":
          return "resolved instant or sorcery";
        case "sacrifice":
          return "sacrificed";
        default:
          return undefined;
      }
    case "point":
      if (act.span === "end") {
        return act.arrow === "source" ? "source line end" : "end";
      }
      switch (act.arrow) {
        case "source":
          return "source line";
        case "attack":
          return "attack line";
        case "block":
          return "block line";
        default: {
          // At another spell or ability on the stack, its arrow loops over the two (UI
          // Conventions, "Two items on the stack").
          const stacked =
            act.to.kind === "ability" ||
            (act.to.kind === "object" &&
              found?.stack.some(
                (item) =>
                  item.kind === "spell" && namedBy(act.to) === item.slot,
              ));
          return stacked ? "stack arrow" : "target arrow";
        }
      }
    case "mark":
      if (act.span === "end") return "end";
      return act.mark === "friendly-target" || act.mark === "hostile-target"
        ? "target"
        : act.mark;
    case "streak":
      return act.stream.kind;
    case "announce":
      return NOTICES[act.notice.kind];
    case "dwell":
      // On a player, it is the table left to be seen as the game ends (script/spans.rs).
      return act.subject.kind === "player" ? "game's end" : undefined;
    case "set":
      if (act.fact.kind === "turn") return "step begins";
      return act.fact.kind === "library" ? "library shuffled" : undefined;
    default:
      return undefined;
  }
}

/** The data's name for the announcement of each kind. */
const NOTICES = { turn: "turn banner", scry: "scry pill", coin: "coin" };

/** How long after one card of a batch begins to leave for exile the next does (UI
 * Conventions, "A batch exile": one at a time, about 0.3 s apart). Arena's instances are of
 * single cards, so its data has no stagger for the case. */
const BATCH_EXILE = 300;

/** A finished ability's: a cut, after which what it left room for closes up. */
const FINISHED: Timing = { ...SNAP, closes: CLOSED };

/** The data's timing of the step `act`, which finds the table `table`: its case's, or its
 * verb's. An ability that has finished is no verb's median: it is removed at once (UI
 * Conventions, "A finished ability"; Arena's is gone between two frames), and what it left
 * room for then closes up. So is a card's name
 * learned where it lies in no one's sight (a look at your own library, which its prompt
 * shows): only a permanent turns over (UI Conventions, "A card turning face up"). `before`
 * is the act of the step before it in its beat. */
export function timingOf(act: Act, table?: PlayerView, before?: Act): Timing {
  if (act.verb === "vanish" && act.subject.kind === "ability") return FINISHED;
  if (act.verb === "flip" && act.object.zone !== "battlefield") return SNAP;
  const found = caseOf(act, table);
  const own =
    (found === undefined ? undefined : cases.get(act.verb)?.get(found)) ??
    verbs.get(act.verb) ??
    SNAP;
  // An arrow or a line is no turn of its beat: it goes with the step that calls for it and
  // holds up none after it. Its end is a cut, the data's cases of no time, as Arena cuts its
  // own (data/arena-motion.json, point: "It is cut, not faded, when it ends"). An arrow or a
  // line of combat is drawn between things at rest: it waits for the spell or ability it
  // leaves to land. The line from an ability to its source does not: it stretches as the
  // ability lifts. The marks of one beat come up together, their delay after its turn comes.
  if (act.verb === "point" || act.verb === "mark") {
    // What says "this, at that" is read before it goes: its end waits until it has stood
    // whole for the end's delay, and then nothing waits between its going and what it
    // pointed at moving (UI Conventions, Verbs, point). The line to a source says only where
    // an ability came from, which its lift has shown: its end waits its own delay from the
    // beat's start, the ability staying that moment after what it did, and the two then go
    // together.
    const led = act.verb === "point";
    if (act.span === "end") {
      const read = led && act.arrow !== "source";
      return read
        ? { ...own, delay: 0, hold: own.delay, with: true }
        : { ...own, with: led };
    }
    const waits = led && act.arrow !== "source";
    const after = waits
      ? [act.from, act.to].flatMap((end) => {
          const name = namedBy(end);
          return name === "you" || name === "opponent" ? [] : [name];
        })
      : [];
    return { ...own, with: led, after };
  }
  // An announcement passes over the table while play goes on under it (UI Conventions, Verbs,
  // announce; Arena's cards move under its turn banner, `IEXcjBn3BXk 19:35.7`): its beat does
  // not wait for it. A coin is waited for: what follows it is what it decided. And it has
  // the table to itself: it is thrown only once the turn's banner has gone, so that its
  // flight is all that moves mid-table (Arena's outlasts its banner, `WCknTZNbraE 17:22.13`).
  if (act.verb === "announce") {
    const coin = act.notice.kind === "coin";
    return { ...own, passes: !coin, alone: coin };
  }
  // A row opens for the permanent that appears in it before that is seen: it has made room
  // by the end of the delay (Arena's token is whole in its own slot from its first frame,
  // `UrGalw_f35w 17:38.63`). A hand dealt has no neighbours to move.
  if (act.verb === "appear" && act.object.zone === "battlefield") {
    return { ...own, opens: true };
  }
  // The blow a creature deals back is one with the strike at it, the step `before`.
  if (meets(act, before)) return { ...own, with: true };
  // The life a streak of damage takes from a player goes as the streak lands on them.
  if (
    act.verb === "count" &&
    act.tally.kind === "life" &&
    before?.verb === "streak" &&
    before.stream.kind === "damage" &&
    before.to.kind === "player" &&
    before.to.player === act.tally.player
  ) {
    return { ...own, delay: timingOf(before, table).duration, with: true };
  }
  // The life a strike takes from a player goes as its blow lands on them, not its own delay
  // after the strike's turn: the total flares on the frame the creature reaches it.
  if (
    act.verb === "count" &&
    act.tally.kind === "life" &&
    before?.verb === "strike" &&
    before.to.kind === "player" &&
    before.to.player === act.tally.player
  ) {
    const lands = STRIKE.hit * timingOf(before, table).duration;
    return { ...own, delay: lands, with: true };
  }
  // A look at several cards of a library takes them off it one after another, each as long
  // as one alone (Arena's leave about 0.5 s apart: the data's stagger).
  const look =
    act.verb === "lift" &&
    act.span === "begin" &&
    "handling" in act &&
    act.handling;
  const count = look && look.zone === "library" ? (look.count ?? 0) : 0;
  if (count > 1 && own.duration > 0) {
    const duration = own.duration + (count - 1) * own.stagger;
    return { ...own, duration, each: own.duration };
  }
  // What follows a countered spell in its beat, the spell that countered it, waits until it
  // is gone, which is as its step ends (UI Conventions, "A countered spell": "It leaves
  // before the spell that countered it").
  if (found === "countered") return { ...own, stagger: own.duration };
  return found === "exiled" ? { ...own, stagger: BATCH_EXILE } : own;
}

/** One of the effects a verb's steps begin, as the data says it: at which end of the step or
 * as its blow lands, of which steps, those of a case or those that leave their card in a
 * zone, and on what, where that is not what the step is about: what it lands on (`struck`),
 * or what a streak leaves (`source`). */
interface Use {
  effect: string;
  when: string;
  case?: string;
  into?: string;
  on?: string;
}

const looks = new Map(
  conventions.effects.map((effect) => [
    effect.name,
    { layer: effect.layer as Layer, ...effect.values },
  ]),
);

const uses = new Map(
  conventions.verbs.map((verb): [string, Use[]] => [
    verb.name,
    ("effects" in verb && verb.effects) || [],
  ]),
);

/** The effects the step `act` begins, which finds the table `table`: its verb's that fit it,
 * and only its case's own where the case has any (a card that leaves for exile is lost in
 * light, and does not break up besides). */
export function effectsOf(act: Act, table?: PlayerView): Effect[] {
  const found = caseOf(act, table);
  const into = "object" in act ? act.object?.zone : undefined;
  const fit = (uses.get(act.verb) ?? []).filter(
    (use) =>
      (use.case === undefined || use.case === found) &&
      (use.into === undefined || use.into === into),
  );
  const own = fit.filter((use) => use.case !== undefined);
  // What a strike or a streak lands on: a card, an ability, or a player's life total. And
  // what a streak leaves, where that is something on the table and no player's pool.
  const hits = act.verb === "strike" || act.verb === "streak";
  const struck = hits ? namedBy(act.to) : undefined;
  const from = act.verb === "streak" ? act.from : undefined;
  const source = from && from.kind !== "player" ? namedBy(from) : undefined;
  const elsewhere: Record<string, On | undefined> = { struck, source };
  return (own.length > 0 ? own : fit).flatMap((use): Effect[] => {
    const look = looks.get(use.effect);
    const on = use.on === undefined ? undefined : elsewhere[use.on];
    if (!look || (use.on !== undefined && on === undefined)) return [];
    return [
      {
        name: use.effect,
        layer: look.layer,
        when: use.when === "end" ? "end" : "start",
        on,
        into: use.when === "blow" ? STRIKE.hit : undefined,
        duration: look.duration ?? undefined,
        delay: look.delay ?? 0,
        ease: CURVES[look.curve] ?? SNAP.ease,
      },
    ];
  });
}

/** Every step at once: reduced motion. */
export const snap = (): Timing => SNAP;

/** How a held card let go travels back to where it rests: as a card flies. */
export const RELEASE: Timing = verbs.get("fly") ?? SNAP;
