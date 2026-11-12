// What the screen draws of a prompt that is answered off the cards on the table, besides its
// bar (PromptBar.tsx): the ways to use a card, an entry of a catalog, and each share of a
// division. All of it is the session's state as sent (plan K26): which ways there are and
// what each is called, the entries and the one chosen, each recipient's share and what is
// left to give. A press goes to the session, whose reply is drawn.
//
// The ways to use a card are Arena's "Choose One" (UI Conventions, "Choosing how to cast"):
// the card once for each way to cast or play it, by the face that way plays, and an ability
// as the stack would show it, each over what the session calls the way. These are the one
// place a card is drawn again while its own node lies where it lay, which the audits allow
// of what a mode prompt is about (ui/scripts/stage-audit.mjs). A catalog is Arena's column of
// entries (`UrGalw_f35w 45:18.5`), and a division its count on each target between the
// steps that raise and lower it (`3sEoP_s4DQE 28:32.5`), over the arrows the stage draws to
// what the prompt marks as a target (prompt.ts).
import { useSyncExternalStore } from "react";
import { landFace, cardEntry } from "../cards";
import { CARD_ASPECT } from "../components/Card";
import { RulesText } from "../components/Mana";
import { StackItem } from "../components/Stack";
import type {
  DecisionView,
  ObjectView,
  PlayerView,
  PromptButton,
} from "../generated/protocol";
import { Look } from "../game/CardLayer";
import { abilityText, BLANK, useAbilityFace } from "../game/faces";
import { waysOf, type Asked, type Prompts } from "./prompt.ts";
import type { ScriptStage } from "./stage.ts";
import type { TraySpace } from "./tray.ts";
import "./choose.css";

type Candidate = DecisionView["candidates"][number];

/** The `nth` activated ability of `object`, called `label`, as the stack will show it: its
 * source's frame, wearing the words of the ability. */
function Ability({
  object,
  nth,
  label,
  width,
}: {
  object: ObjectView;
  nth: number;
  label: string;
  width: number;
}) {
  const face = useAbilityFace(object.name ?? label, object, BLANK);
  return (
    <StackItem
      source={face}
      label="Activated Ability"
      text={abilityText(face.text ?? "", false, nth)}
      width={width}
      showArt
    />
  );
}

/** Whether the way `candidate` is shown as its card: a cast or a land played. */
const shows = ({ option }: Candidate) =>
  option.kind === "cast" || option.kind === "playLand";

/** The card `object` as the way `candidate` uses it: by the face it is cast with, or the
 * land face it is played as, and for an ability the ability. */
function Way({
  object,
  candidate,
  width,
}: {
  object: ObjectView;
  candidate: Candidate;
  width: number;
}) {
  const { option } = candidate;
  const name = object.name ?? "";
  const face =
    option.kind === "playLand"
      ? landFace(name)?.name
      : option.kind === "cast" && option.face > 0
        ? cardEntry(name)?.faces[option.face]?.name
        : undefined;
  if (!shows(candidate)) {
    return (
      <Ability
        object={object}
        nth={candidate.nth ?? 0}
        label={candidate.label}
        width={width}
      />
    );
  }
  return (
    <Look
      object={face ? { ...object, name: face } : object}
      place={{ look: "shown", width }}
      state={{}}
    />
  );
}

/** How wide each of `count` frames is drawn in a row across `space`. */
const frame = ({ card, gap, width }: TraySpace, count: number) =>
  Math.max(card.least, Math.min(card.most, (width - gap) / count - gap));

/** The ways to use the card the open prompt is about, in a row over the table in `space`,
 * where a layout holds a prompt's cards up: a press of one chooses it. An ability the card
 * cannot use now is drawn among them, dimmed and taking no press, as Arena shows a
 * planeswalker's ability its loyalty cannot pay (`2VW6ZzmS8U4 6:24.5`). Nothing for any
 * other prompt. */
