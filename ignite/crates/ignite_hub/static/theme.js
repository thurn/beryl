// The theme choice in the header's settings menu: auto (follow the system),
// light or dark. A choice is saved in this browser and applied as data-theme
// on <html>; an inline script in the page's <head> applies it again before the
// first paint of the next page. Auto removes both.
(() => {
    "use strict";

    const KEY = "cirrus-theme";
    const CHOICES = ["auto", "light", "dark"];

    function saved() {
        try {
            const value = localStorage.getItem(KEY);
            return CHOICES.includes(value) ? value : "auto";
        } catch {
            return "auto";
        }
    }

    function apply(choice) {
        const root = document.documentElement;
        if (choice === "auto") delete root.dataset.theme;
        else root.dataset.theme = choice;
        mark(choice);
    }

    // The header is replaced on every navigation, so the current choice is
    // marked again whenever it changes.
    function mark(choice = saved()) {
        for (const button of document.querySelectorAll("[data-theme-choice]")) {
            button.setAttribute(
                "aria-pressed",
                String(button.dataset.themeChoice === choice),
            );
        }
    }

    document.addEventListener("click", (e) => {
        const button = e.target.closest?.("[data-theme-choice]");
        if (button) {
            const choice = button.dataset.themeChoice;
            try {
                if (choice === "auto") localStorage.removeItem(KEY);
                else localStorage.setItem(KEY, choice);
            } catch {
                // Storage is off: the choice still holds until the page reloads.
            }
            apply(choice);
            button.closest("details")?.removeAttribute("open");
            return;
        }
        // A click anywhere else closes an open menu.
        for (const open of document.querySelectorAll("details.c-settings[open]")) {
            if (!open.contains(e.target)) open.removeAttribute("open");
        }
    });

    document.addEventListener("keydown", (e) => {
        if (e.key !== "Escape") return;
        const open = document.querySelector("details.c-settings[open]");
        if (!open) return;
        open.removeAttribute("open");
        open.querySelector("summary")?.focus();
    });

    // Another tab changed the choice.
    addEventListener("storage", (e) => {
        if (e.key === KEY) apply(saved());
    });

    new MutationObserver(() => mark()).observe(document.body, {
        childList: true,
    });
    apply(saved());
})();
