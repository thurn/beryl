// A scry decision in the overlay shell, as in docs/ui_mockups/scry.png. Static for now: the
// choice overlays bead wires it to decisions and drag ordering.
import { ASPECT, Card, CardBack } from "../components/Card";
import { Overlay } from "../components/Overlay";
import { SAMPLE } from "./samples";

/** Lays a card `width` wide in the tray with its center at (x, y), turned `turn` degrees. */
function fanned(
  x: number,
  y: number,
  width: number,
  turn: number,
  height = width / ASPECT.full,
) {
  return {
    position: "absolute",
    left: x - width / 2,
    top: y - height / 2,
    rotate: `${turn}deg`,
  } as const;
}

export function ScryOverlay({ className }: { className?: string }) {
  return (
    <Overlay
      className={className}
      title="Scry"
      instruction="Drag cards to put them on the top or bottom of your library."
      aside={<Card card={SAMPLE.preordain} width={338} />}
    >
      <div className="scry-zone">
        <CardBack width={220} style={fanned(420, 174, 220, -10)} />
        <CardBack width={220} style={fanned(482, 171, 220, 5)} />
        <Card
          card={SAMPLE.counterspell}
          width={273}
          style={{ ...fanned(715, 229, 273, 3.2), zIndex: 2 }}
        />
        <Card
          card={SAMPLE.island}
          width={297}
          height={366}
          highlight="target"
          style={{ ...fanned(315, 269.5, 297, -8.6, 366), zIndex: 3 }}
        />
        <span className="scry-label scry-bottom">Bottom</span>
        <span className="scry-label scry-top">Top</span>
      </div>
    </Overlay>
  );
}
