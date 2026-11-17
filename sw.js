self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", event => {
    event.waitUntil((async () => {
        for (const key of await caches.keys()) {
            if (key.startsWith("cirrus-")) await caches.delete(key);
        }
        await self.registration.unregister();
        for (const client of await self.clients.matchAll({type: "window"})) {
            const path = new URL(client.url).pathname;
            const ticket = path.match(/\/beads\/([^/]+?)(?:\.html)?$/)?.[1];
            const target = ticket
                ? "https://tickets.thurn.workers.dev/by/beads/" + encodeURIComponent(ticket)
                : "https://tickets.thurn.workers.dev/by/";
            await client.navigate(target);
        }
    })());
});
