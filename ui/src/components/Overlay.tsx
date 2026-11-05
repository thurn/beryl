// The overlay shell every choice screen uses (scry, Jace's put-back, modes): a title and an
// instruction over the dimmed battlefield, a "View Battlefield" escape and a confirm button.
import type { ReactNode } from "react";
import { motion, useReducedMotion } from "motion/react";
import { Button } from "./Controls";

export interface OverlayProps {
  title: string;
  instruction: string;
  children: ReactNode;
  /** The side panel, usually the source card. */
  aside?: ReactNode;
  confirm?: string;
  onConfirm?: () => void;
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
      <header className="overlay-header">
        <h2 className="overlay-title">{title}</h2>
        <p className="overlay-instruction">{instruction}</p>
      </header>
      <Button
        variant="secondary"
        size="small"
        className="overlay-view"
        onClick={onViewBattlefield}
      >
        View Battlefield
      </Button>
      <div className="overlay-body">
        <div className="overlay-main">
          {children}
          <Button className="overlay-confirm" onClick={onConfirm}>
            {confirm}
          </Button>
        </div>
        {aside && <aside className="overlay-aside">{aside}</aside>}
      </div>
    </motion.div>
  );
}
