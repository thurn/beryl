import {
  lazy,
  Suspense,
  useEffect,
  useState,
  useSyncExternalStore,
} from "react";
import { MotionConfig } from "motion/react";
import { appInfo, uiReady, type AppInfo } from "./ipc";
import { Gallery } from "./screens/Gallery";
import { TableScreen } from "./screens/Table";

// Routes live in the hash so they work from the Tauri asset server and from Vite alike.
const subscribe = (onChange: () => void) => {
  window.addEventListener("hashchange", onChange);
  return () => window.removeEventListener("hashchange", onChange);
};
const route = () => window.location.hash.replace(/^#/, "") || "/";

// `?replica=<mockup>` (src/replica/) exists in development only: production builds drop the
// branch, and with it the replica chunk.
const Replica = import.meta.env.DEV
  ? lazy(() => import("./replica/Replica"))
  : undefined;
const replica =
  Replica && new URLSearchParams(window.location.search).get("replica");

function Home({ info }: { info: AppInfo | null }) {
  return (
    <main className="home">
      <h1>Beryl</h1>
      <p>{info ? `v${info.version}` : "connecting…"}</p>
      <nav>
        <a href="#/table">Table</a> · <a href="#/gallery">Component gallery</a>
      </nav>
    </main>
  );
}

export function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const path = useSyncExternalStore(subscribe, route);

  useEffect(() => {
    appInfo().then(setInfo);
  }, []);

  useEffect(() => {
    if (info) {
      void uiReady();
    }
  }, [info]);

  // Motion honors the reduced-motion setting, which `just ui-shots` turns on for still captures.
  return (
    <MotionConfig reducedMotion="user">
      {Replica && replica ? (
        <Suspense>
          <Replica name={replica} />
        </Suspense>
      ) : path === "/gallery" ? (
        <Gallery />
      ) : path === "/table" ? (
        <TableScreen />
      ) : path === "/table/scry" ? (
        <TableScreen overlay="scry" />
      ) : (
        <Home info={info} />
      )}
    </MotionConfig>
  );
}
