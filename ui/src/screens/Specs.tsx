// The spec pages: `/specs` lists the cards that have one, and `/specs/<card>` scrolls through
// every moment of play a card's UI must get right, each a live game from `beryl_cli
// specs-serve` (rust/crates/beryl_cli/src/specs), proxied by Vite under /spec-api. A moment's
// game runs in a frame of its own from when it first nears the viewport, and stays; the page
// hands every frame its views from one event stream. Each moment shows twice, side by side: from
// your seat and from the opponent's, the same game played in step. A moment that plays actions
// out holds them until its play button, which plays both; replay starts both afresh.
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
  /** What the opponent should see of it. */
  theirs: string;
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

/** The seats a moment is seen from, in the spec server's words, each a frame of its own. */
const VIEWS = ["yours", "theirs"] as const;
type View = (typeof VIEWS)[number];

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
  // Each instance is a game of its own; a new one replays the moment, from both seats.
  const [n, setN] = useState(0);
  const instance = `${page}.${n}`;
  const key = (view: View) => `${slug}/${moment.name}/${view}/${instance}`;
  // The views that have shown their first view, by instance.
  const [shown, setShown] = useState<Record<string, true>>({});
  // The instance whose actions have been asked to play out.
  const [played, setPlayed] = useState<string>();
  const waiting = moment.plays && played !== instance;
  const ready = VIEWS.every((v) => shown[key(v)]);
  return (
    <section className="specs-moment" id={moment.name}>
      <header className="specs-moment-head">
        <h2>{moment.title}</h2>
        <div className="specs-moment-actions">
          {waiting ? (
            <button
              type="button"
              className="specs-icon-button"
              title="Play"
              aria-label="Play"
              disabled={!ready}
              onClick={() => {
                setPlayed(instance);
                for (const v of VIEWS) {
                  void fetch(`${API}/m/${key(v)}/play`, { method: "POST" });
                }
              }}
            >
              ▶
            </button>
          ) : (
            <button
              type="button"
              className="specs-icon-button"
              title={moment.plays ? "Replay" : "Restart"}
              aria-label={moment.plays ? "Replay" : "Restart"}
              onClick={() => setN((x) => x + 1)}
            >
              ↻
            </button>
          )}
        </div>
      </header>
      <div className="specs-seats" ref={holder}>
        {VIEWS.map((view) => (
          <Seat
            key={view}
            view={view}
            look={view === "yours" ? moment.look : moment.theirs}
            title={moment.title}
            instanceKey={key(view)}
            full={`${slug}/${moment.name}/${view}/${page}-full-${n}`}
            mounted={reached && open}
            shown={!!shown[key(view)]}
            onShown={() => setShown((s) => ({ ...s, [key(view)]: true }))}
            frames={frames}
          />
        ))}
      </div>
    </section>
  );
}

/** One seat's frame of a moment: what to look for there, and its live game. */
function Seat({
  view,
  look,
  title,
  instanceKey,
  full,
  mounted,
  shown,
  onShown,
  frames,
}: {
  view: View;
  look: string;
  title: string;
  instanceKey: string;
  full: string;
  mounted: boolean;
  shown: boolean;
  onShown: () => void;
  frames: Frames;
}) {
  const live = `${window.location.origin}${API}/m/${instanceKey}`;
  const src = `/?live=${encodeURIComponent(live)}&relay#/game`;
  const frame = useRef<HTMLIFrameElement>(null);
  const shownRef = useRef(onShown);
  shownRef.current = onShown;
  useEffect(() => {
    const onMessage = (e: MessageEvent<{ kind?: string }>) => {
      if (
        e.source === frame.current?.contentWindow &&
        e.data?.kind === "beryl-shown"
      ) {
        shownRef.current();
      }
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [instanceKey]);
  const mount = (iframe: HTMLIFrameElement | null) => {
    frame.current = iframe;
    if (iframe?.contentWindow) frames.set(instanceKey, iframe.contentWindow);
    else frames.delete(instanceKey);
  };
  return (
    <div className="specs-seat">
      <p className="specs-seat-look">
        <span className="specs-seat-name">
          {view === "yours" ? "You" : "Them"}
        </span>
        {look}
        <a
          className="specs-seat-full"
          href={`/?live=${encodeURIComponent(`${window.location.origin}${API}/m/${full}`)}#/game`}
          target="_blank"
          rel="noreferrer"
          title="Open full size"
          aria-label="Open full size"
        >
          ↗
        </a>
      </p>
      <div
        className={shown ? "specs-frame" : "specs-frame specs-frame-loading"}
      >
        {mounted && (
          <iframe key={instanceKey} ref={mount} src={src} title={title} />
        )}
      </div>
    </div>
  );
}
