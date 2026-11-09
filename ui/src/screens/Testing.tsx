import { useEffect, useMemo, useRef, useState } from "react";
import { SoundPanel } from "../audio/SoundControls";
import { cardEntry } from "../cards";
import { Button } from "../components/Controls";
import type { DecisionId, GameUpdate, RelayedEvent } from "../ipc";
import "./testing.css";

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
  opponentMove?: boolean;
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

/** Full-size replays show their paused game before offering playback. */
export function SpecReplayPlay({ url }: { url: string }) {
  const [held, setHeld] = useState(false);
  useEffect(() => {
    void fetch(`${url}/held`)
      .then((response) => response.json() as Promise<{ ok?: boolean }>)
      .then(
        ({ ok }) => setHeld(ok === true),
        () => setHeld(false),
      );
  }, [url]);
  return held ? (
    <Button
      className="spec-replay-play"
      onClick={() => {
        void fetch(`${url}/play`, { method: "POST" }).then(
          (response) => {
            if (response.ok) setHeld(false);
          },
          () => {},
        );
      }}
    >
      Play
    </Button>
  ) : null;
}

/** Step an available recorded opponent cast, without handing the reviewer a different seat. */
export function SpecOpponentMove({
  url,
  update,
}: {
  url: string;
  update: GameUpdate;
}) {
  const [offer, setOffer] = useState<{
    update: GameUpdate;
    decision: DecisionId;
  }>();
  useEffect(() => {
    if (update.view.status.kind !== "thinking") return;
    const request = new AbortController();
    void fetch(`${url}/opponent-move`, { signal: request.signal })
      .then((response) => response.json() as Promise<{ ok: DecisionId | null }>)
      .then(
        ({ ok }) => {
          if (ok && !request.signal.aborted) setOffer({ update, decision: ok });
        },
        () => {},
      );
    return () => request.abort();
  }, [url, update]);
  if (offer?.update !== update) return null;
  return (
    <Button
      className="spec-replay-play"
      onClick={() => {
        setOffer(undefined);
        void fetch(`${url}/opponent-move`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(offer.decision),
        }).catch(() => setOffer(offer));
      }}
    >
      Play opponent’s move
    </Button>
  );
}

interface PlaygroundExample {
  title: string;
  variants: (MomentListing & { label: string })[];
}
interface PlaygroundListing {
  examples: PlaygroundExample[];
  missing: string[];
}

