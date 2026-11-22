// Fleet settings are a versioned snapshot. Submit the changed field with that
// snapshot so a stale page cannot overwrite another operator's changes.
(() => {
    "use strict";

    const conflictKey = "fleetSettingsConflict";
    const editor = () => document.getElementById("c-fleet");

    function showMessage(context, message) {
        const status = context.querySelector("[data-fleet-status]");
        status.textContent = message;
        status.hidden = false;
        status.focus();
    }

    const context = editor();
    if (context && history.state?.[conflictKey]) {
        history.replaceState({ ...history.state, [conflictKey]: false }, "");
        showMessage(context, "Fleet settings changed. The latest settings are shown.");
    }

    document.addEventListener("change", async (event) => {
        const input = event.target;
        if (!(input instanceof HTMLInputElement) || !input.dataset.fleetField) return;
        const context = editor();
        const row = input.closest("[data-fleet-machine], [data-fleet-provider]");
        if (!context?.contains(row) || context.classList.contains("c-editing")) return;
        if (!input.reportValidity()) return;

        const settings = JSON.parse(context.dataset.settings);
        const provider = row.hasAttribute("data-fleet-provider");
        const group = provider ? "providers" : "machines";
        const name = provider ? row.dataset.fleetProvider : row.dataset.fleetMachine;
        const field = input.dataset.fleetField;
        const entries = settings[group];
        const current = Object.hasOwn(entries, name)
            ? entries[name]
            : provider
                ? Object.fromEntries([...row.querySelectorAll("[data-fleet-field]")].map((control) => [
                    control.dataset.fleetField,
                    control.type === "checkbox" ? control.defaultChecked : Number(control.defaultValue),
                ]))
                : {
                    state: "active",
                    disk_cap_gb: null,
                    trees_root: null,
                    tree_space_gb: null,
                    tree_ceiling_gb: Number(row.querySelector('[data-fleet-field="tree_ceiling_gb"]').defaultValue),
                };
        let value = input.value;
        if (input.type === "number") value = input.value === "" ? null : input.valueAsNumber;
        if (input.type === "checkbox") value = input.checked;
        if (field === "trees_root" && value === "") value = null;
        if (field === "tree_space_gb") value = value === "auto" ? value : value === "" ? null : Number(value);
        if (current[field] === value) return;

        const controls = context.querySelectorAll("[data-fleet-field]");
        context.classList.add("c-editing");
        for (const control of controls) control.disabled = true;
        try {
            const response = await fetch(context.dataset.api, {
                method: "PUT",
                headers: { "Content-Type": "application/json", "X-Ignite-Dashboard": "1" },
                credentials: "same-origin",
                cache: "no-store",
                body: JSON.stringify({
                    version: settings.version,
                    settings: {
                        ...settings,
                        [group]: { ...entries, [name]: { ...current, [field]: value } },
                    },
                }),
            });
            if (!context.isConnected) return;
            const conflict = response.status === 409;
            if (response.ok || conflict) {
                history.replaceState(
                    { ...history.state, scroll: scrollY, [conflictKey]: conflict },
                    "",
                );
                location.reload();
                return;
            }
            const data = await response.json().catch(() => null);
            showMessage(context, data?.error || `The hub answered ${response.status}.`);
        } catch {
            if (context.isConnected) showMessage(context, "The hub could not be reached.");
        }
        // Restore the saved value so the operator can repeat the edit.
        if (input.type === "radio") {
            const previous = [...row.querySelectorAll('input[data-fleet-field="state"]')]
                .find((radio) => radio.value === current[field]);
            previous.checked = true;
        } else if (input.type === "checkbox") {
            input.checked = current[field];
        } else {
            input.value = current[field] ?? "";
        }
        for (const control of controls) control.disabled = false;
        context.classList.remove("c-editing");
        document.dispatchEvent(new CustomEvent("cirrus:flush"));
    });
})();
