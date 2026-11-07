// The choice of a permanent's activated ability: a click on a permanent with two or more
// (other than mana abilities, which the payment taps for itself) always opens it, even when
// only one can be activated now, so that activating is always two clicks, card then ability,
// whichever it is; dragging the permanent out opens it too. Each ability is shown as the
// stack would show it, side by side mid-table, those that cannot be activated now dimmed.
import { useEffect } from "react";
import { faceEntry } from "../cards";
import { StackItem } from "../components/Stack";
import type { ObjectView } from "../generated/protocol";
import type { Act } from "./decisions";
import { abilityParagraph, useAbilityFace, withoutReminder } from "./faces";

/** One ability of the chooser: its paragraph of the card's rules text, and its candidate if it
 * can be activated now. */
export interface AbilityChoice {
  at: number;
  text: string;
  act?: Act;
}

/** A mana ability's paragraph: it adds mana ([CR 605.1a](https://yawgatog.com/resources/magic-rules/#R6051a):
 * "An activated ability is a mana ability if it meets all of the following criteria: it doesn't
 * require a target, it could add mana to a player's mana pool when it resolves, and it's not a
 * loyalty ability"). */
const MANA_ABILITY = /:\s*Add\b/;

/** An activated ability's paragraph: a cost, then a colon
 * ([CR 602.1](https://yawgatog.com/resources/magic-rules/#R6021): "Activated abilities have a
 * cost and an effect. They are written as '[Cost]: [Effect.] [Activation instructions (if
 * any).]'"). */
const ACTIVATED = /^[^(]*:/;

/** The abilities a click on `o` chooses among, when all `acts` offers are activations of its own
 * abilities and its rules text has two or more that are not mana abilities; otherwise none. */
export function abilityChoices(
  o: ObjectView | undefined,
  acts: Act[],
): AbilityChoice[] | undefined {
  if (!o?.name || o.zone !== "battlefield" || acts.length === 0) return;
  const own = acts.every(
    (a) => a.option.kind === "activate" && !a.option.granted && a.nth !== null,
  );
  const text = faceEntry(o.name)?.oracle_text;
  if (!own || !text) return;
  const paragraphs = text.split("\n");
  const abilities = paragraphs.flatMap((p, at) =>
    ACTIVATED.test(p) && !MANA_ABILITY.test(p) ? [at] : [],
  );
  if (abilities.length < 2) return;
  const byParagraph = new Map(
    acts.map((a) => [abilityParagraph(text, false, a.nth!), a]),
  );
  // An offered ability the text does not say: no chooser can show it.
  if ([...byParagraph.keys()].some((at) => at === undefined)) return;
  return abilities.map((at) => ({
    at,
    text: withoutReminder(paragraphs[at]!),
    act: byParagraph.get(at),
  }));
}

/** The ability frames' width on the stage. */
const WIDTH = 230;

export function AbilityChooser({
  object,
  choices,
  onPick,
  onClose,
}: {
  object: ObjectView;
  choices: AbilityChoice[];
  onPick(act: Act): void;
  onClose(): void;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  const name = object.name ?? "";
  const face = useAbilityFace(name, object, object);
  const label = object.types.includes("planeswalker")
    ? "Loyalty Ability"
    : "Activated Ability";
  return (
    <>
      <div className="menu-scrim chooser-scrim" onPointerDown={onClose} />
      <div className="ability-chooser" role="group" aria-label={name}>
        {choices.map((c) => (
          <button
            key={c.at}
            type="button"
            className="ability-choice"
            disabled={!c.act}
            aria-label={c.text}
            onClick={() => c.act && onPick(c.act)}
          >
            <StackItem
              source={face}
              label={label}
              text={c.text}
              width={WIDTH}
              showArt
            />
          </button>
        ))}
      </div>
    </>
  );
}
