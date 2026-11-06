// Fitting text to its box by shrinking it.
import { useLayoutEffect, useRef } from "react";

/** Shrinks a text to fit its box rather than cutting it off, as a card's long name in its bar or
 * the turn button's long label: `--fit` scales the text's size (components.css, screens.css) by
 * how far it overflows, measured again whenever the box (the text's parent) resizes, as when a
 * card turns to tap. A text that wraps (a squeezed card's name) shrinks until its lines fit its
 * height, a step at a time, since wrapping changes with the size. */
export function useTextFit(text: string) {
  const ref = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    const box = el?.parentElement;
    if (!el || !box) return;
    // How far the text overflows its element along its line, and whether it overflows down
    // its lines (wrapping).
    const overflow = () => {
      // scrollWidth and clientWidth round to whole pixels, so a text overflowing by less than
      // one would not shrink: also compare the unrounded on-screen lengths of the text
      // and its box, along whichever screen axis the text runs (a tapped card's runs down).
      const text = document.createRange();
      text.selectNodeContents(el);
      const [t, n] = [text.getBoundingClientRect(), el.getBoundingClientRect()];
      const along =
        n.width >= n.height ? t.width / n.width : t.height / n.height;
      return {
        along: Math.max(el.scrollWidth / el.clientWidth, along),
        down: el.scrollHeight > el.clientHeight + 1,
      };
    };
    const fit = () => {
      el.style.removeProperty("--fit");
      let size = 1;
      for (let tries = 0; tries < 12; tries++) {
        const { along, down } = overflow();
        if (along <= 1 && !down) break;
        size *= along > 1 ? 0.98 / along : 0.95;
        el.style.setProperty("--fit", String(size));
      }
    };
    fit();
    void document.fonts.ready.then(fit);
    // A box sized by its text (the priority pill) resizes as the text is fitted: fitted inside
    // the resize's notification, it would be resized again before the observer returns, a loop
    // the browser cuts short with an error. A frame later the fit's own resize is notified
    // afresh, and refitting it changes nothing.
    let frame = 0;
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(fit);
    });
    observer.observe(box);
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
    };
  }, [text]);
  return ref;
}
