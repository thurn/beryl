import {
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { cardRecords } from "../cardRecords";
import { Button } from "../components/Controls";
import { pileTitle, ZoneViewer, type ViewerOptions } from "../game/Piles";
import { exiledWith, zonePiles } from "../game/zoneViews";
import type { ObjectView, PlayerView } from "../generated/protocol";
import { PileSheet } from "../stage/PhoneChrome";
import { REVEALED } from "../stage/Revealed.tsx";
import type { TableClient } from "./client";
import { pickViewerCard } from "./controls";
import { type OpenPile, type PileView, portionSlots } from "./viewers";

const noViewer = () => undefined;
const noSubscription = () => () => {};

export function usePileViewer(client: TableClient | undefined) {
  return useSyncExternalStore(
    client?.viewers?.subscribe ?? noSubscription,
    client?.viewers?.snapshot ?? noViewer,
  );
}

function contents(view: PlayerView, pile: PileView, reveals: ObjectView[]) {
  if (pile.zone === "revealed")
    return {
      title: REVEALED,
      cards: reveals.map(
        (card) =>
          view.objects.find(
            ({ slot, name }) => slot === card.slot && name === card.name,
          ) ?? card,
      ),
    };
  if (pile.zone === "linked")
    return {
      title:
        view.objects.find(({ slot }) => slot === pile.host)?.name ?? "Exile",
      cards: exiledWith(view, pile.host),
    };
  if (pile.zone === "graveyard" || pile.zone === "exile")
    return {
      title: pileTitle({ side: pile.side, zone: pile.zone }),
      cards:
        zonePiles(view).find(
          ({ side, zone }) => side === pile.side && zone === pile.zone,
        )?.cards ?? [],
    };
  const order =
    pile.zone === "library"
      ? portionSlots(view.players[pile.side].library, pile.portion).filter(
          (slot) => pile.slots?.has(slot) ?? true,
        )
      : (view.table?.orders[pile.side].sideboard ?? []);
  const objects = new Map(view.objects.map((object) => [object.slot, object]));
  return {
    title: pile.zone === "library" ? pileTitle(pile) : "Sideboard",
    cards: order.flatMap((slot) => objects.get(slot) ?? []),
  };
}

type Sort = "pile" | "name" | "mana";

function printed(object: ObjectView) {
  return object.name ? cardRecords.find(object.name)?.card : undefined;
}

function manaValue(object: ObjectView) {
  const card = printed(object);
  // Outside the stack, a split card uses the sum of its halves. Other
  // multi-face cards use the first face's cost.
  const faces = card?.layout === "split" ? card.faces : card?.faces.slice(0, 1);
  return (faces ?? []).reduce(
    (total, face) =>
      total +
      [...(face.mana_cost ?? "").matchAll(/\{([^}]+)\}/g)].reduce(
        (sum, [, symbol]) => {
          if (symbol === "X" || symbol === "Y" || symbol === "Z") return sum;
          const generic = Number(symbol?.split("/")[0]);
          return sum + (Number.isFinite(generic) ? generic : 1);
        },
        0,
      ),
    0,
  );
}

const cardTypes =
  /\b(Artifact|Battle|Creature|Enchantment|Instant|Kindred|Land|Planeswalker|Sorcery)\b/g;
function typeGroup(object: ObjectView) {
  const line = printed(object)?.faces[0]?.type_line.split("—")[0];
  return (
    (
      line?.match(cardTypes) ??
      object.types.map((type) => type[0]!.toUpperCase() + type.slice(1))
    ).join(" ") || "Other"
  );
}

function sections(
  cards: ObjectView[],
  query: string,
  sort: Sort,
  grouped: boolean,
) {
  const visible = [...cards]
    .reverse()
    .filter((object) =>
      query ? object.name?.toLocaleLowerCase().includes(query) : true,
    );
  const name = (a: ObjectView, b: ObjectView) =>
    (a.name ?? "").localeCompare(b.name ?? "");
  if (sort === "name") visible.sort(name);
  if (sort === "mana") {
    const cost = new Map(visible.map((card) => [card, manaValue(card)]));
    visible.sort((a, b) => cost.get(a)! - cost.get(b)! || name(a, b));
  }
  if (!grouped) return [{ cards: visible }];
  const groups = new Map<string, ObjectView[]>();
  for (const object of visible) {
    const label = typeGroup(object);
    const group = groups.get(label);
    if (group) group.push(object);
    else groups.set(label, [object]);
  }
  return [...groups]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([label, cards]) => ({ label, cards }));
}

