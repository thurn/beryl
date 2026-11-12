// What a card in a PlayerView looks like: its frame data from its card's record, with the
// battlefield's current power, toughness and loyalty, and its marks.
import type {
  CardFace as FrameFace,
  CardMarks,
  Color,
  CounterMark,
  StatTone,
} from "../components/Card";
import type { AbilityIcon, AbilityMark } from "../components/Mana";
import { isNamelessToken } from "../cardRecords";
import {
  tokenFace,
  useCardArt,
  useNamelessTokenArt,
  type CardArt,
  type CardFace,
} from "../cards";
import type {
  AbilityRef,
  CounterView,
  Keyword,
  ObjectView,
} from "../generated/protocol";

/** The keyword abilities a frame badges, in badge order: the keyword the view names, and the
 * printed text that has it, which tells a gained one from a printed one. */
const ABILITIES: [Keyword, RegExp, AbilityIcon][] = [
  ["flying", /^Flying\b/m, "flying"],
  ["firstStrike", /^First strike\b/m, "first-strike"],
  ["vigilance", /^Vigilance\b/m, "vigilance"],
];

/** The badges for `o`: on the battlefield, the keywords it has now (a gained one lit), else
 * those its printed `text` has. */
function abilityMarks(o: ObjectView, text: string): AbilityMark[] {
  const onField = o.zone === "battlefield";
  return ABILITIES.flatMap(([keyword, printed, icon]) => {
    const has = printed.test(text);
    if (!onField) return has ? [{ icon }] : [];
    return o.keywords.includes(keyword) ? [{ icon, gained: !has }] : [];
  });
}

/** How `current` compares with a printed value, when the printed one is a number. */
export function tone(
  current: number,
  printed: string | undefined,
): StatTone | undefined {
  const p = printed === undefined || printed === "" ? NaN : Number(printed);
  if (!Number.isFinite(p) || current === p) return undefined;
  return current > p ? "up" : "down";
}

/** The face `o` was printed with, from its card face `face`: a token's own where it has one. */
export const printedFace = (o: ObjectView, face: CardFace): CardFace =>
  (o.token && o.name !== null && tokenFace(o.name)) || face;

/** The toughness drawn for `o`: less its marked damage, as Arena draws it, and never below 0,
 * as damage past lethal only destroys it (CR 704.5g: "the total damage marked on it is greater
 * than or equal to its toughness, that creature has been dealt lethal damage and is
 * destroyed"). https://yawgatog.com/resources/magic-rules/#R7045g */
const toughnessLeft = (o: ObjectView, toughness: number) =>
  String(Math.max(0, toughness - o.damage));

/** The face for object `o`, from its card's scan (useCardArt). On the battlefield a creature's
 * toughness is drawn less its marked damage (toughnessLeft), and red then. */
export function frameFace(o: ObjectView, art: CardArt): FrameFace {
  const face = art.face;
  const onField = o.zone === "battlefield";
  const loyalty = o.counters.find((c) => c.kind === "loyalty")?.count;
  const text = face.oracle_text ?? "";
  const pt = onField ? (o.pt ?? undefined) : undefined;
  const printed = printedFace(o, face);
  return {
    name: face.name,
    typeLine: face.type_line,
    colors: face.colors.filter((c): c is Color => "WUBRG".includes(c)),
    manaCost: face.mana_cost || undefined,
    text: text || undefined,
    power: onField ? pt?.power.toString() : face.power,
    toughness: onField ? pt && toughnessLeft(o, pt.toughness) : face.toughness,
    powerTone: pt && tone(pt.power, printed.power),
    toughnessTone:
      pt && (o.damage > 0 ? "down" : tone(pt.toughness, printed.toughness)),
    loyalty: onField
      ? loyalty === undefined
        ? undefined
        : String(loyalty)
      : face.loyalty,
    scan: art.src,
    abilities: abilityMarks(o, text),
  };
}

/** The kinds a permanent wears as a badge; loyalty is its plaque's. */
const BADGED: ReadonlySet<string> = new Set<CounterMark>([
  "plusOne",
  "minusOne",
  "time",
  "story",
  "chorus",
]);

const isBadged = (c: CounterView): c is { kind: CounterMark; count: number } =>
  BADGED.has(c.kind);

/** What the battlefield draws on object `o` beyond its frame. */
export const marksOf = (o: ObjectView): CardMarks => ({
  counters: o.counters.filter(isBadged),
  sick: o.summoningSick,
  token: o.token,
  stolen: o.owner !== o.controller,
});

/** A card the data does not name (it should not happen): its name on a blank frame. */
export const unknownFace = (name: string): FrameFace => ({
  name,
  typeLine: "",
  colors: [],
});

