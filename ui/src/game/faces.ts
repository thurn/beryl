// What a card in a PlayerView looks like: its frame data from data/cards.json, with the
// battlefield's current power, toughness and loyalty.
import type { CardFace as FrameFace, Color } from "../components/Card";
import type { AbilityIcon } from "../components/Mana";
import type { CardArt } from "../cards";
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
