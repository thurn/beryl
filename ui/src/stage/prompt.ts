// The prompt on the stage (plan K26). The session holds a prompt's state: what may be picked,
// what is picked, what each button says and whether it can be pressed. This draws that state
// and sends the session what the player did: a click on a card, an ability or a player, a
// press of a button, a card dragged out of the hand. Nothing here works out what a click
// means or what is allowed; the reply to a click is the prompt's next state, drawn as it is,
// or the word that the click answered, after which the game's next views arrive as any do.
//
// The marks a prompt puts on the table are the stage's own spans (spans.ts), begun and ended
// by `mark` steps played as scripts of their own between the game's, as the session's scripts
// would draw them: a prompt that opens begins its marks, each reply ends those gone and
// begins those new, and the marks left end before the next view's script plays. An arrow runs
// from what asks for a target to each target picked while the prompt waits on a button, and
// the script that then casts at it takes the arrow over where it draws the same one. A
// declaration's lines are drawn the same way, one from each attacker or blocker to what the
// session has it aimed at, from the click that aims it: the script that plays the declaration
// begins the same lines, so each is one line from that click until the script that ends it
// after the damage. A permanent picked to pay a cost carries its mark until the script has
// tapped it, so that it lies turned from the pick on and is not seen to turn twice.
//
// The cards a prompt lays out off the table (the top of a library to sort, a graveyard to
// pick from) are the stage's own nodes too, held up in a tray by a `lift` of the stage's own
// (table.ts `Hold`): begun as the prompt opens with the piles the session sent, begun again
// as each reply sorts them, and ended with the first steps of the script that plays what the
// answer did, so that a card kept flies from where it was held up and the rest go home.
// A click on a card in a tray is a click on that card; one carried from pile to pile of a
// split is a drop, where the layout says it landed (`lands`). An order's items are waiting
// triggers, which the table has no node of: the screen draws them (Tray.tsx) and sends each
// move through `drop`.
//
// A prompt answered by its buttons alone (a yes or no, a choice among a few answers) holds up
// the cards it asks about in the same tray. The choice among the ways to use a card, which a
// click on the card opens over a priority, shows the card once for each way instead
// (Choose.tsx, `waysOf`): Arena's "Choose One". A number is set, a share of a division given
// and an entry of a catalog chosen by controls the screen draws from the prompt's state
// (PromptBar.tsx, Choose.tsx), which send what was pressed through `number` and `entry`.
//
// The stage is asked through this for what feeds it (feed.ts), so that no view's script
// begins under an old prompt's marks, and its pointer has this for its one listener: the
// chrome around the table hears through `listen` what the prompt had no use for.
import type {
  Arrow,
  ButtonId,
  Click,
  ClickReply,
  DecisionId,
  DecisionView,
  PlayerView,
  PromptButton,
  PromptView,
} from "../generated/protocol";
import { isRevealed, type Name, type Point } from "./layout.ts";
import type { Drop, PointerEvents } from "./pointer.ts";
import { keyOf, namedBy, type SpanAct } from "./spans.ts";
import type { ScriptStage } from "./stage.ts";
import type { Script, Step, Tray } from "./table.ts";
import { subjectOf as aboutOf } from "./tracks.ts";
import type { Landing } from "./tray.ts";

type Subject = Extract<Click, { kind: "on" }>["on"];

/** The stage as a prompt uses it. */
export type Prompted = Pick<
  ScriptStage,
  "play" | "skip" | "shown" | "subscribe" | "pointer"
>;

/** Sends the session a click on the prompt of `decision` in its state `revision` (ipc.ts). */
export type Send = (
  decision: DecisionId,
  revision: number,
  click: Click,
) => Promise<ClickReply>;

/** What the screen draws of the prompt: a new one whenever any of it changes. */
export interface Asked {
  /** The open decision whose prompt the stage holds, and that prompt as the session last
   * sent it. None while the game plays on, and for a prompt answered elsewhere. */
  decision?: DecisionView;
  prompt?: PromptView;
  /** A click is with the session: its reply has not come. */
  sending: boolean;
  /** A click answered the prompt, and the view it answered into has not come. */
  answered: boolean;
  /** Why the last click failed, if it did. */
  error?: unknown;
}

