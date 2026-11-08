// The cast step (game/casting.ts): a card's casts, each by its cost, with its X where it has
// one, as the floating prompt's buttons in the action dock: a button casts. What the payment taps is lit on the
// board, where a payment the engine offers a choice of is picked by clicking a glowing
// permanent that another payment taps.
import { useEffect } from "react";
import { cardEntry } from "../cards";
import { Button } from "../components/Controls";
import { ManaCost, RulesText } from "../components/Mana";
import {
  castName,
  paymentOf,
  plansOf,
  xValues,
  type CastChoice,
} from "./casting";
import type { Act } from "./decisions";
import { FloatPrompt, PromptRow } from "./FloatPrompt";

export interface CastStepProps {
  /** The card being cast. */
  name: string;
  casts: Act[];
  choice: CastChoice;
  onChoose(choice: CastChoice): void;
  /** Casts with `choice`. */
  onCast(choice: CastChoice): void;
  onCancel(): void;
}

export function CastStep({
  name,
  casts,
  choice,
  onChoose,
  onCast,
  onCancel,
}: CastStepProps) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onCancel();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);
  const xs = [...new Set(casts.flatMap(xValues))].sort((a, b) => a - b);
  // The X shown, which the casts that ask one take.
  const x = choice.x ?? xs[0];
  const step = (by: number) => {
    const next = xs[xs.indexOf(x ?? xs[0]!) + by];
    if (next === undefined) return;
    const allows = (a: Act) => xValues(a).includes(next);
    onChoose({
      act: allows(choice.act) ? choice.act : casts.find(allows)!,
      x: next,
    });
  };
  const card = cardEntry(name);
  /** A cast at the X chosen, as far as it allows one. */
  const choiceFor = (a: Act): CastChoice =>
    a === choice.act
      ? choice
      : {
          act: a,
          x: x !== undefined && xValues(a).includes(x) ? x : xValues(a)[0],
        };
  // Paying with permanents chosen on the board: a cast among several only selects.
  const paysOnBoard = casts.some((a) => plansOf(choiceFor(a)).length > 1);
  const selects = paysOnBoard && casts.length > 1;
  return (
    <FloatPrompt title={name} label={`Cast ${name}`}>
      {xs.length > 0 && x !== undefined && (
        <div className="cast-x">
          <Button
            variant="secondary"
            aria-label="Decrease X"
            disabled={x === xs[0]}
            onClick={() => step(-1)}
          >
            −
          </Button>
          <output className="cast-x-value" aria-label="X">
            X={x}
          </output>
          <Button
            variant="secondary"
            aria-label="Increase X"
            disabled={x === xs[xs.length - 1]}
            onClick={() => step(1)}
          >
            +
          </Button>
        </div>
      )}
      <PromptRow>
        {/* Choosing among casts paid on the board, Cast is the primary, at the foot. */}
        {selects && <Button onClick={() => onCast(choice)}>Cast</Button>}
        {casts.map((a, i) => {
          const c = choiceFor(a);
          const face =
            a.option.kind === "cast" ? card?.faces[a.option.face] : undefined;
          const label = castName(a, name);
          return (
            <Button
              key={a.index}
              variant={i === 0 && !selects ? "primary" : "secondary"}
              aria-pressed={selects ? a === choice.act : undefined}
              onClick={() => (selects ? onChoose(c) : onCast(c))}
            >
              <CastLabel
                label={label}
                // Only the plain cast falls back to the printed cost: an alternative one paid
                // without mana (Evoke by exiling a card) shows none.
                cost={
                  paymentOf(c)?.cost ||
                  (label === "Cast" ? face?.mana_cost : undefined) ||
                  undefined
                }
              />
            </Button>
          );
        })}
        <Button variant="secondary" onClick={onCancel}>
          Cancel
        </Button>
      </PromptRow>
    </FloatPrompt>
  );
}

/** A cast's button: "Cast" with the card's cost, or the keyword that names it ("Evoke",
 * "Overload {2}{B}{B}", "Kicker {1}"), its cost where the keyword has none. */
function CastLabel({ label, cost }: { label: string; cost?: string }) {
  const word = label.split(/[,—]/)[0]!.trim();
  return (
    <>
      <RulesText text={word} />
      {cost && !word.includes("{") && (
        <>
          {" "}
          <ManaCost cost={cost} />
        </>
      )}
    </>
  );
}
