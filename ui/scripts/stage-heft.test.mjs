// The heft of a held card, with no browser (`npm test`): how a card the pointer picks up rises,
// leans into the way it is moved, casts its shadow and settles flat once let go
// (ui/src/stage/heft.ts). A test steps it as the surface's browser frames do (dom.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { drawn, Heft, HELD_HEIGHT } from "../src/stage/heft.ts";

const FRAME = 1 / 60;
const REACH = [64, 90];

/** Runs `heft` for `seconds`, the carried point moving by `velocity` px/s from `from`;
 * returns where the point ended and whether the card was still moving at the end. */
function run(heft, { from, velocity = [0, 0], seconds, held = true }) {
    let at = from;
    let moving = true;
    for (let t = 0; t < seconds; t += FRAME) {
        at = [at[0] + velocity[0] * FRAME, at[1] + velocity[1] * FRAME];
        moving = heft.advance(
            { x: at[0], y: at[1], held, reach: REACH },
            FRAME,
        );
    }
    return { at, moving };
}

test("a held card rises off the felt and leans the way it is pulled", () => {
    const heft = new Heft(400, 600);
    run(heft, { from: [400, 600], velocity: [900, 0], seconds: 0.4 });
    const { height, tilt } = heft.look;
    assert.ok(Math.abs(height - HELD_HEIGHT) < 2, `held at ${height}`);
    assert.ok(tilt[0] > 0.05, `pulled right, it leans right: ${tilt[0]}`);
    assert.ok(Math.abs(tilt[1]) < 1e-3, `and not up or down: ${tilt[1]}`);

    const up = new Heft(400, 600);
    run(up, { from: [400, 600], velocity: [0, -900], seconds: 0.4 });
    assert.ok(
        up.look.tilt[1] < -0.05,
        `pulled up, it leans up: ${up.look.tilt[1]}`,
    );
});

test("a held card the pointer stops comes back flat and stays up", () => {
    const heft = new Heft(400, 600);
    const { at } = run(heft, {
        from: [400, 600],
        velocity: [900, -300],
        seconds: 0.3,
    });
    const { moving } = run(heft, { from: at, seconds: 1.5 });
    // Still held, it is still moving as far as the surface knows: the pointer may move again.
    assert.ok(moving);
    const { height, tilt } = heft.look;
    assert.ok(Math.abs(height - HELD_HEIGHT) < 0.5, `held at ${height}`);
    assert.ok(Math.hypot(...tilt) < 0.005, `flat: ${tilt}`);
});

test("a card let go settles on the felt and stops running", () => {
    const heft = new Heft(400, 600);
    const { at } = run(heft, {
        from: [400, 600],
        velocity: [600, -600],
        seconds: 0.3,
    });
    let frames = 0;
    while (
        heft.advance({ x: at[0], y: at[1], held: false, reach: REACH }, FRAME)
    ) {
        frames += 1;
        assert.ok(frames < 120, "it settles within two seconds");
    }
    assert.deepEqual(heft.look, { height: 0, tilt: [0, 0] });
});

test("a raised card's shadow falls toward you and fades out as it lands", () => {
    const up = drawn({ height: HELD_HEIGHT, tilt: [0, 0] }, [64, 40], 128, 1);
    assert.ok(up.opacity > 0.4, `a held card casts a shadow: ${up.opacity}`);
    const [x, y] = up.shadow
        .match(/translate\(([-\d.]+)px, ([-\d.]+)px\)/)
        .slice(1)
        .map(Number);
    assert.ok(y > Math.abs(x) && x > 0, `down and a little right: ${x}, ${y}`);
    assert.match(up.card, /scale\(1\.06/);

    const down = drawn({ height: 0, tilt: [0, 0] }, [64, 40], 128, 1);
    assert.equal(down.opacity, 0);
});
