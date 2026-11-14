// The kept checks of the arrow a choice of targets pulls from its spell or ability to the
// pointer (UI Conventions, "An arrow to the pointer"; src/stage/Aim.tsx): where it is not drawn.
import assert from "node:assert/strict";
import {
    check,
    clicked,
    lastDecision,
    load,
    replied,
} from "./kept-harness.mjs";

// Nothing follows the pointer over a prompt's cards: a Snapcaster Mage trigger chooses among the
// graveyard's cards, held up over the dimmed table, and the arrow from the trigger to the
// cursor (UI Conventions, "An arrow to the pointer") is not drawn across them. The page's
// layer says whether the stage pulls anything (`data-aimed`), which a browser without WebGL
// shows no arrow for either. No generic check sees it: it is the absence of a trace that follows
// the mouse, not a state of the table.
await check(
    "no arrow follows the cursor over cards a target prompt holds up",
    async (page) => {
        await page.waitForSelector(".stage-tray");
        for (const [x, y] of [
            [500, 300],
            [900, 500],
        ]) {
            await page.mouse.move(x, y);
            assert.equal(
                await page.locator("[data-aimed]").count(),
                0,
                `no arrow to the cursor at ${x}, ${y}`,
            );
        }
    },
    {
        fixture: "target-graveyard",
        ready: ".stage-tray",
        at: (await load("target-graveyard")).length,
    },
);

// Nothing follows the pointer once a target is picked: Oust's one legal target is the human's
// own Jace, Vryn's Prodigy, so a click on it picks it and the prompt waits on "Submit 1"
// (UI Conventions, exception 7) instead of answering. The stage's own arrow then runs to the
// target, and the arrow to the cursor (UI Conventions, "An arrow to the pointer") is gone
// wherever the mouse goes. No generic check sees it: it is the absence of a trace that follows
// the mouse, not a state of the table.
const own = await lastDecision("target-own");
const [ownTarget] = own.bySlot.keys();
const ownPicked = structuredClone(own.decision.prompt);
ownPicked.body.picked = [{ kind: "object", slot: ownTarget }];
ownPicked.marks = [
    { on: { kind: "object", slot: ownTarget }, mark: "friendly-target" },
];
ownPicked.buttons = [
    ...ownPicked.buttons.filter((b) => b.id.kind !== "confirm"),
    {
        id: { kind: "confirm" },
        label: "Submit 1",
        caption: "",
        enabled: true,
        primary: true,
    },
];
await replied("target-own-picked", "target-own", [
    { kind: "open", prompt: ownPicked },
]);
await check(
    "no arrow follows the cursor once the target is picked",
    async (page) => {
        const away = [
            [400, 450],
            [1100, 300],
        ];
        await page.mouse.move(...away[0]);
        assert.equal(
            await page.locator("[data-aimed]").count(),
            1,
            "an arrow follows the cursor before the pick",
        );
        const target = page.locator(`[data-slot="${ownTarget}"]`);
        const contextMenuAllowed = await target.evaluate((el) => {
            return el.dispatchEvent(
                new MouseEvent("contextmenu", {
                    bubbles: true,
                    cancelable: true,
                    button: 2,
                }),
            );
        });
        assert.equal(
            contextMenuAllowed,
            true,
            "engine prompts allow the browser context menu",
        );
        await target.click({ button: "right" });
        assert.equal(
            await page.locator("[data-aimed]").count(),
            1,
            "right click leaves the prompt waiting",
        );
        await target.click();
        await clicked(page, 1);
        await page.clock.runFor(500);
        for (const [x, y] of away) {
            await page.mouse.move(x, y);
            assert.equal(
                await page.locator("[data-aimed]").count(),
                0,
                `no arrow to the cursor at ${x}, ${y}`,
            );
        }
    },
    {
        fixture: "target-own-picked",
        at: own.views.length,
        ready: `[data-slot="${ownTarget}"]`,
    },
);
