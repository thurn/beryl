// A scry decision in the overlay shell, as in docs/ui_mockups/scry.png. Static for now: the
// choice overlays bead wires it to decisions and drag ordering.
import { Card, CardBack } from "../components/Card";
import { Overlay } from "../components/Overlay";
import { SAMPLE } from "./samples";

export function ScryOverlay({ className }: { className?: string }) {
  return (
    <Overlay
      className={className}
      title="Scry"
      instruction="Drag cards to put them on the top or bottom of your library."
      aside={<Card card={SAMPLE.preordain} width={300} />}
    >
      <div className="scry-zone">
        <Card
          card={SAMPLE.island}
          width={300}
          highlight="target"
          style={{
            position: "absolute",
            left: 150,
            top: 60,
            rotate: -8,
            zIndex: 3,
          }}
        />
        <CardBack
          width={270}
          style={{
            position: "absolute",
            left: 390,
            top: 10,
            transform: "rotate(-5deg)",
          }}
        />
        <Card
          card={SAMPLE.counterspell}
          width={280}
          style={{
            position: "absolute",
            left: 590,
            top: 30,
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
