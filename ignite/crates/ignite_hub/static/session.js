// The page of a CLI worker (dashboard/session.rs), laid out like a conversation in the Claude
// desktop app. `#c-session` names the session, its ticket and the API address; `#c-transcript`
// is the conversation, `#c-transcript-lines` the JSON of the lines the page was served with and
// `data-after` the position of the last of them.
//
// The script renders every line itself: what the worker wrote as Markdown, each run of tool
// calls as one collapsed block with each call's result inside, each prompt as the user's
// message and each turn's end as a divider. It follows later lines with a long poll on
// `GET /transcripts/<session>?after=<seq>&wait=<s>`, which answers as soon as a line exists.
// cirrus.js replaces the page's main element when someone navigates, so a MutationObserver
// mounts the conversation whenever a new `#c-transcript` appears and stops following an old one.
(() => {
    "use strict";

    const WAIT_S = 25;
    const RETRY_MIN_MS = 250;
    const RETRY_MAX_MS = 30_000;
    const context = () => document.getElementById("c-session");

    // ---- Markdown ------------------------------------------------------------
    // Transcripts are untrusted text. Every piece of it is escaped before it is put between the
    // few tags below, and a link's address must start with http:// or https://.

    const esc = (text) =>
        String(text).replace(/[&<>"']/g, (ch) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[ch]);

    function inline(text) {
        return text
            .split(/(`+[^`]*?`+)/)
            .map((part, i) => {
                if (i % 2 === 1) {
                    const code = part.replace(/^`+|`+$/g, "");
                    return `<code>${esc(code)}</code>`;
                }
                return esc(part)
                    .replace(/\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/g, '<a href="$2" target="_blank" rel="noopener noreferrer">$1</a>')
                    .replace(/\*\*([^*]+?)\*\*|__([^_]+?)__/g, (_, a, b) => `<strong>${a ?? b}</strong>`)
                    .replace(/(^|[^\w*])\*([^*\s][^*]*?)\*(?![\w*])/g, "$1<em>$2</em>");
            })
            .join("");
    }

    const FENCE = /^\s*(`{3,}|~{3,})/;
    const HEADING = /^(#{1,6})\s+(.*)$/;
    const ITEM = /^(\s*)([-*+]|\d+[.)])\s+(.*)$/;
    const QUOTE = /^\s*>\s?/;
    const TABLE_RULE = /^\s*\|?\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)*\|?\s*$/;
    const startsBlock = (line, next) =>
        FENCE.test(line) || HEADING.test(line) || ITEM.test(line) || QUOTE.test(line) ||
        (line.includes("|") && TABLE_RULE.test(next ?? ""));
    const cells = (line) => line.trim().replace(/^\||\|$/g, "").split("|").map((cell) => inline(cell.trim()));

    function markdown(source) {
        const lines = source.replace(/\r\n?/g, "\n").split("\n");
        const out = [];
        let i = 0;
        while (i < lines.length) {
            const line = lines[i];
            const fence = line.match(FENCE);
            if (fence) {
                const body = [];
                i++;
                while (i < lines.length && !lines[i].trim().startsWith(fence[1])) body.push(lines[i++]);
                i++;
                out.push(`<pre class="c-md-code"><code>${esc(body.join("\n"))}</code></pre>`);
            } else if (!line.trim()) {
                i++;
            } else if (HEADING.test(line)) {
                const [, marks, text] = line.match(HEADING);
                const level = Math.min(6, marks.length + 2);
                out.push(`<h${level} class="c-md-heading">${inline(text)}</h${level}>`);
                i++;
            } else if (QUOTE.test(line)) {
                const body = [];
                while (i < lines.length && QUOTE.test(lines[i])) body.push(lines[i++].replace(QUOTE, ""));
                out.push(`<blockquote>${markdown(body.join("\n"))}</blockquote>`);
            } else if (ITEM.test(line)) {
                const ordered = /\d/.test(line.match(ITEM)[2]);
                const items = [];
                while (i < lines.length) {
                    const item = lines[i].match(ITEM);
                    if (item) {
                        const nested = item[1].length >= 2 ? ' class="c-md-nested"' : "";
                        items.push([nested, item[3]]);
                    } else if (lines[i].trim() && /^\s+/.test(lines[i]) && items.length) {
                        items[items.length - 1][1] += ` ${lines[i].trim()}`;
                    } else break;
                    i++;
                }
                const tag = ordered ? "ol" : "ul";
                out.push(`<${tag}>${items.map(([nested, text]) => `<li${nested}>${inline(text)}</li>`).join("")}</${tag}>`);
            } else if (line.includes("|") && TABLE_RULE.test(lines[i + 1] ?? "")) {
                const head = cells(line);
                i += 2;
                const rows = [];
                while (i < lines.length && lines[i].includes("|") && lines[i].trim()) rows.push(cells(lines[i++]));
                out.push(
                    `<div class="c-md-table"><table><thead><tr>${head.map((cell) => `<th>${cell}</th>`).join("")}</tr></thead>` +
                        `<tbody>${rows.map((row) => `<tr>${row.map((cell) => `<td>${cell}</td>`).join("")}</tr>`).join("")}</tbody></table></div>`,
                );
            } else {
                const body = [];
                while (i < lines.length && lines[i].trim() && !(body.length && startsBlock(lines[i], lines[i + 1]))) {
                    body.push(lines[i++].trim());
                }
                out.push(`<p>${inline(body.join(" "))}</p>`);
            }
        }
        return out.join("");
    }

    // ---- Tool calls ----------------------------------------------------------

    // What a run of tool calls is summed up as, by the kind of tool.
    const CATEGORIES = [
        { names: ["Bash", "command_execution", "BashOutput", "KillShell"], one: "ran a command", many: (n) => `ran ${n} commands`, icon: "terminal" },
        { names: ["Read", "NotebookRead"], one: "read a file", many: (n) => `read ${n} files`, icon: "file" },
        { names: ["Edit", "MultiEdit", "Write", "NotebookEdit", "file_change"], one: "edited a file", many: (n) => `edited ${n} files`, icon: "pencil" },
        { names: ["Grep", "Glob", "WebSearch", "web_search", "ToolSearch"], one: "searched once", many: (n) => `searched ${n} times`, icon: "search" },
        { names: ["Agent", "Task"], one: "ran an agent", many: (n) => `ran ${n} agents`, icon: "spark" },
    ];
    const OTHER = { one: "used a tool", many: (n) => `used ${n} tools`, icon: "tool" };
    const category = (name) => CATEGORIES.find((c) => c.names.includes(name)) ?? OTHER;

    const ICONS = {
        terminal: '<path d="M3 4.5h10v7H3z"/><path d="M5 7l1.5 1.2L5 9.4M8 9.5h2.5"/>',
        file: '<path d="M4.5 2.5h4.5l2.5 2.5v8.5h-7z"/><path d="M9 2.5V5h2.5"/>',
        pencil: '<path d="M3 13l1-3.2 6.6-6.6 2.2 2.2L6.2 12z"/>',
        search: '<circle cx="7" cy="7" r="3.5"/><path d="M9.6 9.6L13 13"/>',
        spark: '<path d="M8 2.5v3M8 10.5v3M2.5 8h3M10.5 8h3M4.4 4.4l1.8 1.8M9.8 9.8l1.8 1.8M4.4 11.6l1.8-1.8M9.8 6.2l1.8-1.8"/>',
        tool: '<path d="M10.5 2.8a3 3 0 00-3.4 4L3 10.9 5.1 13l4.1-4.1a3 3 0 004-3.4L11.5 7.2 9.8 6.2 8.8 4.5z"/>',
        chevron: '<path d="M6 4l4 4-4 4"/>',
    };
    const icon = (name, cls = "c-tool-icon") =>
        `<svg class="${cls}" viewBox="0 0 16 16" aria-hidden="true">${ICONS[name]}</svg>`;

    // A tool line is `<name>: <first line of input>`, or just `<name>`.
    function splitTool(text) {
        const at = text.indexOf(": ");
        return at < 0 ? [text, ""] : [text.slice(0, at), text.slice(at + 2)];
    }

    function summary(calls) {
        const counts = new Map();
        for (const call of calls) {
            const c = category(splitTool(call.line.text)[0]);
            counts.set(c, (counts.get(c) ?? 0) + 1);
        }
        const text = [...counts].map(([c, n]) => (n === 1 ? c.one : c.many(n))).join(", ");
        return text.charAt(0).toUpperCase() + text.slice(1);
    }

    function callRow(call) {
        const [name, input] = splitTool(call.line.text);
        const row = `${icon(category(name).icon)}<span class="c-tool-name">${esc(name)}</span>` +
            `<span class="c-tool-input">${esc(input)}</span>`;
        if (call.result === undefined) return `<div class="c-tool-row">${row}</div>`;
        return `<details class="c-tool-call" data-key="${call.line.index}"><summary class="c-tool-row">${row}` +
            `${icon("chevron", "c-tool-chevron")}</summary><pre class="c-tool-result">${esc(call.result)}</pre></details>`;
    }

    // ---- The conversation -----------------------------------------------------

    // The blocks the lines are drawn as, each with a key that stays the same as lines arrive
    // and a signature that changes when the block must be drawn again.
    function blocks(lines) {
        const results = new Map();
        for (const line of lines) {
            if (line.kind === "tool_result" && line.call != null) results.set(`${line.turn}:${line.call}`, line.text);
        }
        const out = [];
        for (const line of lines) {
            if (line.kind === "tool_result") continue;
            if (line.kind === "tool") {
                const call = { line, result: results.get(`${line.turn}:${line.index}`) };
                const last = out[out.length - 1];
                if (last?.kind === "tools" && last.turn === line.turn) last.calls.push(call);
                else out.push({ kind: "tools", turn: line.turn, key: `tools-${line.turn}-${line.index}`, calls: [call] });
            } else {
                out.push({ kind: line.kind, key: `${line.kind}-${line.turn}-${line.index}`, line });
            }
        }
        for (const block of out) {
            block.sig = block.kind === "tools"
                ? `${block.calls.length}/${block.calls.filter((call) => call.result !== undefined).length}`
                : "1";
        }
        return out;
    }

    function turnEnd(line) {
        const text = line.text;
        const ok = /^(success|completed)\b/.test(text);
        const div = document.createElement("div");
        div.className = "c-turn-end";
        if (!ok) div.dataset.failed = "";
        const label = document.createElement("span");
        label.textContent = ok ? `Turn ${line.turn} ended` : `Turn ${line.turn} · ${text}`;
        div.append(label);
        return div;
    }

    function draw(block) {
        if (block.kind === "tools") {
            const el = document.createElement("details");
            el.className = "c-tools";
            if (block.calls.length === 1) {
                // One call is its own row; opening it shows the result.
                const [call] = block.calls;
                const [name, input] = splitTool(call.line.text);
                el.innerHTML = `<summary class="c-tool-row">${icon(category(name).icon)}<span class="c-tool-name">${esc(name)}</span>` +
                    `<span class="c-tool-input">${esc(input)}</span>${icon("chevron", "c-tool-chevron")}</summary>` +
                    (call.result === undefined
                        ? '<p class="c-tool-none">No output yet.</p>'
                        : `<pre class="c-tool-result">${esc(call.result)}</pre>`);
            } else {
                const last = splitTool(block.calls[block.calls.length - 1].line.text)[1];
                el.innerHTML = `<summary class="c-tool-row">${icon(category(splitTool(block.calls[0].line.text)[0]).icon)}` +
                    `<span class="c-tool-name">${esc(summary(block.calls))}</span>` +
                    `<span class="c-tool-input">${esc(last)}</span>${icon("chevron", "c-tool-chevron")}</summary>` +
                    `<div class="c-tool-list">${block.calls.map(callRow).join("")}</div>`;
            }
            return el;
        }
        if (block.kind === "result") return turnEnd(block.line);
        const el = document.createElement("div");
        if (block.kind === "prompt") {
            el.className = "c-msg c-msg-user";
            const bubble = document.createElement("div");
            bubble.className = "c-bubble";
            bubble.textContent = block.line.text;
            el.append(bubble);
        } else {
            el.className = "c-msg c-msg-assistant";
            el.innerHTML = markdown(block.line.text);
        }
        return el;
    }

    // Brings `box` up to date with `lines`, drawing again only the blocks that changed and
    // keeping which tool blocks were open.
    function render(box, lines) {
        const wanted = blocks(lines);
        wanted.forEach((block, i) => {
            const current = box.children[i];
            if (current?.dataset.key === block.key && current.dataset.sig === block.sig) return;
            const fresh = draw(block);
            fresh.dataset.key = block.key;
            fresh.dataset.sig = block.sig;
            if (current?.dataset.key === block.key && current.open) {
                fresh.open = true;
                const open = new Set([...current.querySelectorAll("details[open]")].map((d) => d.dataset.key));
                fresh.querySelectorAll("details").forEach((d) => (d.open = open.has(d.dataset.key)));
            }
            if (current) current.replaceWith(fresh);
            else box.append(fresh);
        });
        while (box.children.length > wanted.length) box.lastElementChild.remove();
    }

    const atBottom = () => innerHeight + scrollY >= document.documentElement.scrollHeight - 80;

    // Shows the lines of `box` and follows later ones until stopped.
    function follow(box) {
        let lines = [];
        try {
            lines = JSON.parse(document.getElementById("c-transcript-lines")?.textContent || "[]");
        } catch {}
        render(box, lines);
        scrollTo(0, document.documentElement.scrollHeight);
        let after = Number(box.dataset.after || 0);
        let controller;
        let running = false;
        let stopped = false;
        let paused = false;

        async function loop() {
            if (running) return;
            running = true;
            let delay = RETRY_MIN_MS;
            try {
                while (!stopped && !paused && box.isConnected) {
                    const ctx = context();
                    if (!ctx) return;
                    controller = new AbortController();
                    const asked = performance.now();
                    let page;
                    try {
                        const response = await fetch(
                            `${ctx.dataset.api}/transcripts/${encodeURIComponent(ctx.dataset.session)}?after=${after}&wait=${WAIT_S}`,
                            { cache: "no-store", credentials: "same-origin", signal: controller.signal },
                        );
                        if (!response.ok) throw new Error(String(response.status));
                        page = await response.json();
                    } catch {
                        if (stopped || paused) return;
                        await new Promise((resolve) => setTimeout(resolve, delay));
                        delay = Math.min(RETRY_MAX_MS, delay * 2);
                        continue;
                    }
                    if (!page.lines.length) {
                        // The hub answers at once, empty, when too many requests already wait.
                        if (performance.now() - asked < WAIT_S * 500) {
                            await new Promise((resolve) => setTimeout(resolve, delay));
                            delay = Math.min(RETRY_MAX_MS, delay * 2);
                        }
                        continue;
                    }
                    delay = RETRY_MIN_MS;
                    const stick = atBottom();
                    lines = lines.concat(page.lines);
                    after = page.lines[page.lines.length - 1].seq;
                    box.dataset.after = String(after);
                    render(box, lines);
                    document.querySelector(".c-transcript-empty")?.setAttribute("hidden", "");
                    if (stick) scrollTo(0, document.documentElement.scrollHeight);
                }
            } finally {
                running = false;
            }
        }
        loop();
        return {
            box,
            stop() {
                stopped = true;
                controller?.abort();
            },
            pause() {
                paused = true;
                controller?.abort();
            },
            resume() {
                paused = false;
                loop();
            },
        };
    }

    let follower;
    function mount() {
        if (follower && !follower.box.isConnected) {
            follower.stop();
            follower = undefined;
        }
        const box = document.getElementById("c-transcript");
        if (!box || box.dataset.mounted) return;
        box.dataset.mounted = "1";
        follower?.stop();
        follower = follow(box);
    }
    new MutationObserver(mount).observe(document.documentElement, { childList: true, subtree: true });
    addEventListener("pagehide", () => follower?.pause());
    addEventListener("pageshow", () => follower?.resume());
    if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", mount);
    else mount();

    // ---- Telling the worker ---------------------------------------------------

    async function json(url, init) {
        const response = await fetch(url, {
            ...init,
            headers: { "Content-Type": "application/json", "X-Ignite-Dashboard": "1" },
            credentials: "same-origin",
            cache: "no-store",
        });
        let data = null;
        try {
            data = await response.json();
        } catch {}
        if (response.status === 401) {
            throw new Error("Telling a worker works only on the private dashboard, through Tailscale.");
        }
        return { response, data };
    }

    // The comment's number is the one after the ticket's last, so the batch names the version it
    // read; when someone commented in between, the hub answers 409 and this reads again.
    async function tell(form) {
        const ctx = context();
        const area = form.querySelector("textarea");
        const text = area.value.trim();
        if (!ctx || !ctx.dataset.ticket || !text) return;
        const now = form.querySelector("input[name=now]").checked;
        const error = form.querySelector(".c-edit-error") ?? form.appendChild(
            Object.assign(document.createElement("div"), { className: "c-edit-error", role: "alert" }),
        );
        error.textContent = "";
        form.querySelectorAll("button, textarea, input").forEach((el) => (el.disabled = true));
        try {
            const id = ctx.dataset.ticket;
            for (let attempt = 0; attempt < 4; attempt++) {
                const read = await json(
                    `${ctx.dataset.api}/tickets/${encodeURIComponent(id)}?comments=1`,
                );
                if (!read.response.ok) throw new Error(read.data?.error || "The ticket could not be read.");
                const n = Math.max(0, ...(read.data.comments ?? []).map((comment) => comment.n)) + 1;
                const meta = { tell: n };
                if (now) meta[form.dataset.nowKey] = n;
                const sent = await json(`${ctx.dataset.api}/batch`, {
                    method: "POST",
                    body: JSON.stringify({
                        if: [{ id, version: read.data.version }],
                        ops: [
                            { op: "comment", id, text },
                            { op: "update", id, set_meta: meta },
                        ],
                    }),
                });
                if (sent.response.status === 409) continue;
                if (!sent.response.ok) throw new Error(sent.data?.error || `The hub answered ${sent.response.status}.`);
                area.value = "";
                form.querySelector("input[name=now]").checked = false;
                return;
            }
            throw new Error("The ticket kept changing. Send it again.");
        } catch (failure) {
            error.textContent = failure.message;
        } finally {
            form.querySelectorAll("button, textarea, input").forEach((el) => (el.disabled = false));
        }
    }

    document.addEventListener("submit", (e) => {
        const form = e.target.closest?.("form[data-tell]");
        if (!form) return;
        e.preventDefault();
        tell(form);
    });
    document.addEventListener("keydown", (e) => {
        const form = e.target.closest?.("form[data-tell]");
        if (form && e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            tell(form);
        }
    });
})();
