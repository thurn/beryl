// The spec pages: `/specs` lists the cards that have one, and `/specs/<card>` scrolls through
// every moment of play a card's UI must get right, each a live game from `beryl_cli
// specs-serve` (rust/crates/beryl_cli/src/specs), proxied by Vite under /spec-api. A moment's
// game runs in a frame of its own only while it is near the viewport: each holds an event
// stream open, and a browser allows a host only a few connections. Replay starts it afresh,
// which plays its actions out again.
import { useEffect, useRef, useState } from "react";
import { cardEntry } from "../cards";
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

function SpecPage({ spec }: { spec: SpecListing }) {
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
          <MomentView key={m.name} slug={spec.slug} moment={m} />
        ))}
      </main>
    </div>
  );
}

/** Whether `ref`'s element is within a screen of the viewport. */
function useNear(ref: React.RefObject<HTMLElement | null>): boolean {
  const [near, setNear] = useState(false);
  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const observer = new IntersectionObserver(
      ([entry]) => setNear(entry?.isIntersecting ?? false),
      { rootMargin: "50% 0px" },
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, [ref]);
  return near;
}

function MomentView({ slug, moment }: { slug: string; moment: MomentListing }) {
  const frame = useRef<HTMLDivElement>(null);
  const near = useNear(frame);
  // Each instance is a game of its own; a new one replays the moment.
  const [instance, setInstance] = useState(() => Date.now());
  const live = `${window.location.origin}${API}/m/${slug}/${moment.name}/${instance}`;
  const src = `/?live=${encodeURIComponent(live)}#/game`;
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
            onClick={() => setInstance(Date.now())}
          >
            ↻
          </button>
          <a
            className="specs-icon-button"
            href={`/?live=${encodeURIComponent(`${live}-full`)}#/game`}
            target="_blank"
            rel="noreferrer"
            title="Open full size"
            aria-label="Open full size"
          >
            ↗
          </a>
        </div>
      </header>
      <div className="specs-frame" ref={frame}>
        {near && <iframe key={instance} src={src} title={moment.title} />}
      </div>
    </section>
  );
}
