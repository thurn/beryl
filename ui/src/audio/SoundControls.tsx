// The player's say over the sound: mute and the music and effects levels (the start screen's
// speaker, and the game's settings), and the credits for the music and sounds, from the
// manifest.
import {
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type ReactNode,
  type RefObject,
} from "react";
import manifest from "./manifest.json";
import {
  audioSettings,
  setAudioSettings,
  setMusic,
  subscribeAudio,
  type Music,
} from "./sound";

const useAudioSettings = () =>
  useSyncExternalStore(subscribeAudio, audioSettings);

/** Plays `music` while the calling screen shows, or silence for none. */
export function useMusic(music: Music | undefined) {
  useEffect(() => setMusic(music), [music]);
}

/** Mute beside a slider for the music and one for the effects. */
export function SoundPanel() {
  const { music, sfx, muted } = useAudioSettings();
  return (
    <div className={`sound-panel${muted ? " sound-muted" : ""}`}>
      <button
        type="button"
        className="icon-button sound-mute"
        aria-label={muted ? "Unmute" : "Mute"}
        aria-pressed={muted}
        onClick={() => setAudioSettings({ muted: !muted })}
      >
        <SpeakerIcon muted={muted} />
      </button>
      <div className="sound-levels">
        <Level
          label="Music"
          icon={<NoteIcon />}
          value={music}
          onChange={(v) => setAudioSettings({ music: v, muted: false })}
        />
        <Level
          label="Effects"
          icon={<WavesIcon />}
          value={sfx}
          onChange={(v) => setAudioSettings({ sfx: v, muted: false })}
        />
      </div>
    </div>
  );
}

function Level({
  label,
  icon,
  value,
  onChange,
}: {
  label: string;
  icon: ReactNode;
  value: number;
  onChange(value: number): void;
}) {
  return (
    <label className="sound-level" title={label}>
      {icon}
      <input
        type="range"
        min={0}
        max={1}
        step={0.05}
        value={value}
        aria-label={label}
        onChange={(e) => onChange(Number(e.target.value))}
      />
    </label>
  );
}

/** Closes `open` on a press outside `ref`'s element or on Escape. */
function useDismiss(
  open: boolean,
  close: () => void,
  ref: RefObject<HTMLElement | null>,
) {
  useEffect(() => {
    if (!open) return;
    const press = (e: PointerEvent) => {
      if (e.target instanceof Node && !ref.current?.contains(e.target)) close();
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    document.addEventListener("pointerdown", press, true);
    document.addEventListener("keydown", key, true);
    return () => {
      document.removeEventListener("pointerdown", press, true);
      document.removeEventListener("keydown", key, true);
    };
  }, [open, close, ref]);
}

/** The start screen's corner: the credits and the speaker, each opening its panel over it. */
export function SoundCorner() {
  const [open, setOpen] = useState<"sound" | "credits">();
  const ref = useRef<HTMLDivElement>(null);
  const close = useRef(() => setOpen(undefined)).current;
  useDismiss(open !== undefined, close, ref);
  const { muted } = useAudioSettings();
  const toggle = (panel: "sound" | "credits") =>
    setOpen((o) => (o === panel ? undefined : panel));
  return (
    <div className="sound-corner" ref={ref}>
      {open === "sound" && (
        <div className="corner-panel">
          <SoundPanel />
        </div>
      )}
      {open === "credits" && (
        <div className="corner-panel">
          <Credits />
        </div>
      )}
      <button
        type="button"
        className="credits-button"
        aria-expanded={open === "credits"}
        onClick={() => toggle("credits")}
      >
        Credits
      </button>
      <button
        type="button"
        className="icon-button"
        aria-label="Sound"
        aria-expanded={open === "sound"}
        onClick={() => toggle("sound")}
      >
        <SpeakerIcon muted={muted} />
      </button>
    </div>
  );
}

interface Credit {
  author: string;
  license: string;
  kinds: Set<"Music" | "Sound">;
  packs: Map<string, string>;
}

/** Each author whose work the manifest uses, with what kind and from which packs. */
function credits(): Credit[] {
  const byAuthor = new Map<string, Credit>();
  const add = (
    kind: "Music" | "Sound",
    cues: Record<string, { variants: readonly { source: string }[] }>,
  ) => {
    for (const cue of Object.values(cues)) {
      for (const { source } of cue.variants) {
        const s = manifest.sources[source as keyof typeof manifest.sources];
        const credit = byAuthor.get(s.author) ?? {
          author: s.author,
          license: s.license,
          kinds: new Set(),
          packs: new Map(),
        };
        credit.kinds.add(kind);
        credit.packs.set(s.title, s.url);
        byAuthor.set(s.author, credit);
      }
    }
  };
  add("Music", manifest.music);
  add("Sound", manifest.sounds);
  return [...byAuthor.values()];
}

const CREDITS = credits();

/** In the app a link goes to the browser (beryl_app's links.rs); in a browser, to a tab. */
const outward =
  "__TAURI_INTERNALS__" in window
    ? {}
    : { target: "_blank", rel: "noreferrer noopener" };

/** Who made the music and sounds, under what license, with links to their packs. */
export function Credits() {
  return (
    <dl className="credits">
      {CREDITS.map((c) => (
        <div key={c.author} className="credit">
          <dt>{[...c.kinds].join(" & ")}</dt>
          <dd>
            <span className="credit-author">
              {c.author} <small>{c.license}</small>
            </span>
            {[...c.packs].map(([title, url]) => (
              <a key={url} href={url} {...outward}>
                {title}
              </a>
            ))}
          </dd>
        </div>
      ))}
    </dl>
  );
}

function SpeakerIcon({ muted }: { muted: boolean }) {
  return (
    <svg className="sound-icon" viewBox="0 0 24 24" aria-hidden>
      <path className="sound-icon-fill" d="M4 9h4l5-4v14l-5-4H4z" />
      {muted ? (
        <path d="M16.5 9.5l5 5M21.5 9.5l-5 5" />
      ) : (
        <path d="M16 9a4 4 0 0 1 0 6M18.5 6.5a7.5 7.5 0 0 1 0 11" />
      )}
    </svg>
  );
}

function NoteIcon() {
  return (
    <svg className="sound-icon" viewBox="0 0 24 24" aria-hidden>
      <path d="M9 18V5l11-2v13" />
      <circle className="sound-icon-fill" cx="6.5" cy="18" r="2.5" />
      <circle className="sound-icon-fill" cx="17.5" cy="16" r="2.5" />
    </svg>
  );
}

function WavesIcon() {
  return (
    <svg className="sound-icon" viewBox="0 0 24 24" aria-hidden>
      <path d="M4 10v4M8 7v10M12 4v16M16 7v10M20 10v4" />
    </svg>
  );
}
