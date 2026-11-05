// What the headless capture scripts share: opening a page at a fixed size, waiting until it has
// settled, and a blank page whose canvas crops and composites PNGs (no image library).
import { webkit } from "playwright";

export const launch = () => webkit.launch();

/** A page at `width`×`height` with device scale factor 1 and motion reduced, collecting the
 * page's errors and console errors into `errors`. */
export async function openPage(browser, { width, height, scale = 1 }) {
    const context = await browser.newContext({
        viewport: { width, height },
        deviceScaleFactor: scale,
        reducedMotion: "reduce",
    });
    const page = await context.newPage();
    const errors = [];
    page.on("pageerror", (e) => errors.push(e.message));
    page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
    return { context, page, errors };
}

/** Waits until the app has rendered (a lazily loaded screen shows nothing before its chunk
 * arrives), fonts and images have loaded, finite animations are done and two frames were drawn
 * after them, so measured layout (arrows) is final. */
export async function settle(page) {
    await page.waitForFunction(
        () => (document.getElementById("root")?.childElementCount ?? 0) > 0,
    );
    await page.evaluate(async () => {
        await document.fonts.ready;
        await Promise.all(
            [...document.images].map((i) =>
                i.complete ? undefined : i.decode().catch(() => undefined),
            ),
        );
        const finite = document
            .getAnimations()
            .filter(
                (a) => a.effect?.getComputedTiming().iterations !== Infinity,
            );
        await Promise.all(finite.map((a) => a.finished.catch(() => undefined)));
        await new Promise((r) =>
            requestAnimationFrame(() => requestAnimationFrame(r)),
        );
    });
}

/** A blank page with an image toolkit on `window.kit`; see `KIT`. */
export async function imagePage(browser) {
    const context = await browser.newContext({ deviceScaleFactor: 1 });
    const page = await context.newPage();
    await page.evaluate(KIT);
    return page;
}

/** Passes PNG bytes into the image page: `kit.load(name, base64)` keeps a decoded bitmap. */
export async function loadImage(page, name, png) {
    await page.evaluate(
        ([n, b64]) => window.kit.load(n, b64),
        [name, Buffer.from(png).toString("base64")],
    );
}

/** PNG bytes from a data URL the image page returned. */
export const pngBytes = (dataUrl) =>
    Buffer.from(dataUrl.slice(dataUrl.indexOf(",") + 1), "base64");

