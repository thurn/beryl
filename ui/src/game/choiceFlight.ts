// Browsers and prompts borrow a card from the table while they need their own hit area
// and layout. Keep the table element and its slot alive so it can take the card back.
import { useLayoutEffect, type RefObject } from "react";
import { animate, useReducedMotion } from "motion/react";

type Slot = number | "library";

interface Borrower {
  element: HTMLElement;
  released: boolean;
}

interface Departure {
  element: HTMLElement;
  box: DOMRect;
  visible: boolean;
}

interface CardLoan {
  owners: Borrower[];
  departure?: Departure;
}

interface Flight {
  moving: boolean;
  stop(): void;
}

const loans = new WeakMap<HTMLElement, Map<Slot, CardLoan>>();
const animations = new WeakMap<HTMLElement, Flight>();

function tableCard(stage: HTMLElement, slot: Slot) {
  return stage.querySelector<HTMLElement>(
    slot === "library"
      ? '[data-table-library="you"]'
      : `.card-layer > .card-el[data-slot="${slot}"]`,
  );
}

function stageScale(stage: HTMLElement) {
  return stage.getBoundingClientRect().width / stage.offsetWidth;
}

export const BORROW_FLIGHT_MS = 340;

function flyIn(element: HTMLElement, from: DOMRect, stage: HTMLElement) {
  animations.get(element)?.stop();
  const to = element.getBoundingClientRect();
  if (!from.width || !to.width) return;
  const scale = stageScale(stage);
  // Measure the flight in screen pixels, then undo any tilt of the tray or pile.
  let axes = new DOMMatrix();
  for (let p = element.parentElement; p && p !== stage; p = p.parentElement) {
    const transform = getComputedStyle(p).transform;
    if (transform !== "none") axes = new DOMMatrix(transform).multiply(axes);
  }
  const offset = new DOMPoint(
    (from.left + from.width / 2 - to.left - to.width / 2) / scale,
    (from.top + from.height / 2 - to.top - to.height / 2) / scale,
    0,
    0,
  );
  const { x, y } = axes.inverse().transformPoint(offset);
  // The library's anchor measures the packet. Its actual cards live in CardLayer so
  // they can leave individually. On return, translate those cards together.
  const packet = element.hasAttribute("data-table-library");
  const size = from.width / to.width;
  const paint = (progress: number) => {
    const left = 1 - progress;
    if (packet) {
      stage.style.setProperty("--library-return-x", `${offset.x * left}px`);
      stage.style.setProperty("--library-return-y", `${offset.y * left}px`);
    } else {
      element.style.translate = `${x * left}px ${y * left}px`;
      element.style.scale = String(size + (1 - size) * progress);
    }
  };
  const finish = () => {
    flight.moving = false;
    if (packet) {
      stage.style.removeProperty("--library-return-x");
      stage.style.removeProperty("--library-return-y");
    } else {
      element.style.translate = "";
      element.style.scale = "";
    }
  };
  paint(0);
  const run = animate(0, 1, {
    duration: BORROW_FLIGHT_MS / 1000,
    ease: [0.2, 0.75, 0.3, 1],
    onUpdate: paint,
    onComplete: finish,
  });
  const flight = {
    moving: true,
    stop() {
      run.stop();
      finish();
    },
  };
  animations.set(element, flight);
}

function stageBox(stage: HTMLElement, box: DOMRect) {
  const origin = stage.getBoundingClientRect();
  const scale = stageScale(stage);
  return {
    x: (box.left - origin.left) / scale,
    y: (box.top - origin.top) / scale,
    width: box.width / scale,
  };
}

export type CardHandoff = ReturnType<typeof stageBox>;
export const CARD_RETURN = "beryl-card-return";

/** A held tray can outlast the view that moves one of its cards. Hand it over at the
 * start of the zone animation instead of leaving it behind in the tray. */
export function takeMovedCard(table: HTMLElement, slot: number) {
  const stage = table.closest<HTMLElement>(".stage");
  const slots = stage && loans.get(stage);
  const loan = slots?.get(slot);
  if (!stage || !slots || !loan) return;
  const top = loan.owners.at(-1);
  const box =
    top?.element.getBoundingClientRect() ??
    (loan.departure?.visible ? loan.departure.box : undefined);
  for (const owner of loan.owners) {
    owner.released = true;
    owner.element.setAttribute("data-card-borrowed", "");
  }
  loan.departure = undefined;
  slots.delete(slot);
  table.removeAttribute("data-card-borrowed");
  return box && stageBox(stage, box);
}

/** The ref wraps the visual, inside any wrapper used for hit testing or dragging. */
export function useBorrowedCard(
  ref: RefObject<HTMLElement | null>,
  slot: Slot | undefined,
) {
  const reduce = useReducedMotion();
  useLayoutEffect(() => {
    const element = ref.current;
    const stage = element?.closest<HTMLElement>(".stage");
    if (!element || !stage || slot === undefined) return;

    let slots = loans.get(stage);
    if (!slots) loans.set(stage, (slots = new Map()));
    let loan = slots.get(slot);
    if (!loan) slots.set(slot, (loan = { owners: [] }));
    const before = loan.owners.at(-1)?.element ?? tableCard(stage, slot);
    const origin = before?.querySelector(".card-body") ?? before;
    const last = loan.departure;
    loan.departure = undefined;
    const from =
      last?.visible && last.element !== element
        ? last.box
        : origin?.getBoundingClientRect();

    const owner = { element, released: false };
    loan.owners.push(owner);
    element.removeAttribute("data-card-borrowed");
    before?.setAttribute("data-card-borrowed", "");

    const list = element.closest(".zone-viewer-cards");
    const inView = (box: DOMRect) => {
      const clip = list?.getBoundingClientRect();
      return !clip || (box.top < clip.bottom && box.bottom > clip.top);
    };
    if (!reduce && from && inView(element.getBoundingClientRect())) {
      flyIn(element, from, stage);
    }

    return () => {
      const box = element.getBoundingClientRect();
      const animation = animations.get(element);
      const visible = animation?.moving === true || inView(box);
      animation?.stop();
      if (owner.released) return;
      const wasTop = loan.owners.at(-1) === owner;
      loan.owners.splice(loan.owners.indexOf(owner), 1);
      if (!wasTop) return;

      const departure = { element, box, visible };
      loan.departure = departure;
      // Use the destination from the completed commit. A new prompt in this commit
      // can also take the departure directly without an intermediate table frame.
      queueMicrotask(() => {
        if (loan.departure !== departure) return;
        loan.departure = undefined;
        const next = loan.owners.at(-1)?.element;
        if (!next) slots.delete(slot);
        const target = next ?? tableCard(stage, slot);
        if (!target) return;
        target.removeAttribute("data-card-borrowed");
        if (reduce || !visible || !box.width) return;
        if (next || slot === "library") {
          flyIn(target, box, stage);
        } else {
          target.dispatchEvent(
            new CustomEvent<CardHandoff>(CARD_RETURN, {
              detail: stageBox(stage, box),
            }),
          );
        }
      });
    };
  }, [ref, slot, reduce]);
}
