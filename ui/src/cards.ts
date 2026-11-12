// Card display data and the art for it: the card store's records (data/card-store.tsv), which
// the page asks for by name and caches (cardRecords.ts). A game's views arrive with their
// cards' records cached already (ipc.ts), so these lookups answer at once.
import { useEffect, useState } from "react";
import { cardRecords, type Ask } from "./cardRecords";
import type {
  CardEntry,
  CardFace,
  CardImages,
  ObjectView,
} from "./generated/protocol";
import { artAvailable, cardImageUrl } from "./ipc";

export type { CardEntry, CardFace };
export type ImageVersion = keyof CardImages;

/** The card a card or face name names; a token's where no card has the name. */
export const cardEntry = (name: string) => cardRecords.find(name)?.card;

/** The face a card named `name` is played with as a land: its first land face
 * ([CR 712.9](https://yawgatog.com/resources/magic-rules/#R7129)), as Sink into Stupor is
 * played as Soporific Springs. */
export const landFace = (name: string) =>
  cardEntry(name)?.faces.find((f) => /\bLand\b/.test(f.type_line));

/** The face a card or face name names: a card's first, or the face of that name. */
export function faceEntry(name: string): CardFace | undefined {
  const found = cardRecords.find(name);
  return found?.card.faces[found.face];
}

/** The printed face of a token named `name` where it has its own, as Thundertrap Trainer's
 * offspring, a 1/1 copy, has beside the card it shares a name with. */
export const tokenFace = (name: string) => cardRecords.token(name)?.faces[0];

/** Caches the records of `names`, asking `ask` for those it lacks, and draws its caller again
 * once it has them: for a screen that shows cards outside a game, whose views bring theirs. */
export function useCardRecords(names: string[], ask: Ask) {
  const [, setLoaded] = useState<string>();
  const key = JSON.stringify(names);
  useEffect(() => {
    let mounted = true;
    void Promise.resolve(
      cardRecords.load(JSON.parse(key) as string[], ask),
    ).then(() => mounted && setLoaded(key));
    return () => {
      mounted = false;
    };
  }, [key, ask]);
}

export interface CardArt {
  card: CardEntry;
  face: CardFace;
  /** The image once it has loaded; until then, or if it fails, show a placeholder. */
  src: string | undefined;
  status: "loading" | "loaded" | "failed";
}

/**
 * The display text and art of a card face, by card or face name. `face` picks a face by index
 * (a name of one face picks that face). Undefined for a name the card store has no record of.
 */
export function useCardArt(
  name: string,
  face?: number,
  version: ImageVersion = "art_crop",
): CardArt | undefined {
  const found = cardRecords.find(name);
  const entry = found?.card.faces[face ?? found.face];
  const url =
    entry && artAvailable() ? cardImageUrl(entry.images[version]) : undefined;
  const status = useImage(url);
  if (!found || !entry) {
    return undefined;
  }
  return {
    card: found.card,
    face: entry,
    src: status === "loaded" ? url : undefined,
    status,
  };
}

/** The card back's image once it has loaded: undefined until then, if it fails, and where
 * images do not load (`artAvailable`). */
export function useCardBack(): string | undefined {
  return useLoaded(cardRecords.cardBack());
}

/** The art of `o` if it is a token with no name, once it has loaded, as `useCardBack`. */
export function useNamelessTokenArt(o: ObjectView): string | undefined {
  return useLoaded(cardRecords.namelessArt(o));
}

function useLoaded(scryfallUrl: string | undefined): string | undefined {
  const url =
    scryfallUrl && artAvailable() ? cardImageUrl(scryfallUrl) : undefined;
  return useImage(url) === "loaded" ? url : undefined;
}

/** Whether the image at `url` has loaded. */
function useImage(url: string | undefined): CardArt["status"] {
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

  return !url
    ? "failed"
    : !loaded || loaded.url !== url
      ? "loading"
      : loaded.ok
        ? "loaded"
        : "failed";
}