const IDLE: Asked = { sending: false, answered: false };

/** The purposes the opening screen answers (screens/Opening.tsx), over a table that shows no
 * dock: who plays first, and whether to keep a hand. */
const OPENING = new Set(["play-or-draw", "mulligan"]);

/** Whether the stage holds `prompt`: every shape the session holds the state of, but for
 * what the opening screen asks. */
export function onTable(prompt: PromptView) {
  return prompt.state !== null && !OPENING.has(prompt.purpose);
}

/** The ways to use a card that `prompt` offers, where it is the choice among them a click on
 * the card opened: each is drawn as the card or its ability (Choose.tsx), not as a button of
 * the bar. None for any other prompt. */
export function waysOf(prompt: PromptView | undefined): PromptButton[] {
  if (prompt?.state?.shape !== "mode" || prompt.purpose !== "act") return [];
  return prompt.buttons.filter(({ id }) => id.kind === "option");
}

/** The cards `prompt` holds up off the table, as the session lists them: a pick's tray, a
 * split's two piles, each laid out for as many cards as it must come to hold, or the cards a
 * yes or no or a choice among answers is about (the card whose ways are chosen among is
 * shown once a way instead). */
export function trayOf(prompt: PromptView | undefined): Tray | undefined {
  const state = prompt?.state;
  if (state?.shape === "pick" && state.tray.length > 0) {
    return { piles: [{ cards: state.tray }] };
  }
  const asks = state?.shape === "yesNo" || state?.shape === "mode";
  if (asks && state.about.length > 0 && waysOf(prompt).length === 0) {
    return { piles: [{ cards: state.about }] };
  }
  if (state?.shape !== "split") return undefined;
  return {
    piles: state.piles.map(({ cards, exactly }) => ({
      cards,
      ...(exactly === null ? {} : { room: exactly }),
    })),
  };
}

const sameTray = (a: Tray | undefined, b: Tray | undefined) =>
  JSON.stringify(a) === JSON.stringify(b);

/** `script` with the cards of `tray` let go as its first steps about any of them begin, or
 * with none about them, as its first steps do. */
function lowered(script: Script, tray: Tray): Script {
  const held = new Set<Name>(tray.piles.flatMap((pile) => pile.cards));
  const about = script.beats.findIndex((beat) =>
    beat.some(({ act }) => {
      const subject = aboutOf(act);
      return subject !== undefined && held.has(subject);
    }),
  );
  const at = Math.max(0, about);
  const end: Step = { act: { verb: "lift", span: "end", tray } };
  const beats = script.beats.length > 0 ? [...script.beats] : [[]];
  beats[at] = [end, ...beats[at]!];
  return { ...script, beats };
}

/** `script` with the marks `paying` ended once it has tapped what they are on: in the beat
 * after the last that taps one of them, or in one more. */
function paid(script: Script, paying: readonly SpanAct[]): Script {
  if (paying.length === 0) return script;
  const marked = new Set(paying.map(keyOf));
  const taps = ({ act }: Step) =>
    act.verb === "tap" && marked.has(`paying ${act.object.slot}`);
  let after = 0;
  for (const [i, beat] of script.beats.entries()) {
    if (beat.some(taps)) after = i + 1;
  }
  const ends = paying.map((act) => ({
    act: { ...act, span: "end" as const },
  }));
  const beats = [...script.beats];
  beats[after] = [...ends, ...(beats[after] ?? [])];
  return { ...script, beats };
}

/** What a click on the node `name` is on. */
export const subjectOf = (name: Name): Subject =>
  typeof name === "number"
    ? { kind: "object", slot: name }
    : { kind: "ability", id: Number(name.slice(1)) };

const sameDecision = (a?: DecisionId, b?: DecisionId) =>
  a !== undefined && a.session === b?.session && a.seq === b.seq;

const AIMED: Partial<Record<string, Arrow>> = {
  "friendly-target": "friendly-target",
  "hostile-target": "hostile-target",
};

