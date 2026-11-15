import type { ComponentProps, ReactNode } from "react";
import { Stage } from "../game/Stage";
import { usePhone } from "../mobile/phone";
import { PhoneStageBoard } from "../stage/PhoneStageBoard";
import { StageBoard } from "../stage/StageBoard";

type BoardProps = Omit<ComponentProps<typeof StageBoard>, "dimmed"> & {
  opening?: boolean;
  dimmed?: boolean;
  children?: ReactNode;
};

/** The table uses the desktop stage frame or the phone's viewport, with screens over it. */
export function GameBoard({
  opening = false,
  dimmed = opening,
  children,
  ...props
}: BoardProps) {
  if (usePhone()) {
    return (
      <>
        <PhoneStageBoard {...props} opening={opening} />
        {children}
      </>
    );
  }
  return (
    <Stage>
      <StageBoard {...props} dimmed={dimmed} />
      {children}
    </Stage>
  );
}
