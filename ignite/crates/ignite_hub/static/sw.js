// Cirrus's service worker: every page answers from the cache at once and is
// revalidated in the background ("stale while revalidate"). When the network
// copy differs from what was shown, the open pages are told, and cirrus.js
// swaps in the new content. Content-hashed assets (name.<10 hex>.css|js) never
// change, so they are served from the cache without asking the network.

const CACHE = "cirrus-pages-v1";
const ASSETS = "cirrus-assets-v1";
const HASHED = /\.[0-9a-f]{10}\.(css|js)$/;

self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", (event) =>
    event.waitUntil(self.clients.claim()),
);

self.addEventListener("fetch", (event) => {
    const request = event.request;
    if (request.method !== "GET") return;
    const url = new URL(request.url);
    if (url.origin !== self.location.origin) return;
    if (HASHED.test(url.pathname)) {
        event.respondWith(fromAssets(request));
        return;
    }
    // The project scope includes API URLs. Keep JSON and API conditional
    // requests out of the page cache.
    const root = new URL(self.registration.scope).pathname;
    if (!url.pathname.startsWith(root)) return;
    const path = url.pathname.slice(root.length);
    if (url.searchParams.has("fragment")) return;
    // Let old ticket URLs reach the Worker so they redirect to the canonical ID.
    if (path === "" || /^tickets\/T[^/]+$/.test(path)) {
        event.respondWith(staleWhileRevalidate(event, request));
    }
});

async function fromAssets(request) {
    const cache = await caches.open(ASSETS);
    const hit = await cache.match(request);
    if (hit) return hit;
    const response = await fetch(request);
    if (response.ok) cache.put(request, response.clone());
    return response;
}

function stamp(response) {
    return (
        response.headers.get("etag") ||
        response.headers.get("last-modified") ||
        ""
    );
}

async function staleWhileRevalidate(event, request) {
    const cache = await caches.open(CACHE);
    const key = new Request(request.url, { credentials: "same-origin" });
    const cached = await cache.match(key);
    const tag = cached?.headers.get("etag");
    const network = fetch(request.url, {
        cache: "no-store",
        credentials: "same-origin",
        redirect: request.redirect,
        headers: tag ? { "If-None-Match": tag } : {},
    }).then(async (response) => {
        if (response.status === 304 && cached) return cached;
        if (response.ok && !response.redirected) {
            const changed = cached && stamp(cached) !== stamp(response);
            await cache.put(key, response.clone());
            if (changed) notify(request.url);
        } else if (response.status === 404) {
            await cache.delete(key);
        }
        return response;
    });
    if (cached) {
        event.waitUntil(network.catch(() => {}));
        return cached;
    }
    return network;
}

async function notify(url) {
    for (const client of await self.clients.matchAll({ type: "window" })) {
        client.postMessage({ type: "cirrus-updated", url });
    }
}
