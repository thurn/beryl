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
        const machine = input.closest("[data-fleet-machine]");
        if (!context?.contains(machine) || context.classList.contains("c-editing")) return;
        if (!input.reportValidity()) return;

        const settings = JSON.parse(context.dataset.settings);
        const name = machine.dataset.fleetMachine;
        const field = input.dataset.fleetField;
        const current = Object.hasOwn(settings.machines, name)
            ? settings.machines[name]
            : { state: "active", disk_cap_gb: null };
        const value = input.type === "number"
            ? (input.value === "" ? null : input.valueAsNumber)
            : input.value;
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
                        machines: { ...settings.machines, [name]: { ...current, [field]: value } },
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
        if (field === "state") {
            const previous = [...machine.querySelectorAll('input[data-fleet-field="state"]')]
                .find((radio) => radio.value === current.state);
            previous.checked = true;
        } else {
            input.value = current.disk_cap_gb ?? "";
        }
        for (const control of controls) control.disabled = false;
        context.classList.remove("c-editing");
        document.dispatchEvent(new CustomEvent("cirrus:flush"));
    });
})();
