// The game screen read against the view it shows, for the checks that open one (shots.mjs,
// specs.mjs, playthrough.mjs, by way of capture.mjs).

/** Where the game screen disagrees with the view the fixture last released
 * (`window.berylFixture.view()`, src/ipc.ts), or on a live page the view its stage says it
 * shows (`window.berylStage.shown.view`), once it has settled: each permanent, spell and
 * card in the viewer's hand drawn exactly once, in its zone, under its name; a permanent's
 * power and toughness (less marked damage), loyalty, counters and orientation; both life
 * totals; each item of the desktop's stack marked as its controller's, with the X it was
 * announced with and the costs a spell was cast with (a phone's stack is its cards' scans
 * alone) and clear of every card of your hand; a player's piles on the stage ending on one
 * line and lying alike; the ways to use a card each drawing its card at one top and size
 * and its label from one height; and no card drawn that the view does not have.
 * Each is described for a failure message. Nothing on a page with neither. */
export async function screenFaults(page) {
    return page.evaluate(() => {
        const view =
            window.berylFixture?.view?.() ?? window.berylStage?.shown.view;
        // A screen other than the game's, as the menu, draws no table to check.
        if (!view || !document.querySelector("[data-life]")) return [];
        const phone = document.querySelector(".phone-board") !== null;
        const faults = [];
        const drawn = new Map();
        for (const e of document.querySelectorAll(".card-el[data-slot]")) {
            const slot = Number(e.dataset.slot);
            drawn.set(slot, [...(drawn.get(slot) ?? []), e]);
        }
        const objects = new Map(view.objects.map((o) => [o.slot, o]));
        const text = (e) => e?.textContent?.replace(/\s+/g, "") ?? "nothing";
        for (const o of view.objects) {
            const shown =
                o.zone === "battlefield" ||
                o.zone === "stack" ||
                (o.zone === "hand" && o.owner === "you");
            if (!shown) continue;
            const what = `${o.name ?? "a hidden card"} (slot ${o.slot}, ${o.zone})`;
            const els = drawn.get(o.slot) ?? [];
            // A resolving spell may be drawn beside the choice it asks rather than on the table:
            // as a choice overlay's source card, known by its name.
            const beside =
                o.zone === "stack" &&
                els.length === 0 &&
                (document.querySelector(`[data-slot="${o.slot}"]`) ||
                    [...document.querySelectorAll(".shown-card")].some(
                        (e) => o.name && e.textContent?.includes(o.name),
                    ));
            if (beside) continue;
            if (els.length !== 1) {
                faults.push(`${what} drawn ${els.length} times`);
                continue;
            }
            const e = els[0];
            if (e.dataset.zone !== o.zone) {
                faults.push(`${what} drawn in the ${e.dataset.zone}`);
            }
            if (o.name && e.dataset.card !== o.name) {
                faults.push(`${what} drawn as ${e.dataset.card ?? "no card"}`);
            }
            if (o.zone !== "battlefield") continue;
            const stats = text(e.querySelector(".card-pt, .card-loyalty b"));
            const loyalty = o.counters.find((c) => c.kind === "loyalty");
            // A phone draws a face-up creature's scan alone: its printed P/T stands.
            const printed = phone && o.facing === "up";
            const want =
                o.pt && !printed
                    ? `${o.pt.power}/${Math.max(0, o.pt.toughness - o.damage)}`
                    : loyalty && String(loyalty.count);
            if (want !== undefined && stats !== want) {
                faults.push(`${what} shows ${stats} for ${want}`);
            }
            for (const c of o.counters) {
                if (c.kind === "loyalty") continue;
                // +1/+1 and -1/-1 counters show as one pill naming the kind, with a count
                // past one.
                const pill = e.querySelector(".card-pt-counter");
                const many = pill?.querySelector("b");
                const badge =
                    c.kind === "plusOne" || c.kind === "minusOne"
                        ? pill
                            ? many
                                ? text(many).slice(1)
                                : "1"
                            : undefined
                        : text(e.querySelector(`.mark-${c.kind} b`));
                if (badge !== String(c.count)) {
                    faults.push(
                        `${what} shows ${badge} ${c.kind} counters for ${c.count}`,
                    );
                }
            }
            // A tapped card lies turned a quarter, wider than tall.
            const box = e.getBoundingClientRect();
            const tapped = o.tapped;
            if (box.width > box.height !== tapped) {
                faults.push(`${what} drawn ${tapped ? "untapped" : "tapped"}`);
            }
        }
        // Each emblem is drawn as a card, under a slot no object has (layout's emblemSlot);
        // the phone draws no emblem yet (UI Conventions, "The phone").
        for (const side of phone ? [] : ["you", "opponent"]) {
            for (const name of view.players[side].emblems) {
                const card = [...drawn].some(
                    ([slot, els]) =>
                        slot < 0 && els.some((e) => e.dataset.card === name),
                );
                if (!card) faults.push(`${side}'s emblem ${name} not drawn`);
            }
        }
        // What a stack item's frame cannot say stands on its edges (components/Stack.tsx
        // StackMarks): a spell's on its card, and the abilities' taken together, since a view
        // does not say which node is which ability's.
        const edges = (e) => ({
            controller: [...e.querySelectorAll("[data-controller]")].map(
                (mark) => mark.dataset.controller,
            ),
            x: [...e.querySelectorAll(".stack-x")].map((x) => text(x)),
            paid: [...e.querySelectorAll(".stack-paid")].map(
                (paid) => paid.dataset.paid,
            ),
        });
        const marked = (controller, x, paid = []) => ({
            controller: [controller],
            x: x > 0 ? [`X=${x}`] : [],
            paid: paid.length ? [paid.join(" ")] : [],
        });
        const said = (marks) => JSON.stringify(marks);
        const abilities = [];
        // On top is what asks the viewer something as it resolves, which a view leaves out
        // of its list (src/stage/table.ts `tabled`).
        const resolving = view.status.decision?.resolving;
        const listed = resolving ? [...view.stack, resolving] : view.stack;
        for (const item of phone ? [] : listed) {
            if (item.kind !== "spell") {
                abilities.push(said(marked(item.controller, item.x)));
                continue;
            }
            const spell = objects.get(item.slot);
            const [e] = drawn.get(item.slot) ?? [];
            if (!spell || !e || e.dataset.zone !== "stack") continue;
            const want = said(marked(spell.controller, item.x, item.paid));
            if (said(edges(e)) !== want) {
                faults.push(
                    `${spell.name} on the stack marked ${said(edges(e))} for ${want}`,
                );
            }
        }
        const shown = [...document.querySelectorAll(".stack-ability")].map(
            (e) => said(edges(e)),
        );
        if (!phone && said(shown.sort()) !== said(abilities.sort())) {
            faults.push(
                `the stack's abilities marked ${shown.join(" ")} for ${abilities.join(" ")}`,
            );
        }
        // Your hand, however many cards it holds and however a prompt raises it, lies clear
        // of the stack: a card of it is picked where it lies, and an item of the stack read.
        const boxOf = (e) => e.getBoundingClientRect();
        const stacked = [
            ...document.querySelectorAll(
                '.card-el[data-zone="stack"], .stack-ability',
            ),
        ].map(boxOf);
        for (const o of phone ? [] : view.objects) {
            const [e] = drawn.get(o.slot) ?? [];
            if (o.zone !== "hand" || o.owner !== "you" || !e) continue;
            // The card's own frame: its element's box takes in the turn of a fan.
            const card = boxOf(e.querySelector(".card-slot, .card-back") ?? e);
            const under = stacked.some(
                (item) =>
                    card.left < item.right &&
                    item.left < card.right &&
                    card.top < item.bottom &&
                    item.top < card.bottom,
            );
            if (under) faults.push(`${o.name} in your hand lies on the stack`);
        }
        // A player's piles lie alike, as the stage has them at rest: the top cards of the
        // library, the graveyard and the exile pile end on one line, each as wide and as
        // upright as the others, the library's among them.
        const resting = window.berylStage?.shown.resting ?? new Map();
        for (const side of ["you", "opponent"]) {
            const tops = new Map();
            for (const [name, { object, pose, dress }] of resting) {
                const piled =
                    dress.look === "pile" &&
                    ["library", "graveyard", "exile"].includes(dress.zone);
                if (!piled || object?.owner !== side) continue;
                const top = tops.get(dress.zone);
                if (!top || pose.depth > top.pose.depth)
                    tops.set(dress.zone, { name, pose });
            }
            const lie = [...tops].map(([zone, { name, pose }]) => ({
                zone,
                bottom: document
                    .querySelector(`[data-stage="${name}"]`)
                    ?.getBoundingClientRect().bottom,
                lies: [pose.width, pose.rotation, pose.turn].join(" "),
            }));
            for (const pile of lie.slice(1)) {
                const [first] = lie;
                const whose = side === "you" ? "your" : "the opponent's";
                const what = `${whose} ${pile.zone} against the ${first.zone}`;
                if (!(Math.abs(pile.bottom - first.bottom) < 1)) {
                    faults.push(
                        `${what} ends at ${pile.bottom} for ${first.bottom}`,
                    );
                }
                if (pile.lies !== first.lies) {
                    faults.push(`${what} lies ${pile.lies} for ${first.lies}`);
                }
            }
        }
        // The ways to use a card (src/stage/Choose.tsx `Ways`) lie alike: each way's card
        // at one top and one size, and each label starting at one height under them, however
        // many lines it wraps to.
        const ways = [...document.querySelectorAll(".stage-way")].map((way) => {
            const box = (selector) =>
                way.querySelector(selector)?.getBoundingClientRect();
            const face = box(".stage-way-face");
            const says = box(".stage-way-says");
            return {
                name: way.getAttribute("aria-label"),
                lies: face
                    ? [face.top, face.width, face.height]
                          .map(Math.round)
                          .join(" ")
                    : "no card",
                says: says ? Math.round(says.top) : null,
            };
        });
        for (const way of ways.slice(1)) {
            const [first] = ways;
            const what = `the way "${way.name}" against "${first.name}"`;
            if (way.lies !== first.lies) {
                faults.push(
                    `${what}: its card lies ${way.lies} for ${first.lies}`,
                );
            }
            if (way.says !== first.says) {
                faults.push(
                    `${what}: its label starts at ${way.says} for ${first.says}`,
                );
            }
        }
        for (const slot of drawn.keys()) {
            if (slot >= 0 && !objects.has(slot))
                faults.push(`slot ${slot} drawn but not in the view`);
        }
        for (const side of ["you", "opponent"]) {
            const total = document.querySelector(
                `[data-life="${side}"] .life-orb-total`,
            );
            const life = String(view.players[side].life);
            if (total && text(total) !== life) {
                faults.push(`${side} at ${text(total)} life for ${life}`);
            }
        }
        return faults;
    });
}
