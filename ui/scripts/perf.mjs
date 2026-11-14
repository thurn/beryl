// Frame timing of recorded fixtures played live, as a player sees them: the stage follows the
// browser's frames (no stepped clock), and each frame's length is measured in the page. Usage:
//   node scripts/perf.mjs <base-url> <fixtures-dir> [--browser chromium|webkit]
//        [--throttle N] [--profile out.cpuprofile] [--phone] [--runs N] [name...]
// Serve a production build (`vite build && vite preview`): React's development build is several
// times slower and would hide what a change does. `--throttle` slows Chromium's CPU N times (CDP
// `Emulation.setCPUThrottlingRate`), to stand in for an older laptop than this one; WebKit has no
// such switch. `--profile` writes a CPU profile of the whole play (Chromium only), which Chrome's
// DevTools opens. Prints, per fixture, how many frames ran, their median and 95th/99th
// percentile lengths, the share longer than 1.5 frames at 60 Hz (a dropped frame), the longest,
// and Chromium's time spent in script, style, layout and the rest of the main thread.
import { writeFileSync } from "node:fs";
import { readFile, writeFile } from "node:fs/promises";
import { chromium, webkit } from "playwright";
import { serveRecordings } from "./fixture-views.mjs";
import { option } from "./moments.mjs";

const args = process.argv.slice(2);
const engine = option(args, "--browser") ?? "chromium";
const throttle = Number(option(args, "--throttle") ?? 1);
const profile = option(args, "--profile");
const phone = option(args, "--phone", true);
const runs = Number(option(args, "--runs") ?? 1);
const trace = option(args, "--trace");
const css = option(args, "--css");
const verbs = option(args, "--verbs", true);
const [base, fixtures, ...names] = args;
if (!base || !fixtures || names.length === 0) {
    console.error(
        "usage: node scripts/perf.mjs <base-url> <fixtures-dir> [--browser chromium|webkit] [--throttle N] [--profile out] [--phone] [--runs N] name...",
    );
    process.exit(2);
}

const browser = await (engine === "webkit" ? webkit : chromium).launch(
    engine === "chromium"
        ? {
              // The full Chromium in its new headless mode composites and rasterizes on the
              // GPU, as the app's webview does; the default headless shell draws in software.
              channel: "chromium",
              args: [
                  "--enable-gpu",
                  "--ignore-gpu-blocklist",
                  "--enable-gpu-rasterization",
              ],
          }
        : {},
);

/** The milliseconds the page's main thread and the GPU process's main thread were busy, and
 * the GPU's share spent rasterizing, from a trace; writes the trace with `--trace`. */
function threads(buffer, name) {
    if (trace)
        writeFileSync(trace.replace(/(\.json)?$/, `-${name}.json`), buffer);
    const parsed = JSON.parse(buffer.toString("utf8"));
    const events = Array.isArray(parsed) ? parsed : parsed.traceEvents;
    const named = new Map();
    for (const e of events) {
        if (e.ph === "M" && e.name === "thread_name")
            named.set(`${e.pid}:${e.tid}`, e.args.name);
    }
    const sum = { main: 0, gpu: 0, raster: 0 };
    for (const e of events) {
        if (e.ph !== "X" || e.dur === undefined) continue;
        const thread = named.get(`${e.pid}:${e.tid}`);
        if (e.name === "RunTask" && thread === "CrRendererMain")
            sum.main += e.dur / 1000;
        if (e.name === "RunTask" && thread === "CrGpuMain")
            sum.gpu += e.dur / 1000;
        if (e.name === "RasterDecoderImpl::DoEndRasterCHROMIUM")
            sum.raster += e.dur / 1000;
    }
    return sum;
}

/** For each verb, the frames it played in, the share of them that dropped, how late those
 * were in all and how much script ran in them. */
