// A live table uses the same loopback server and URL as just ui-live. The kept harness
// already builds beryl_cli while generating the run's fixtures.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

const binary = new URL(
    process.platform === "win32"
        ? "../../rust/target/run/debug/beryl_cli.exe"
        : "../../rust/target/debug/beryl_cli",
    import.meta.url,
);
let starting;
let server;
let stopped;

async function startServer() {
    const child = spawn(fileURLToPath(binary), ["serve", "--port", "0"], {
        stdio: ["ignore", "pipe", "inherit"],
    });
    server = child;
    stopped = new Promise((resolve) => {
        child.once("close", resolve);
    });
    process.once("exit", killServer);
    return new Promise((resolve, reject) => {
        child.once("error", reject);
        child.once("exit", (code) =>
            reject(new Error(`table server exited (${code})`)),
        );
        let seen = "";
        child.stdout.setEncoding("utf8");
        child.stdout.on("data", (chunk) => {
            seen += chunk;
            const address = seen.match(
                /^beryl_cli: serving on (http:\/\/127\.0\.0\.1:\d+)\r?\n/m,
            );
            if (address) resolve(address[1]);
        });
    });
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

export async function tableRoute(page, phone = false, session) {
    // Live mode normally fetches card scans. These checks are offline.
    await page.route(/^https:\/\//, (route) =>
        route.fulfill({
            contentType: "image/svg+xml",
            body: '<svg xmlns="http://www.w3.org/2000/svg" width="100" height="140"/>',
        }),
    );
    const query = new URLSearchParams({ live: await url() });
    if (session) query.set("session", session);
    if (phone) query.set("phone", "");
    return `/?${query}#/`;
}

export async function tableCommand(command, args = {}, session) {
    const query = session ? `?${new URLSearchParams({ session })}` : "";
    const response = await fetch(`${await url()}/invoke/${command}${query}`, {
        method: "POST",
        body: JSON.stringify(args),
    });
    const reply = await response.json();
    assert.ok("ok" in reply, JSON.stringify(reply));
    return reply.ok;
}

export async function tableView(session) {
    return (await tableCommand("table_current", {}, session)).view;
}

const pageSession = (page) => new URL(page.url()).searchParams.get("session");

async function openTable(page) {
    await page.evaluate(() => {
        window.location.hash = "#/table";
    });
    await page.locator("[data-manual-table]").waitFor();
}

/** Give a live host page a guest in the same paused-clock browser context. */
export async function pair(
    host,
    link = "local",
    { fromStart = false, hostName = "Host" } = {},
) {
    const guest = await host.context().newPage();
    try {
        const route = await tableRoute(guest, false, "guest");
        const guestUrl = new URL(route, host.url());
        if (fromStart) {
            await guest.goto(guestUrl.href);
            return { host, guest };
        }
        const { invite } = await tableCommand(
            "table_host",
            {
                request: { link, settings: { name: hostName } },
            },
            "host",
        );
        await tableCommand(
            "table_join",
            {
                request: { invite, name: "Guest" },
            },
            "guest",
        );
        guestUrl.hash = "#/table";
        await guest.goto(guestUrl.href);
        await Promise.all([
            openTable(host),
            guest.locator("[data-manual-table]").waitFor(),
        ]);
        return { host, guest };
    } catch (error) {
        await guest.close();
        await tableCommand("table_leave", {}, "host");
        await tableCommand("table_leave", {}, "guest");
        throw error;
    }
}

/** Opens a fresh table with a decklist and optional setup actions. Setup callbacks receive
 * each latest published view so their object references come from the host's current view. */
export async function startTable(page, text, actions = []) {
    const session = pageSession(page);
    const deck = await tableCommand(
        "deck_import",
        {
            request: { name: "Table check", text },
        },
        session,
    );
    assert.deepEqual(deck.unknown, []);
    await tableCommand("table_solo", { request: { deck } }, session);
    let view = await tableView(session);
    for (const setup of actions) {
        const action = typeof setup === "function" ? setup(view) : setup;
        const reply = await tableCommand(
            "table_act",
            { request: { action } },
            session,
        );
        assert.deepEqual(reply, { ok: true });
        view = await tableView(session);
    }
    await openTable(page);
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

/** Wait for the action sent by the page and, on success, finish drawing its update. */
export async function clickTableResult(page) {
    const response = await page.waitForResponse(
        (result) => new URL(result.url()).pathname === "/invoke/table_act",
    );
    return applyTableReply(page, response, pageSession(page));
}

async function applyTableReply(page, response, authority) {
    const { ok } = await response.json();
    if (ok.ok) {
        const view = await tableView(authority);
        await showTableView(page, view);
        return authority === pageSession(page)
            ? view
            : tableView(pageSession(page));
    }
    return ok;
}

export async function clickTable(page, target, { tap, ...options } = {}) {
    const [result] = await Promise.all([
        clickTableResult(page),
        tap ? target.tap(options) : target.click(options),
    ]);
    return result;
}

export async function clickAnswer(page, target, kind) {
    const [response] = await Promise.all([
        page.waitForResponse(
            (result) =>
                new URL(result.url()).pathname === "/invoke/table_answer" &&
                result.request().postDataJSON()?.request.answer.kind === kind,
        ),
        target.click(),
    ]);
    // The host has committed the answer before the guest's streamed view
    // reaches the browser. Wait for that host log position on either page.
    const authority =
        pageSession(page) === "guest" ? "host" : pageSession(page);
    const result = await applyTableReply(page, response, authority);
    assert.ok(result.table, JSON.stringify(result));
    return result;
}
