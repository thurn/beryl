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

/** Flying, drawn as on the mockups' badges: an eagle facing left, wings raised, the right one
 * feathered. */
const EAGLE =
  "M2.7 3.2 L5.5 3.9 L13.9 15.1 L30 2.6 L28.6 7.6 L23.7 10.1 L27.9 10.8 L21.6 15.1 L25.8 16.4 L16.7 20.1 L16 24.5 L14.6 29.5 L11.1 29.5 L10.4 23.9 L2.7 23.9 L3.4 22 L9.7 19.5 L11.1 17.6 Z";

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
