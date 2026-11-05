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

/** Flying, drawn as on the mockups' badges: an eagle, wings raised. */
const EAGLE =
  "M4 4 L9 3.5 L16 13 L23.5 2.5 L28.5 3.5 L25 10 L29.5 9.5 L24.5 15.5 L28 16.5 L20 20.5 L20.5 26.5 L17 30.5 L13 27.5 L13.5 23.5 L8.5 23.5 L3 25.5 L7.5 20.5 L12.5 18 L6 10 Z";

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