export function Testing({ path }: { path: string }) {
  const specs = useSpecs();
  const [section, slug, momentName, seat] = path.replace(/^\//, "").split("/");
  const playground = section === "playground";
  const base = playground ? "/playground" : "/scenarios";
  const [query, setQuery] = useState("");
  const [data, setData] = useState<PlaygroundListing>();
  const [error, setError] = useState<string>();
  useEffect(() => {
    document.title = playground ? "Beryl Playground" : "Beryl Scenarios";
    if (!playground || !slug) return;
    const abort = new AbortController();
    fetch(`${API}/playground/${slug}`, { signal: abort.signal })
      .then((r) => {
        if (!r.ok) throw Error("Could not load playground");
        return r.json() as Promise<PlaygroundListing>;
      })
      .then(setData)
      .catch((e: Error) => {
        if (!abort.signal.aborted) setError(e.message);
      });
    return () => abort.abort();
  }, [playground, slug]);
  if (typeof specs === "string" || error)
    return (
      <div className="specs specs-empty" role="alert">
        {error ?? (typeof specs === "string" ? specs : "Could not load cards")}
      </div>
    );
  if (!specs) return <div className="specs specs-empty">Loading…</div>;
  const spec = specs.find((s) => s.slug === slug);
  const selected = playground
    ? data?.examples
        .flatMap((e) => e.variants)
        .find((m) => m.name === momentName)
    : spec?.moments.find((m) => m.name === momentName);
  if (spec && momentName && !selected && (!playground || data))
    return (
      <div className="specs specs-empty">
        Example not found.{" "}
        <a href={`${base}/${spec.slug}`}>Back to {spec.card}</a>
      </div>
    );
  if (spec && selected && seat && VIEWS.includes(seat as View))
    return <SeatPage slug={spec.slug} moment={selected} seat={seat as View} />;
  return (
    <div className="specs">
      <header className="testing-header">
        <nav aria-label="Testing tools">
          <a href="/playground" aria-current={playground ? "page" : undefined}>
            Playground
          </a>
          <a href="/scenarios" aria-current={!playground ? "page" : undefined}>
            Scenarios
          </a>
        </nav>
        <SoundPanel />
        <a href={`${API}/bug-reports.zip`} download>
          Export bugs
        </a>
      </header>
      {!slug ? (
        <>
          <div className="testing-title">
            <h1>{playground ? "Playground" : "Scenarios"}</h1>
            <input
              type="search"
              aria-label="Find a card"
              placeholder="Find a card"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
          <ul className="testing-cards">
            {[...specs]
              .sort(
                (a, b) =>
                  Number(!!a.reviewed) - Number(!!b.reviewed) ||
                  a.card.localeCompare(b.card),
              )
              .filter((s) => s.card.toLowerCase().includes(query.toLowerCase()))
              .map((s) => (
                <li key={s.slug}>
                  <a href={`${base}/${s.slug}`}>
                    <CardImage name={s.card} className="testing-card-art" />
                    <span>{s.card}</span>
                    <small>
                      {s.reviewed ? "✓ Reviewed" : "Not reviewed"}
                      {s.reports > 0 ? ` · ${s.reports} bugs` : ""}
                    </small>
                  </a>
                </li>
              ))}
          </ul>
        </>
      ) : !spec ? (
        <p className="specs-empty">Card not found.</p>
      ) : playground && !data ? (
        <p className="specs-empty">Preparing examples…</p>
      ) : (
        <TestingCard
          key={slug}
          spec={spec}
          playground={playground}
          data={data}
          selected={selected}
        />
      )}
    </div>
  );
}

function SeatPage({
  slug,
  moment,
  seat,
}: {
  slug: string;
  moment: MomentListing;
  seat: View;
}) {
  useEffect(() => {
    const search = new URLSearchParams(window.location.search);
    const id =
      search.get("instance") ??
      `seat-${Math.random().toString(36).slice(2, 10)}`;
    const live = `${window.location.origin}${API}/m/${slug}/${moment.name}/${seat}/${id}`;
    window.location.replace(
      `/?live=${encodeURIComponent(live)}&spec${search.has("hold") ? "" : "&specReplay"}#/game`,
    );
  }, [slug, moment, seat]);
  return <div className="specs specs-empty">Opening game…</div>;
}

function TestingCard({
  spec,
  playground,
  data,
  selected,
}: {
  spec: SpecListing;
  playground: boolean;
  data?: PlaygroundListing;
  selected?: MomentListing;
}) {
  const page = useMemo(() => Math.random().toString(36).slice(2, 10), []);
  const frames = useRef<Frames>(new Map());
  const [open, setOpen] = useState(false);
  const [reports, setReports] = useState(spec.reports);
  const [reviewed, setReviewed] = useState(spec.reviewed !== null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();
  const groups = [
    ...new Set(spec.moments.map((m) => m.group || "Ability probes")),
  ].sort(
    (a, b) => Number(a === "Ability probes") - Number(b === "Ability probes"),
  );
  const [group, setGroup] = useState(
    selected ? selected.group || "Ability probes" : groups[0],
  );
  const checkpoints = spec.moments.filter(
    (m) => (m.group || "Ability probes") === group,
  );
  const [checkpoint, setCheckpoint] = useState(
    selected?.name ?? checkpoints[0]?.name,
  );
  useEffect(() => {
    const stream = new EventSource(`${API}/stream/${page}`);
    stream.onopen = () => setOpen(true);
    stream.onerror = () => setOpen(false);
    stream.addEventListener("game-update", (e) => {
      const { instance, update } = JSON.parse(
        (e as MessageEvent<string>).data,
      ) as { instance: string; update: unknown };
      frames.current.get(instance)?.postMessage(
        {
          kind: "beryl-relay",
          event: "game-update",
          payload: update,
        } satisfies RelayedEvent,
        window.location.origin,
      );
    });
    return () => stream.close();
  }, [page]);
  const filed = () => setReports((n) => n + 1);
  const save = async () => {
    setSaving(true);
    setError(undefined);
    try {
      const result = await post<{ reviewed: string | null }>(
        `/review/${spec.slug}`,
        { reviewed: !reviewed },
      );
      setReviewed(result.reviewed !== null);
    } catch {
      setError("Review could not be saved.");
    } finally {
      setSaving(false);
    }
  };
  const board = (moment: MomentListing, view: View) => (
    <MomentView
      key={`${moment.name}-${view}`}
      slug={spec.slug}
      moment={moment}
      view={view}
      page={page}
      open={open}
      frames={frames.current}
      onFiled={filed}
      playground={playground}
    />
  );
  const moment =
    checkpoints.find((m) => m.name === checkpoint) ?? checkpoints[0];
  return (
    <main className="testing-main">
      <div className="testing-title">
        <h1>{spec.card}</h1>
        {playground && (
          <a className="testing-more" href={`/scenarios/${spec.slug}`}>
            More cases →
          </a>
        )}
        {playground && (
          <button
            onClick={() => void save()}
            disabled={saving}
            aria-pressed={reviewed}
          >
            {reviewed ? "✓ Reviewed" : "Mark reviewed"}
          </button>
        )}
        {reports > 0 && <span>{reports} open bugs</span>}
        {error && <span role="alert">{error}</span>}
      </div>
      {playground ? (
        <>
          {data?.missing.map((m) => (
            <p role="status" key={m}>
              {m}
            </p>
          ))}
          {data?.examples
            .filter(
              (e) =>
                !selected ||
                e.variants.includes(
                  selected as PlaygroundExample["variants"][number],
                ),
            )
            .map((example, i) => (
              <ExampleBoards
                key={example.title}
                example={example}
                number={i + 1}
                board={board}
                selected={selected}
              />
            ))}
        </>
      ) : (
        <>
          <div className="testing-selectors">
            <label>
              Scenario
              <select
                value={group}
                onChange={(e) => {
                  setGroup(e.target.value);
                  setCheckpoint(undefined);
                }}
              >
                {groups.map((g) => (
                  <option key={g}>{g}</option>
                ))}
              </select>
            </label>
            <label>
              Checkpoint
              <select
                value={moment?.name ?? ""}
                onChange={(e) => setCheckpoint(e.target.value)}
              >
                {checkpoints.map((m) => (
                  <option value={m.name} key={m.name}>
                    {m.title}
                  </option>
                ))}
              </select>
            </label>
          </div>
          {moment && VIEWS.map((view) => board(moment, view))}
        </>
      )}
    </main>
  );
}

function ExampleBoards({
  example,
  number,
  board,
  selected,
}: {
  example: PlaygroundExample;
  number: number;
  board: (m: MomentListing, v: View) => React.ReactNode;
  selected?: MomentListing;
}) {
  const [variant, setVariant] = useState(
    selected?.name ?? example.variants[0]?.name,
  );
  const moment =
    example.variants.find((v) => v.name === variant) ?? example.variants[0];
  if (!moment) return null;
  return (
    <section className="testing-example">
      <div className="testing-example-title">
        <h2>
          {number}. {example.title}
        </h2>
        {example.variants.length > 1 && (
          <label>
            Variation
            <select
              value={moment.name}
              onChange={(e) => setVariant(e.target.value)}
            >
              {example.variants.map((v) => (
                <option key={v.name} value={v.name}>
                  {v.label}
                </option>
              ))}
            </select>
          </label>
        )}
      </div>
      {board(moment, moment.first ?? "yours")}
      {board(moment, otherSeat(moment.first ?? "yours"))}
    </section>
  );
}

type Frames = Map<string, Window>;
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

function otherSeat(view: View): View {
  return view === "yours" ? "theirs" : "yours";
}

/** Each seat as a moment's header names it. */
const SEAT_NAMES: Record<View, string> = { yours: "You", theirs: "Them" };

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
  playground = false,
}: {
  slug: string;
  moment: MomentListing;
  view: View;
  page: string;
  open: boolean;
  frames: Frames;
  onFiled: () => void;
  playground?: boolean;
}) {
  const holder = useRef<HTMLDivElement>(null);
  const reached = useReached(holder);
  // Each instance is a game of its own; a new one replays the moment.
  const [n, setN] = useState(0);
  const key = `${slug}/${moment.name}/${view}/${page}.${n}`;
  const live = `${window.location.origin}${API}/m/${key}`;
  const opponent = moment.opponentMove ? "&specOpponent" : "";
  const src = `/?live=${encodeURIComponent(live)}&relay&spec${opponent}#/game`;
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
          <span className="specs-seat">
            {playground
              ? view === (moment.first ?? "yours")
                ? "You play"
                : "Opponent plays"
              : SEAT_NAMES[view]}
          </span>
          {!playground && moment.title}
        </h2>
        <div className="specs-moment-actions">
          {waiting ? (
            <button
              type="button"
              className="specs-icon-button"
              title="Play"
              aria-label="Play"
              disabled={held[key] === undefined}
              onClick={() => {
                void fetch(`${API}/m/${key}/play`, { method: "POST" }).then(
                  (r) => {
                    if (r.ok) setPlayed(key);
                  },
                );
              }}
            >
              ▶
            </button>
          ) : null}
          <button
            type="button"
            className="specs-icon-button"
            title="Reset"
            aria-label="Reset"
            onClick={() => setN((x) => x + 1)}
          >
            ↻
          </button>
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
            href={`/${playground ? "playground" : "scenarios"}/${slug}/${moment.name}`}
            title="This moment"
            aria-label="This moment"
          >
            #
          </a>
          <a
            className="specs-icon-button"
            href={`/${playground ? "playground" : "scenarios"}/${slug}/${moment.name}/${view}`}
            target="_blank"
            rel="noreferrer"
            title="Open full size"
            aria-label="Open full size"
          >
            ↗
          </a>
        </div>
      </header>
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
