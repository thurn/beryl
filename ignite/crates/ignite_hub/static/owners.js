// The default owner in the header's settings panel: the machine and app whose workers take every
// ticket with no owner of its own or an ancestor's (ignite_core::owner). The panel names the API
// in `data-owners`. Opening it loads `GET <api>/owners`; picking another owner sends
// `POST <api>/owners/default`, which the hub accepts only on the private dashboard, from a
// Tailscale user (auth.rs).
(() => {
    "use strict";

    const label = (owner) => {
        const [machine, app] = owner.split("/");
        return app ? `${machine} · ${app[0].toUpperCase()}${app.slice(1)}` : machine;
    };

    async function call(section, path, init = {}) {
        const response = await fetch(section.dataset.owners + path, {
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
            throw new Error("Change it on the private dashboard, through Tailscale.");
        }
        if (!response.ok) throw new Error(data?.error ?? `The hub answered ${response.status}.`);
        return data;
    }

    function show(section, data) {
        const select = section.querySelector("select");
        const note = section.querySelector(".c-settings-note");
        select.replaceChildren(
            ...data.choices.map((owner) => {
                const option = document.createElement("option");
                option.value = owner;
                option.textContent = label(owner);
                return option;
            }),
        );
        select.value = data.default ?? "";
        select.dataset.current = select.value;
        select.disabled = data.choices.length === 0;
        note.textContent = data.picked
            ? `Tickets with no owner go here. Set by ${data.picked.by}.`
            : "Tickets with no owner go here.";
    }

    async function load(section) {
        const note = section.querySelector(".c-settings-note");
        try {
            show(section, await call(section, "/owners"));
        } catch (failure) {
            note.textContent = failure.message;
        }
    }

    // The header is replaced on every navigation, so the panel loads each time it opens.
    document.addEventListener(
        "toggle",
        (e) => {
            const section = e.target.matches?.("details.c-settings[open]")
                ? e.target.querySelector("[data-owners]")
                : null;
            if (section) load(section);
        },
        true,
    );

    document.addEventListener("change", async (e) => {
        const select = e.target.closest?.("[data-owners] select");
        if (!select) return;
        const section = select.closest("[data-owners]");
        const note = section.querySelector(".c-settings-note");
        select.disabled = true;
        try {
            const body = JSON.stringify({ owner: select.value });
            show(section, await call(section, "/owners/default", { method: "POST", body }));
        } catch (failure) {
            select.value = select.dataset.current ?? "";
            select.disabled = false;
            note.textContent = failure.message;
        }
    });
})();
