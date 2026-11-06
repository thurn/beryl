// The overlay shell every choice screen uses (scry, Jace's put-back, modes), laid out on the
// 1600×900 table stage as in docs/ui_mockups/scry.png: a title and an instruction over the
// choice, a confirm button under it, and on the right a "View Battlefield" escape over the
// source card, all on the dimmed battlefield.
import type { ReactNode } from "react";
import { motion, useReducedMotion } from "motion/react";
import { Button } from "./Controls";

/** A button under the choice. */
export interface OverlayAction {
  label: ReactNode;
  onClick?: () => void;
  disabled?: boolean;
}

export interface OverlayProps {
  title: ReactNode;
  /** What to do, where the title and the cards do not say it. */
  instruction?: ReactNode;
  children: ReactNode;
  /** The side panel, usually the source card. */
  aside?: ReactNode;
  confirm?: string;
  onConfirm?: () => void;
  /** The buttons under the choice, the first one primary; given, they replace `confirm`. */
  actions?: OverlayAction[];
  onViewBattlefield?: () => void;
  className?: string;
}

export function Overlay({
  title,
  instruction,
  children,
  aside,
  confirm = "Done",
  onConfirm,
  actions,
  onViewBattlefield,
  className,
}: OverlayProps) {
  // Reduced motion (and so `just ui-shots`) shows it at once rather than fading it in.
  const reduce = useReducedMotion();
  return (
    <motion.div
      className={["overlay", className].filter(Boolean).join(" ")}
      initial={reduce ? false : { opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={{ duration: 0.25 }}
    >
      <div className="overlay-backdrop" />
      <div className="overlay-main">
        <header className="overlay-header">
          <h2 className="overlay-title">{title}</h2>
          {instruction && <p className="overlay-instruction">{instruction}</p>}
        </header>
        {children}
        {actions ? (
          <div className="overlay-actions">
            {actions.map((a, i) => (
              <Button
                key={i}
                variant={i === 0 ? "primary" : "secondary"}
                className={i === 0 ? "overlay-confirm" : "overlay-alternative"}
                disabled={a.disabled}
                onClick={a.onClick}
              >
                {a.label}
              </Button>
            ))}
          </div>
        ) : (
          <Button className="overlay-confirm" onClick={onConfirm}>
            {confirm}
          </Button>
        )}
      </div>
      <div className="overlay-side">
        <Button
          variant="secondary"
          className="overlay-view"
          onClick={onViewBattlefield}
        >
          View Battlefield
        </Button>
        {aside && <aside className="overlay-aside">{aside}</aside>}
      </div>
    </motion.div>
  );
}