function byVerb(frames, during, scripts) {
    const tally = new Map();
    frames.forEach((length, i) => {
        for (const verb of during[i].length ? during[i] : ["(none)"]) {
            const t = tally.get(verb) ?? {
                frames: 0,
                dropped: 0,
                over: 0,
                script: 0,
            };
            t.frames += 1;
            if (length > 25) {
                t.dropped += 1;
                t.over += length - 1000 / 60;
                t.script += scripts[i];
            }
            tally.set(verb, t);
        }
    });
    return tally;
}

const quantile = (sorted, q) =>
    sorted.length
        ? sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))]
        : 0;

async function measure(name) {
    const size = phone
        ? { width: 393, height: 759 }
        : { width: 1600, height: 900 };
    const context = await browser.newContext({
        viewport: size,
        deviceScaleFactor: 2,
    });
    await serveRecordings(context, fixtures);
    // Times every animation-frame callback, the stage's tick among them, and every task
    // React's scheduler (a MessageChannel) and timers run, in every engine.
    await context.addInitScript(() => {
        window.berylFrameScript = 0;
        window.berylTaskScript = 0;
        const timed =
            (callback, into) =>
            (...args) => {
                const start = performance.now();
                try {
                    return callback(...args);
                } finally {
                    window[into] += performance.now() - start;
                }
            };
        const request = window.requestAnimationFrame.bind(window);
        window.requestAnimationFrame = (callback) =>
            request(timed(callback, "berylFrameScript"));
        const later = window.setTimeout.bind(window);
        window.setTimeout = (callback, ...rest) =>
            typeof callback === "function"
                ? later(timed(callback, "berylTaskScript"), ...rest)
                : later(callback, ...rest);
        const Channel = window.MessageChannel;
        window.MessageChannel = class extends Channel {
            constructor() {
                super();
                const port = this.port1;
                const set = Object.getOwnPropertyDescriptor(
                    MessagePort.prototype,
                    "onmessage",
                ).set;
                Object.defineProperty(port, "onmessage", {
                    set(callback) {
                        set.call(port, timed(callback, "berylTaskScript"));
                    },
                });
            }
        };
    });
    const page = await context.newPage();
    const errors = [];
    page.on("pageerror", (e) => errors.push(e.message));
    await page.goto(
        new URL(`/?${phone ? "phone&" : ""}fixture=${name}&at=1#/game`, base)
            .href,
    );
    await page.waitForFunction(() => window.berylStage && window.berylFixture);
    await page.evaluate(() => document.fonts.ready.then(() => undefined));
    await page.evaluate(() => window.berylStage.idle());
    if (css) await page.addStyleTag({ content: await readFile(css, "utf8") });
    await page.waitForTimeout(500);
    const cdp =
        engine === "chromium" ? await context.newCDPSession(page) : undefined;
    if (cdp) {
        await cdp.send("Performance.enable");
        if (throttle > 1)
            await cdp.send("Emulation.setCPUThrottlingRate", {
                rate: throttle,
            });
        if (profile) {
            await cdp.send("Profiler.enable");
            await cdp.send("Profiler.setSamplingInterval", { interval: 200 });
            await cdp.send("Profiler.start");
        }
    }
    const metric = async () =>
        cdp
            ? Object.fromEntries(
                  (await cdp.send("Performance.getMetrics")).metrics.map(
                      (m) => [m.name, m.value],
                  ),
              )
            : {};
    // The threads' busy time comes from a trace: CDP's metrics see only the page's main
    // thread, and rasterizing and compositing happen in the GPU process.
    if (cdp) {
        await browser.startTracing(page, {
            categories: trace
                ? [
                      "devtools.timeline",
                      "disabled-by-default-devtools.timeline",
                      "disabled-by-default-devtools.timeline.invalidationTracking",
                      "blink",
                      "cc",
                      "gpu",
                      "viz",
                  ]
                : ["disabled-by-default-devtools.timeline", "gpu"],
        });
    }
    const before = await metric();
    const {
        lengths: frames,
        during,
        scripts,
        script: rafScript,
    } = await page.evaluate(async () => {
        const lengths = [];
        // The steps playing at each frame, by verb (and a notice's kind), to tell which
        // animations the long frames come in.
        const playing = new Map();
        const during = [];
        const stop = window.berylStage.follow(({ edge, step }) => {
            const act = step.act;
            const name =
                act.verb === "announce"
                    ? `announce:${act.notice?.kind ?? ""}`
                    : act.verb;
            if (edge === "start") playing.set(step, name);
            else playing.delete(step);
        });
        // The script run in each frame's time: its own callbacks and the tasks before it.
        const scripts = [];
        let ran = window.berylFrameScript + window.berylTaskScript;
        let last;
        let on = true;
        const frame = (t) => {
            if (last !== undefined) {
                lengths.push(t - last);
                during.push([...new Set(playing.values())]);
                const js = window.berylFrameScript + window.berylTaskScript;
                scripts.push(js - ran);
                ran = js;
            }
            last = t;
            if (on) requestAnimationFrame(frame);
        };
        requestAnimationFrame(frame);
        window.berylFrameScript = 0;
        const replay = window.berylFixture;
        for (;;) {
            const view = replay.view();
            await replay.invoke("choose", { request: {} });
            await window.berylStage.idle();
            if (replay.view() === view) break;
        }
        on = false;
        stop();
        return { lengths, during, scripts, script: window.berylFrameScript };
    });
    const after = await metric();
    const busy = cdp ? threads(await browser.stopTracing(), name) : {};
    if (cdp && profile) {
        const { profile: cpu } = await cdp.send("Profiler.stop");
        await writeFile(
            profile.replace(/(\.cpuprofile)?$/, `-${name}.cpuprofile`),
            JSON.stringify(cpu),
        );
    }
    await context.close();
    const sorted = [...frames].sort((a, b) => a - b);
    const total = frames.reduce((a, b) => a + b, 0);
    const spent = (key) => ((after[key] ?? 0) - (before[key] ?? 0)) * 1000;
    return {
        name,
        frames: frames.length,
        seconds: total / 1000,
        p50: quantile(sorted, 0.5),
        p95: quantile(sorted, 0.95),
        p99: quantile(sorted, 0.99),
        max: sorted.at(-1) ?? 0,
        dropped:
            frames.filter((f) => f > 25).length / Math.max(1, frames.length),
        raf: rafScript,
        verbs: byVerb(frames, during, scripts),
        script: spent("ScriptDuration"),
        style: spent("RecalcStyleDuration"),
        layout: spent("LayoutDuration"),
        main: busy.main ?? 0,
        gpu: busy.gpu ?? 0,
        raster: busy.raster ?? 0,
        errors,
    };
}

