// The font sheet (`?replica=fonts`, rendered by `just ui-fonts`): each candidate family sets the
// sample's text at the sample's size, beside the mockup crop, over it at 50% and alone on the
// crop's mean color. Bundled families load from ./fonts; system families are referenced by name
// and flagged when this Mac lacks them.
import { useEffect, useState, type CSSProperties } from "react";
import "./fonts";

export interface FontSheetSpec {
  /** The mockup crop, as a data URL. */
  sample: string;
  rect: [number, number, number, number];
  text: string;
  size: number;
  weight: number;
  italic: boolean;
  color: string;
  align: "left" | "center" | "right";
  zoom: number;
  families: { name: string; bundled: boolean; system: boolean }[];
}

declare global {
  interface Window {
    __berylFontSheet?: FontSheetSpec;
  }
}

/** A family name in CSS: quoted, except keywords such as -apple-system. */
const css = (family: string) =>
  family.startsWith("-") ? family : `"${family}"`;

/** Whether a family renders: text in it measures differently from each generic fallback. */
function installed(family: string): boolean {
  const canvas = document.createElement("canvas").getContext("2d");
  if (!canvas) return false;
  const probe = "mmmmmmmmmwwwwwlliI0O";
  return ["monospace", "serif", "sans-serif"].some((fallback) => {
    canvas.font = `72px ${fallback}`;
    const base = canvas.measureText(probe).width;
    canvas.font = `72px ${css(family)}, ${fallback}`;
    return canvas.measureText(probe).width !== base;
  });
}

function meanColor(src: string): Promise<string> {
  return new Promise((resolve) => {
    const image = new Image();
    image.onload = () => {
      const canvas = document.createElement("canvas");
      canvas.width = image.width;
      canvas.height = image.height;
      const g = canvas.getContext("2d");
      if (!g) return resolve("#000");
      g.drawImage(image, 0, 0);
      const data = g.getImageData(0, 0, image.width, image.height).data;
      const sum = [0, 0, 0];
      for (let i = 0; i < data.length; i += 4) {
        for (let k = 0; k < 3; k++) sum[k]! += data[i + k]!;
      }
      const n = data.length / 4;
      resolve(`rgb(${sum.map((v) => Math.round(v / n)).join(" ")})`);
    };
    image.src = src;
  });
}

export function FontSheet() {
  const spec = window.__berylFontSheet;
  const [ready, setReady] = useState<{
    background: string;
    missing: Set<string>;
  }>();

  useEffect(() => {
    if (!spec) return;
    const style = spec.italic ? "italic " : "";
    void Promise.all(
      spec.families.map((f) =>
        document.fonts
          .load(
            `${style}${spec.weight} ${spec.size}px ${css(f.name)}`,
            spec.text,
          )
          .catch(() => []),
      ),
    ).then(async () => {
      const missing = new Set(
        spec.families.filter((f) => !installed(f.name)).map((f) => f.name),
      );
      setReady({ background: await meanColor(spec.sample), missing });
    });
  }, [spec]);

  if (!spec) {
    return <p style={{ color: "#ddd" }}>No font sheet spec.</p>;
  }
  const [, , w, h] = spec.rect;
  // Fixed boxes: a wide sample must not shrink to fit the row (`by ui-fonts` sizes the page).
  const box: CSSProperties = {
    position: "relative",
    flexShrink: 0,
    width: w * spec.zoom,
    height: h * spec.zoom,
    overflow: "hidden",
  };
  const crop = (
    <img
      src={spec.sample}
      alt=""
      style={{ ...box, display: "block", imageRendering: "pixelated" }}
    />
  );
  const text = (family: string, opacity = 1): CSSProperties => ({
    position: "absolute",
    inset: 0,
    display: "flex",
    alignItems: "center",
    justifyContent:
      spec.align === "left"
        ? "flex-start"
        : spec.align === "right"
          ? "flex-end"
          : "center",
    fontFamily: `${css(family)}, monospace`,
    fontSize: spec.size * spec.zoom,
    fontWeight: spec.weight,
    fontStyle: spec.italic ? "italic" : "normal",
    lineHeight: 1,
    whiteSpace: "nowrap",
    color: spec.color,
    opacity,
  });
  return (
    <main
      data-ready={ready ? "" : undefined}
      style={{
        padding: 16,
        background: "#111",
        color: "#ccc",
        font: "12px Menlo, monospace",
        minHeight: "100vh",
      }}
    >
      <p>
        “{spec.text}” at {spec.size}px, weight {spec.weight}
        {spec.italic ? ", italic" : ""}: mockup · over it at 50% · alone
      </p>
      {ready &&
        spec.families.map((f) => (
          <section key={f.name} style={{ marginTop: 12 }}>
            <div>
              {f.name} ({f.bundled ? "bundled" : f.system ? "system" : "other"}
              {ready.missing.has(f.name) ? ", NOT INSTALLED" : ""})
            </div>
            <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
              {crop}
              <div style={box}>
                {crop}
                <span style={text(f.name, 0.5)}>{spec.text}</span>
              </div>
              <div style={{ ...box, background: ready.background }}>
                <span style={text(f.name)}>{spec.text}</span>
              </div>
            </div>
          </section>
        ))}
    </main>
  );
}
