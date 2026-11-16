// Cirrus's one script. It makes a server-rendered site feel instant:
// - navigation fetches the next page (usually already prefetched), swaps the
//   header and <main> inside a view transition, and keeps scroll positions;
// - links are prefetched on hover, focus and touch, and cards as they scroll
//   into view while the browser is idle;
// - [data-tip] shows a tooltip and [data-hovercard] a card from a <template>,
//   on hover, keyboard focus or a tap;
// - [data-include] fragments load after the page, replacing a skeleton;
// - <time> shows a relative time in the viewer's clock;
// - the page polls the site's version.json and, when the site was rebuilt,
//   refetches the current page and swaps in what changed;
// - a service worker (sw.js) answers from its cache at once and revalidates.
// Without script every page is still complete and every link still works.
(() => {
    "use strict";

    const PREFETCH_LIMIT = 300;
    const PREFETCH_TTL_MS = 60_000;
    const HOVER_INTENT_MS = 60;
    const TIP_DELAY_MS = 350;
    const PROGRESS_DELAY_MS = 120;
    const VERSION_POLL_MS = 30_000;

    const meta = (name) =>
        document.querySelector(`meta[name="${name}"]`)?.getAttribute("content");
    const rootUrl = () => new URL(meta("cirrus-root") || "./", location.href);
    const reducedMotion = () =>
        matchMedia("(prefers-reduced-motion: reduce)").matches;

    // ---- Service worker -----------------------------------------------------

    if ("serviceWorker" in navigator && location.protocol !== "file:") {
        navigator.serviceWorker
            .register(new URL("sw.js", rootUrl()), {
                scope: rootUrl().pathname,
            })
            .catch(() => {});
        navigator.serviceWorker.addEventListener("message", (event) => {
            const data = event.data || {};
            if (data.type === "cirrus-updated" && sameDocument(data.url)) {
                refresh();
            }
        });
    }

    function sameDocument(url) {
        const a = new URL(url, location.href);
        return a.pathname === location.pathname && a.search === location.search;
    }

    // ---- Fetching and the prefetch cache ----------------------------------

    const pages = new Map(); // url -> {at, promise}

    function key(url) {
        const u = new URL(url, location.href);
        u.hash = "";
        return u.href;
    }

    function fetchPage(url, fresh = false) {
        const k = key(url);
        const hit = pages.get(k);
        if (!fresh && hit && performance.now() - hit.at < PREFETCH_TTL_MS) {
            return hit.promise;
        }
        const promise = fetch(k, {
            credentials: "same-origin",
            cache: fresh ? "no-cache" : "default",
        }).then((r) => {
            if (
                !r.ok ||
                !(r.headers.get("content-type") || "").includes("html")
            ) {
                throw new Error(String(r.status));
            }
            return r.text();
        });
        promise.catch(() => pages.delete(k));
        pages.set(k, { at: performance.now(), promise });
        if (pages.size > PREFETCH_LIMIT) {
            pages.delete(pages.keys().next().value);
        }
        return promise;
    }

    function navigable(a) {
        if (!a || !a.href || a.target || a.hasAttribute("download"))
            return false;
        const u = new URL(a.href, location.href);
        if (u.origin !== location.origin) return false;
        if (!u.pathname.startsWith(rootUrl().pathname)) return false;
        if (/\.(json|png|jpg|svg|css|js|txt)$/.test(u.pathname)) return false;
        return !(u.pathname === location.pathname && u.hash);
    }

    const prefetch = (a) => {
        if (navigable(a)) fetchPage(a.href).catch(() => {});
    };

    let hoverTimer = 0;
    document.addEventListener("pointerover", (e) => {
        const a = e.target.closest?.("a");
        if (!a) return;
        clearTimeout(hoverTimer);
        hoverTimer = setTimeout(() => prefetch(a), HOVER_INTENT_MS);
    });
    document.addEventListener("pointerout", () => clearTimeout(hoverTimer));
    document.addEventListener(
        "touchstart",
        (e) => prefetch(e.target.closest?.("a")),
        { passive: true },
    );
    document.addEventListener("focusin", (e) =>
        prefetch(e.target.closest?.("a")),
    );

    // Cards in view are prefetched while the browser is idle, two at a time.
    const idle = window.requestIdleCallback || ((f) => setTimeout(f, 200));
    const queue = [];
    let running = 0;
    function pump() {
        while (running < 2 && queue.length) {
            const a = queue.shift();
            if (!navigable(a)) continue;
            running++;
            fetchPage(a.href)
                .catch(() => {})
                .finally(() => {
                    running--;
                    idle(pump);
                });
        }
    }
    const watcher =
        "IntersectionObserver" in window && !navigator.connection?.saveData
            ? new IntersectionObserver((entries) => {
                  for (const entry of entries) {
                      if (entry.isIntersecting) {
                          watcher.unobserve(entry.target);
                          queue.push(entry.target);
                      }
                  }
                  idle(pump);
              })
            : null;

    // ---- Navigation ----------------------------------------------------------

    const bar = () => document.querySelector(".c-progress-bar");
    let navigation = 0;

    async function navigate(url, { push = true, scroll = null } = {}) {
        const ticket = ++navigation;
        const progress = setTimeout(() => {
            bar()?.classList.remove("c-done");
            bar()?.classList.add("c-loading");
        }, PROGRESS_DELAY_MS);
        let html;
        try {
            html = await fetchPage(url);
        } catch {
            location.href = url;
            return;
        } finally {
            clearTimeout(progress);
        }
        if (ticket !== navigation) return;
        if (bar()?.classList.contains("c-loading")) {
            bar().classList.replace("c-loading", "c-done");
        }
        const doc = new DOMParser().parseFromString(html, "text/html");
        if (push) {
            history.replaceState({ ...history.state, scroll: scrollY }, "");
            history.pushState({ scroll: 0 }, "", url);
        }
        const apply = () => {
            swap(doc);
            const target = new URL(url, location.href).hash;
            if (scroll !== null) scrollTo(0, scroll);
            else if (target)
                document
                    .querySelector(decodeURIComponent(target))
                    ?.scrollIntoView();
            else scrollTo(0, 0);
        };
        if (document.startViewTransition && !reducedMotion()) {
            document.startViewTransition(apply);
        } else {
            apply();
        }
    }

    function swap(doc, { quiet = false } = {}) {
        document.title = doc.title;
        for (const name of ["cirrus-root"]) {
            const value = doc
                .querySelector(`meta[name="${name}"]`)
                ?.getAttribute("content");
            const mine = document.querySelector(`meta[name="${name}"]`);
            if (value !== undefined && value !== null && mine)
                mine.setAttribute("content", value);
        }
        for (const id of ["c-header", "c-main"]) {
            const next = doc.getElementById(id);
            const current = document.getElementById(id);
            if (!next || !current) continue;
            if (quiet && next.innerHTML === current.innerHTML) continue;
            current.replaceWith(document.adoptNode(next));
        }
        hidePopover();
        enhance(document);
    }

    document.addEventListener("click", (e) => {
        if (e.defaultPrevented || e.button !== 0) return;
        if (e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
        const a = e.target.closest?.("a");
        if (!navigable(a)) return;
        e.preventDefault();
        navigate(a.href);
    });

    addEventListener("popstate", (e) => {
        navigate(location.href, { push: false, scroll: e.state?.scroll ?? 0 });
    });

    if ("scrollRestoration" in history) history.scrollRestoration = "manual";

    // ---- Freshness -----------------------------------------------------------

    // Refetch the current page and swap whatever changed, without a transition or
    // a scroll jump: the page updates in place while the reader reads it.
    async function refresh() {
        pages.clear();
        try {
            const html = await fetchPage(location.href, true);
            const doc = new DOMParser().parseFromString(html, "text/html");
            const y = scrollY;
            swap(doc, { quiet: true });
            scrollTo(0, y);
        } catch {}
    }

    // The version is not written into pages, so that a rebuild leaves unchanged
    // pages byte for byte the same; the first answer is the baseline.
    let polling = 0;
    let seen = null;
    async function checkVersion() {
        if (document.visibilityState !== "visible") return;
        try {
            const r = await fetch(new URL("version.json", rootUrl()), {
                cache: "no-store",
            });
            if (!r.ok) return;
            const { version } = await r.json();
            if (seen !== null && version && version !== seen) await refresh();
            seen = version;
        } catch {}
    }
    function startPolling() {
        clearInterval(polling);
        polling = setInterval(checkVersion, VERSION_POLL_MS);
    }
    document.addEventListener("visibilitychange", () => {
        if (document.visibilityState === "visible") checkVersion();
    });

    // ---- Tooltips and hovercards --------------------------------------------

    let popover = null;
    let popoverOwner = null;
    let tipTimer = 0;

    function hidePopover() {
        clearTimeout(tipTimer);
        if (!popover) return;
        const old = popover;
        old.classList.remove("c-shown");
        setTimeout(() => old.remove(), 200);
        popover = null;
        popoverOwner = null;
    }

    function showPopover(owner) {
        if (popoverOwner === owner) return;
        hidePopover();
        const el = document.createElement("div");
        const cardId = owner.getAttribute("data-hovercard");
        if (cardId) {
            const template = document.getElementById(cardId);
            if (!template) return;
            el.className = "c-popover c-interactive";
            const card = document.createElement("div");
            card.className = "c-hovercard";
            card.append(template.content.cloneNode(true));
            el.append(card);
        } else {
            el.className = "c-popover";
            const tip = document.createElement("div");
            tip.className = "c-tooltip";
            tip.textContent = owner.getAttribute("data-tip");
            el.append(tip);
        }
        el.setAttribute("role", "tooltip");
        document.body.append(el);
        place(el, owner);
        enhance(el);
        requestAnimationFrame(() => el.classList.add("c-shown"));
        popover = el;
        popoverOwner = owner;
    }

    // Beside the owner's first line box, so a link that wraps gets its card under
    // where it starts, not under the whole paragraph's width.
    function place(el, owner) {
        const r = owner.getClientRects()[0] || owner.getBoundingClientRect();
        const w = el.offsetWidth;
        const h = el.offsetHeight;
        const gap = 8;
        let top = r.bottom + gap;
        if (top + h > innerHeight - gap && r.top - gap - h > gap)
            top = r.top - gap - h;
        const left = Math.min(Math.max(gap, r.left), innerWidth - w - gap);
        el.style.top = `${Math.round(top)}px`;
        el.style.left = `${Math.round(left)}px`;
    }

    const owner = (t) => t.closest?.("[data-tip], [data-hovercard]");

    document.addEventListener("pointerover", (e) => {
        if (e.pointerType === "touch") return;
        const o = owner(e.target);
        if (popover && (popover.contains(e.target) || o === popoverOwner)) {
            clearTimeout(tipTimer);
            return;
        }
        clearTimeout(tipTimer);
        if (o) {
            const delay = popover ? 0 : TIP_DELAY_MS;
            tipTimer = setTimeout(() => showPopover(o), delay);
        } else if (popover) {
            tipTimer = setTimeout(hidePopover, 120);
        }
    });
    document.addEventListener("focusin", (e) => {
        const o = owner(e.target);
        if (o && e.target.matches(":focus-visible")) showPopover(o);
    });
    document.addEventListener("focusout", (e) => {
        if (popover && !popover.contains(e.relatedTarget)) hidePopover();
    });
    // A tap on a tooltip's owner toggles it; a link still navigates on its tap.
    document.addEventListener("click", (e) => {
        const o = owner(e.target);
        if (o && !e.target.closest("a")) {
            if (popoverOwner === o) hidePopover();
            else showPopover(o);
        } else if (popover && !popover.contains(e.target)) {
            hidePopover();
        }
    });
    document.addEventListener("keydown", (e) => {
        if (e.key === "Escape") hidePopover();
    });
    addEventListener(
        "scroll",
        () => popover && popoverOwner && place(popover, popoverOwner),
        {
            passive: true,
        },
    );

    // ---- Relative times ------------------------------------------------------

    const rtf = new Intl.RelativeTimeFormat(undefined, {
        numeric: "auto",
        style: "short",
    });
    const full = new Intl.DateTimeFormat(undefined, {
        dateStyle: "medium",
        timeStyle: "short",
    });
    const UNITS = [
        ["year", 31_536_000],
        ["month", 2_592_000],
        ["week", 604_800],
        ["day", 86_400],
        ["hour", 3_600],
        ["minute", 60],
    ];
    function relative(date) {
        const seconds = (date.getTime() - Date.now()) / 1000;
        for (const [unit, size] of UNITS) {
            if (Math.abs(seconds) >= size)
                return rtf.format(Math.round(seconds / size), unit);
        }
        return "just now";
    }
    function times(scope) {
        for (const t of scope.querySelectorAll("time[datetime]")) {
            const date = new Date(t.getAttribute("datetime"));
            if (Number.isNaN(date.getTime())) continue;
            t.textContent = relative(date);
            t.setAttribute("data-tip", full.format(date));
        }
    }
    setInterval(() => times(document), 60_000);

    // ---- Fragments -----------------------------------------------------------

    function includes(scope) {
        for (const el of scope.querySelectorAll("[data-include]")) {
            const url = new URL(el.getAttribute("data-include"), location.href);
            el.removeAttribute("data-include");
            fetch(url, { credentials: "same-origin" })
                .then((r) => (r.ok ? r.text() : Promise.reject(r.status)))
                .then((html) => {
                    el.innerHTML = html;
                })
                .catch(() => {
                    const p = document.createElement("p");
                    p.className = "c-empty";
                    p.textContent = el.getAttribute("data-missing") || "";
                    el.replaceChildren(p);
                })
                .finally(() => {
                    el.classList.add("c-reveal");
                    enhance(el);
                });
        }
    }

    // ---- Wiring --------------------------------------------------------------

    function enhance(scope) {
        times(scope);
        includes(scope);
        if (watcher) {
            for (const a of scope.querySelectorAll("a[data-prefetch]"))
                watcher.observe(a);
        }
    }

    enhance(document);
    if (location.hash) {
        document
            .querySelector(decodeURIComponent(location.hash))
            ?.scrollIntoView();
    }
    checkVersion();
    startPolling();
})();
