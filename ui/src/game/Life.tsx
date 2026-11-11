// A player's life orb on the table with the combat damage they are about to take, the same on
// the old board and the stage's.
import { useEffect, useState, type CSSProperties } from "react";
import {
  animate,
  motion,
  useMotionValue,
  useReducedMotion,
} from "motion/react";
import { LifeOrb } from "../components/Player";
import type { Side } from "../generated/protocol";
import type { Act } from "./decisions";

/** The element id of the oval of a player's life orb. */
export const lifeId = (side: Side) => `${side}-life-orb`;

/** A life orb that counts to a new total and pulses as it changes; it glows cyan as a target
 * and is then clicked to pick that player. It flares while its player is the active player,
 * whoever holds priority: the priority pill says who that is. */
export function Life({
  side,
  life,
  incoming,
  active,
  style,
  target,
  onPick,
  onAttack,
}: {
  side: Side;
  life: number;
  /** In combat, before its damage: what the unblocked attackers will deal this player. */
  incoming?: number;
  active: boolean;
  style: CSSProperties;
  target?: Act;
  onPick(act: Act): void;
  onAttack?(): void;
}) {
  const reduce = useReducedMotion() ?? false;
  const value = useMotionValue(life);
  const pulse = useMotionValue(1);
  const [shown, setShown] = useState(life);
  useEffect(() => {
    if (value.get() === life) return;
    const count = animate(value, life, {
      duration: reduce ? 0 : 0.7,
      ease: "easeOut",
      onUpdate: (v) => setShown(Math.round(v)),
    });
    const beat = reduce
      ? undefined
      : animate(pulse, [1.14, 1], { duration: 0.5, ease: "easeOut" });
    return () => {
      count.stop();
      beat?.stop();
    };
  }, [life, value, pulse, reduce]);
  const targetable = target !== undefined || onAttack !== undefined;
  return (
    <motion.div
      className={targetable ? "hud life life-aimed" : "hud life"}
      data-life={side}
      style={{ ...style, scale: pulse }}
    >
      {/* As a target, the orb's whole face is clicked, not just its number; the stops on
          either side keep theirs. */}
      {targetable && (
        <span
          className="life-hit life-target"
          role="button"
          aria-label={
            onAttack
              ? "Attack opponent"
              : side === "you"
                ? "Target yourself"
                : "Target opponent"
          }
          onClick={onAttack ?? (() => target && onPick(target))}
        />
      )}
      <LifeOrb
        life={shown}
        active={active}
        seat={side === "opponent" ? "opponent" : "you"}
        anchor={lifeId(side)}
      />
      {incoming !== undefined && (
        <Incoming amount={incoming} lethal={incoming > 0 && incoming >= life} />
      )}
    </motion.div>
  );
}

/** The combat damage a player is about to take, beside their orb: a number, red when it is
 * lethal. */
export function Incoming({
  amount,
  lethal,
}: {
  amount: number;
  lethal: boolean;
}) {
  return (
    <span
      className={
        lethal ? "life-incoming life-incoming-lethal" : "life-incoming"
      }
      role="status"
      aria-label={`${amount} combat damage${lethal ? ", lethal" : ""}`}
      data-incoming={amount}
      data-lethal={lethal || undefined}
    >
      {amount > 0 ? `\u2212${amount}` : "0"}
    </span>
  );
}
