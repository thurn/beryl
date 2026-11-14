export interface Stats {
  power: number;
  toughness: number;
}

/** Reads an integer, treating an explicit sign as a change when a current value exists. */
export function parse(text: string, current?: number): number | undefined {
  const entry = text.trim().replace(/^−/, "-");
  if (!/^[+-]?\d+$/.test(entry)) return undefined;
  const amount = Number(entry);
  if (!Number.isSafeInteger(amount)) return undefined;
  const relative = entry.startsWith("+") || entry.startsWith("-");
  const value = relative && current !== undefined ? current + amount : amount;
  return Number.isSafeInteger(value) ? value + 0 : undefined;
}

/** Each side of the slash can set its stat or change it by a signed amount. */
export function parseStats(text: string, current: Stats): Stats | undefined {
  const [left, right, extra] = text.split("/");
  if (left === undefined || right === undefined || extra !== undefined)
    return undefined;
  const power = parse(left, current.power);
  const toughness = parse(right, current.toughness);
  return power === undefined || toughness === undefined
    ? undefined
    : { power, toughness };
}

export function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}
