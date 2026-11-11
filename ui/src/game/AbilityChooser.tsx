// What the mode chooser (game/ModeChooser.tsx) offers. A click on a permanent with two or more
// activated abilities (other than mana abilities, which the payment taps for itself) always
// opens it, even when only one can be activated now, so that activating is always two clicks,
// card then ability, whichever it is (never a drag, which reads as moving the card). A card off
// the battlefield with two or more uses (casts by different costs, a land played, cycling)
// opens it too, on a click or a drop. Pure functions of the acts.
import { faceEntry } from "../cards";
import type { ObjectView } from "../generated/protocol";
import type { Act } from "./decisions";
import { abilityParagraph, withoutReminder } from "./faces";

/** One ability of the chooser: its paragraph of the card's rules text, and its candidate if it
 * can be activated now. */
export interface AbilityChoice {
  at: number;
  text: string;
  act?: Act;
}

/** A mana ability's paragraph: it adds mana ([CR 605.1a](https://yawgatog.com/resources/magic-rules/#R6051a):
 * "An activated ability is a mana ability if it meets all of the following criteria: it doesn't
 * require a target ..., it could add mana to a player's mana pool when it resolves, it's not a
 * loyalty ability ..., and its cost and effect don't move any card to or from a library"). */
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

/** A use of a card off the battlefield, as the mode chooser shows it: a cast or a land played
 * shows the card; an ability activated from where it lies (cycling) shows its paragraph. */
export interface PlayChoice {
  act: Act;
  /** The ability's paragraph, for an activation. */
  text?: string;
}

/** The uses a click on a card off the battlefield chooses among: two or more, each a cast, a
 * land played or an activation of its own ability; otherwise none. */
export function playChoices(
  o: ObjectView | undefined,
  acts: Act[],
): PlayChoice[] | undefined {
  if (!o?.name || o.zone === "battlefield" || acts.length < 2) return;
  const uses = acts.every(
    (a) =>
      a.option.kind === "cast" ||
      a.option.kind === "playLand" ||
      (a.option.kind === "activate" && !a.option.granted),
  );
  if (!uses) return;
  const text = faceEntry(o.name)?.oracle_text ?? "";
  const paragraphs = text.split("\n");
  return acts.map((act) => {
    if (act.option.kind !== "activate") return { act };
    const at =
      act.nth === null ? undefined : abilityParagraph(text, false, act.nth);
    // An activation off the battlefield is never a mana ability, which a land's paragraph
    // counted among its abilities may be.
    if (at !== undefined && !MANA_ABILITY.test(paragraphs[at]!)) {
      return { act, text: withoutReminder(paragraphs[at]!) };
    }
    // A keyword ability ("Cycling {2}") says its cost, colon and effect in its reminder text:
    // "{2}, Discard this card: Draw a card." (A land's mana reminder is no such ability.)
    const reminder = paragraphs
      .filter((p) => !ACTIVATED.test(p))
      .map((p) => /\(([^)]*:[^)]*)\)/.exec(p)?.[1])
      .find((r) => r !== undefined && !MANA_ABILITY.test(r));
    return { act, text: reminder ?? act.label };
  });
}
