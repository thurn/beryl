self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", event => {
    event.waitUntil((async () => {
        for (const key of await caches.keys()) {
            if (key.startsWith("cirrus-")) await caches.delete(key);
        }
        await self.registration.unregister();
        for (const client of await self.clients.matchAll({type: "window"})) {
            const url = new URL(client.url);
            const ticket = url.pathname.match(/\/beads\/([^/]+?)(?:\.html)?$/)?.[1];
            const target = ticket
                ? "https://jarvis.tail4b0774.ts.net:8443/beryl/beads/" + encodeURIComponent(ticket)
                : "https://jarvis.tail4b0774.ts.net:8443/beryl/";
            await client.navigate(target + url.search + url.hash);
        }
    })());
});
