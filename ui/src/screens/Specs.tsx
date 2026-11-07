// The spec pages: `/specs` lists the cards that have one, and `/specs/<card>` scrolls through
// every moment of play a card's UI must get right, each a live game from `beryl_cli
// specs-serve` (rust/crates/beryl_cli/src/specs), proxied by Vite under /spec-api. A moment's
// game runs in a frame of its own from when it first nears the viewport, and stays; the page
// hands every frame its views from one event stream. Replay starts it afresh, which plays its
// actions out again.
import { useEffect, useMemo, useRef, useState } from "react";
import { cardEntry } from "../cards";
import type { RelayedEvent } from "../ipc";
import "./specs.css";

/** Where Vite proxies the spec server (vite.config.ts). */
const API = "/spec-api";

interface MomentListing {
  name: string;
  title: string;
  look: string;
  plays: boolean;
}

interface SpecListing {
  card: string;
  slug: string;
  moments: MomentListing[];
}

function useSpecs(): SpecListing[] | string | undefined {
  const [specs, setSpecs] = useState<SpecListing[] | string>();
  useEffect(() => {
    fetch(`${API}/specs`)
      .then((r) => {
        if (!r.ok) throw new Error(`HTTP ${r.status}`);
        return r.json() as Promise<SpecListing[]>;
      })
      .then(setSpecs, () => setSpecs("No spec server: run `just specs`."));
  }, []);
  return specs;
}

/** A card's image from data/cards.json, straight from Scryfall. */
function CardImage({ name, className }: { name: string; className?: string }) {
  const url = cardEntry(name)?.faces[0]?.images.normal;
  return url ? (
    <img className={className} src={url} alt={name} loading="lazy" />
  ) : (
    <div className={className} />
  );
}

/** The spec pages, by the path after `/specs`. */
export function Specs({ path }: { path: string }) {
  const specs = useSpecs();
  const slug = path.replace(/^\/specs\/?/, "").replace(/\/$/, "");
  useEffect(() => {
    document.title = "Beryl specs";
  }, []);
  if (specs === undefined) return <div className="specs" />;
  if (typeof specs === "string") {
    return (
      <div className="specs">
        <p className="specs-empty">{specs}</p>
      </div>
    );
  }
  if (!slug) return <SpecIndex specs={specs} />;
  const spec = specs.find((s) => s.slug === slug);
  return spec ? (
    <SpecPage spec={spec} />
  ) : (
    <div className="specs">
      <p className="specs-empty">No spec for {slug}.</p>
    </div>
  );
}

function SpecIndex({ specs }: { specs: SpecListing[] }) {
  return (
    <div className="specs">
      <h1 className="specs-heading">Specs</h1>
      <ul className="specs-index">
        {specs.map((s) => (
          <li key={s.slug}>
            <a href={`/specs/${s.slug}`} className="specs-index-card">
              <CardImage name={s.card} className="specs-index-image" />
              <span className="specs-index-name">{s.card}</span>
              <span className="specs-index-count">{s.moments.length}</span>
            </a>
          </li>
        ))}
      </ul>
    </div>
  );
}

/** A page's frames by their instance's key on the spec server, each handed its views. */
type Frames = Map<string, Window>;

function SpecPage({ spec }: { spec: SpecListing }) {
  // One event stream for the page, named for it, which carries every frame's views
  // (`specs-serve`'s /stream): a frame holding its own would use up the few connections a
  // browser allows a host, and frames would have to leave as others came.
  const page = useMemo(() => Math.random().toString(36).slice(2, 10), []);
  const frames = useRef<Frames>(new Map());
  const [open, setOpen] = useState(false);
  useEffect(() => {
    const stream = new EventSource(`${API}/stream/${page}`);
    stream.addEventListener("open", () => setOpen(true));
    stream.addEventListener("game-update", (e) => {
      const { instance, update } = JSON.parse(
        (e as MessageEvent<string>).data,
      ) as {
        instance: string;
        update: unknown;
      };
      const relayed: RelayedEvent = {
        kind: "beryl-relay",
        event: "game-update",
        payload: update,
      };
      frames.current
        .get(instance)
        ?.postMessage(relayed, window.location.origin);
    });
    return () => stream.close();
  }, [page]);
  return (
    <div className="specs specs-page">
      <nav className="specs-nav">
        <a href="/specs" className="specs-back" aria-label="All specs">
          ←
        </a>
        <CardImage name={spec.card} className="specs-card" />
        <ol className="specs-toc">
          {spec.moments.map((m) => (
            <li key={m.name}>
              <a href={`#${m.name}`}>{m.title}</a>
            </li>
          ))}
        </ol>
      </nav>
      <main className="specs-moments">
        {spec.moments.map((m) => (
          <MomentView
            key={m.name}
            slug={spec.slug}
            moment={m}
            page={page}
            open={open}
            frames={frames.current}
          />
        ))}
      </main>
    </div>
  );
}

/** Whether `ref`'s element has come within a screen of the viewport: once it has, it stays. */
function useReached(ref: React.RefObject<HTMLElement | null>): boolean {
  const [reached, setReached] = useState(false);
  useEffect(() => {
    const element = ref.current;
    if (!element || reached) return;
    const observer = new IntersectionObserver(
      ([entry]) => entry?.isIntersecting && setReached(true),
      { rootMargin: "100% 0px" },
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, [ref, reached]);
  return reached;
}

function MomentView({
  slug,
  moment,
  page,
  open,
  frames,
}: {
  slug: string;
  moment: MomentListing;
  page: string;
  open: boolean;
  frames: Frames;
}) {
  const holder = useRef<HTMLDivElement>(null);
  const reached = useReached(holder);
  // Each instance is a game of its own; a new one replays the moment.
  const [n, setN] = useState(0);
  const key = `${slug}/${moment.name}/${page}.${n}`;
  const live = `${window.location.origin}${API}/m/${key}`;
  const src = `/?live=${encodeURIComponent(live)}&relay#/game`;
  // Hidden until it shows its first view, over a placeholder.
  const [shown, setShown] = useState<string>();
  const frame = useRef<HTMLIFrameElement>(null);
  useEffect(() => {
    const onMessage = (e: MessageEvent<{ kind?: string }>) => {
      if (
        e.source === frame.current?.contentWindow &&
        e.data?.kind === "beryl-shown"
      ) {
        setShown(key);
      }
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [key]);
  const mount = (iframe: HTMLIFrameElement | null) => {
    frame.current = iframe;
    if (iframe?.contentWindow) frames.set(key, iframe.contentWindow);
    else frames.delete(key);
  };
  return (
    <section className="specs-moment" id={moment.name}>
      <header className="specs-moment-head">
        <h2>{moment.title}</h2>
        <p>{moment.look}</p>
        <div className="specs-moment-actions">
          <button
            type="button"
            className="specs-icon-button"
            title={moment.plays ? "Replay" : "Restart"}
            aria-label={moment.plays ? "Replay" : "Restart"}
            onClick={() => setN((x) => x + 1)}
          >
            ↻
          </button>
          <a
            className="specs-icon-button"
            href={`/?live=${encodeURIComponent(`${window.location.origin}${API}/m/${slug}/${moment.name}/${page}-full-${n}`)}#/game`}
            target="_blank"
            rel="noreferrer"
            title="Open full size"
            aria-label="Open full size"
          >
            ↗
          </a>
        </div>
      </header>
      <div
        className={
          shown === key ? "specs-frame" : "specs-frame specs-frame-loading"
        }
        ref={holder}
      >
        {reached && open && (
          <iframe key={key} ref={mount} src={src} title={moment.title} />
        )}
      </div>
    </section>
  );
}