export class Prompts {
  readonly #stage: Prompted;
  readonly #send: Send;
  /** Called as a click is taken up: one that changed the prompt or answered it. */
  heard: () => void = () => {};
  /** Which nodes a click on is the prompt's: all, until a board says otherwise (a phone's
   * hand is read by a touch and played by a drag). */
  clicks: (name: Name) => boolean = () => true;
  /** Where in `tray` a card let go at `at` lands, as the board's layout has it laid out:
   * nowhere, until a board says. */
  lands: (tray: Tray, at: Point) => Landing | undefined = () => undefined;
  /** The view last played out, whose prompt this has opened or let be. */
  #view: PlayerView | undefined;
  #asked: Asked = IDLE;
  /** The spans this has begun on the stage and not ended, by what each is. */
  readonly #drawn = new Map<string, SpanAct>();
  /** The tray this has lifted on the stage and not let go. */
  #up: Tray | undefined;
  /** All the stage is playing is a script of marks. */
  #marking = false;
  /** A card waits where a drop left it for what its click did. */
  #carried = false;
  #shut = false;
  readonly #listeners = new Set<() => void>();
  readonly #pointing = new Set<PointerEvents>();

  constructor(stage: Prompted, send: Send) {
    this.#stage = stage;
    this.#send = send;
    stage.subscribe(this.#played);
    stage.pointer.holdable = (name) => this.#lifts(name);
    stage.pointer.listen({
      hover: (name) => this.#tell((to) => to.hover?.(name)),
      lift: (name) => this.#tell((to) => to.lift?.(name)),
      click: (name, at) => this.#clicked(name, at),
      drop: (drop) => this.#dropped(drop),
    });
  }

  /** What the screen draws of the prompt now. */
  readonly asked = () => this.#asked;

  /** Calls `listener` whenever `asked` is replaced, and `stage.shown` (feed.ts); returns how
   * to stop. */
  readonly subscribe = (listener: () => void) => {
    this.#listeners.add(listener);
    const unsubscribe = this.#stage.subscribe(listener);
    return () => {
      this.#listeners.delete(listener);
      unsubscribe();
    };
  };

  /** Tells `events` what the pointer does that is not the prompt's: every hover and lift, and
   * a click the prompt had no use for; returns how to stop. */
  listen(events: PointerEvents) {
    this.#pointing.add(events);
    return () => {
      this.#pointing.delete(events);
    };
  }

  get shown() {
    return this.#stage.shown;
  }

  /** Gives the stage `view` and its script to play (`ScriptStage.play`), the open prompt's
   * marks ended first: but for the open decision shown again, as a change of stops shows
   * it, whose prompt stays as it is. */
  play(view: PlayerView, sent?: Script) {
    let script = sent;
    // A script of marks alone is not waited out: the game's own steps begin now.
    if (this.#marking) this.#stage.skip();
    const before = this.#stage.shown.view;
    const next = view.status.kind === "deciding" && view.status.decision;
    const again = next && sameDecision(next.id, this.#asked.decision?.id);
    if (!script || script.reset) {
      // The stage starts over, with no span and nothing held up.
      this.#drawn.clear();
      this.#up = undefined;
    } else if (!again && before) {
      // An arrow the script draws itself is the script's from here on.
      const begun = new Set(
        script.beats
          .flat()
          .flatMap(({ act }) =>
            act.verb === "point" && act.span === "begin" ? [keyOf(act)] : [],
          ),
      );
      // What the script taps keeps the mark that turned it until it has.
      const taps = new Set(
        script.beats
          .flat()
          .flatMap(({ act }) =>
            act.verb === "tap" ? [`paying ${act.object.slot}`] : [],
          ),
      );
      const kept = (act: SpanAct) => taps.has(keyOf(act));
      const over = [...this.#drawn]
        .filter(([key]) => !begun.has(key))
        .map(([, act]) => act);
      const ends = over
        .filter((act) => !kept(act))
        .map((act) => ({ act: { ...act, span: "end" as const } }));
      this.#drawn.clear();
      if (ends.length > 0) this.#stage.play(before, { beats: [ends] });
      script = paid(script, over.filter(kept));
      if (this.#up) script = lowered(script, this.#up);
      this.#up = undefined;
    }
    if (!again) this.#set(IDLE);
    this.#stage.play(view, script);
  }

  skip() {
    this.#stage.skip();
  }

  /** Draws no prompt from here on, and ends the marks of the one open: for a capture of the
   * stage's own motion (ui/scripts/verbs.mjs), which answers through the transport and
   * writes the marks it measures itself. */
  shut() {
    this.#shut = true;
    this.#set(IDLE);
    this.#mark();
  }

  /** A click on what the stage has no node of: a player, whose life total the chrome draws. */
  on(subject: Subject) {
    this.#click({ kind: "on", on: subject });
  }

  /** Presses the button `button` of the open prompt. */
  press(button: ButtonId) {
    this.#click({ kind: "button", button });
  }

  /** Drops `on` into the pile `pile` of the open prompt, before what lies at `at` there:
   * for what the stage has no node of, an order's waiting trigger. */
  drop(on: Subject, pile: number, at: number) {
    this.#click({ kind: "drop", on, pile, at });
  }

  /** Sets the open number prompt to `n`, or with `on`, gives that recipient of the open
   * division the share `n`. */
  number(n: number, on: Subject | null = null) {
    this.#click({ kind: "number", n, on });
  }

  /** Chooses the entry at `index` of the open catalog. */
  entry(index: number) {
    this.#click({ kind: "entry", index });
  }

  /** A view played out, or the stage went idle: the prompt of a view newly shown opens. */
  readonly #played = () => {
    const { view, busy } = this.#stage.shown;
    if (!busy) this.#marking = false;
    // More is queued behind it: the view after it says what is asked.
    if (view === this.#view || busy) return;
    this.#view = view;
    const decision =
      view?.status.kind === "deciding" ? view.status.decision : undefined;
    const held =
      !this.#shut &&
      decision &&
      (onTable(decision.prompt) ||
        sameDecision(decision.id, this.#asked.decision?.id));
    this.#set(held ? { ...IDLE, decision, prompt: decision.prompt } : IDLE);
    this.#mark();
  };

  #set(asked: Asked) {
    if (asked === this.#asked) return;
    this.#asked = asked;
    for (const listener of [...this.#listeners]) listener();
  }

  #tell(each: (to: PointerEvents) => void) {
    for (const to of [...this.#pointing]) each(to);
  }

  /** The tray held up, if it has piles to carry a card between and `name` is in one. */
  #piled(name: Name): Tray | undefined {
    const tray = this.#up;
    const piled = tray && tray.piles.length > 1;
    return piled &&
      tray.piles.some((pile) => pile.cards.includes(name as number))
      ? tray
      : undefined;
  }

  /** Whether a press may pick the node `name` up: a card the prompt marks, in a hand or in
   * a tray of piles. */
  #lifts(name: Name) {
    const { prompt, sending } = this.#asked;
    if (!prompt || sending) return false;
    const resting = this.#stage.shown.resting.get(name);
    return (
      (resting?.dress.zone === "hand" || this.#piled(name) !== undefined) &&
      prompt.marks.some(({ on }) => namedBy(on) === name)
    );
  }

  #clicked(name: Name, at: Point) {
    const elsewhere = () => this.#tell((to) => to.click?.(name, at));
    // A face in the Revealed Cards pile is no prompt's: a click on it opens the pile.
    if (isRevealed(name)) return elsewhere();
    // A card held up is the prompt's to click, wherever it came from.
    const held = this.#up?.piles.some((pile) =>
      pile.cards.includes(name as number),
    );
    if (!this.#asked.prompt || !(held || this.clicks(name))) return elsewhere();
    this.#click({ kind: "on", on: subjectOf(name) }, elsewhere);
  }

  /** A card carried out of the hand is used as a click on it uses it; let go over where it
   * rests, it goes back. One carried in a tray of piles is dropped where it lands, and goes
   * back let go outside every pile. A drop taken waits where it was let go for what the
   * click did. */
  #dropped({ name, at }: Drop) {
    const rests = this.#stage.shown.resting.get(name);
    if (!rests || !this.#asked.prompt) return false;
    const tray = this.#piled(name);
    const on = subjectOf(name);
    const landed = tray && this.lands(tray, at);
    if (tray ? !landed : at.y >= rests.pose.y) return false;
    this.#carried = this.#click(
      landed ? { kind: "drop", on, ...landed } : { kind: "on", on },
    );
    return this.#carried;
  }

  /** Sends `click` on the open prompt, and takes its reply for the prompt's state; `unused`
   * is called if the reply is the state the click was made on. Returns whether it was sent:
   * one click is with the session at a time. */
  #click(click: Click, unused?: () => void): boolean {
    const { decision, prompt, sending } = this.#asked;
    if (!decision || !prompt || sending) return false;
    this.#set({ decision, prompt, sending: true, answered: false });
    const open = () => sameDecision(decision.id, this.#asked.decision?.id);
    this.#send(decision.id, prompt.revision, click).then(
      (reply) => {
        // A view that arrived meanwhile closed the prompt: the reply is of no state shown.
        if (!open()) return;
        if (reply.kind === "answered") {
          this.heard();
          // Its marks stay until the view it answered into ends them, and a card carried
          // out of the hand waits for that view's script where it was let go.
          this.#set({ sending: false, answered: true });
          return;
        }
        if (reply.prompt.revision === prompt.revision) unused?.();
        else this.heard();
        this.#set({ ...IDLE, decision, prompt: reply.prompt });
        this.#mark();
      },
      (error: unknown) => {
        if (open()) this.#set({ ...IDLE, decision, prompt, error });
      },
    );
    return true;
  }

  /** Plays the steps that make the stage's marks and arrows those of the prompt as it is
   * now: the ends of what it no longer has, then the beginnings of what is new. A card that
   * waits where a drop left it goes home as a script ends, so one is played for it even of
   * no step. */
  #mark() {
    // The marks still coming up are whole at once: the next ones follow them.
    if (this.#marking) this.#stage.skip();
    const { decision, prompt } = this.#asked;
    const wanted = new Map<string, SpanAct>();
    const resting = this.#stage.shown.resting;
    const from = decision?.source ?? null;
    for (const { on, mark } of prompt?.marks ?? []) {
      const marked: SpanAct = { verb: "mark", span: "begin", mark, on };
      wanted.set(keyOf(marked), marked);
      // From the spell or ability that asks, to a target picked: one the table shows an
      // arrow to once it is cast, a permanent or a player.
      const arrow = AIMED[mark];
      const there =
        on.kind === "player" ||
        (on.kind === "object" &&
          resting.get(on.slot)?.dress.zone === "battlefield");
      if (!arrow || from === null || !resting.has(from) || !there) continue;
      const source: Subject = { kind: "object", slot: from };
      const aimed: SpanAct = {
        verb: "point",
        span: "begin",
        arrow,
        from: source,
        to: on,
      };
      wanted.set(keyOf(aimed), aimed);
    }
    // A declaration's line, from each attacker or blocker to what it is aimed at, where the
    // session says one is drawn: the script draws the same once it is declared.
    const state = prompt?.state;
    const declared =
      state?.shape === "declare" && state.lined ? state : undefined;
    for (const { from, to } of declared?.pairs ?? []) {
      const source: Subject = { kind: "object", slot: from };
      const { arrow } = declared!;
      const aimed: SpanAct = {
        verb: "point",
        span: "begin",
        arrow,
        from: source,
        to,
      };
      wanted.set(keyOf(aimed), aimed);
    }
    const ends: Step[] = [...this.#drawn]
      .filter(([key]) => !wanted.has(key))
      .map(([, act]) => ({ act: { ...act, span: "end" as const } }));
    const begins: Step[] = [...wanted]
      .filter(([key]) => !this.#drawn.has(key))
      .map(([, act]) => ({ act }));
    this.#drawn.clear();
    for (const [key, act] of wanted) this.#drawn.set(key, act);
    const { view, busy } = this.#stage.shown;
    // The cards held up are lifted as the prompt has them now, or let go with it.
    const tray = trayOf(prompt);
    const up = this.#up;
    const sorted = up !== undefined;
    const held: Step[] = sameTray(tray, up)
      ? []
      : tray
        ? [{ act: { verb: "lift", span: "begin", tray, sorted } }]
        : [{ act: { verb: "lift", span: "end", tray: up! } }];
    const steps = [...ends, ...held, ...begins];
    if (!view || busy || (steps.length === 0 && !this.#carried)) return;
    this.#up = tray;
    this.#carried = false;
    this.#marking = true;
    this.#stage.play(view, { beats: [steps] });
  }
}
