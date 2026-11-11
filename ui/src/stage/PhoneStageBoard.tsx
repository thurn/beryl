// The phone's game screen drawn on the stage (plan K25 and K29), in place of
// mobile/PhoneBoard.tsx behind `?stage` (switch.tsx): the phone's chrome as it is
// (mobile/chrome.tsx), and one node an object or ability where the page's stage has it, laid
// out for the phone (phone.ts) and drawn in the phone's faces (mobile/faces.tsx). The same
// scripts play as on the desktop. What the chrome shows of the table (life, the piles, the
// phase) is the stage's table, so that what a step changes shows as the step plays; the dock
// answers the view last played out. No card takes the pointer yet: a touch skips what is
// playing.
import {
  useLayoutEffect,
  useMemo,
  useSyncExternalStore,
  type ComponentProps,
} from "react";
import { interactions } from "../game/decisions";
import {
  MenuButton,
  Middle,
  Orb,
  PhoneDock,
  Piles,
  useViewport,
} from "../mobile/chrome";
import { PHONE } from "../mobile/faces";
import { phoneLayout } from "../mobile/layout";
import type { PhoneBoard } from "../mobile/PhoneBoard";
import "../mobile/phone.css";
import { Faces } from "./Faces.tsx";
import { useSurface } from "./mount.ts";
import { phone } from "./phone.ts";
import { pageStage } from "./playback.ts";
import "./stage.css";

export function PhoneStageBoard({
  view,
  waiting,
  choose,
  passOn,
  onLeave,
}: ComponentProps<typeof PhoneBoard>) {
  const { stage, feed } = pageStage();
  const { ref, viewport } = useViewport();
  const { layer, surface } = useSurface(stage);
  const { table } = useSyncExternalStore(stage.subscribe, () => stage.shown);
  // The table is laid out for a phone this size while this board shows it.
  useLayoutEffect(() => {
    if (viewport) stage.relayout(phone(viewport));
  }, [stage, viewport]);
  const bands = useMemo(
    () => viewport && table && phoneLayout(table, viewport).bands,
    [table, viewport],
  );
  const can = useMemo(() => interactions(view), [view]);
  const objects = useMemo(
    () => new Map(view.objects.map((o) => [o.slot, o])),
    [view],
  );
  return (
    <div className="phone-board" ref={ref} onPointerDown={() => feed.skip()}>
      {bands && table && (
        <>
          <div
            className="phone-strip phone-strip-opponent"
            style={bands.opponentStrip}
          >
            <Piles side="opponent" view={table} tops={false} />
            <Orb side="opponent" view={table} />
            <MenuButton onLeave={onLeave} />
          </div>
          <Middle band={bands.middle} view={table} />
          <div className="phone-strip phone-strip-you" style={bands.yourStrip}>
            <Piles side="you" view={table} tops={false} />
            <Orb side="you" view={table} />
            <PhoneDock
              view={view}
              waiting={waiting}
              can={can}
              objects={objects}
              choose={choose}
              passOn={passOn}
            />
          </div>
        </>
      )}
      <div className="card-layer phone-cards" ref={layer} />
      {surface && <Faces stage={stage} surface={surface} skin={PHONE} />}
    </div>
  );
}
