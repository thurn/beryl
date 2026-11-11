// What the stage's tests share (stage.test.mjs, stage-slice.test.mjs): a stage on a surface
// that only records, over a layout that lays the table out in rows, so every expectation is a
// number worked out by hand.
import { abilityName } from "../src/stage/layout.ts";
import { ScriptStage, SNAP } from "../src/stage/stage.ts";

export const object = (slot, zone = "battlefield", more = {}) => ({
    slot,
    zone,
    owner: "you",
    controller: "you",
    name: `Card ${slot}`,
    facing: "up",
    tapped: false,
    ...more,
});

/** A view holding `battlefield`, in order, and the cards in `hand`. */
export const view = (battlefield, hand = [], more = {}) => ({
    objects: [
        ...battlefield.map((slot) => object(slot)),
        ...hand.map((slot) => object(slot, "hand")),
    ].sort((a, b) => a.slot - b.slot),
    battlefield,
    stack: [],
    players: {
        you: { life: 20, library: [], graveyard: [] },
        opponent: { life: 20, library: [], graveyard: [] },
    },
    turn: { number: 1, active: "you", step: "precombatMain" },
    ...more,
});

/** Permanents in a row 400 apart at y 0; a hand's cards in a row at y 500; the stack's items
 * in a column 100 apart at x 2000, bottom first. Cards in no zone drawn here (a graveyard's)
 * rest nowhere. An ability lifts out of its source 10 in from its corner, at half its width. */
export const row = {
    rest({ view: table, abilities }) {
        const out = new Map();
        const pose = (x, y, depth) => ({
            x,
            y,
            width: 100,
            rotation: 0,
            skew: 0,
            opacity: 1,
            depth,
            face: "front",
        });
        const dress = (zone) => ({
            zone,
            look: zone === "battlefield" ? "field" : zone,
            frame: 100,
            back: false,
            rim: false,
        });
        const place = (slot, x, y) => {
            const object = table.objects.find((o) => o.slot === slot);
            out.set(slot, {
                object,
                pose: pose(x, y, slot),
                dress: dress(object.zone),
            });
        };
        table.battlefield.forEach((slot, i) => place(slot, i * 400, 0));
        table.objects
            .filter((o) => o.zone === "hand")
            .forEach((o, i) => place(o.slot, i * 400, 500));
        let numbered = 0;
        table.stack.forEach((item, i) => {
            if (item.kind === "spell") return place(item.slot, 2000, i * 100);
            out.set(abilityName(abilities[numbered++]), {
                ability: item,
                source: out.get(item.source)?.object,
                pose: pose(2000, i * 100, 400 + i),
                dress: dress("stack"),
            });
        });
        return out;
    },
    anchors() {
        throw new Error("no test asks for an anchor");
    },
    emerging: (source, rest) => ({
        ...rest,
        x: source.x + 10,
        y: source.y + 10,
        width: source.width / 2,
    }),
};

/** A surface that records what the stage did to it. */
export function recorder() {
    const log = [];
    const nodes = new Map();
    return {
        log,
        nodes,
        make: (slot) => (nodes.set(slot, null), log.push(`make ${slot}`)),
        remove: (slot) => (nodes.delete(slot), log.push(`remove ${slot}`)),
        dress: () => {},
        draw: (slot, pose) => nodes.set(slot, pose),
    };
}

/** A stage on a held clock. `durations` gives the verbs that take time, or says of each act
 * how long it takes. */
export function stage(durations = {}) {
    const surface = recorder();
    const timed =
        typeof durations === "function"
            ? durations
            : (act) => durations[act.verb];
    const made = new ScriptStage({
        layout: row,
        surface,
        timings: (act) => ({ ...SNAP, ...timed(act) }),
    });
    return { stage: made, surface };
}

/** A step moving the permanent in `slot` to place `at` of the battlefield's order. */
export const move = (slot, at) => ({
    act: {
        verb: "fly",
        from: "battlefield",
        why: "effect",
        object: object(slot),
        at,
    },
});

/** The step `made` is playing, by where and when. */
export function playing(made) {
    const { index, verb, from, to } = made.step;
    return { index, verb, from, to };
}

/** Advances the clock to `time` in ticks of `tick`. */
export function reach(clock, time, tick) {
    while (clock.now < time) clock.advance(Math.min(tick, time - clock.now));
}
