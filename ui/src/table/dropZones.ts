import { CARD_ASPECT } from "../components/Card";
import { battlefieldRow } from "../game/battlefield";
import { HAND, type Box } from "../game/placement";
import { STAGE } from "../game/perspective";
import { PILES, pileLook } from "../game/pileLook";
import { STACK } from "../game/stackLayout";
import { ROWS } from "../game/tableRows";
import type { RowName, Side } from "../generated/protocol";
import type { PhoneLayout, Viewport } from "../mobile/layout";
import { named } from "../stage/dom";
import type { Name, Point } from "../stage/layout";
import type { CardTarget, TableClient } from "./client";

type Pile = "library" | "graveyard" | "exile" | "sideboard";
type AreaKind = Pile | "hand" | "stack" | RowName;
export interface DropArea {
  kind: AreaKind;
  side: Side;
  box: Box;
}
export interface Destination extends DropArea {
  card?: CardTarget;
  position?: "top" | "bottom";
}

const box = (
  left: number,
  top: number,
  right: number,
  bottom: number,
): Box => ({ left, top, right, bottom });

export function desktopDropAreas(): DropArea[] {
  const { width, height } = STAGE;
  const middle = (ROWS.opponent.permanents + ROWS.you.permanents) / 2 + 54;
  const far = ROWS.opponent.permanents - 12;
  const near = ROWS.you.back - 7;
  return [
    ...(["opponent", "you"] as const).flatMap((side): DropArea[] => {
      const yours = side === "you";
      const row = ROWS[side];
      const backTop = yours ? near : row.back;
      const backBottom = yours ? HAND.top : far;
      return [
        ...(["library", "graveyard", "exile"] as const).map((kind) => {
          const at = pileLook(side, PILES[side][kind]);
          return {
            side,
            kind,
            box: box(
              at.x,
              at.y,
              at.x + at.width,
              at.y + at.width / CARD_ASPECT,
            ),
          };
        }),
        {
          side,
          kind: "hand",
          box: box(0, yours ? HAND.top : 0, width, yours ? height : 48),
        },
        { side, kind: "lands", box: box(0, backTop, width / 2, backBottom) },
        {
          side,
          kind: "others",
          box: box(width / 2, backTop, width, backBottom),
        },
        {
          side,
          kind: "creatures",
          box: box(0, yours ? middle : far, width, yours ? near : middle),
        },
      ];
    }),
    {
      side: "you" as const,
      kind: "stack" as const,
      box: box(
        STACK.x,
        STACK.foot - STACK.width / CARD_ASPECT,
        STACK.x + STACK.width,
        STACK.foot,
      ),
    },
  ].sort((a, b) => Number(b.kind === "stack") - Number(a.kind === "stack"));
}

export function phoneDropAreas(
  bands: PhoneLayout["bands"],
  { width, height }: Viewport,
): DropArea[] {
  return [
    {
      side: "you",
      kind: "stack",
      box: box(
        width * 0.55,
        bands.middle.top,
        width,
        bands.middle.top + bands.middle.height,
      ),
    },
    ...(["opponent", "you"] as const).flatMap((side): DropArea[] => {
      const yours = side === "you";
      const hand = yours ? bands.hand : bands.opponentStrip;
      const rows = yours
        ? [bands.yourCreatures, bands.yourOther, bands.yourLands]
        : [bands.opponentCreatures, bands.opponentOther, bands.opponentLands];
      return [
        {
          side,
          kind: "hand",
          box: box(
            0,
            hand.top,
            width,
            Math.min(height, hand.top + hand.height),
          ),
        },
        ...(["creatures", "others", "lands"] as const).map((kind, i) => {
          const row = rows[i]!;
          return {
            side,
            kind,
            box: box(0, row.top, width, row.top + row.height),
          };
        }),
      ];
    }),
  ];
}

const zones = ["library", "graveyard", "exile", "sideboard", "hand"] as const;
export const pileZone = (zone: string | undefined) =>
  zones.find((kind) => kind === zone);

export function destinationAt(
  client: TableClient,
  areas: readonly DropArea[],
  at: Point,
  held: readonly Name[],
): Destination | undefined {
  const surface = client.surface;
  if (!surface) return;
  let viewer = false;
  for (const hit of surface.hits(at)) {
    const pile = hit.closest<HTMLElement>("[data-pile][data-side]");
    const kind = pileZone(pile?.dataset.pile);
    const side = pile?.dataset.side;
    if (pile && kind && (side === "you" || side === "opponent")) {
      return located({ kind, side, box: surface.bounds(pile) }, at);
    }
    if (hit.closest("[data-selection-viewer], .viewer-scrim, .phone-scrim"))
      viewer = true;
    else if (!viewer && hit.closest("button, [role=menu], input")) return;
    const node = hit.closest<HTMLElement>("[data-stage]");
    const said = node?.dataset.stage;
    if (said === undefined || !node || held.includes(named(said))) continue;
    const card = client.card(named(said));
    const object = card?.object;
    const bounds = surface.bounds(node);
    if (card && object?.zone === "battlefield") {
      return {
        kind: battlefieldRow(object),
        side: object.controller,
        box: bounds,
        card,
      };
    }
    if (card && object?.zone === "hand") {
      return {
        kind: "hand",
        side: object.zoneSide ?? object.owner,
        box: bounds,
      };
    }
    if (card?.kind === "stack_entry") {
      return { kind: "stack", side: "you", box: bounds };
    }
  }
  const area = areas.find(
    ({ box }) =>
      at.x >= box.left &&
      at.x <= box.right &&
      at.y >= box.top &&
      at.y <= box.bottom,
  );
  return area && located(area, at);
}

function located(area: DropArea, at: Point): Destination {
  return area.kind === "library"
    ? {
        ...area,
        position:
          at.y < (area.box.top + area.box.bottom) / 2 ? "top" : "bottom",
      }
    : area;
}