export function Ways({
  asked: { prompt, decision, sending },
  prompts,
  view,
  space,
}: {
  asked: Asked;
  prompts: Prompts;
  view: PlayerView;
  space: TraySpace;
}) {
  const ways = waysOf(prompt);
  const state = prompt?.state;
  if (ways.length === 0 || state?.shape !== "mode" || !decision) return null;
  const object = view.objects.find((o) => o.slot === state.about[0]);
  const width = frame(space, ways.length);
  const drawn = (way: PromptButton) => {
    const candidate =
      way.id.kind === "option" ? decision.candidates[way.id.index] : undefined;
    return (
      <button
        key={way.label}
        type="button"
        className="stage-way"
        aria-label={way.label}
        disabled={sending || !way.enabled}
        onClick={() => prompts.press(way.id)}
      >
        {object && way.id.kind === "ability" && (
          <span
            className="stage-way-face"
            style={{ width, height: width / CARD_ASPECT }}
            aria-hidden
          >
            <Ability
              object={object}
              nth={way.id.nth}
              label={way.label}
              width={width}
            />
          </span>
        )}
        {object && candidate && (
          <span
            className="stage-way-face"
            // The card shown again: what the audits know a way's frame by.
            data-card={
              shows(candidate) ? (object.name ?? undefined) : undefined
            }
            style={{ width, height: width / CARD_ASPECT }}
            aria-hidden
          >
            <Way object={object} candidate={candidate} width={width} />
          </span>
        )}
        <span className="stage-way-says" style={{ maxWidth: width }}>
          <RulesText text={way.label} />
        </span>
      </button>
    );
  };
  return (
    <div
      className="stage-choose stage-ways"
      style={{
        left: space.x,
        top: space.y,
        width: space.width,
        height: space.height,
        gap: space.gap,
      }}
    >
      {ways.map(drawn)}
    </div>
  );
}

/** The entries of the open prompt, where it is a catalog, in a column over the table in
 * `space`: a press of one chooses it, and the one chosen is lit. Nothing for any other
 * prompt. */
export function Entries({
  asked: { prompt, sending },
  prompts,
  space,
}: {
  asked: Asked;
  prompts: Prompts;
  space: TraySpace;
}) {
  const state = prompt?.state;
  if (state?.shape !== "catalog") return null;
  return (
    <div
      className="stage-choose stage-entries"
      style={{
        left: space.x,
        top: space.y,
        width: space.width,
        height: space.height,
      }}
    >
      <ul>
        {state.entries.map((entry, index) => (
          <li key={index}>
            <button
              type="button"
              className="stage-entry"
              data-entry={index}
              aria-pressed={state.chosen === index}
              disabled={sending}
              onClick={() => prompts.entry(index)}
            >
              {entry}
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}

/** Each recipient's share of the open prompt, where it is a division, over the recipient
 * where the stage rests it: the share between the step that raises it by one and the step
 * that lowers it. The session says what a step may do: one that asks for more than is left,
 * or for less than a share may be, changes nothing. Nothing for any other prompt. */
export function Shares({
  asked: { prompt, sending },
  prompts,
  stage,
}: {
  asked: Asked;
  prompts: Prompts;
  stage: ScriptStage;
}) {
  const { resting } = useSyncExternalStore(stage.subscribe, () => stage.shown);
  const state = prompt?.state;
  if (state?.shape !== "divide") return null;
  return (
    <>
      {state.shares.map(({ on, amount }) => {
        const pose = on.kind === "object" && resting.get(on.slot)?.pose;
        if (!pose || on.kind !== "object") return null;
        const step = (by: number, label: string, off: boolean) => (
          <button
            type="button"
            className="stage-share-step"
            aria-label={label}
            disabled={sending || off}
            onClick={() => prompts.number(amount + by, on)}
          >
            {by > 0 ? "▲" : "▼"}
          </button>
        );
        return (
          <div
            key={on.slot}
            className="stage-share"
            data-share={on.slot}
            style={{
              left: pose.x + pose.width / 2,
              top: pose.y + pose.width / CARD_ASPECT / 2,
              ["--frame" as string]: `${pose.width}px`,
            }}
          >
            {step(1, "More", state.left === 0)}
            <b role="status">{amount}</b>
            {step(-1, "Less", amount === 0)}
          </div>
        );
      })}
    </>
  );
}
