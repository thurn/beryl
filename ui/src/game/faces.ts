// What a card in a PlayerView looks like: its frame data from data/cards.json, with the
// battlefield's current power, toughness and loyalty.
import type { CardFace as FrameFace, Color } from "../components/Card";
import type { AbilityIcon } from "../components/Mana";
import { useCardArt, type CardArt } from "../cards";
import type { ObjectView } from "../generated/protocol";

const ABILITIES: [RegExp, AbilityIcon][] = [
  [/^Flying\b/m, "flying"],
  [/^First strike\b/m, "first-strike"],
  [/^Vigilance\b/m, "vigilance"],
];

/** The frame for object `o`, from its card art (useCardArt). */
export function frameFace(o: ObjectView, art: CardArt): FrameFace {
  const face = art.face;
  const onField = o.zone === "battlefield";
  const loyalty = o.counters.find((c) => c.kind === "loyalty")?.count;
  const text = face.oracle_text ?? "";
  return {
    name: face.name,
    typeLine: face.type_line,
    colors: face.colors.filter((c): c is Color => "WUBRG".includes(c)),
    manaCost: face.mana_cost || undefined,
    text: text || undefined,
    power: onField ? o.pt?.power.toString() : face.power,
    toughness: onField ? o.pt?.toughness.toString() : face.toughness,
    loyalty: onField
      ? loyalty === undefined
        ? undefined
        : String(loyalty)
      : face.loyalty,
    art: art.src,
    ability: ABILITIES.find(([pattern]) => pattern.test(text))?.[1],
  };
}

/** A card the data does not name (it should not happen): its name on a blank frame. */
export const unknownFace = (name: string): FrameFace => ({
  name,
  typeLine: "",
  colors: [],
});

/** The frame for object `o`, as the game draws it. */
export function useFace(o: ObjectView): FrameFace {
  const art = useCardArt(o.name ?? "");
  return art ? frameFace(o, art) : unknownFace(o.name ?? "");
}

/** The frame an ability named `name` shows on the stack: its source's, from the card data by
 * name, since the source may have left its zone. */
export function useAbilityFace(
  name: string,
  source: ObjectView | undefined,
  blank: ObjectView,
): FrameFace {
  const art = useCardArt(name);
  return art ? frameFace(source ?? blank, art) : unknownFace(name);
}

/** The paragraph of a card's rules text that says one of its abilities: the `nth` triggered
 * ability ("When", "Whenever", "At", or a keyword whose reminder text says one, as offspring
 * and impending do) or activated one (a cost and a colon, loyalty abilities included),
 * counting as the engine lists them; the whole text when no paragraph matches, as for the
 * second of two triggers one paragraph says ("Whenever this enchantment enters or
 * attacks"). */
export function abilityText(
  text: string,
  triggered: boolean,
  nth: number,
): string {
  const kind = triggered
    ? /^(When|Whenever|At)\b|\(.*\b([Ww]hen|[Ww]henever|At the beginning)\b/
    : /^[^(]*:/;
  return text.split("\n").filter((p) => kind.test(p))[nth] ?? text;
}
