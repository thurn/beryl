// The page's card records, with no browser (`npm test`): what a lookup finds in the store's
// answers, when a view waits for its cards' records, and that the UI bundles no card data.
import assert from "node:assert/strict";
import { readdir, readFile } from "node:fs/promises";
import { dirname, join, relative, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { CardRecordCache, strings } from "../src/cardRecords.ts";

const face = (name) => ({ name, type_line: "", colors: [], images: {} });
const card = (name, ...faces) => ({
    name,
    faces: (faces.length > 0 ? faces : [name]).map(face),
});
const records = (cards = {}, tokens = {}) => ({
    card_back: "back",
    nameless_token: "copy",
    cards,
    tokens,
});

/** A server whose answers the test gives when it chooses: `asked` is each request's names. */
function server() {
    const asked = [];
    const answers = [];
    return {
        asked,
        ask(names) {
            asked.push(names);
            return new Promise((resolve, reject) =>
                answers.push({ resolve, reject }),
            );
        },
        answer: (nth, cards = {}) => answers[nth].resolve(records(cards)),
        fail: (nth) => answers[nth].reject(new Error("unreachable")),
    };
}

/** Every promise already settled has run what waits on it. */
const settled = () => new Promise((done) => setImmediate(done));

test("a name finds its card, its face or its token", () => {
    const cache = new CardRecordCache();
    const springs = card("Stupor // Springs", "Stupor", "Springs");
    const offspring = card("Trainer");
    cache.keep(
        ["Springs", "Trainer", "Spirit", "Nothing"],
        records(
            { Springs: springs, Trainer: card("Trainer") },
            { Trainer: offspring, Spirit: card("Spirit") },
        ),
    );
    assert.deepEqual(cache.find("Springs"), { card: springs, face: 1 });
    // A card wins over the token of its name, which is still the token's own face.
    assert.notEqual(cache.find("Trainer").card, offspring);
    assert.equal(cache.token("Trainer"), offspring);
    assert.equal(cache.find("Spirit").card.name, "Spirit");
    assert.equal(cache.token("Springs"), undefined);
    assert.equal(cache.find("Nothing"), undefined);
    assert.equal(cache.cardBack(), "back");
    // Which image a face asks for (src/game/faces.ts useFace): a token with no name, a copy
    // of a face-down permanent, the store's art for one, the Double Masters Copy token's
    // (beryl_session's card_records tests hold which that is); anything else, its name's.
    // The old board's check of it opened a page (by-w8gv.21.65).
    const token = { token: true, name: "" };
    assert.equal(cache.namelessArt(token), "copy");
    assert.equal(cache.namelessArt({ token: true, name: "Spirit" }), undefined);
    assert.equal(cache.namelessArt({ token: false, name: "" }), undefined);
    assert.equal(cache.namelessArt({ token: false, name: null }), undefined);
    assert.equal(new CardRecordCache().namelessArt(token), undefined);
});

test("every string a view carries is asked about, wherever it is", () => {
    const view = {
        objects: [{ name: "Island", slot: 1 }, { name: null }],
        stack: [{ kind: "ability", name: "Tundra" }],
        players: { you: { emblems: ["Teferi Emblem"] } },
    };
    assert.deepEqual(
        [...strings(view)].sort(),
        ["Island", "Teferi Emblem", "Tundra", "ability"].sort(),
    );
});

test("a view whose records are cached is delivered in the same tick", () => {
    const cache = new CardRecordCache();
    cache.keep(["Island"], records({ Island: card("Island") }));
    const ask = () => assert.fail("nothing is missing");
    let delivered = false;
    cache.whenCached({ name: "Island" }, ask, () => (delivered = true));
    assert.ok(delivered);
});

test("records in hand are kept without a tick passing", () => {
    const cache = new CardRecordCache();
    let found;
    cache.whenCached(
        { name: "Island" },
        () => records({ Island: card("Island") }),
        () => (found = cache.find("Island")),
    );
    assert.equal(found?.card.name, "Island");
});

test("views wait for their records, and arrive in order", async () => {
    const cache = new CardRecordCache();
    const { ask, asked, answer } = server();
    const delivered = [];
    const deliver = (n) => () =>
        delivered.push([n, cache.find("Island")?.card.name]);
    cache.whenCached({ name: "Island" }, ask, deliver(1));
    // Nothing of the second is missing, and it still follows the first.
    cache.whenCached({}, ask, deliver(2));
    cache.whenCached({ name: "Tundra" }, ask, deliver(3));
    await settled();
    assert.deepEqual(delivered, []);
    // The later request is answered first.
    answer(1);
    await settled();
    assert.deepEqual(delivered, []);
    answer(0, { Island: card("Island") });
    await settled();
    assert.deepEqual(delivered, [
        [1, "Island"],
        [2, "Island"],
        [3, "Island"],
    ]);
    assert.deepEqual(asked, [["Island"], ["Tundra"]]);
    // Nothing is held back any longer: the next is in its own tick again.
    cache.whenCached({ name: "Island" }, ask, deliver(4));
    assert.equal(delivered.length, 4);
});

test("a name is asked about once, by one request an update", async () => {
    const cache = new CardRecordCache();
    const { ask, asked, answer } = server();
    let delivered = 0;
    cache.whenCached(["Island", "Tundra"], ask, () => (delivered += 1));
    // Island is on its way already.
    cache.whenCached(["Island", "Plains", "Karakas"], ask, () => {
        delivered += 1;
    });
    assert.deepEqual(asked, [
        ["Island", "Tundra"],
        ["Plains", "Karakas"],
    ]);
    answer(1);
    answer(0);
    await settled();
    assert.equal(delivered, 2);
    // Nor is a name the store had no record for asked about again.
    cache.whenCached(["Tundra", "Karakas"], ask, () => (delivered += 1));
    assert.equal(delivered, 3);
    assert.equal(asked.length, 2);
});

test("a view is delivered though its request fails, and asks again", async () => {
    const cache = new CardRecordCache();
    const { ask, asked, fail } = server();
    let delivered = 0;
    cache.whenCached(["Island"], ask, () => (delivered += 1));
    fail(0);
    await settled();
    assert.equal(delivered, 1);
    cache.whenCached(["Island"], ask, () => (delivered += 1));
    assert.deepEqual(asked, [["Island"], ["Island"]]);
});

// The card store has a record for every card there is: bundled, it would be most of the
// page.
// The design language's data is the one file of data/ the UI is built with.
test("UI data imports are limited to UI conventions and table commands", async () => {
    const ui = resolve(dirname(fileURLToPath(import.meta.url)), "..");
    const data = resolve(ui, "..", "data");
    const allowed = new Set([
        join(data, "ui-conventions.json"),
        join(data, "table-commands.json"),
    ]);
    const imported =
        /(?:\bfrom|\bimport)\s*\(?\s*["']([^"']+)["']|\bnew URL\(\s*["']([^"']+)["']/g;
    const found = [];
    for (const entry of await readdir(join(ui, "src"), {
        recursive: true,
        withFileTypes: true,
    })) {
        if (!entry.isFile() || !/\.(ts|tsx|css)$/.test(entry.name)) continue;
        const file = join(entry.parentPath, entry.name);
        const text = await readFile(file, "utf8");
        for (const [, module, url] of text.matchAll(imported)) {
            const target = resolve(dirname(file), module ?? url);
            if (
                !relative(data, target).startsWith("..") &&
                !allowed.has(target)
            ) {
                found.push(`${relative(ui, file)} imports ${module ?? url}`);
            }
        }
    }
    assert.deepEqual(found, []);
});
