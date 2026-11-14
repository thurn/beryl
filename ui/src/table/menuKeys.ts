export interface MenuPosition {
  row: number;
  child?: number;
}

export interface MenuSelection {
  active: MenuPosition;
  open?: number;
}

type MenuKeyAction =
  | { kind: "move"; selection: MenuSelection }
  | { kind: "choose"; position: MenuPosition }
  | { kind: "close" };

/** Keys act on the active level. Headings and separators are absent from childCounts. */
export function menuKey(
  key: string,
  selection: MenuSelection,
  childCounts: readonly number[],
): MenuKeyAction | undefined {
  const { active } = selection;
  if (key === "Escape") return { kind: "close" };
  const { row, child } = active;
  const children = childCounts[row] ?? 0;
  const move = (selection: MenuSelection): MenuKeyAction => ({
    kind: "move",
    selection,
  });

  switch (key) {
    case "ArrowUp":
    case "ArrowDown":
    case "Home":
    case "End": {
      const count = child === undefined ? childCounts.length : children;
      if (!count) return;
      const index = child ?? row;
      const next =
        key === "Home"
          ? 0
          : key === "End"
            ? count - 1
            : (index + count + (key === "ArrowDown" ? 1 : -1)) % count;
      return child === undefined
        ? move({ active: { row: next } })
        : move({ active: { row, child: next }, open: row });
    }
    case "ArrowLeft":
      if (selection.open !== undefined) return move({ active: { row } });
      return;
    case "ArrowRight":
      if (children && child === undefined) {
        return move({ active: { row, child: 0 }, open: row });
      }
      return;
    case "Enter":
    case " ":
      if (!childCounts.length) return;
      return children && child === undefined
        ? move({ active: { row, child: 0 }, open: row })
        : { kind: "choose", position: active };
  }
}
