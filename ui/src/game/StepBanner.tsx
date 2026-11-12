// A step's announcement over the battlefield.
import { useEffect, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { Coin } from "../components/Player";

/** How long an announcing banner shows, in ms. */
export const BANNER_MS = 2500;

/** Whether the banner of this key is still showing: for BANNER_MS after it appears, then not,
 * until it goes and comes again. The spinner under the opponent's orb hides meanwhile, since the
 * banner lies where it does. */
export function useBannerUp(key: string | undefined): boolean {
  const [gone, setGone] = useState<string>();
  useEffect(() => {
    if (key === undefined) {
      setGone(undefined);
      return;
    }
    const timer = setTimeout(() => setGone(key), BANNER_MS);
    return () => clearTimeout(timer);
  }, [key]);
  return key !== undefined && gone !== key;
}

/** A step announcement over the battlefield, such as "Combat" over "Blockers declared", between
 * the opponent's life orb and their permanents: it fades away once no longer `shown`. */
export function StepBanner({
  title,
  detail,
  shown,
  coin,
  staged,
}: {
  title: string;
  detail: string;
  shown: boolean;
  coin: boolean;
  /** The stage's announcement of a coin: drawn from how far through its step is
   * (stage/stage.css `.stage-notice`). */
  staged?: boolean;
}) {
  const reduce = useReducedMotion() ?? false;
  return (
    <AnimatePresence>
      {shown && (
        <motion.div
          className={staged ? "step-banner stage-notice" : "step-banner"}
          role="status"
          exit={
            reduce ? undefined : { opacity: 0, transition: { duration: 0.4 } }
          }
        >
          {coin ? (
            <Coin className="step-banner-coin" first={false} size={40} />
          ) : (
            <>
              <i className="step-banner-rule step-banner-rule-left" />
              <i className="step-banner-diamond" />
            </>
          )}
          <span className="step-banner-title">{title}</span>
          <i className="step-banner-dot" />
          <i className="step-banner-rule step-banner-rule-right" />
          <i className="step-banner-underline" />
          <span className="step-banner-detail">{detail}</span>
          <svg className="step-banner-notch" viewBox="0 0 140 12" aria-hidden>
            <path d="M0 1.5 H60.5 L70 10.5 L79.5 1.5 H140" />
          </svg>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
