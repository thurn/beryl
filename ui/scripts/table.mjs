// A live table uses the same loopback server and URL as just ui-live. The kept harness
// already builds beryl_cli while generating the run's fixtures.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import net from "node:net";
import { fileURLToPath } from "node:url";

const binary = new URL(
    process.platform === "win32"
        ? "../../rust/target/run/debug/beryl_cli.exe"
        : "../../rust/target/debug/beryl_cli",
    import.meta.url,
);
const loopback = "127.0.0.1";
let starting;
let server;
let stopped;

async function startServer() {
    const socket = net.createServer().listen(0, loopback);
    await once(socket, "listening");
    const { port } = socket.address();
    await new Promise((resolve, reject) =>
        socket.close((error) => (error ? reject(error) : resolve())),
    );

    const url = `http://${loopback}:${port}`;
    const child = spawn(
        fileURLToPath(binary),
        ["serve", "--port", String(port)],
        {
            stdio: ["ignore", "pipe", "inherit"],
        },
    );
    server = child;
    stopped = new Promise((resolve) => {
        child.once("close", resolve);
    });
    process.once("exit", killServer);
    await new Promise((resolve, reject) => {
        child.once("error", reject);
        child.once("exit", (code) =>
            reject(new Error(`table server exited (${code})`)),
        );
        let seen = "";
        child.stdout.setEncoding("utf8");
        child.stdout.on("data", (chunk) => {
            seen += chunk;
            if (seen.includes(`serving on ${url}`)) resolve();
        });
    });
    return url;
}

function killServer() {
    server?.kill();
}

export async function stopTableServer() {
    process.removeListener("exit", killServer);
    killServer();
    await stopped;
}

const url = () => (starting ??= startServer());

export async function tableRoute(page, phone = false) {
    // Live mode normally fetches card scans. These checks are offline.
    await page.route(/^https:\/\//, (route) =>
        route.fulfill({
            contentType: "image/svg+xml",
            body: '<svg xmlns="http://www.w3.org/2000/svg" width="100" height="140"/>',
        }),
    );
    return `/?live=${encodeURIComponent(await url())}${phone ? "&phone" : ""}#/`;
}

export async function tableCommand(command, args = {}) {
    const response = await fetch(`${await url()}/invoke/${command}`, {
        method: "POST",
        body: JSON.stringify(args),
    });
    const reply = await response.json();
    assert.ok("ok" in reply, JSON.stringify(reply));
    return reply.ok;
}

export async function tableView() {
    return (await tableCommand("table_current")).view;
}

/** Opens a fresh table with a decklist and optional setup actions. Setup callbacks receive
 * each latest published view so their object references come from the host's current view. */
export async function startTable(page, text, actions = []) {
    const deck = await tableCommand("deck_import", {
        request: { name: "Table check", text },
    });
    assert.deepEqual(deck.unknown, []);
    await tableCommand("table_solo", { request: { deck } });
    let view = await tableView();
    for (const setup of actions) {
        const action = typeof setup === "function" ? setup(view) : setup;
        const reply = await tableCommand("table_act", { request: { action } });
        assert.deepEqual(reply, { ok: true });
        view = await tableView();
    }
    await page.evaluate(() => {
        window.location.hash = "#/table";
    });
    await page.locator("[data-manual-table]").waitFor();
    return page;
}

export function tableActionRef(view, subject) {
    const action = view.table.objectActions.find(
        ({ subject: match }) =>
            match.kind === subject.kind &&
            (subject.kind === "object"
                ? match.slot === subject.slot
                : match.id === subject.id),
    );
    assert.ok(action, JSON.stringify(subject));
    return { objects: [action.reference], in: action.zone };
}

/** Waits for playback to receive a specific state, and plays it out on the paused clock.
 * The stage tells the check when data arrives even when the host or card cache is slow. */
export async function showTableView(page, expected) {
    await page.evaluate(
        (logLength) =>
            new Promise((resolve) => {
                const stage = window.berylStage;
                const show = () => {
                    stage.skip();
                    if (stage.shown.view?.table.log.length === logLength) {
                        stop();
                        resolve();
                    }
                };
                const stop = stage.subscribe(() => queueMicrotask(show));
                show();
            }),
        expected.table.log.length,
    );
}

export async function clickTable(page, target, { tap, ...options } = {}) {
    const [response] = await Promise.all([
        page.waitForResponse((result) =>
            result.url().endsWith("/invoke/table_act"),
        ),
        tap ? target.tap(options) : target.click(options),
    ]);
    const { ok } = await response.json();
    if (ok.ok) {
        const view = await tableView();
        await showTableView(page, view);
        return view;
    }
    return ok;
}
