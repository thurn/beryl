import { useEffect, useState } from "react";
import { appInfo, uiReady, type AppInfo } from "./ipc";

export function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);

  useEffect(() => {
    appInfo().then(setInfo);
  }, []);

  useEffect(() => {
    if (info) {
      void uiReady();
    }
  }, [info]);

  return (
    <main className="app">
      <h1>Beryl</h1>
      <p>{info ? `v${info.version}` : "connecting…"}</p>
    </main>
  );
}
