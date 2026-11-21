// Cirrus's one script. It makes a server-rendered site feel instant:
// - navigation fetches the next page (usually already prefetched), swaps the
//   header and <main> inside a view transition, and keeps scroll positions;
// - links are prefetched on hover, focus and touch, and cards as they scroll
//   into view while the browser is idle;
// - [data-tip] shows a tooltip, [data-hovercard] a card from a <template> and
//   [data-card] a data card built from JSON, on hover, keyboard focus or a tap;
// - [data-include] fragments load after the page, replacing a skeleton;
// - <time> shows a relative time in the viewer's clock;
// - a service worker (sw.js) answers from its cache at once and revalidates.
// Without script every page is still complete and every link still works.
(() => {
    "use strict";

    const PREFETCH_LIMIT = 300;
    const PREFETCH_TTL_MS = 60_000;
    const HOVER_INTENT_MS = 60;
    const TIP_DELAY_MS = 350;
    const PROGRESS_DELAY_MS = 120;
    const RECONNECT_MIN_MS = 250;
    const RECONNECT_MAX_MS = 30_000;
    const FEED_WAIT_S = 25;

    const meta = (name) =>
        document.querySelector(`meta[name="${name}"]`)?.getAttribute("content");
    const rootUrl = () => new URL(meta("cirrus-root") || "./", location.href);
    const reducedMotion = () =>
        matchMedia("(prefers-reduced-motion: reduce)").matches;

    // ---- Service worker -----------------------------------------------------

    if ("serviceWorker" in navigator && location.protocol !== "file:") {
        navigator.serviceWorker
            .register(new URL("/sw.js", location.origin), {
                scope: rootUrl().pathname,
            })
            .catch(() => {});
        navigator.serviceWorker.addEventListener("message", (event) => {
            const data = event.data || {};
            if (data.type !== "cirrus-updated") return;
            pages.delete(key(data.url));
            if (sameDocument(data.url)) freshFromCache(data.url);
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
            cache: fresh ? "no-store" : "default",
        }).then((r) => {
            if (
                !r.ok ||
                !(r.headers.get("content-type") || "").includes("html")
            ) {
                throw new Error(String(r.status));
            }
            return r.text();
        });
        promise.catch(() => {
            if (pages.get(k)?.promise === promise) pages.delete(k);
        });
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
    const skipInitialPrefetch = new WeakSet();
    let running = 0;
    function pump() {
        while (running < 2 && queue.length) {
            const a = queue.shift();
            if (!a.isConnected || !navigable(a)) continue;
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
                      // A live replacement must not re-prefetch cards already
                      // in view. Keep watching them for a later scroll.
                      if (skipInitialPrefetch.delete(entry.target)) continue;
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
    let navigating = false;

    // The home page's last scroll position. A link back to the home page (the
    // wordmark or a breadcrumb) returns to it, as the Back button does, instead
    // of opening the page at its top. It is kept in sessionStorage, so it also
    // outlives a full page load.
    const HOME_SCROLL = "cirrus-home-scroll";
    const isHome = (url) => new URL(url, location.href).pathname === rootUrl().pathname;

    function rememberHomeScroll() {
        if (!isHome(location.href)) return;
        try {
            sessionStorage.setItem(HOME_SCROLL, String(scrollY));
        } catch {}
    }

    function homeScroll() {
        try {
            const value = Number(sessionStorage.getItem(HOME_SCROLL));
            return Number.isFinite(value) ? value : 0;
        } catch {
            return 0;
        }
    }

    addEventListener("pagehide", rememberHomeScroll);

    async function navigate(url, { push = true, scroll = null } = {}) {
        if (push) rememberHomeScroll();
        if (push && scroll === null && isHome(url) && !new URL(url, location.href).hash)
            scroll = homeScroll();
        const ticket = ++navigation;
        navigating = true;
        const progress = setTimeout(() => {
            bar()?.classList.remove("c-done");
            bar()?.classList.add("c-loading");
        }, PROGRESS_DELAY_MS);
        let doc;
        const parse = (html) => new DOMParser().parseFromString(html, "text/html");
        try {
            doc = parse(await fetchPage(url));
            if (ticket !== navigation) return;
            // The cache can hold a board from before a closure even when the open
            // detail page already shows Closed. Check the server before showing it.
            const seq = stamp(doc);
            if (seq >= 0 && seq < Math.max(stamp(document), since)) {
                try {
                    doc = parse(await fetchPage(url, true));
                } catch {} // Keep cached navigation available offline.
            }
        } catch {
            if (ticket === navigation) location.href = url;
            return;
        } finally {
            clearTimeout(progress);
        }
        if (ticket !== navigation) return;
        if (bar()?.classList.contains("c-loading")) {
            bar().classList.replace("c-loading", "c-done");
        }
        if (push) {
            history.replaceState({ ...history.state, scroll: scrollY }, "");
            history.pushState({ scroll: 0 }, "", url);
        }
        const apply = () => {
            if (ticket !== navigation) return;
            swap(doc);
            const target = new URL(url, location.href).hash;
            if (scroll !== null) scrollTo(0, scroll);
            else if (target)
                document
                    .querySelector(decodeURIComponent(target))
                    ?.scrollIntoView();
            else scrollTo(0, 0);
            navigating = false;
            if (Number(main()?.dataset.feedSeq) < since) freshFromCache(location.href);
            flushChanges();
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
            if (id === "c-main") {
                watcher?.disconnect();
                if (quiet) preserveControls(current, next);
            }
            current.replaceWith(document.adoptNode(next));
        }
        hidePopover();
        enhance(document, quiet);
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

    // ---- Live updates -------------------------------------------------------

    const main = () => document.getElementById("c-main");
    const stamp = (page) => Number(page.getElementById("c-main")?.dataset.feedSeq ?? -1);
    const home = () => location.pathname === rootUrl().pathname;
    let since = Number(main()?.dataset.feedSeq ?? 0);
    let polling;
    let following = false;
    let retryTimer;
    let retryDelay = RECONNECT_MIN_MS;
    let suspended = false;
    let refreshing = false;
    let pendingTickets = new Set();
    let pendingStatus = false;
    let resync = false;

    // An open editor (edit.js) holds back live updates until it closes and sends
    // `cirrus:flush`, so a change elsewhere does not replace what is being typed.
    const editing = () => document.querySelector(".c-editing") !== null;
    document.addEventListener("cirrus:flush", (e) => {
        if (e.detail?.resync) resync = true;
        flushChanges();
    });

    function preserveControls(current, next) {
        const status = current.querySelector("#project-status");
        // A board update can carry a new status fragment. Keep the mounted
        // panel until that fragment is ready, including its first load.
        const placeholder = next.querySelector("#project-status");
        if (status && placeholder) placeholder.replaceWith(status);
        const disclosures = (scope) => [...scope.querySelectorAll("details")];
        const disclosureKey = (el) => (el.closest("section[id]")?.id ?? "")
            + "/" + el.querySelector("summary")?.textContent;
        const opened = new Set(disclosures(current).filter((el) => el.open).map(disclosureKey));
        for (const el of disclosures(next)) {
            if (opened.has(disclosureKey(el))) el.open = true;
        }
    }

    function swapInPlace(doc) {
        const top = scrollY;
        swap(doc, { quiet: true });
        scrollTo({ top, behavior: "instant" });
    }

    async function freshFromCache(url) {
        // Navigation's background revalidation has already fetched this page.
        // Read its result without making a second request.
        if (!sameDocument(url) || navigating || editing()) return;
        try {
            const response = await caches.match(key(url));
            if (!response) return;
            const doc = new DOMParser().parseFromString(await response.text(), "text/html");
            if (!sameDocument(url) || navigating) return;
            if (stamp(doc) < stamp(document)) return;
            const assets = (page) => [...page.querySelectorAll("link[rel=stylesheet], script[src]")]
                .map((el) => el.getAttribute("href") ?? el.getAttribute("src")).join(" ");
            if (assets(doc) !== assets(document)) {
                history.replaceState({ ...history.state, scroll: scrollY }, "");
                location.reload();
                return;
            }
            if (stamp(doc) > stamp(document)) {
                pages.delete(key(url));
                swapInPlace(doc);
            }
        } catch {}
    }

    function watching(id) {
        const dependencies = main()?.dataset.feedTickets?.split(" ") ?? [];
        return dependencies.some((ticket) => id === ticket || id.startsWith(ticket + "."));
    }

    async function flushChanges() {
        if (refreshing || navigating || suspended || editing()) return;
        const tickets = resync || (home() ? pendingTickets.size > 0 : [...pendingTickets].some(watching));
        const status = home() && pendingStatus;
        if (!tickets && !status) {
            pendingTickets.clear();
            pendingStatus = false;
            return;
        }
        pendingTickets.clear();
        pendingStatus = false;
        const catchingUp = resync;
        const cursor = since;
        resync = false;
        refreshing = true;
        const at = navigation;
        const url = new URL(location.href);
        url.searchParams.set("fragment", tickets ? "all" : "status");
        try {
            const response = await fetch(url, { cache: "no-store" });
            if (!response.ok) throw new Error(String(response.status));
            const doc = new DOMParser().parseFromString(await response.text(), "text/html");
            if (at !== navigation || suspended) {
                resync = true;
                return;
            }
            if (catchingUp) {
                const seq = Number(response.headers.get("X-Cirrus-Sequence"));
                since = since === cursor ? seq : Math.max(since, seq);
                pages.clear();
            }
            if (tickets && (catchingUp || stamp(doc) >= stamp(document))) swapInPlace(doc);
            const panel = document.querySelector("#project-status .c-include");
            const content = tickets ? doc.getElementById("c-status-fragment")?.content : doc.body;
            if (panel && content) {
                panel.removeAttribute("data-include");
                panel.removeAttribute("data-include-loading");
                panel.replaceChildren(...content.childNodes);
                enhance(panel);
            }
        } catch {
            // A failed read leaves the displayed page in place. Catch up with
            // one fragment request once the feed is reachable again.
            resync = true;
            return;
        } finally {
            refreshing = false;
            if (at !== navigation && !navigating) flushChanges();
        }
        flushChanges();
    }

    // The hub's feed is a long poll: `api/feed?since=<seq>&wait=<s>` answers as soon as anything
    // newer than `since` exists, or empty when the wait ends, and the next request follows at once.
    function handleBatch(batch) {
        if (batch.changes.some((change) => change.kind === "resync")) {
            since = batch.seq;
            pages.clear();
            resync = true;
        } else if (batch.seq > since) {
            since = batch.seq;
            const tickets = batch.changes.filter((change) => change.kind === "ticket" && change.ticket);
            if (tickets.length) pages.clear();
            for (const change of tickets) pendingTickets.add(change.ticket);
            pendingStatus ||= batch.changes.some((change) => change.kind !== "ticket");
        }
        flushChanges();
    }

    async function pollFeed() {
        while (!suspended && main()?.hasAttribute("data-feed-seq")) {
            const controller = polling = new AbortController();
            const url = new URL("api/feed", rootUrl());
            url.searchParams.set("since", String(since));
            url.searchParams.set("wait", String(FEED_WAIT_S));
            let batch;
            const asked = performance.now();
            try {
                const response = await fetch(url, { cache: "no-store", signal: controller.signal });
                if (!response.ok) throw new Error(String(response.status));
                batch = await response.json();
            } catch {
                if (polling === controller) polling = undefined;
                if (suspended || controller.signal.aborted) return;
                resync = true;
                await new Promise((resolve) => { retryTimer = setTimeout(resolve, retryDelay); });
                retryTimer = undefined;
                retryDelay = Math.min(RECONNECT_MAX_MS, retryDelay * 2);
                continue;
            }
            if (polling === controller) polling = undefined;
            // The hub answers at once, empty, when it will not hold the request open: on the
            // public listener without a token, or when too many requests are already waiting.
            // Asking again at once would send hundreds of requests a second, so an empty reply
            // that came back well before the wait ended is followed by the same growing pause
            // as a failure.
            const early = performance.now() - asked < FEED_WAIT_S * 500;
            if (batch.changes.length === 0 && early) {
                await new Promise((resolve) => { retryTimer = setTimeout(resolve, retryDelay); });
                retryTimer = undefined;
                retryDelay = Math.min(RECONNECT_MAX_MS, retryDelay * 2);
                if (suspended) return;
                continue;
            }
            retryDelay = RECONNECT_MIN_MS;
            handleBatch(batch);
        }
    }

    function connectFeed() {
        if (!main()?.hasAttribute("data-feed-seq") || following || suspended) return;
        following = true;
        pollFeed().finally(() => { following = false; });
    }

    addEventListener("pagehide", () => {
        suspended = true;
        clearTimeout(retryTimer);
        retryTimer = undefined;
        polling?.abort();
        polling = undefined;
    });
    addEventListener("pageshow", () => {
        suspended = false;
        connectFeed();
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
        if (owner.hasAttribute("data-card")) {
            const card = window.cirrusDataCard?.(owner.getAttribute("data-card"));
            if (!card) return;
            el.className = "c-popover c-interactive";
            el.append(card);
        } else if (cardId) {
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
    // An owner marked [data-hover-side], such as a segment of a tall strip, gets its card
    // beside it, centred on it, so the card never covers the strip.
    function place(el, owner) {
        const r = owner.getClientRects()[0] || owner.getBoundingClientRect();
        const w = el.offsetWidth;
        const h = el.offsetHeight;
        const gap = 8;
        if (owner.hasAttribute("data-hover-side")) {
            let left = r.right + gap;
            if (left + w > innerWidth - gap) left = r.left - gap - w;
            const middle = Math.max(r.top, 0) / 2 + Math.min(r.bottom, innerHeight) / 2;
            const top = Math.min(Math.max(gap, middle - h / 2), innerHeight - h - gap);
            el.style.top = `${Math.round(top)}px`;
            el.style.left = `${Math.round(Math.max(gap, left))}px`;
            return;
        }
        let top = r.bottom + gap;
        if (top + h > innerHeight - gap && r.top - gap - h > gap)
            top = r.top - gap - h;
        const left = Math.min(Math.max(gap, r.left), innerWidth - w - gap);
        el.style.top = `${Math.round(top)}px`;
        el.style.left = `${Math.round(left)}px`;
    }

    const owner = (t) =>
        t.closest?.("[data-tip], [data-hovercard], [data-card]");

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
            el.setAttribute("data-include-loading", "");
            fetch(url, { credentials: "same-origin" })
                .then((r) => (r.ok ? r.text() : Promise.reject(r.status)))
                .then((html) => {
                    if (!el.hasAttribute("data-include-loading")) return;
                    el.innerHTML = html;
                })
                .catch(() => {
                    if (!el.hasAttribute("data-include-loading")) return;
                    const p = document.createElement("p");
                    p.className = "c-empty";
                    p.textContent = el.getAttribute("data-missing") || "";
                    el.replaceChildren(p);
                })
                .finally(() => {
                    if (!el.hasAttribute("data-include-loading")) return;
                    el.removeAttribute("data-include-loading");
                    el.classList.add("c-reveal");
                    enhance(el);
                });
        }
    }

    // ---- Table of contents -------------------------------------------------
    //
    // The entry for the part of the page in view carries aria-current, and the
    // top-level entry above it data-within; both follow the reader's scrolling.
    // A click scrolls smoothly to its section and marks it at once.

    // How far below the viewport's top a section's heading counts as in view:
    // the sticky header (and, on a narrow screen, the contents row) plus slack.
    const wide = matchMedia("(min-width: 960px)");
    const tocLine = () =>
        (document
            .querySelector(wide.matches ? ".c-header" : ".c-toc-rail")
            ?.getBoundingClientRect().bottom ?? 0) + 24;
    let tocFrame = 0;
    // Until when a click's mark stays put while its scroll runs, in ms; the
    // scroll's scrollend releases it sooner.
    let tocHeld = 0;

    function tocEntries() {
        return [...document.querySelectorAll(".c-toc-link")]
            .map((a) => ({
                a,
                target: document.getElementById(
                    decodeURIComponent(a.hash.slice(1))
                ),
            }))
            .filter((e) => e.target);
    }

    function markToc(current) {
        const links = [...document.querySelectorAll(".c-toc-link")];
        // The top-level entry at or above the current one.
        const top = links
            .slice(0, links.indexOf(current) + 1)
            .reverse()
            .find((x) => x.closest(".c-toc-depth-0"));
        for (const a of links) {
            if (a === current) a.setAttribute("aria-current", "location");
            else a.removeAttribute("aria-current");
            if (a === top && a !== current) a.dataset.within = "";
            else delete a.dataset.within;
        }
        // On a narrow screen the contents scroll sideways: keep the mark in view.
        const row = current?.closest("ul");
        if (row && row.scrollWidth > row.clientWidth) {
            const left =
                current.offsetLeft - (row.clientWidth - current.offsetWidth) / 2;
            row.scrollTo({ left, behavior: reducedMotion() ? "auto" : "smooth" });
        }
    }

    function spyToc() {
        tocFrame = 0;
        if (performance.now() < tocHeld) return;
        const entries = tocEntries();
        if (!entries.length) return;
        const line = tocLine();
        let current = entries[0].a;
        for (const { a, target } of entries) {
            if (target.getBoundingClientRect().top <= line) current = a;
        }
        const end =
            innerHeight + scrollY >= document.documentElement.scrollHeight - 2;
        if (end) current = entries[entries.length - 1].a;
        if (current.getAttribute("aria-current") === null) markToc(current);
    }

    addEventListener(
        "scroll",
        () => {
            if (!tocFrame) tocFrame = requestAnimationFrame(spyToc);
        },
        { passive: true }
    );
    // The scroll a click started has ended: from here the reader's scrolling
    // moves the mark again.
    addEventListener("scrollend", () => {
        tocHeld = 0;
    });

    document.addEventListener("click", (e) => {
        if (e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey) return;
        const a = e.target.closest?.(".c-toc-link");
        const target =
            a && document.getElementById(decodeURIComponent(a.hash.slice(1)));
        if (!target) return;
        e.preventDefault();
        history.replaceState(history.state, "", a.hash);
        markToc(a);
        // A short way glides; a long way jumps, since a glide over several
        // screens of cards only delays the reader.
        const far =
            Math.abs(target.getBoundingClientRect().top) > 2 * innerHeight;
        const glide = !far && !reducedMotion();
        tocHeld = performance.now() + 3000;
        target.scrollIntoView({ behavior: glide ? "smooth" : "auto" });
    });

    // ---- Wiring --------------------------------------------------------------

    function enhance(scope, quiet = false) {
        times(scope);
        includes(scope);
        requestAnimationFrame(spyToc);
        if (watcher) {
            for (const a of scope.querySelectorAll("a[data-prefetch]")) {
                if (quiet) skipInitialPrefetch.add(a);
                watcher.observe(a);
            }
        }
    }

    enhance(document);
    connectFeed();
    if (history.state?.scroll) {
        scrollTo(0, history.state.scroll); // a reload for new assets, or a return
    } else if (location.hash) {
        document
            .querySelector(decodeURIComponent(location.hash))
            ?.scrollIntoView();
    }
})();