const f = (n) => n.toFixed(1).padStart(6);
console.log(
    `${engine}${throttle > 1 ? ` throttled ${throttle}x` : ""}${phone ? " phone" : ""}`,
);
console.log(
    "fixture                    frames   secs    p50    p95    p99    max  drop%     raf" +
        (engine === "chromium"
            ? "  script  style layout    main     gpu  raster"
            : ""),
);
for (const name of names) {
    for (let run = 0; run < runs; run++) {
        const r = await measure(name);
        console.log(
            `${r.name.padEnd(25)} ${String(r.frames).padStart(7)} ${f(r.seconds)} ${f(r.p50)} ${f(r.p95)} ${f(r.p99)} ${f(r.max)} ${f(100 * r.dropped)} ${f(r.raf).padStart(7)}${engine === "chromium" ? ` ${f(r.script).padStart(7)} ${f(r.style)} ${f(r.layout)} ${f(r.main).padStart(7)} ${f(r.gpu).padStart(7)} ${f(r.raster).padStart(7)}` : ""}`,
        );
        for (const e of r.errors) console.log(`  error: ${e}`);
        if (verbs) {
            for (const [verb, t] of [...r.verbs].sort(
                (a, b) => b[1].over - a[1].over,
            )) {
                console.log(
                    `  ${verb.padEnd(24)} ${String(t.frames).padStart(6)} frames ${f((100 * t.dropped) / t.frames)}% dropped ${f(t.over)} ms late, ${f(t.script)} ms of it script`,
                );
            }
        }
    }
}
await browser.close();
