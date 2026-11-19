// Ticket editing, on pages the hub renders with edit controls: only on the private listener, for
// a Tailscale user (dashboard/edit.rs). The server renders the hooks and this script builds the
// editors:
// - `.c-editable[data-field][data-kind]` is a value, with its rendered HTML in `.c-edit-view`
//   and its raw text in `<template class="c-edit-source">`;
// - `[data-edit=<field>]` is the pencil that opens that value's editor;
// - `[data-action]` buttons release, close or reopen the ticket, `[data-add]` buttons show an
//   empty text section, and `form[data-composer]` adds a comment;
// - `#c-edit` names the ticket, the version an edit expects and the batch address.
// Every edit is one `POST /batch`. While an editor is open its element carries `c-editing`, and
// cirrus.js holds back live page updates so the editor is not replaced; closing it sends
// `cirrus:flush`, which brings the page up to date.
(() => {
    "use strict";

    const PRIORITIES = ["Critical", "High", "Medium", "Low", "Backlog"];
    const STATUSES = [
        ["open", "Approved"],
        ["in_progress", "In progress"],
        ["blocked", "Blocked"],
        ["deferred", "Deferred"],
        ["pending_approval", "Pending approval"],
    ];
    // Text fields name the version they were edited from, so a change someone else made in the
    // meantime is not overwritten without a second Save.
    const GUARDED = new Set(["line", "markdown"]);

    const context = () => document.getElementById("c-edit");
    const flush = (resync) =>
        document.dispatchEvent(new CustomEvent("cirrus:flush", { detail: { resync } }));
    const unitOf = (field) =>
        document.querySelector(`.c-editable[data-field="${CSS.escape(field)}"]`);
    const rawOf = (unit) =>
        unit.querySelector(":scope > .c-edit-source")?.content.textContent ?? "";

    class Conflict extends Error {}

    async function send(ops, guarded = false) {
        const ctx = context();
        if (!ctx) throw new Error("This page cannot edit tickets.");
        const body = { ops };
        if (guarded) body.if = [{ id: ctx.dataset.ticket, version: Number(ctx.dataset.version) }];
        let response;
        try {
            response = await fetch(ctx.dataset.api, {
                method: "POST",
                headers: { "Content-Type": "application/json", "X-Ignite-Dashboard": "1" },
                body: JSON.stringify(body),
                credentials: "same-origin",
                cache: "no-store",
            });
        } catch {
            throw new Error("The hub could not be reached.");
        }
        let data = null;
        try {
            data = await response.json();
        } catch {}
        if (response.status === 409 && data?.current) {
            ctx.dataset.version = String(data.current.version ?? ctx.dataset.version);
            throw new Conflict(
                "This ticket changed since the page loaded. Save again to replace it.",
            );
        }
        if (response.status === 401) {
            throw new Error("Editing works only on the private dashboard, through Tailscale.");
        }
        if (!response.ok) throw new Error(data?.error || `The hub answered ${response.status}.`);
        // The ticket's version is now the feed position of this write.
        if (data?.seq !== undefined) ctx.dataset.version = String(data.seq);
        return data;
    }

    // ---- Building editors ----------------------------------------------------

    function element(tag, props = {}, children = []) {
        const el = Object.assign(document.createElement(tag), props);
        el.append(...children);
        return el;
    }

    function buttons(saveText, onCancel, hint) {
        const row = element("div", { className: "c-edit-buttons" }, [
            element("button", {
                type: "submit",
                className: "c-edit-action c-primary",
                textContent: saveText,
            }),
            element("button", {
                type: "button",
                className: "c-edit-action",
                textContent: "Cancel",
                onclick: onCancel,
            }),
        ]);
        if (hint) row.append(element("span", { className: "c-edit-hint", textContent: hint }));
        return row;
    }

    function select(options, value) {
        const el = element("select");
        for (const [optionValue, text] of options) {
            el.append(element("option", { value: optionValue, textContent: text }));
        }
        el.value = value;
        return el;
    }

    // The control for a value of `kind`, starting at `raw`.
    function control(kind, raw, label) {
        switch (kind) {
            case "markdown": {
                const area = element("textarea", { value: raw, rows: 8 });
                area.setAttribute("aria-label", label);
                const grow = () => {
                    area.style.height = "auto";
                    area.style.height = `${Math.min(area.scrollHeight + 4, innerHeight * 0.7)}px`;
                };
                area.addEventListener("input", grow);
                requestAnimationFrame(grow);
                return area;
            }
            case "priority":
                return select(
                    PRIORITIES.map((name, index) => [String(index), `P${index} ${name}`]),
                    raw,
                );
            case "status":
                return select(STATUSES, raw);
            case "count": {
                const input = element("input", {
                    type: "number",
                    min: "0",
                    step: "1",
                    value: raw,
                    placeholder: "None",
                });
                input.setAttribute("aria-label", label);
                return input;
            }
            default: {
                const input = element("input", { type: "text", value: raw });
                input.setAttribute("aria-label", label);
                if (kind === "labels") input.placeholder = "Labels, separated by spaces";
                return input;
            }
        }
    }

    const words = (text) => text.split(/[\s,]+/).filter(Boolean);

    // The batch operations that set `field` of `kind` from `raw` to `value`.
    function operations(field, kind, raw, value) {
        const id = context().dataset.ticket;
        switch (kind) {
            case "line":
                if (!value.trim()) throw new Error("The title cannot be empty.");
                return [{ op: "update", id, set: { [field]: value.trim() } }];
            case "priority":
                return [{ op: "update", id, set: { priority: Number(value) } }];
            case "labels": {
                const before = new Set(words(raw));
                const after = new Set(words(value));
                return [
                    {
                        op: "update",
                        id,
                        add_labels: [...after].filter((label) => !before.has(label)),
                        remove_labels: [...before].filter((label) => !after.has(label)),
                    },
                ];
            }
            case "count": {
                const text = value.trim();
                if (text && !/^\d+$/.test(text)) throw new Error("Enter a whole number, or nothing.");
                return [{ op: "update", id, set: { [field]: text ? Number(text) : null } }];
            }
            default:
                return [{ op: "update", id, set: { [field]: value } }];
        }
    }

    // ---- Opening, saving and closing an editor -------------------------------

    function open(field) {
        const unit = unitOf(field);
        if (!unit || !context()) return;
        if (unit.classList.contains("c-editing")) {
            unit.querySelector(".c-edit-form input, .c-edit-form textarea, .c-edit-form select")?.focus();
            return;
        }
        const kind = unit.dataset.kind;
        const raw = rawOf(unit);
        const pencil = document.querySelector(`[data-edit="${CSS.escape(field)}"]`);
        const label = (pencil?.getAttribute("aria-label") ?? field).replace(/^Edit /, "");
        const input = control(kind, raw, label);
        const error = element("div", { className: "c-edit-error", role: "alert" });
        const form = element("form", { className: "c-edit-form" }, [input, error]);
        const section = unit.closest("section[data-edit-empty]");
        const close = () => {
            form.remove();
            unit.classList.remove("c-editing");
            if (section && !rawOf(unit).trim()) {
                section.hidden = true;
                document.querySelector(`[data-add="${CSS.escape(field)}"]`)?.removeAttribute("hidden");
            }
            flush(false);
        };
        const save = async () => {
            const value = input.value;
            if (value === raw && !form.dataset.conflict) {
                close();
                return;
            }
            error.textContent = "";
            form.querySelectorAll("button, input, select, textarea").forEach((el) => (el.disabled = true));
            try {
                await send(operations(field, kind, raw, value), GUARDED.has(kind) && !form.dataset.conflict);
            } catch (failure) {
                form.querySelectorAll("button, input, select, textarea").forEach((el) => (el.disabled = false));
                if (failure instanceof Conflict) form.dataset.conflict = "1";
                error.textContent = failure.message;
                input.focus();
                return;
            }
            unit.querySelector(":scope > .c-edit-source").textContent = value;
            unit.classList.add("c-saving");
            form.remove();
            unit.classList.remove("c-editing");
            flush(true);
        };
        const cancel = () => {
            if (input.value !== raw && kind === "markdown" && !confirm("Discard your changes?")) return;
            close();
        };
        form.addEventListener("submit", (e) => {
            e.preventDefault();
            save();
        });
        if (input.tagName === "SELECT") {
            // A choice saves at once; leaving it unchanged closes it.
            input.addEventListener("change", save);
            input.addEventListener("blur", () => {
                if (input.value === raw && form.isConnected && !input.disabled) close();
            });
        } else {
            const hint = kind === "markdown" ? "⌘↩ to save · Esc to cancel" : "↩ to save · Esc to cancel";
            form.insertBefore(buttons("Save", cancel, hint), error);
        }
        form.addEventListener("keydown", (e) => {
            if (e.key === "Escape") {
                e.preventDefault();
                e.stopPropagation();
                cancel();
            } else if (e.key === "Enter" && (kind === "markdown" ? e.metaKey || e.ctrlKey : true)) {
                if (input.tagName === "SELECT") return;
                e.preventDefault();
                save();
            }
        });
        if (section) {
            section.hidden = false;
            document.querySelector(`[data-add="${CSS.escape(field)}"]`)?.setAttribute("hidden", "");
        }
        unit.classList.add("c-editing");
        unit.append(form);
        // A text area opens at the start of the text, not scrolled to the caret at its end.
        input.focus({ preventScroll: true });
        if (input.tagName === "TEXTAREA") {
            input.setSelectionRange(0, 0);
            input.scrollTop = 0;
        } else if (input.type === "text") {
            input.select();
        }
        unit.scrollIntoView({ block: "nearest" });
    }

    // ---- Status actions -------------------------------------------------------

    async function act(button) {
        const action = button.dataset.action;
        const bar = button.closest(".c-edit-actions");
        const id = context()?.dataset.ticket;
        if (!id || !bar) return;
        if (action === "close") {
            closeForm(bar, id);
            return;
        }
        const ops = action === "release"
            ? [{ op: "update", id, set: { status: "open" } }]
            : [{ op: "reopen", id }];
        bar.querySelectorAll("button").forEach((el) => (el.disabled = true));
        try {
            await send(ops);
            bar.classList.add("c-saving");
            flush(true);
        } catch (failure) {
            bar.querySelectorAll("button").forEach((el) => (el.disabled = false));
            showError(bar, failure.message);
        }
    }

    function showError(bar, message) {
        let error = bar.querySelector(":scope > .c-edit-error");
        if (!error) {
            error = element("div", { className: "c-edit-error", role: "alert" });
            bar.append(error);
        }
        error.textContent = message;
    }

    function closeForm(bar, id) {
        if (bar.querySelector(".c-edit-form")) return;
        const reason = element("textarea", { rows: 3, placeholder: "Reason" });
        reason.setAttribute("aria-label", "Close reason");
        const error = element("div", { className: "c-edit-error", role: "alert" });
        const form = element("form", { className: "c-edit-form" }, [reason]);
        const done = () => {
            form.remove();
            bar.classList.remove("c-editing");
            flush(false);
        };
        form.append(buttons("Close ticket", done), error);
        form.addEventListener("keydown", (e) => {
            if (e.key === "Escape") {
                e.preventDefault();
                done();
            } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                e.preventDefault();
                form.requestSubmit();
            }
        });
        form.addEventListener("submit", async (e) => {
            e.preventDefault();
            if (!reason.value.trim()) {
                error.textContent = "Say why it is closed.";
                reason.focus();
                return;
            }
            form.querySelectorAll("button, textarea").forEach((el) => (el.disabled = true));
            try {
                await send([{ op: "close", id, reason: reason.value.trim() }]);
            } catch (failure) {
                form.querySelectorAll("button, textarea").forEach((el) => (el.disabled = false));
                error.textContent = failure.message;
                return;
            }
            form.remove();
            bar.classList.remove("c-editing");
            bar.classList.add("c-saving");
            flush(true);
        });
        bar.classList.add("c-editing");
        bar.append(form);
        reason.focus();
    }

    // ---- Comments -------------------------------------------------------------

    async function comment(form) {
        const area = form.querySelector("textarea");
        const id = context()?.dataset.ticket;
        if (!area || !id || !area.value.trim()) return;
        let error = form.querySelector(".c-edit-error");
        if (!error) {
            error = element("div", { className: "c-edit-error", role: "alert" });
            form.append(error);
        }
        error.textContent = "";
        form.querySelectorAll("button, textarea").forEach((el) => (el.disabled = true));
        try {
            await send([{ op: "comment", id, text: area.value.trim() }]);
            area.value = "";
            form.classList.remove("c-editing");
            flush(true);
        } catch (failure) {
            error.textContent = failure.message;
        } finally {
            form.querySelectorAll("button, textarea").forEach((el) => (el.disabled = false));
        }
    }

    // ---- Wiring ---------------------------------------------------------------
    // Delegated listeners, since cirrus.js replaces the page's content as it navigates and
    // updates.

    document.addEventListener("click", (e) => {
        const pencil = e.target.closest?.("[data-edit]");
        if (pencil) {
            e.preventDefault();
            open(pencil.dataset.edit);
            return;
        }
        const add = e.target.closest?.("[data-add]");
        if (add) {
            e.preventDefault();
            open(add.dataset.add);
            return;
        }
        const action = e.target.closest?.("button[data-action]");
        if (action && action.closest(".c-edit-actions")) {
            e.preventDefault();
            act(action);
        }
    });

    document.addEventListener("submit", (e) => {
        const form = e.target.closest?.("form[data-composer]");
        if (!form) return;
        e.preventDefault();
        comment(form);
    });

    document.addEventListener("keydown", (e) => {
        const form = e.target.closest?.("form[data-composer]");
        if (form && e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            comment(form);
        }
    });

    // A comment being written holds back live updates too, so it is not wiped by one.
    document.addEventListener("input", (e) => {
        const form = e.target.closest?.("form[data-composer]");
        if (!form) return;
        const writing = e.target.value.trim() !== "";
        form.classList.toggle("c-editing", writing);
        if (!writing) flush(false);
    });

    addEventListener("beforeunload", (e) => {
        if (document.querySelector(".c-editing")) e.preventDefault();
    });
})();
