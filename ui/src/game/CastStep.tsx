// The cast step chooses the spell and X, and can preview the payment on the board.
import { useEffect } from "react";
import { cardEntry } from "../cards";
import { Button } from "../components/Controls";
import { useOpenSound } from "../audio/SoundControls";
import { ManaCost, RulesText } from "../components/Mana";
import {
  castName,
  firstChoice,
  paymentOf,
  plansOf,
  xValues,
  type CastChoice,
} from "./casting";
import type { Act } from "./decisions";
import { FloatPrompt, PromptRow } from "./FloatPrompt";
import { PaymentButton } from "./PaymentButton";

export interface CastStepProps {
  /** The card being cast. */
  name: string;
  casts: Act[];
  choice: CastChoice;
  onChoose(choice: CastChoice): void;
  /** Casts with `choice`. */
  onCast(choice: CastChoice): void;
  onPaymentPreview(on: boolean, choice: CastChoice): void;
  onCancel(): void;
}

export function CastStep({
  name,
  casts,
  choice,
  onChoose,
  onCast,
  onPaymentPreview,
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
        {selects && (
          <PaymentButton
            onPreview={(on) => onPaymentPreview(on, choice)}
            onPay={() => onCast(choice)}
          >
            Auto Pay
          </PaymentButton>
        )}
        {casts.map((a, i) => {
          const c = choiceFor(a);
          const face =
            a.option.kind === "cast" ? card?.faces[a.option.face] : undefined;
          const label = castName(a, name);
          const caption = (
            <CastLabel
              label={
                !selects && paysOnBoard && label === "Cast" ? "Auto Pay" : label
              }
              cost={
                paymentOf(c)?.cost ||
                (label === "Cast" ? face?.mana_cost : undefined) ||
                undefined
              }
            />
          );
          return selects ? (
            <Button
              key={a.index}
              variant="secondary"
              aria-pressed={a === choice.act}
              onClick={() => onChoose(c)}
            >
              {caption}
            </Button>
          ) : (
            <PaymentButton
              key={a.index}
              variant={i === 0 ? "primary" : "secondary"}
              onPreview={(on) => onPaymentPreview(on, c)}
              onPay={() => onCast(c)}
            >
              {caption}
            </PaymentButton>
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
export function CastLabel({ label, cost }: { label: string; cost?: string }) {
  useOpenSound();
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

/** A cast's caption in the mode chooser: as its button in the cast step says it, at its least
 * X and default payment. */
export function CastCaption({ act, name }: { act: Act; name: string }) {
  const face =
    act.option.kind === "cast"
      ? cardEntry(name)?.faces[act.option.face]
      : undefined;
  const label = castName(act, name);
  return (
    <CastLabel
      label={label}
      cost={
        paymentOf(firstChoice(act))?.cost ||
        (label === "Cast" ? face?.mana_cost : undefined) ||
        undefined
      }
    />
  );
}