// Runs in the image page. Rectangles are [x, y, w, h] in the source image's pixels; every scaling
// is nearest-neighbor so pixels stay countable.
const KIT = `(() => {
  const images = new Map();
  const canvas = (w, h) => {
    const c = document.createElement("canvas");
    c.width = Math.max(1, Math.round(w));
    c.height = Math.max(1, Math.round(h));
    const g = c.getContext("2d", { willReadFrequently: true });
    g.imageSmoothingEnabled = false;
    return [c, g];
  };
  const png = (c) => c.toDataURL("image/png");
  const crop = (name, [x, y, w, h], zoom = 1) => {
    const [c, g] = canvas(w * zoom, h * zoom);
    g.drawImage(images.get(name), x, y, w, h, 0, 0, w * zoom, h * zoom);
    return c;
  };
  window.kit = {
    async load(name, b64) {
      const bytes = Uint8Array.from(atob(b64), (ch) => ch.charCodeAt(0));
      images.set(name, await createImageBitmap(new Blob([bytes], { type: "image/png" })));
      const image = images.get(name);
      return [image.width, image.height];
    },
    size(name) {
      const image = images.get(name);
      return [image.width, image.height];
    },
    /** The window [x, y, w, h], turned by \`rotate\` degrees about its center in the image, cut
     * out upright. */
    cut(name, [x, y, w, h], rotate = 0) {
      const [c, g] = canvas(w, h);
      g.imageSmoothingEnabled = true;
      g.translate(w / 2, h / 2);
      g.rotate((-rotate * Math.PI) / 180);
      g.drawImage(images.get(name), -(x + w / 2), -(y + h / 2));
      return png(c);
    },
    /** Pixels of a region, row by row, as [r, g, b] triples. */
    pixels(name, [x, y, w, h]) {
      const data = crop(name, [x, y, w, h]).getContext("2d").getImageData(0, 0, w, h).data;
      const out = [];
      for (let i = 0; i < data.length; i += 4) out.push([data[i], data[i + 1], data[i + 2]]);
      return out;
    },
    /** A zoomed crop with a grid line every \`step\` source pixels, labeled in source coordinates. */
    grid(name, [x, y, w, h], zoom, step) {
      const pad = 34;
      const [c, g] = canvas(w * zoom + pad, h * zoom + pad);
      g.fillStyle = "#000";
      g.fillRect(0, 0, c.width, c.height);
      g.drawImage(crop(name, [x, y, w, h], zoom), pad, pad);
      g.font = "10px Menlo, monospace";
      g.textBaseline = "top";
      const first = (v) => Math.ceil(v / step) * step;
      for (let gx = first(x); gx <= x + w; gx += step) {
        const px = pad + (gx - x) * zoom;
        g.fillStyle = gx % (step * 5) === 0 ? "rgba(0,255,255,0.75)" : "rgba(0,255,255,0.35)";
        g.fillRect(px, pad, 1, h * zoom);
        g.save();
        g.translate(px - 5, 2);
        g.rotate(Math.PI / 2);
        g.fillStyle = "#0ff";
        g.fillText(String(gx), 0, 0);
        g.restore();
      }
      for (let gy = first(y); gy <= y + h; gy += step) {
        const py = pad + (gy - y) * zoom;
        g.fillStyle = gy % (step * 5) === 0 ? "rgba(255,0,255,0.75)" : "rgba(255,0,255,0.35)";
        g.fillRect(pad, py, w * zoom, 1);
        g.fillStyle = "#f0f";
        g.fillText(String(gy), 1, py - 5);
      }
      return png(c);
    },
    /** The comparison images of one part: both crops, side by side, a 50% onion skin, and the
     * absolute difference with the masks (source rectangles) blacked out. */
    compare(a, b, [x, y, w, h], masks, zoom) {
      const ca = crop(a, [x, y, w, h], zoom);
      const cb = crop(b, [x, y, w, h], zoom);
      const [W, H] = [ca.width, ca.height];
      const gap = Math.max(4, zoom * 2);
      const [side, gs] = canvas(W * 2 + gap, H);
      gs.fillStyle = "#ff00ff";
      gs.fillRect(0, 0, side.width, H);
      gs.drawImage(ca, 0, 0);
      gs.drawImage(cb, W + gap, 0);
      const [onion, go] = canvas(W, H);
      go.drawImage(ca, 0, 0);
      go.globalAlpha = 0.5;
      go.drawImage(cb, 0, 0);
      const [diff, gd] = canvas(W, H);
      const da = ca.getContext("2d").getImageData(0, 0, W, H);
      const db = cb.getContext("2d").getImageData(0, 0, W, H).data;
      for (let i = 0; i < da.data.length; i += 4) {
        for (let k = 0; k < 3; k++) da.data[i + k] = Math.abs(da.data[i + k] - db[i + k]);
        da.data[i + 3] = 255;
      }
      gd.putImageData(da, 0, 0);
      gd.fillStyle = "#000";
      for (const [mx, my, mw, mh] of masks) {
        gd.fillRect((mx - x) * zoom, (my - y) * zoom, mw * zoom, mh * zoom);
      }
      return { mockup: png(ca), replica: png(cb), side: png(side), onion: png(onion), diff: png(diff) };
    },
    /** A part's fingerprint: the region with its masks blacked out, as the mean color of each
     * cell of an 8x8 grid (row by row, RGB bytes), in base64. */
    sketch(name, [x, y, w, h], masks) {
      const [, g] = canvas(w, h);
      g.drawImage(images.get(name), x, y, w, h, 0, 0, w, h);
      g.fillStyle = "#000";
      for (const [mx, my, mw, mh] of masks) g.fillRect(mx - x, my - y, mw, mh);
      const data = g.getImageData(0, 0, w, h).data;
      const n = 8;
      const sums = new Float64Array(n * n * 4);
      for (let py = 0; py < h; py++) {
        for (let px = 0; px < w; px++) {
          const cell = (Math.floor((py * n) / h) * n + Math.floor((px * n) / w)) * 4;
          for (let k = 0; k < 3; k++) sums[cell + k] += data[(py * w + px) * 4 + k];
          sums[cell + 3]++;
        }
      }
      let text = "";
      for (let c = 0; c < n * n; c++) {
        const count = Math.max(1, sums[c * 4 + 3]);
        for (let k = 0; k < 3; k++) text += String.fromCharCode(Math.round(sums[c * 4 + k] / count));
      }
      return btoa(text);
    },
    /** The whole image with each named rectangle outlined and labeled (cyan, or \`color\`), masks
     * filled red. */
    outline(name, parts) {
      const [w, h] = this.size(name);
      const [c, g] = canvas(w, h);
      g.drawImage(images.get(name), 0, 0);
      g.font = "11px Menlo, monospace";
      g.textBaseline = "top";
      for (const { label, rect: [x, y, rw, rh], masks = [], color = "#0ff" } of parts) {
        g.strokeStyle = color;
        g.lineWidth = 1;
        g.strokeRect(x + 0.5, y + 0.5, rw - 1, rh - 1);
        g.fillStyle = "rgba(255,40,40,0.45)";
        for (const [mx, my, mw, mh] of masks) g.fillRect(mx, my, mw, mh);
        const tw = g.measureText(label).width + 4;
        g.fillStyle = "rgba(0,0,0,0.7)";
        g.fillRect(x, y, tw, 13);
        g.fillStyle = color;
        g.fillText(label, x + 2, y + 1);
      }
      return png(c);
    },
  };
})()`;
