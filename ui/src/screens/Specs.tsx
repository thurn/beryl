// The spec pages: `/specs` lists the cards that have one, and `/specs/<card>` scrolls through
// every moment of play a card's UI must get right, each a live game from `beryl_cli
// specs-serve` (rust/crates/beryl_cli/src/specs), proxied by Vite under /spec-api. A moment's
// game runs in a frame of its own from when it first nears the viewport, and stays; the page
// hands every frame its views from one event stream. Each moment shows twice: from your seat,
// then again in the page's second half from the opponent's. Whatever would play after a frame's
// first view (the moment's own actions, or the game going on to the viewer's first decision)
// waits for its play button; replay starts it afresh.
//
// Reviewing: the index lists the cards not yet marked reviewed first, and a card's page marks
// it (the spec server keeps the marks, `specs/reviews.rs`). Each moment's 🐞 files a note on it
// as a bug report, for `just bugs` and the `bugfix` skill, naming the moment and seat.
import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { SoundPanel } from "../audio/SoundControls";
import { cardEntry } from "../cards";
import type { RelayedEvent } from "../ipc";
import "./specs.css";

/** Where Vite proxies the spec server (vite.config.ts). */
const API = "/spec-api";

interface MomentListing {
  name: string;
  title: string;
  /** The scenario it comes from, by name; empty for the card's own moments. */
  group: string;
  plays: boolean;
  first?: View;
  roles?: [string, string];
  replay?: string | null;
}

interface ReviewListing {
  examples: MomentListing[];
  unreached: string[];
}

interface SpecListing {
  card: string;
  slug: string;
  moments: MomentListing[];
  /** When it was marked reviewed, or null. */
  reviewed: string | null;
  /** Its open bug reports. */
  reports: number;
}

/** A spec server answer: `{ok}` or `{err}`, as `serve`'s commands answer. */
async function post<T>(path: string, body: unknown): Promise<T> {
  const r = await fetch(`${API}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const answer = (await r.json()) as { ok?: T; err?: { message?: string } };
  if ("err" in answer) throw new Error(answer.err?.message ?? "refused");
  return answer.ok as T;
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
  const [slug, only, seat] = path
    .replace(/^\/specs\/?/, "")
    .replace(/\/$/, "")
    .split("/");
  const manual = !!slug && (!only || only.startsWith("review-"));
  const [checkpoints, setCheckpoints] = useState<ReviewListing | string>();
  useEffect(() => {
    if (!manual) return;
    let cancelled = false;
    fetch(`${API}/checkpoints/${slug}`)
      .then((r) => {
        if (!r.ok) throw new Error();
        return r.json() as Promise<ReviewListing>;
      })
      .then(
        (moments) => {
          if (!cancelled) setCheckpoints(moments);
        },
        () => {
          if (!cancelled) setCheckpoints("Review examples could not load.");
        },
      );
    return () => {
      cancelled = true;
    };
  }, [manual, slug]);
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
  if (spec && manual) {
    if (!checkpoints)
      return <div className="specs">Loading review examples…</div>;
    if (typeof checkpoints === "string")
      return <div className="specs">{checkpoints}</div>;
  }
  const review =
    manual && typeof checkpoints !== "string" ? checkpoints : undefined;
  const moments = review?.examples ?? spec?.moments ?? [];
  const moment = moments.find((m) => m.name === only);
  if (spec && moment && seat) {
    return <SeatRedirect slug={spec.slug} moment={moment.name} seat={seat} />;
  }
  return spec && (moment || !only || only === "replay") ? (
    <SpecPage
      spec={spec}
      moments={moments}
      only={moment}
      manual={manual}
      unreached={review?.unreached}
    />
  ) : (
    <div className="specs">
      <p className="specs-empty">
        No spec for {slug}
        {only ? ` / ${only}` : ""}.
      </p>
    </div>
  );
}

/** `/specs/<card>/<moment>/<yours|theirs>`: the moment from one seat, full size, as the game
 * screen plays it: a fresh game each load, for a driver (`by specs --drive`) or a link. With
 * `?hold` it waits for its spec server instance's `play`, so a driver can start watching
 * first; otherwise it plays a beat after it shows. */
function SeatRedirect({
  slug,
  moment,
  seat,
}: {
  slug: string;
  moment: string;
  seat: string;
}) {
  useEffect(() => {
    const search = new URLSearchParams(window.location.search);
    const hold = search.has("hold");
    const shared = search.get("instance");
    const id =
      shared && /^[a-z0-9-]+$/.test(shared)
        ? shared
        : `${hold ? "hold-" : "seat-"}${Math.random().toString(36).slice(2, 10)}`;
    const live = `${window.location.origin}${API}/m/${slug}/${moment}/${seat}/${id}`;
    window.location.replace(`/?live=${encodeURIComponent(live)}#/game`);
  }, [slug, moment, seat]);
  return <div className="specs" />;
}

