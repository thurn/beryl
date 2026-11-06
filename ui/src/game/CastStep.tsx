// The cast step (game/casting.ts): a card's casts, each by its cost, its X and its payment in
// one panel where the turn's prompts go, while the board lights the permanents the payment
// taps. A payment the engine offers a choice of is picked here by what it taps, or by clicking
// a permanent on the board that another payment taps.
import { useEffect } from "react";
import { cardEntry } from "../cards";
import { Button } from "../components/Controls";
import { ManaCost, RulesText } from "../components/Mana";
import {
  castName,
  castParagraphs,
  paymentOf,
  plansOf,
  tapList,
  xValues,
  type CastChoice,
} from "./casting";
import type { Act } from "./decisions";

export interface CastStepProps {
  /** The card being cast. */
  name: string;
  casts: Act[];
  choice: CastChoice;
  /** The name of the permanent in a slot. */
  nameOf(slot: number): string;
  onChoose(choice: CastChoice): void;
  onCast(): void;
  onCancel(): void;
}

export function CastStep({
  name,
  casts,
  choice,
  nameOf,
  onChoose,
  onCast,
  onCancel,
}: CastStepProps) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onCancel();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);
  const xs = xValues(choice.act);
  const x = choice.x;
  const step = (by: number) => {
    const next = xs[xs.indexOf(x ?? xs[0]!) + by];
    if (next !== undefined) onChoose({ act: choice.act, x: next });
  };
  const plans = plansOf(choice);
  const paying = paymentOf(choice);
  const taps = (slots: number[]) =>
    slots.length > 0 ? tapList(slots.map(nameOf)) : "nothing";
  const card = cardEntry(name);
  const faceOf = (a: Act) =>
    a.option.kind === "cast" ? card?.faces[a.option.face] : undefined;
  // What the chosen cast adds, in the card's words, as kicking or evoking it does.
  const said = castParagraphs(
    faceOf(choice.act)?.oracle_text ?? "",
    castName(choice.act, name),
  );
  return (
    <div className="prompt cast-step" role="dialog" aria-label={`Cast ${name}`}>
      <h2 className="prompt-title">Cast {name}</h2>
      {casts.length > 1 && (
        <div className="cast-row" role="radiogroup" aria-label="How to cast">
          {casts.map((a) => (
            <button
              key={a.index}
              type="button"
              role="radio"
              aria-checked={a === choice.act}
              className="cast-pill"
              onClick={() =>
                a !== choice.act && onChoose({ act: a, x: xValues(a)[0] })
              }
            >
              <CastLabel
                label={castName(a, name)}
                cost={faceOf(a)?.mana_cost}
              />
            </button>
          ))}
        </div>
      )}
      {casts.length > 1 &&
        said.map((p) => (
          <p key={p} className="cast-caption cast-said">
            <RulesText text={p} />
          </p>
        ))}
      {x !== undefined && (
        <div className="cast-row cast-x">
          <span className="cast-x-label">X</span>
          <Button
            size="small"
            variant="secondary"
            aria-label="Decrease X"
            disabled={x === xs[0]}
            onClick={() => step(-1)}
          >
            −
          </Button>
          <output className="cast-x-value" aria-label="X">
            {x}
          </output>
          <Button
            size="small"
            variant="secondary"
            aria-label="Increase X"
            disabled={x === xs[xs.length - 1]}
            onClick={() => step(1)}
          >
            +
          </Button>
          {paying?.cost && (
            <span className="cast-x-cost" aria-label="Total cost">
              <ManaCost cost={paying.cost} />
            </span>
          )}
        </div>
      )}
      {plans.length > 1 ? (
        <div className="cast-plans" role="radiogroup" aria-label="Pay with">
          <span className="cast-caption">
            Tap these, or click a glowing permanent:
          </span>
          {plans.map((p) => (
            <button
              key={p.plan}
              type="button"
              role="radio"
              aria-checked={p === paying}
              className="cast-pill"
              onClick={() => onChoose({ ...choice, plan: p.plan ?? undefined })}
            >
              {taps(p.taps)}
            </button>
          ))}
        </div>
      ) : (
        <p className="cast-caption cast-taps">
          Taps {taps(paying?.taps ?? [])}
        </p>
      )}
      <div className="cast-buttons">
        <Button size="small" onClick={onCast}>
          Cast
        </Button>
        <Button size="small" variant="secondary" onClick={onCancel}>
          Cancel
        </Button>
      </div>
    </div>
  );
}

/** A cast's name in the cast step, by its cost: the card cast as printed by its mana cost,
 * another cast by what it adds, as its name says ("Kicker {X}"). */
function CastLabel({ label, cost }: { label: string; cost?: string }) {
  return label === "Cast" && cost ? (
    <>
      Cast <ManaCost cost={cost} />
    </>
  ) : (
    <RulesText text={label} />
  );
}
