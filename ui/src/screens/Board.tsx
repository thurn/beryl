import type { ComponentProps, ReactNode } from "react";
import { Stage } from "../game/Stage";
import { usePhone } from "../mobile/phone";
import { PhoneStageBoard } from "../stage/PhoneStageBoard";
import { StageBoard } from "../stage/StageBoard";

type BoardProps = Omit<ComponentProps<typeof StageBoard>, "dimmed"> & {
  opening?: boolean;
  dimmed?: boolean;
  children?: ReactNode;
  freeze?: boolean;
  notice?: ReactNode;
  onCommandList?: () => void;
  onSelectionActions?: (element: HTMLElement) => void;
};

/** The table uses the desktop stage frame or the phone's viewport, with screens over it. */
export function GameBoard({
  opening = false,
  dimmed = opening,
  children,
  freeze,
  notice,
  onCommandList,
  onSelectionActions,
  ...props
}: BoardProps) {
  const phone = usePhone();
  const board = phone ? (
    <PhoneStageBoard
      {...props}
      opening={opening}
      onCommandList={onCommandList}
      onSelectionActions={onSelectionActions}
    />
  ) : (
    <StageBoard {...props} dimmed={dimmed} />
  );
  const underlay = (
    <>
      {board}
      {children}
    </>
  );
  const screen =
    freeze === undefined ? (
      underlay
    ) : (
      <>
        <div className="table-link-underlay" inert={freeze}>
          {underlay}
        </div>
        {notice}
      </>
    );
  return phone ? screen : <Stage>{screen}</Stage>;
}
