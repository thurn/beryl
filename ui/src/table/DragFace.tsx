import { Card, CardBack } from "../components/Card";
import { useFace } from "../game/faces";
import type { ObjectView } from "../generated/protocol";

export function DragFace({
  object,
  width,
}: {
  object: ObjectView;
  width: number;
}) {
  const face = useFace(object);
  return object.name === null || object.face === "down" ? (
    <CardBack width={width} />
  ) : (
    <Card card={face} width={width} />
  );
}
