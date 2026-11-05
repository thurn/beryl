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

const ABILITY_CODES: Record<AbilityIcon, number> = {
  flying: 0xe952,
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

export function AbilityBadge({ ability }: { ability: AbilityIcon }) {
  return (
    <span className="ability-badge" role="img" aria-label={ability}>
      {String.fromCharCode(ABILITY_CODES[ability])}
    </span>
  );
}