function SpecIndex({ specs }: { specs: SpecListing[] }) {
  const todo = specs.filter((s) => !s.reviewed);
  const done = specs.filter((s) => s.reviewed);
  return (
    <div className="specs">
      <header className="specs-index-head">
        <h1 className="specs-heading">
          Specs{" "}
          <span className="specs-heading-count">
            {done.length}/{specs.length}
          </span>
        </h1>
        <SoundPanel />
        <a
          className="specs-index-export"
          href={`${API}/bug-reports.zip`}
          download
          title="Includes screenshots, replays, and logs"
        >
          Export Bug Reports
        </a>
      </header>
      <SpecCards specs={todo} />
      {done.length > 0 && todo.length > 0 && (
        <hr className="specs-index-rule" />
      )}
      <SpecCards specs={done} />
    </div>
  );
}

function SpecCards({ specs }: { specs: SpecListing[] }) {
  if (specs.length === 0) return null;
  return (
    <ul className="specs-index">
      {specs.map((s) => (
        <li key={s.slug}>
          <a
            href={`/specs/${s.slug}`}
            className={
              s.reviewed
                ? "specs-index-card specs-index-reviewed"
                : "specs-index-card"
            }
          >
            <CardImage name={s.card} className="specs-index-image" />
            <span className="specs-index-name">{s.card}</span>
            <span className="specs-index-marks">
              {s.reports > 0 && (
                <span className="specs-bugs" title="Open bug reports">
                  🐞{s.reports}
                </span>
              )}
              {s.reviewed ? (
                <span className="specs-check" title="Reviewed">
                  ✓
                </span>
              ) : (
                <span className="specs-index-count">{s.moments.length}</span>
              )}
            </span>
          </a>
        </li>
      ))}
    </ul>
  );
}

