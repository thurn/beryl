// Card display data from data/cards.json (written by `by cards`) and the art for it.
import { useEffect, useState } from "react";
import cardData from "../../data/cards.json";
import { artAvailable, cardImageUrl } from "./ipc";

export type ImageVersion = "art_crop" | "normal" | "large";

export interface CardFace {
  name: string;
  mana_cost?: string;
  type_line: string;
  oracle_text?: string;
  power?: string;
  toughness?: string;
  loyalty?: string;
  colors: string[];
  images: Record<ImageVersion, string>;
}

export interface CardEntry {
  name: string;
  scryfall_id: string;
  set: string;
  collector_number: string;
  layout: string;
  artist: string;
  /** Tokens and emblems only: the cards that make it. */
  made_by?: string[];
  faces: CardFace[];
}

const data = cardData as { cards: CardEntry[]; tokens: CardEntry[] };

// By card name and by each face's name, with the face it names: the names the engine gives
// faces, tokens and emblems (`by cards` writes tokens under them, and a beryl_cards test checks
// every one is here). Tokens come first so a card wins over a token of the same name
// (Thundertrap Trainer's offspring).
const byName = new Map<string, { card: CardEntry; face: number }>();
for (const card of [...data.tokens, ...data.cards]) {
  byName.set(card.name, { card, face: 0 });
  card.faces.forEach((f, face) => byName.set(f.name, { card, face }));
}

export const cardEntry = (name: string) => byName.get(name)?.card;

export interface CardArt {
  card: CardEntry;
  face: CardFace;
  /** The image once it has loaded; until then, or if it fails, show a placeholder. */
  src: string | undefined;
  status: "loading" | "loaded" | "failed";
}

/**
 * The display text and art of a card face, by card or face name. `face` picks a face by index
 * (a name of one face picks that face). Undefined for a name not in data/cards.json.
 */
export function useCardArt(
  name: string,
  face?: number,
  version: ImageVersion = "art_crop",
): CardArt | undefined {
  const found = byName.get(name);
  const entry = found?.card.faces[face ?? found.face];
  const url =
    entry && artAvailable() ? cardImageUrl(entry.images[version]) : undefined;
  const [loaded, setLoaded] = useState<{ url: string; ok: boolean }>();

  useEffect(() => {
    if (!url) {
      return;
    }
    const image = new Image();
    image.onload = () => setLoaded({ url, ok: true });
    image.onerror = () => setLoaded({ url, ok: false });
    image.src = url;
    return () => {
      image.onload = image.onerror = null;
    };
  }, [url]);

  if (!found || !entry) {
    return undefined;
  }
  const status = !url
    ? "failed"
    : !loaded || loaded.url !== url
      ? "loading"
      : loaded.ok
        ? "loaded"
        : "failed";
  return {
    card: found.card,
    face: entry,
    src: status === "loaded" ? url : undefined,
    status,
  };
}
