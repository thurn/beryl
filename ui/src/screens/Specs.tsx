// The spec pages: `/specs` lists the cards that have one, and `/specs/<card>` scrolls through
// every moment of play a card's UI must get right, each a live game from `beryl_cli
// specs-serve` (rust/crates/beryl_cli/src/specs), proxied by Vite under /spec-api. A moment's
// game runs in a frame of its own from when it first nears the viewport, and stays; the page
// hands every frame its views from one event stream. Each moment shows twice: from your seat,
// then again in the page's second half from the opponent's. Whatever would play after a frame's
// first view (the moment's own actions, or the game going on to the viewer's first decision)
// waits for its play button; replay starts it afresh.
import { useEffect, useMemo, useRef, useState } from "react";
import { cardEntry } from "../cards";
import type { RelayedEvent } from "../ipc";
import "./specs.css";

/** Where Vite proxies the spec server (vite.config.ts). */
const API = "/spec-api";

interface MomentListing {
  name: string;
  title: string;
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
              <a href={`#${anchor(m, "yours")}`}>{m.title}</a>
            </li>
          ))}
        </ol>
        <a href="#theirs" className="specs-toc-them">
          Them
        </a>
      </nav>
      <main className="specs-moments">
        {VIEWS.map((view) => (
          <div key={view} id={view}>
            {view === "theirs" && <h1 className="specs-section-head">Them</h1>}
            {spec.moments.map((m) => (
              <MomentView
                key={m.name}
                slug={spec.slug}
                moment={m}
                view={view}
                page={page}
                open={open}
                frames={frames.current}
              />
            ))}
          </div>
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

/** The seats a moment is seen from, in the spec server's words, each a frame of its own. */
const VIEWS = ["yours", "theirs"] as const;
type View = (typeof VIEWS)[number];

/** A moment's place on the page, from one seat. */
function anchor(moment: MomentListing, view: View): string {
  return view === "yours" ? moment.name : `${moment.name}-theirs`;
}

/** One moment from one seat: what made it, and its live game. */
function MomentView({
  slug,
  moment,
  view,
  page,
  open,
  frames,
}: {
  slug: string;
  moment: MomentListing;
  view: View;
  page: string;
  open: boolean;
  frames: Frames;
}) {
  const holder = useRef<HTMLDivElement>(null);
  const reached = useReached(holder);
  // Each instance is a game of its own; a new one replays the moment.
  const [n, setN] = useState(0);
  const key = `${slug}/${moment.name}/${view}/${page}.${n}`;
  const live = `${window.location.origin}${API}/m/${key}`;
  const src = `/?live=${encodeURIComponent(live)}&relay#/game`;
  const full = `${slug}/${moment.name}/${view}/${page}-full-${n}`;
  // Hidden until it shows its first view, over a placeholder.
  const [shown, setShown] = useState<string>();
  // Whether it holds views to play, by instance, asked once it has shown.
  const [held, setHeld] = useState<Record<string, boolean>>({});
  // The instance whose held views have been asked to play out.
  const [played, setPlayed] = useState<string>();
  useEffect(() => {
    if (shown !== key || held[key] !== undefined) return;
    void fetch(`${API}/m/${key}/held`)
      .then((r) => r.json() as Promise<{ ok?: boolean }>)
      .then(
        (r) => r.ok === true,
        () => false,
      )
      .then((holds) => setHeld((h) => ({ ...h, [key]: holds })));
  }, [shown, key, held]);
  const waiting = held[key] !== false && played !== key;
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
    <section className="specs-moment" id={anchor(moment, view)}>
      <header className="specs-moment-head">
        <h2>{moment.title}</h2>
        <div className="specs-moment-actions">
          {waiting ? (
            <button
              type="button"
              className="specs-icon-button"
              title="Play"
              aria-label="Play"
              disabled={held[key] === undefined}
              onClick={() => {
                setPlayed(key);
                void fetch(`${API}/m/${key}/play`, { method: "POST" });
              }}
            >
              ▶
            </button>
          ) : (
            <button
              type="button"
              className="specs-icon-button"
              title={played === key ? "Replay" : "Restart"}
              aria-label={played === key ? "Replay" : "Restart"}
              onClick={() => setN((x) => x + 1)}
            >
              ↻
            </button>
          )}
          <a
            className="specs-icon-button"
            href={`/?live=${encodeURIComponent(`${window.location.origin}${API}/m/${full}`)}#/game`}
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
