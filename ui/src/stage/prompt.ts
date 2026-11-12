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
// the script that then casts at it takes the arrow over where it draws the same one.
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
  PromptView,
  Script,
} from "../generated/protocol";
import type { Name, Point } from "./layout.ts";
import type { Drop, PointerEvents } from "./pointer.ts";
import { keyOf, namedBy, type SpanAct } from "./spans.ts";
import type { ScriptStage } from "./stage.ts";

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

/** Whether the stage holds `prompt`: one the session holds the state of, picked on the table
 * as it lies. The other shapes come with their own beads, and are answered over the table
 * until then. */
export const onTable = (prompt: PromptView) =>
  prompt.state?.shape === "pick" && prompt.state.tray.length === 0;

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
  /** The view last played out, whose prompt this has opened or let be. */
  #view: PlayerView | undefined;
  #asked: Asked = IDLE;
  /** The spans this has begun on the stage and not ended, by what each is. */
  readonly #drawn = new Map<string, SpanAct>();
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
  play(view: PlayerView, script?: Script) {
    // A script of marks alone is not waited out: the game's own steps begin now.
    if (this.#marking) this.#stage.skip();
    const before = this.#stage.shown.view;
    const next = view.status.kind === "deciding" && view.status.decision;
    const again = next && sameDecision(next.id, this.#asked.decision?.id);
    if (!script || script.reset) {
      // The stage starts over, with no span.
      this.#drawn.clear();
    } else if (!again && before) {
      // An arrow the script draws itself is the script's from here on.
      const begun = new Set(
        script.beats
          .flat()
          .flatMap(({ act }) =>
            act.verb === "point" && act.span === "begin" ? [keyOf(act)] : [],
          ),
      );
      const ends = [...this.#drawn]
        .filter(([key]) => !begun.has(key))
        .map(([, act]) => ({ act: { ...act, span: "end" as const } }));
      this.#drawn.clear();
      if (ends.length > 0) this.#stage.play(before, { beats: [ends] });
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

  /** Whether a press may pick the node `name` up: a card the prompt marks, in a hand. */
  #lifts(name: Name) {
    const { prompt, sending } = this.#asked;
    if (!prompt || sending) return false;
    const resting = this.#stage.shown.resting.get(name);
    return (
      resting?.dress.zone === "hand" &&
      prompt.marks.some(({ on }) => namedBy(on) === name)
    );
  }

  #clicked(name: Name, at: Point) {
    const elsewhere = () => this.#tell((to) => to.click?.(name, at));
    if (!this.#asked.prompt || !this.clicks(name)) return elsewhere();
    this.#click({ kind: "on", on: subjectOf(name) }, elsewhere);
  }

  /** A card carried out of the hand is used as a click on it uses it; let go over where it
   * rests, it goes back. A drop taken waits where it was let go for what the click did. */
  #dropped({ name, at }: Drop) {
    const rests = this.#stage.shown.resting.get(name);
    if (!rests || at.y >= rests.pose.y || !this.#asked.prompt) return false;
    this.#carried = this.#click({ kind: "on", on: subjectOf(name) });
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
    const ends = [...this.#drawn]
      .filter(([key]) => !wanted.has(key))
      .map(([, act]) => ({ act: { ...act, span: "end" as const } }));
    const begins = [...wanted]
      .filter(([key]) => !this.#drawn.has(key))
      .map(([, act]) => ({ act }));
    this.#drawn.clear();
    for (const [key, act] of wanted) this.#drawn.set(key, act);
    const { view, busy } = this.#stage.shown;
    const steps = [...ends, ...begins];
    if (!view || busy || (steps.length === 0 && !this.#carried)) return;
    this.#carried = false;
    this.#marking = true;
    this.#stage.play(view, { beats: [steps] });
  }
}
