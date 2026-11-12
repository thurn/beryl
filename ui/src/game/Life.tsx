// What stands beside a player's life orb: the combat damage they are about to take, and by
// how much their total is changing.

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

/** By how much a life total is changing, beside the total for as long as its `count` step
 * plays on the stage: Arena's red number under the avatar for a loss, its green one over it
 * for a gain (data/arena-motion.json, count). */
export function LifeChange({ by }: { by: number }) {
  return (
    <span
      className={`life-change life-change-${by > 0 ? "gain" : "loss"}`}
      role="status"
      aria-label={`${Math.abs(by)} life ${by > 0 ? "gained" : "lost"}`}
      data-life-change={by}
    >
      {by > 0 ? `+${by}` : `\u2212${-by}`}
    </span>
  );
}