/** Marks the card reviewed, or not, and shows its open reports. */
function ReviewBar({ spec, reports }: { spec: SpecListing; reports: number }) {
  const [reviewed, setReviewed] = useState(spec.reviewed !== null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const toggle = () => {
    setBusy(true);
    setError(false);
    post<{ reviewed: string | null; next: string | null }>(
      `/review/${spec.slug}`,
      { reviewed: !reviewed },
    )
      .then((r) => {
        setReviewed(r.reviewed !== null);
        if (r.reviewed !== null) {
          window.location.replace(r.next ? `/specs/${r.next}` : "/specs");
        }
      })
      .catch(() => setError(true))
      .finally(() => setBusy(false));
  };
  return (
    <div className="specs-review">
      <button
        type="button"
        className={
          reviewed ? "specs-reviewed specs-reviewed-on" : "specs-reviewed"
        }
        aria-pressed={reviewed}
        disabled={busy}
        onClick={toggle}
      >
        ✓ {reviewed ? "Reviewed" : "Mark reviewed"}
      </button>
      {error && <span role="alert">Save failed</span>}
      {reports > 0 && (
        <span className="specs-bugs" title="Open bug reports">
          🐞{reports}
        </span>
      )}
    </div>
  );
}

/** A page's frames by their instance's key on the spec server, each handed its views. */
type Frames = Map<string, Window>;

/** A card's spec: every moment from your seat, then every moment from theirs; or, given
 * `only`, that moment from both seats side by side. */
function SpecPage({
  spec,
  moments,
  only,
  manual,
  unreached,
}: {
  spec: SpecListing;
  moments: MomentListing[];
  only?: MomentListing;
  manual: boolean;
  unreached?: string[];
}) {
  // One event stream for the page, named for it, which carries every frame's views
  // (`specs-serve`'s /stream): a frame holding its own would use up the few connections a
  // browser allows a host, and frames would have to leave as others came.
  const page = useMemo(() => Math.random().toString(36).slice(2, 10), []);
  const frames = useRef<Frames>(new Map());
  const [open, setOpen] = useState(false);
  const [reports, setReports] = useState(spec.reports);
  const filed = () => setReports((n) => n + 1);
  const [resets, setResets] = useState<Record<string, number>>({});
  const reset = (name: string) =>
    setResets((r) => ({ ...r, [name]: (r[name] ?? 0) + 1 }));
  const shownMoments = only && manual ? [only] : moments;
  const firstSeat = (m: MomentListing) => m.first ?? "yours";
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
        <SoundPanel />
        <CardImage name={spec.card} className="specs-card" />
        <ReviewBar spec={spec} reports={reports} />
        <a
          className="specs-mode"
          href={manual ? `/specs/${spec.slug}/replay` : `/specs/${spec.slug}`}
        >
          {manual ? "Scenario replays" : "Manual review"}
        </a>
        {manual ? (
          <ol className="specs-toc">
            {moments.map((m) => (
              <li key={m.name}>
                <a
                  href={
                    only
                      ? `/specs/${spec.slug}/${m.name}`
                      : `#${anchor(m, firstSeat(m))}`
                  }
                >
                  {m.title}
                </a>
              </li>
            ))}
          </ol>
        ) : (
          (only ? (["yours"] as const) : VIEWS).map((view) => (
            <ol key={view} className="specs-toc">
              {view === "theirs" && (
                <li>
                  <a href="#theirs" className="specs-toc-them">
                    <SeatBadge view={view} />
                  </a>
                </li>
              )}
              {moments.map((m, i) => (
                <Fragment key={m.name}>
                  {startsGroup(moments, i) && (
                    <li className="specs-toc-group">{m.group}</li>
                  )}
                  <li>
                    <a
                      href={
                        only
                          ? `/specs/${spec.slug}/${m.name}`
                          : `#${anchor(m, view)}`
                      }
                      aria-current={m === only ? "page" : undefined}
                    >
                      {m.title}
                    </a>
                  </li>
                </Fragment>
              ))}
            </ol>
          ))
        )}
      </nav>
      {manual ? (
        <main className="specs-moments">
          {shownMoments.length === 0 && (
            <p className="specs-empty">No review checkpoints were reached.</p>
          )}
          {!!unreached?.length && (
            <details>
              <summary>Scenarios without review examples</summary>
              <ul>
                {unreached.map((name) => (
                  <li key={name}>{name}</li>
                ))}
              </ul>
            </details>
          )}
          {shownMoments.map((m) => (
            <div key={m.name}>
              {[firstSeat(m), otherSeat(firstSeat(m))].map((view) => (
                <MomentView
                  key={view}
                  slug={spec.slug}
                  moment={m}
                  view={view}
                  page={page}
                  open={open}
                  frames={frames.current}
                  onFiled={filed}
                  review={{
                    revision: resets[m.name] ?? 0,
                    reset: () => reset(m.name),
                  }}
                />
              ))}
            </div>
          ))}
        </main>
      ) : only ? (
        <main className="specs-moments specs-pair">
          {VIEWS.map((view) => (
            <MomentView
              key={view}
              slug={spec.slug}
              moment={only}
              view={view}
              page={page}
              open={open}
              frames={frames.current}
              onFiled={filed}
            />
          ))}
        </main>
      ) : (
        <main className="specs-moments">
          {VIEWS.map((view) => (
            <div key={view} id={view}>
              {view === "theirs" && (
                <h1 className="specs-section-head">
                  <SeatBadge view={view} /> Opponent's seat
                </h1>
              )}
              {moments.map((m, i) => (
                <Fragment key={m.name}>
                  {startsGroup(moments, i) && (
                    <h2 className="specs-group-head">{m.group}</h2>
                  )}
                  <MomentView
                    slug={spec.slug}
                    moment={m}
                    view={view}
                    page={page}
                    open={open}
                    frames={frames.current}
                    onFiled={filed}
                  />
                </Fragment>
              ))}
            </div>
          ))}
        </main>
      )}
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

/** Whether the moment at `i` heads a scenario's moments: the first from it. */
function startsGroup(moments: MomentListing[], i: number): boolean {
  const group = moments[i]?.group;
  return !!group && moments[i - 1]?.group !== group;
}

/** The seats a moment is seen from, in the spec server's words, each a frame of its own. */
const VIEWS = ["yours", "theirs"] as const;
type View = (typeof VIEWS)[number];

function otherSeat(view: View): View {
  return view === "yours" ? "theirs" : "yours";
}

/** Each seat as a moment's header names it. */
const SEAT_NAMES: Record<View, string> = { yours: "You", theirs: "Them" };

/** Which seat a moment is seen from, tagged ahead of its title: Them stands out. */
function SeatBadge({ view }: { view: View }) {
  return (
    <span className={`specs-seat specs-seat-${view}`}>{SEAT_NAMES[view]}</span>
  );
}

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
  onFiled,
  review,
}: {
  slug: string;
  moment: MomentListing;
  view: View;
  page: string;
  open: boolean;
  frames: Frames;
  onFiled: () => void;
  review?: { revision: number; reset: () => void };
}) {
  const holder = useRef<HTMLDivElement>(null);
  const reached = useReached(holder);
  // Each instance is a game of its own; a new one replays the moment.
  const [n, setN] = useState(0);
  const key = `${slug}/${moment.name}/${view}/${page}.${review?.revision ?? n}`;
  const live = `${window.location.origin}${API}/m/${key}`;
  const src = `/?live=${encodeURIComponent(live)}&relay#/game`;
  // Hidden until it shows its first view, over a placeholder.
  const [shown, setShown] = useState<string>();
  // Whether it holds views to play, by instance, asked once it has shown.
  const [held, setHeld] = useState<Record<string, boolean>>({});
  // The instance whose held views have been asked to play out.
  const [played, setPlayed] = useState<string>();
  useEffect(() => {
    if (review || shown !== key || held[key] !== undefined) return;
    void fetch(`${API}/m/${key}/held`)
      .then((r) => r.json() as Promise<{ ok?: boolean }>)
      .then(
        (r) => r.ok === true,
        () => false,
      )
      .then((holds) => setHeld((h) => ({ ...h, [key]: holds })));
  }, [shown, key, held, review]);
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
  const [noting, setNoting] = useState(false);
  const [filed, setFiled] = useState(0);
  const mount = (iframe: HTMLIFrameElement | null) => {
    frame.current = iframe;
    if (iframe?.contentWindow) frames.set(key, iframe.contentWindow);
    else frames.delete(key);
  };
  return (
    <section className="specs-moment" id={anchor(moment, view)}>
      <header className="specs-moment-head">
        <h2>
          {review ? (
            <span className="specs-seat">
              {moment.roles?.[view === "yours" ? 0 : 1]}
            </span>
          ) : (
            <SeatBadge view={view} />
          )}
          {moment.title}
        </h2>
        <div className="specs-moment-actions">
          {review && moment.replay && (
            <a
              className="specs-switch-seat"
              href={`/specs/${slug}/${moment.replay}/${view}`}
              target="_blank"
              rel="noreferrer"
            >
              Watch counter
            </a>
          )}
          {review && (
            <a
              className="specs-switch-seat"
              href={`#${anchor(moment, otherSeat(view))}`}
            >
              Switch seat
            </a>
          )}
          {review ? (
            <button
              type="button"
              className="specs-icon-button"
              title="Reset"
              aria-label="Reset"
              onClick={review.reset}
            >
              ↻
            </button>
          ) : waiting ? (
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
          <button
            type="button"
            className={
              filed > 0 ? "specs-icon-button specs-filed" : "specs-icon-button"
            }
            title="Report a bug"
            aria-label="Report a bug"
            aria-expanded={noting}
            data-filed={filed || undefined}
            onClick={() => setNoting((x) => !x)}
          >
            🐞
          </button>
          <a
            className="specs-icon-button"
            href={`/specs/${slug}/${moment.name}`}
            title="This moment"
            aria-label="This moment"
          >
            #
          </a>
          <a
            className="specs-icon-button"
            href={`/specs/${slug}/${moment.name}/${view}`}
            target="_blank"
            rel="noreferrer"
            title="Open full size"
            aria-label="Open full size"
          >
            ↗
          </a>
        </div>
      </header>
      {review && moment.replay && view === moment.first && (
        <p className="specs-review-hint">Cast, then switch seats to respond.</p>
      )}
      {noting && (
        <BugNote
          instance={key}
          target={anchor(moment, view)}
          onClose={() => setNoting(false)}
          onFiled={() => {
            setNoting(false);
            setFiled((n) => n + 1);
            onFiled();
          }}
        />
      )}
      <div
        className={
          shown === key ? "specs-frame" : "specs-frame specs-frame-loading"
        }
        ref={holder}
      >
        {reached && open && (
          <iframe
            key={key}
            ref={mount}
            src={src}
            title={`${SEAT_NAMES[view]}: ${moment.title}`}
          />
        )}
      </div>
    </section>
  );
}

/** A note on one moment from one seat, filed as a bug report about it: the spec server adds
 * the instance's game as it stands. ⌘↩ files it; Esc closes it. */
function BugNote({
  instance,
  target,
  onClose,
  onFiled,
}: {
  instance: string;
  target: string;
  onClose: () => void;
  onFiled: () => void;
}) {
  const [text, setText] = useState("");
  const [viewerBug, setViewerBug] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const file = () => {
    if (!text.trim() || busy) return;
    setBusy(true);
    setError(undefined);
    post<{ id: string; path: string }>("/report", {
      key: instance,
      description: text,
      viewerPage: viewerBug
        ? {
            url: `${window.location.pathname}${window.location.search}#${target}`,
            width: window.innerWidth,
            height: window.innerHeight,
            scale: window.devicePixelRatio,
          }
        : undefined,
    }).then(onFiled, (e: unknown) => {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    });
  };
  return (
    <form
      className="specs-note"
      onSubmit={(e) => {
        e.preventDefault();
        file();
      }}
    >
      <textarea
        autoFocus
        rows={3}
        value={text}
        placeholder="What's wrong?"
        aria-label="What's wrong?"
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            file();
          } else if (e.key === "Escape") {
            onClose();
          }
        }}
      />
      <label className="specs-note-viewer">
        <input
          type="checkbox"
          checked={viewerBug}
          onChange={(e) => setViewerBug(e.target.checked)}
        />
        Bug in the spec viewer
      </label>
      <div className="specs-note-actions">
        {error && <span className="specs-note-error">{error}</span>}
        <button type="button" className="specs-note-cancel" onClick={onClose}>
          Cancel
        </button>
        <button
          type="submit"
          className="specs-note-file"
          disabled={busy || !text.trim()}
        >
          File
        </button>
      </div>
    </form>
  );
}
