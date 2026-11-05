// A scry decision in the overlay shell, as in docs/ui_mockups/scry.png. Static for now: the
// choice overlays bead wires it to decisions and drag ordering.
import { Card, CardBack, useSlotArt } from "../components/Card";
import { Overlay } from "../components/Overlay";
import { SAMPLE } from "./samples";

export function ScryOverlay({ className }: { className?: string }) {
  const a = useSlotArt();
  return (
    <Overlay
      className={className}
      title="Scry"
      instruction="Drag cards to put them on the top or bottom of your library."
      aside={<Card card={a(SAMPLE.preordain, "scry-preordain")} width={338} />}
    >
      <div className="scry-zone">
        <CardBack
          width={262}
          style={{
            position: "absolute",
            left: 392,
            top: 6,
            transform: "rotate(-3deg)",
          }}
        />
        <CardBack
          width={262}
          style={{
            position: "absolute",
            left: 300,
            top: 14,
            transform: "rotate(-8deg)",
          }}
        />
        <Card
          card={a(SAMPLE.fullArtIsland, "scry-island")}
          width={290}
          height={358}
          highlight="target"
          style={{
            position: "absolute",
            left: 140,
            top: 66,
            rotate: -9,
            zIndex: 3,
          }}
        />
        <Card
          card={a(SAMPLE.counterspell, "scry-counterspell")}
          width={288}
          style={{
            position: "absolute",
            left: 575,
            top: 36,
            rotate: 3,
            zIndex: 2,
          }}
        />
        <span className="scry-label scry-bottom">Bottom</span>
        <span className="scry-label scry-top">Top</span>
      </div>
    </Overlay>
  );
}
