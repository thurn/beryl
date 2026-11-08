// Which of the viewer's own tapped lands an "untap up to N lands" picks to begin with: those
// whose mana best lets them cast what they hold in the time the untap leaves them, as an
// end-step untap leaves the opponent's turn for instants. A pure function of the view and the card data, which it is given (`faceEntry` in the app),
// so the ui-test can run it under Node.
import type { CardFace } from "../cards";
import type { ObjectView, PlayerView } from "../generated/protocol";

/** A card's printed face by the name the engine gives it. */
export type FaceOf = (name: string) => CardFace | undefined;

type Color = "W" | "U" | "B" | "R" | "G";

/** The colors a land's mana may be: the colored symbols its "Add" abilities name, a basic
 * land type's among them, which the card data gives as reminder text. */
export function landColors(faceOf: FaceOf, name: string | null): Set<Color> {
  const face = name ? faceOf(name) : undefined;
  const colors = new Set<Color>();
  for (const [, add] of (face?.oracle_text ?? "").matchAll(/Add ([^.]*)/g)) {
    for (const [, c] of add!.matchAll(/\{([WUBRG])\}/g)) colors.add(c as Color);
  }
  return colors;
}

/** A mana cost as its colored symbols, each one color (a hybrid's choices among them), and
 * its total mana value. */
function costOf(cost: string): { pips: Color[][]; total: number } {
  const pips: Color[][] = [];
  let total = 0;
  for (const [, symbol] of cost.matchAll(/\{([^}]+)\}/g)) {
    const n = Number(symbol);
    if (Number.isInteger(n)) {
      total += n;
      continue;
    }
    if (symbol === "X") continue;
    total += 1;
    const colors = [...symbol!.matchAll(/[WUBRG]/g)].map((m) => m[0] as Color);
    if (colors.length > 0) pips.push(colors);
  }
  return { pips, total };
}

/** Whether `sources`, one mana each of any of its colors, pay `cost`: each colored symbol
 * from a source of its own, and the rest from any. */
function pays(cost: string, sources: Set<Color>[]): boolean {
  const { pips, total } = costOf(cost);
  if (sources.length < total) return false;
  const used = sources.map(() => false);
  const assign = (i: number): boolean => {
    if (i === pips.length) return true;
    for (let s = 0; s < sources.length; s++) {
      if (used[s] || !pips[i]!.some((c) => sources[s]!.has(c))) continue;
      used[s] = true;
      if (assign(i + 1)) return true;
      used[s] = false;
    }
    return false;
  };
  return assign(0);
}

/** Every way of choosing `k` of `items`, in order. */
function choose<T>(items: T[], k: number): T[][] {
  if (k === 0) return [[]];
  return items.flatMap((item, i) =>
    choose(items.slice(i + 1), k - 1).map((rest) => [item, ...rest]),
  );
}

/** The viewer's own lands among `eligible` to untap to begin with, at most `max`: those that,
 * with their untapped lands, cast the most of the cards in their hand they could cast before
 * their next untap step (instants and flash, or anything at all in their own main phase);
 * then those making the most colors, a dual land over a basic; then the first. */
export function untapChoice(
  view: PlayerView,
  eligible: Iterable<number>,
  max: number,
  faceOf: FaceOf,
): number[] {
  const objects = new Map(view.objects.map((o) => [o.slot, o]));
  const own = [...eligible]
    .filter((slot) => objects.get(slot)?.controller === "you")
    .sort((a, b) => a - b);
  const k = Math.min(max, own.length);
  if (k === own.length) return own;
  const colors = new Map(
    own.map((slot) => [slot, landColors(faceOf, objects.get(slot)!.name)]),
  );
  const untapped = view.objects
    .filter(
      (o) =>
        o.zone === "battlefield" &&
        o.controller === "you" &&
        !o.tapped &&
        o.types.includes("land"),
    )
    .map((o) => landColors(faceOf, o.name));
  const sorcerySpeed =
    view.turn.active === "you" &&
    (view.turn.step === "precombatMain" || view.turn.step === "postcombatMain");
  const costs = view.objects
    .filter((o: ObjectView) => o.zone === "hand" && o.owner === "you")
    .flatMap((o) => {
      const face = o.name ? faceOf(o.name) : undefined;
      if (!face || /\bLand\b/.test(face.type_line)) return [];
      const quick =
        /\bInstant\b/.test(face.type_line) ||
        /^flash\b/im.test(face.oracle_text ?? "");
      return quick || sorcerySpeed ? [face.mana_cost ?? ""] : [];
    });
  const score = (picks: number[]) => {
    const sources = [...untapped, ...picks.map((s) => colors.get(s)!)];
    return [
      costs.filter((cost) => pays(cost, sources)).length,
      picks.reduce((sum, s) => sum + colors.get(s)!.size, 0),
      new Set(picks.flatMap((s) => [...colors.get(s)!])).size,
    ];
  };
  let best = own.slice(0, k);
  let bestScore = score(best);
  for (const picks of choose(own, k)) {
    const s = score(picks);
    const i = s.findIndex((v, j) => v !== bestScore[j]);
    if (i >= 0 && s[i]! > bestScore[i]!) [best, bestScore] = [picks, s];
  }
  return best;
}
