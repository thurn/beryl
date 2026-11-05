// Replica mode (`?replica=<mockup>`), development builds only: renders a mockup's board in
// docs/ui_mockups with the game's own screens, each card's art cut from the mockup itself
// (`by ui-compare` and `by ui-shots` cut it into ui/replica-art/). `just ui-compare` captures
// these at the mockup's native size and compares them part by part. `?replica=fonts` is the font
// sheet `just ui-fonts` renders.
import regions from "../../../docs/ui_mockups/regions.json";
import { SlotArt } from "../components/Card";
import { Gallery } from "../screens/Gallery";
import { TableScreen } from "../screens/Table";
import { BOARDS } from "./boards";
import { FontSheet } from "./FontSheet";

const MOCKUPS: Record<string, { art: Record<string, unknown> }> =
  regions.mockups;

export default function Replica({ name }: { name: string }) {
  if (name === "fonts") {
    return <FontSheet />;
  }
  const mockup = MOCKUPS[name];
  if (!mockup) {
    throw new Error(
      `no replica of ${name}; one of ${Object.keys(MOCKUPS).join(", ")}, fonts`,
    );
  }
  const art = (slot: string) =>
    slot in mockup.art ? `/replica-art/${name}/${slot}.png` : undefined;
  return (
    <SlotArt.Provider value={art}>
      {name === "ui_system" ? (
        <Gallery />
      ) : (
        <TableScreen
          board={BOARDS[name]}
          overlay={name === "scry" ? "scry" : undefined}
        />
      )}
    </SlotArt.Provider>
  );
}