export function TablePileViewer({
  client,
  view,
  pile,
  reveals,
  phone,
}: {
  client: TableClient;
  view: PlayerView;
  pile: OpenPile;
  reveals: ObjectView[];
  phone?: boolean;
}) {
  const { title, cards } = useMemo(
    () => contents(view, pile, reveals),
    [view, pile, reveals],
  );
  const viewers = client.viewers!;
  const [confirmShuffle, setConfirmShuffle] = useState(false);
  const revealed = pile.zone === "revealed";
  const topFirst =
    pile.zone === "library" ||
    pile.zone === "graveyard" ||
    pile.zone === "exile";
  const full = pile.zone === "library" && pile.portion === "all";
  const close = (shuffle = false) => {
    void viewers.close(pile, shuffle).catch(viewers.reportError);
  };
  const onClose = () => {
    if (full && !confirmShuffle) setConfirmShuffle(true);
    else close();
  };
  let footer;
  if (confirmShuffle) {
    footer = (
      <div className="table-viewer-actions">
        <Button onClick={() => close(true)}>Shuffle</Button>
        <Button variant="secondary" onClick={() => close()}>
          Don't shuffle
        </Button>
      </div>
    );
  } else if (
    phone &&
    pile.zone === "graveyard" &&
    zonePiles(view).some(
      ({ side, zone, cards }) =>
        side === pile.side && zone === "exile" && cards.length,
    )
  ) {
    footer = (
      <div className="table-viewer-actions">
        <Button
          variant="secondary"
          size="small"
          onClick={() => viewers.open({ zone: "exile", side: pile.side })}
        >
          View exile
        </Button>
      </div>
    );
  }
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<Sort>("pile");
  const [grouped, setGrouped] = useState(false);
  const search = useRef<HTMLInputElement>(null);
  const requestClose = useRef(onClose);
  requestClose.current = onClose;
  useLayoutEffect(
    () =>
      viewers.controls({
        close: () => requestClose.current(),
        search: () => search.current?.focus(),
        group: () => setGrouped((previous) => !previous),
        sort: setSort,
      }),
    [viewers],
  );

  const options: ViewerOptions = {
    drop: "side" in pile ? pile : undefined,
    actionable: revealed
      ? (card) => client.card(card.slot, true)?.object === card
      : undefined,
    topFirst: topFirst && sort === "pile" && !grouped,
    sections: sections(cards, query.trim().toLocaleLowerCase(), sort, grouped),
    controls: (
      <div className="table-viewer-controls">
        <input
          ref={search}
          type="search"
          aria-label="Search cards"
          placeholder="Search"
          value={query}
          onChange={(event) => setQuery(event.currentTarget.value)}
        />
        <label>
          <input
            type="checkbox"
            checked={grouped}
            onChange={(event) => setGrouped(event.currentTarget.checked)}
          />
          Group by type
        </label>
        <select
          aria-label="Sort"
          value={sort}
          onChange={(event) => {
            const value = event.currentTarget.value;
            if (value === "pile" || value === "name" || value === "mana")
              setSort(value);
          }}
        >
          <option value="pile">Pile order</option>
          <option value="name">Sort by name</option>
          <option value="mana">Sort by mana value</option>
        </select>
        {revealed && (
          <Button variant="secondary" onClick={onClose}>
            Hide
          </Button>
        )}
      </div>
    ),
    footer,
  };
  const shared = {
    client,
    title,
    pile: { cards },
    options,
    onClose,
    onPlay: (object: ObjectView, event: React.MouseEvent<HTMLElement>) =>
      pickViewerCard(client, object, event),
  };
  if (phone) return <PileSheet {...shared} onInspect={() => {}} />;
  return <ZoneViewer {...shared} />;
}
