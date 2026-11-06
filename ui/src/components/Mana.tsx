// Mana and ability symbols from the Mana font (src/design/fonts.ts). Glyph code points follow
// mana-font's mana.css.
import { Fragment, type ReactNode } from "react";

// Code points in the Mana font.
const CODES: Record<string, number> = {
  W: 0xe600,
  U: 0xe601,
  B: 0xe602,
  R: 0xe603,
  G: 0xe604,
  C: 0xe904,
  X: 0xe615,
  S: 0xe619,
  T: 0xe61a,
  Q: 0xe61b,
  E: 0xe907,
  ...Object.fromEntries(
    Array.from({ length: 16 }, (_, n) => [String(n), 0xe605 + n]),
  ),
};

export type AbilityIcon = "flying" | "first-strike" | "vigilance";

const ABILITY_CODES: Record<Exclude<AbilityIcon, "flying">, number> = {
  "first-strike": 0xe950,
  vigilance: 0xe968,
};

/** One symbol from `{...}` notation, such as `U`, `2` or `T`. */
export function ManaSymbol({ symbol }: { symbol: string }) {
  const code = CODES[symbol];
  if (code === undefined) {
    return <span className="mana-symbol mana-unknown">{symbol}</span>;
  }
  const tone = /^[WUBRGC]$/.test(symbol) ? symbol.toLowerCase() : "generic";
  return (
    <span
      className={`mana-symbol mana-${tone}`}
      aria-label={`{${symbol}}`}
      role="img"
    >
      {String.fromCharCode(code)}
    </span>
  );
}

/** A cost such as `{1}{U}{U}`. */
export function ManaCost({ cost }: { cost: string }) {
  return (
    <span className="mana-cost">
      {[...cost.matchAll(/\{([^}]+)\}/g)].map((m, i) => (
        <ManaSymbol key={i} symbol={m[1] ?? ""} />
      ))}
    </span>
  );
}

/** Rules text with its `{...}` symbols drawn. */
export function RulesText({ text }: { text: string }) {
  const parts: ReactNode[] = text
    .split(/(\{[^}]+\})/)
    .map((part, i) =>
      part.startsWith("{") ? (
        <ManaSymbol key={i} symbol={part.slice(1, -1)} />
      ) : (
        <Fragment key={i}>{part}</Fragment>
      ),
    );
  return <>{parts}</>;
}

/** A loyalty ability's cost ([CR 606.4](https://yawgatog.com/resources/magic-rules/#R6064)):
 * "+1", "−2" or "0" on the printed loyalty symbol, an arrow up, down or flat. */
export function LoyaltyCost({ cost }: { cost: string }) {
  const tone = cost.startsWith("+")
    ? "up"
    : cost.startsWith("\u2212")
      ? "down"
      : "zero";
  return (
    <span
      className={`loyalty-cost loyalty-${tone}`}
      role="img"
      aria-label={cost}
    >
      <span className="loyalty-glyph" aria-hidden>
        {String.fromCharCode(LOYALTY_CODES[tone])}
      </span>
      <span className="loyalty-number" aria-hidden>
        {cost}
      </span>
    </span>
  );
}

const LOYALTY_CODES = { up: 0xe627, down: 0xe625, zero: 0xe626 } as const;

/** Whether `text` is a loyalty cost, as the engine writes one: "+1", "−2", "0". */
export const isLoyaltyCost = (text: string) => /^([+\u2212]\d+|0)$/.test(text);

/** An activation cost: a loyalty symbol, or rules text with its symbols drawn. */
export function CostText({ text }: { text: string }) {
  return isLoyaltyCost(text) ? (
    <LoyaltyCost cost={text} />
  ) : (
    <RulesText text={text} />
  );
}

/** Flying, drawn as on the mockups' badges: an eagle facing left, wings raised, the right one
 * feathered. */
const EAGLE =
  "M2.7 3.2 L7.6 3.2 L15.3 15.1 L31.4 2.6 L30.7 8.2 L25.8 10.8 L30 11.4 L24.4 15.8 L27.9 17 L18.8 21.4 L18.1 25.1 L16.7 30.1 L12.5 30.8 L11.1 25.1 L9 25.1 L3.4 25.1 L4.1 22.6 L9.7 20.1 L6.2 12.6 Z";

export function AbilityBadge({ ability }: { ability: AbilityIcon }) {
  return (
    <span className="ability-badge" role="img" aria-label={ability}>
      {ability === "flying" ? (
        <svg viewBox="0 0 32 32" aria-hidden>
          <path d={EAGLE} />
        </svg>
      ) : (
        <span className="ability-glyph">
          {String.fromCharCode(ABILITY_CODES[ability])}
        </span>
      )}
    </span>
  );
}