/** An object with nothing on it, for an ability whose source has left its zone. */
export const BLANK: ObjectView = {
  slot: 0,
  zone: "stack",
  owner: "you",
  controller: "you",
  name: null,
  facing: "up",
  token: false,
  tapped: false,
  summoningSick: false,
  damage: 0,
  counters: [],
  types: [],
  pt: null,
  keywords: [],
  attachedTo: null,
  exiledBy: null,
  exiledWith: null,
  combat: null,
};

/** A token with no name, a copy of a face-down permanent (CR 708.2): no card data, so its
 * types and current power and toughness on a custom frame with generic Copy art. */
const namelessFace = (o: ObjectView): FrameFace => ({
  name: "",
  typeLine:
    "Token " +
    o.types.map((t) => t.charAt(0).toUpperCase() + t.slice(1)).join(" "),
  colors: [],
  power: o.pt?.power.toString(),
  toughness: o.pt ? toughnessLeft(o, o.pt.toughness) : undefined,
  toughnessTone: o.damage > 0 ? "down" : undefined,
});

/** A face-down permanent (CR 708.2a: "a 2/2 face-down creature with no text, no name, no
 * subtypes, and no mana cost"): no name, whatever card its controller knows it is, and its
 * current power and toughness. https://yawgatog.com/resources/magic-rules/#R7082a */
export const faceDownFace = (o: ObjectView): FrameFace => ({
  ...namelessFace(o),
  typeLine: "",
});

/** The scan every card is drawn with: one size wherever it is, so each card downloads once,
 * large enough for the stack and the inspector on a Retina screen. */
export const SCAN = "large";

/** The frame for object `o`, as the game draws it. */
export function useFace(o: ObjectView): FrameFace {
  const art = useCardArt(o.name ?? "", undefined, SCAN);
  const copyArt = useNamelessTokenArt(o);
  if (isNamelessToken(o)) return { ...namelessFace(o), art: copyArt };
  if (art) return frameFace(o, art);
  return o.name === "" ? namelessFace(o) : unknownFace(o.name ?? "");
}

/** The frame an ability named `name` shows on the stack: its source's, from the card data by
 * name, since the source may have left its zone. */
export function useAbilityFace(
  name: string,
  source: ObjectView | undefined,
  blank: ObjectView,
): FrameFace {
  const art = useCardArt(name, undefined, SCAN);
  return art ? frameFace(source ?? blank, art) : unknownFace(name);
}

/** The paragraph of a card's rules text that says one of its abilities: the `nth` triggered
 * ability ("When", "Whenever", "At", or a keyword whose reminder text says one, as offspring
 * and impending do) or activated one (a cost and a colon, loyalty abilities included),
 * counting paragraphs as the session does (one that says two triggers, as "Whenever this
 * enchantment enters or attacks" does, is counted once); the whole text when no paragraph
 * matches. */
export function abilityText(
  text: string,
  triggered: boolean,
  nth: number,
): string {
  const at = abilityParagraph(text, triggered, nth);
  if (at !== undefined) return text.split("\n")[at]!;
  // A delayed triggered ability is created by another ability's words rather than printed
  // as one of the card's own (CR 603.7a: "Delayed triggered abilities are created during the
  // resolution of spells or abilities"), as a loyalty ability's "At the beginning of the
  // next end step, ..." does: its words from where they begin.
  const delayed = triggered
    ? /(?:^|[.:] )((?:At the beginning|When|Whenever)\b[^\n]*)/m.exec(text)
    : null;
  return delayed?.[1] ?? text;
}

/** The index of the paragraph abilityText finds, if one matches. */
export function abilityParagraph(
  text: string,
  triggered: boolean,
  nth: number,
): number | undefined {
  const kind = triggered
    ? /^(When|Whenever|At)\b|\(.*\b([Ww]hen|[Ww]henever|At the beginning)\b/
    : /^[^(]*:/;
  return text.split("\n").flatMap((p, i) => (kind.test(p) ? [i] : []))[nth];
}

/** The paragraph of a card's rules text that says ability `ref`: a triggered or activated one
 * as `abilityParagraph` finds it, or the `nth` static one, a paragraph that is neither and is
 * a sentence (keyword lines such as "Flying" are not). */
export function paragraphOf(text: string, ref: AbilityRef): number | undefined {
  if (ref.kind !== "static")
    return abilityParagraph(text, ref.kind === "triggered", ref.nth);
  const other = (p: string) =>
    abilityParagraph(p, true, 0) !== undefined ||
    abilityParagraph(p, false, 0) !== undefined;
  return text
    .split("\n")
    .flatMap((p, i) => (/\.\)?$/.test(p) && !other(p) ? [i] : []))[ref.nth];
}
