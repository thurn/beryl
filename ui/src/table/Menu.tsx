import {
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import { createPortal } from "react-dom";
import type { Point } from "../stage/layout";
import { menuKey, type MenuPosition, type MenuSelection } from "./menuKeys";
import "./menu.css";

export interface SubmenuRow {
  id: string;
  label: string;
  key?: string;
  checked?: boolean;
  click?: boolean;
  submenu?: never;
}

export interface Row extends Omit<SubmenuRow, "submenu"> {
  submenu?: readonly SubmenuRow[];
}

export interface Group {
  heading?: string;
  rows: readonly Row[];
}

export interface MenuProps {
  header: { title: string; whose: string };
  groups: readonly Group[];
  /** The opening point in viewport CSS pixels. */
  at: Point;
  presentation: "menu" | "sheet";
  onChoose: (id: string) => void;
  onClose: () => void;
}

const EDGE = 8;

function fit(start: number, size: number, limit: number) {
  return Math.max(EDGE, Math.min(start, limit - size - EDGE));
}

function itemId(menu: string, { row, child }: MenuPosition) {
  return child === undefined ? `${menu}-${row}` : `${menu}-${row}-${child}`;
}

/** Place the panels after measuring their content. The submenu stays beside its parent row. */
function place(
  panel: HTMLElement,
  sub: HTMLElement | null,
  parent: Element | null,
  at: Point,
) {
  const { clientWidth: width, clientHeight: height } = document.documentElement;
  const bounds = panel.getBoundingClientRect();
  const x = at.x + bounds.width + EDGE > width ? at.x - bounds.width : at.x;
  const y = at.y + bounds.height + EDGE > height ? at.y - bounds.height : at.y;
  panel.style.left = `${fit(x, bounds.width, width)}px`;
  panel.style.top = `${fit(y, bounds.height, height)}px`;
  if (sub && parent) {
    const { left, right } = panel.getBoundingClientRect();
    const { width: subWidth, height: subHeight } = sub.getBoundingClientRect();
    sub.style.left = `${fit(
      right + subWidth + EDGE <= width ? right : left - subWidth,
      subWidth,
      width,
    )}px`;
    sub.style.top = `${fit(parent.getBoundingClientRect().top, subHeight, height)}px`;
  }
}

export function Menu({
  header,
  groups,
  at,
  presentation,
  onChoose,
  onClose,
}: MenuProps) {
  const id = useId();
  const layer = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const sub = useRef<HTMLDivElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);
  const scrollToActive = useRef(false);
  const [selection, setSelection] = useState<MenuSelection>({
    active: { row: 0 },
  });
  const rows = groups.flatMap((group) => group.rows);
  const { active, open } = selection;
  const children = open === undefined ? undefined : rows[open]?.submenu;

  useLayoutEffect(() => {
    const previous = document.activeElement;
    const overlay = layer.current;
    if (previous instanceof HTMLElement && !overlay?.contains(previous)) {
      previousFocus.current = previous;
    }
    return () => {
      if (
        overlay?.contains(document.activeElement) ||
        document.activeElement === document.body
      ) {
        previousFocus.current?.focus({ preventScroll: true });
      }
    };
  }, []);

  useLayoutEffect(() => {
    const root = panel.current;
    if (!root) return;
    const child = sub.current;
    const parent =
      open === undefined
        ? null
        : document.getElementById(itemId(id, { row: open }));
    const reposition = () => place(root, child, parent, at);
    reposition();
    const observer = new ResizeObserver(reposition);
    observer.observe(document.documentElement);
    observer.observe(root);
    if (child) observer.observe(child);
    root.addEventListener("scroll", reposition);
    window.addEventListener("resize", reposition);
    return () => {
      observer.disconnect();
      root.removeEventListener("scroll", reposition);
      window.removeEventListener("resize", reposition);
    };
  }, [at, id, open]);

  useLayoutEffect(() => {
    const focused = document.getElementById(itemId(id, active));
    const target = focused ?? panel.current;
    target?.focus({ preventScroll: true });
    if (scrollToActive.current) {
      focused?.scrollIntoView({ block: "nearest", inline: "nearest" });
      scrollToActive.current = false;
    }
  }, [active, id]);

  const choose = (row: SubmenuRow | Row) => {
    onClose();
    onChoose(row.id);
  };
  const select = (position: MenuPosition) => {
    const row = rows[position.row];
    if (!row) return;
    const { child } = position;
    if (child !== undefined) {
      const command = row.submenu?.[child];
      if (command) choose(command);
    } else if (row.submenu?.length) {
      setSelection({
        active: { row: position.row, child: 0 },
        open: position.row,
      });
    } else {
      choose(row);
    }
  };

  const keyDown = (event: KeyboardEvent) => {
    event.stopPropagation();
    if (event.nativeEvent.isComposing) return;
    if (event.key === "Tab") {
      onClose();
      return;
    }
    const action = menuKey(
      event.key,
      selection,
      rows.map((row) => row.submenu?.length ?? 0),
    );
    if (!action) return;
    event.preventDefault();
    switch (action.kind) {
      case "close":
        onClose();
        break;
      case "choose":
        select(action.position);
        break;
      case "move":
        scrollToActive.current = true;
        setSelection(action.selection);
        break;
    }
  };

  const renderRow = (row: Row, position: MenuPosition) => {
    const hasSubmenu = !!row.submenu?.length;
    const expanded = hasSubmenu && position.row === open;
    const selected =
      position.row === active.row && position.child === active.child;
    return (
      <div
        key={row.id}
        id={itemId(id, position)}
        role={row.checked === undefined ? "menuitem" : "menuitemcheckbox"}
        tabIndex={selected ? 0 : -1}
        aria-checked={row.checked}
        aria-haspopup={hasSubmenu ? "menu" : undefined}
        aria-expanded={hasSubmenu ? expanded : undefined}
        aria-controls={expanded ? `${id}-submenu` : undefined}
        data-active={selected || expanded || undefined}
        className="table-menu-row"
        onPointerEnter={(event) => {
          if (event.pointerType !== "mouse") return;
          setSelection({
            active: position,
            open:
              hasSubmenu || position.child !== undefined
                ? position.row
                : undefined,
          });
        }}
        onClick={() => select(position)}
      >
        <span aria-hidden="true" className="table-menu-check">
          {row.checked ? "✓" : null}
        </span>
        <span>{row.label}</span>
        <span aria-hidden="true" className="table-menu-hint">
          {row.click ? "click" : row.key}
          {hasSubmenu && (
            <span
              className={
                row.click || row.key ? "table-menu-chevron" : undefined
              }
            >
              ›
            </span>
          )}
        </span>
      </div>
    );
  };

  let rowIndex = 0;
  return createPortal(
    <div
      ref={layer}
      className="table-menu-layer"
      data-presentation={presentation}
      onKeyDown={keyDown}
      onPointerDown={(event) => {
        event.stopPropagation();
        if (event.target === event.currentTarget) event.preventDefault();
      }}
      onClick={(event) => {
        event.stopPropagation();
        if (event.target === event.currentTarget) onClose();
      }}
      onContextMenu={(event) => {
        event.preventDefault();
        event.stopPropagation();
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        ref={panel}
        className="table-menu"
        role="menu"
        aria-labelledby={`${id}-title`}
        tabIndex={-1}
      >
        <div id={`${id}-title`} className="table-menu-title">
          <strong>{header.title}</strong>
          <span>{header.whose}</span>
        </div>
        {groups.map((group, index) => (
          <div
            key={index}
            role="group"
            aria-label={group.heading}
            className="table-menu-group"
          >
            {group.heading && (
              <div className="table-menu-heading" aria-hidden="true">
                {group.heading}
              </div>
            )}
            {group.rows.map((row) => renderRow(row, { row: rowIndex++ }))}
          </div>
        ))}
      </div>
      {children?.length && open !== undefined ? (
        <div
          ref={sub}
          className="table-menu"
          id={`${id}-submenu`}
          role="menu"
          aria-labelledby={itemId(id, { row: open })}
        >
          {children.map((row, child) => renderRow(row, { row: open, child }))}
        </div>
      ) : null}
    </div>,
    document.getElementById("root") ?? document.body,
  );
}
