import { type CSSProperties, useEffect, useState } from "react";

/** The part of a fixed phone layout still covered after any native viewport resize. */
export function useKeyboardInset(enabled: boolean): CSSProperties | undefined {
  const [keyboardInset, setKeyboardInset] = useState(0);
  useEffect(() => {
    if (!enabled) return;
    const fullHeight = window.innerHeight;
    let nativeInset = 0;
    const update = () => {
      const viewport = window.visualViewport;
      const covered = viewport
        ? fullHeight - viewport.height - viewport.offsetTop
        : 0;
      // Some Android WebViews keep the visual viewport unchanged under the keyboard.
      // If the layout viewport shrank too, do not count that space twice.
      setKeyboardInset(
        Math.max(
          0,
          Math.max(covered, nativeInset) - (fullHeight - window.innerHeight),
        ),
      );
    };
    const onKeyboard = (event: Event) => {
      nativeInset =
        (event as CustomEvent<number>).detail / window.devicePixelRatio;
      update();
    };
    window.visualViewport?.addEventListener("resize", update);
    window.visualViewport?.addEventListener("scroll", update);
    window.addEventListener("resize", update);
    window.addEventListener("beryl:keyboard-inset", onKeyboard);
    return () => {
      window.visualViewport?.removeEventListener("resize", update);
      window.visualViewport?.removeEventListener("scroll", update);
      window.removeEventListener("resize", update);
      window.removeEventListener("beryl:keyboard-inset", onKeyboard);
    };
  }, [enabled]);
  return enabled
    ? ({ "--phone-keyboard-inset": `${keyboardInset}px` } as CSSProperties)
    : undefined;
}
