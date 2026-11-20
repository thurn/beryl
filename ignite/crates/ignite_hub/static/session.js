// The page of a CLI worker (dashboard/session.rs): follows its transcript digest and sends a
// message to the worker, as `ignite tell` does. `#c-session` names the session, its ticket and
// the API address; `#c-transcript` holds the lines and the position of the last one. The
// script looks the elements up on every tick, so it keeps working after the page is replaced.
(() => {
    "use strict";

    const KINDS = { assistant: "Assistant", tool: "Tool", result: "Result", prompt: "Prompt" };
    const context = () => document.getElementById("c-session");

    function item(line) {
        const kind = KINDS[line.kind] ? line.kind : "assistant";
        const li = document.createElement("li");
        li.className = "c-transcript-line";
        li.dataset.kind = kind;
        const label = document.createElement("span");
        label.className = "c-transcript-kind";
        label.textContent = KINDS[kind];
        const text = document.createElement("span");
        text.className = "c-transcript-text";
        text.textContent = line.text;
        li.append(label, text);
        return li;
    }

    let polling = false;
    async function poll() {
        const ctx = context();
        const list = document.getElementById("c-transcript");
        if (!ctx || !list || polling) return;
        polling = true;
        try {
            const after = Number(list.dataset.after || 0);
            const response = await fetch(
                `${ctx.dataset.api}/transcripts/${encodeURIComponent(ctx.dataset.session)}?after=${after}`,
                { cache: "no-store", credentials: "same-origin" },
            );
            if (!response.ok) return;
            const page = await response.json();
            if (!page.lines.length) return;
            const stick = innerHeight + scrollY >= document.documentElement.scrollHeight - 40;
            for (const line of page.lines) {
                list.append(item(line));
                list.dataset.after = String(line.seq);
            }
            document.querySelector(".c-transcript-empty")?.setAttribute("hidden", "");
            if (stick) scrollTo(0, document.documentElement.scrollHeight);
        } catch {
            // The next tick tries again.
        } finally {
            polling = false;
        }
    }
    setInterval(poll, 3000);

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
