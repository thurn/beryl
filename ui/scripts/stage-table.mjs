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
 * in a column 100 apart at x 2000, bottom first; a library's cards in one place, at x 3000
 * and y 500. Cards in no zone drawn here (a graveyard's) rest nowhere. A card with no name
 * shows its back. An ability lifts out of its source 10 in from its corner, at half its width,
 * and a drawn card is held up 300 above its place in the hand at twice its width. A card is
 * 1.4 times as tall as it is wide, and a tapped permanent lies on its side; the players' life
 * totals are at x 1000, yours at y 900 and the opponent's at y -400. */
export const row = {
    rest({ view: table, abilities }) {
        const out = new Map();
        const pose = (x, y, depth, face = "front") => ({
            x,
            y,
            width: 100,
            rotation: 0,
            skew: 0,
            turn: 0,
            opacity: 1,
            depth,
            face,
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
                pose: pose(x, y, slot, object.name === null ? "back" : "front"),
                dress: dress(object.zone),
            });
        };
        table.battlefield.forEach((slot, i) => place(slot, i * 400, 0));
        table.objects
            .filter((o) => o.zone === "hand")
            .forEach((o, i) => place(o.slot, i * 400, 500));
        for (const side of ["you", "opponent"]) {
            for (const slot of table.players[side].library) {
                place(slot, 3000, 500);
            }
        }
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
    anchors: () => ({
        life: (side) => ({
            middle: { x: 1000, y: side === "you" ? 900 : -400 },
        }),
    }),
    middle({ object, dress }, pose) {
        const lies = dress.zone === "battlefield" && object?.tapped;
        const [wide, tall] = lies ? [1.4, 1] : [1, 1.4];
        return {
            x: pose.x + (pose.width * wide) / 2,
            y: pose.y + (pose.width * tall) / 2,
        };
    },
    emerging: (source, rest) => ({
        ...rest,
        x: source.x + 10,
        y: source.y + 10,
        width: source.width / 2,
    }),
    // A drawn card is held up 300 above where it will lie, at twice its width.
    drawn: (rest) => ({ ...rest, y: rest.y - 300, width: rest.width * 2 }),
};

/** A surface that records what the stage did to it: the nodes it made and removed (`log`),
 * how it dressed each, as its zone or as a card back (`dressed`), each node's pose, and how
 * far through each effect drawn on a node is, by `<node> <effect> <layer>` (`effects`). */
export function recorder() {
    const log = [];
    const dressed = [];
    const nodes = new Map();
    const effects = new Map();
    return {
        log,
        dressed,
        nodes,
        effects,
        effect(on, { name, layer }, progress) {
            const drawn = `${on} ${name} ${layer}`;
            if (progress === undefined) effects.delete(drawn);
            else effects.set(drawn, progress);
        },
        make: (slot) => (nodes.set(slot, null), log.push(`make ${slot}`)),
        remove(slot) {
            nodes.delete(slot);
            // What was drawn on a node goes with it.
            for (const drawn of [...effects.keys()]) {
                if (drawn.startsWith(`${slot} `)) effects.delete(drawn);
            }
            log.push(`remove ${slot}`);
        },
        dress: (slot, { dress }) =>
            dressed.push(`${slot} ${dress.back ? "back" : dress.zone}`),
        draw: (slot, pose) => nodes.set(slot, pose),
    };
}

/** A stage on a held clock. `durations` gives the verbs that take time, or says of each act
 * how long it takes; `effects`, the effects each act begins. */
export function stage(durations = {}, effects = undefined) {
    const surface = recorder();
    const timed =
        typeof durations === "function"
            ? durations
            : (act) => durations[act.verb];
    const made = new ScriptStage({
        layout: row,
        surface,
        timings: (act) => ({ ...SNAP, ...timed(act) }),
        effects,
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
