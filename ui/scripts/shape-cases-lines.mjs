// What the shape suite (shapes.mjs) holds of a declaration's lines besides which are drawn
// (shape-cases-table.mjs `declared`, and its combat line followed to the damage): how each
// is drawn, whether two cross, and the line the pointer pulls out of a creature declared
// that awaits what it is aimed at (src/stage/Aim.tsx).
import assert from "node:assert/strict";

/** In the page: each line of `arrow` the page holds, by its key: where it starts and ends in
 * the window, how many strokes and closed shapes its path is made of, and how it is painted
 * where it is (the paint of a path that light is drawn in place of is still its own). */
function lines(arrow) {
    const layer = document.querySelector(".card-layer");
    const frame = layer.getBoundingClientRect();
    const scale = frame.width / layer.offsetWidth || 1;
    const seen = (x, y) => ({
        x: frame.left + x * scale,
        y: frame.top + y * scale,
    });
    return [...document.querySelectorAll(`.stage-line-${arrow}`)].map(
        (path) => {
            const d = path.getAttribute("d") ?? "";
            const numbers = (d.match(/-?[\d.]+(?:e-?\d+)?/g) ?? []).map(Number);
            const style = getComputedStyle(path);
            return {
                key: path.dataset.line ?? null,
                from: seen(numbers[0], numbers[1]),
                to: seen(numbers.at(-2), numbers.at(-1)),
                strokes: (d.match(/M/gi) ?? []).length,
                closed: (d.match(/Z/gi) ?? []).length,
                fill: style.fill,
                width: parseFloat(style.strokeWidth) * scale,
            };
        },
    );
}

/** Whether the stretch from `a` to `b` crosses the one from `c` to `d`. */
export function cross([a, b], [c, d]) {
    const side = (p, q, r) =>
        Math.sign((q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x));
    return (
        side(a, b, c) * side(a, b, d) < 0 && side(c, d, a) * side(c, d, b) < 0
    );
}

/** The widest a line of combat is drawn, in the window's pixels: a thread beside an arrow at
 * a target, which is twice that and has a head (stage.css). */
const THIN = 2.5;

/** Holds how the lines of the declaration `held` shows are drawn, once the screen has drawn
 * them: each is one thin stroke with no head, so that a heavy arrow always means a target
 * (stage.css). */
export async function lineLooks(page, held, what) {
    const state = held.prompt?.state;
    if (state?.shape !== "declare" || !state.lined) return;
    const drawn = (await page.evaluate(lines, state.arrow)).filter(
        (line) => line.key !== null,
    );
    for (const line of drawn) {
        const said = `${what}: the line ${line.key}`;
        assert.equal(line.strokes, 1, `${said} is one stroke`);
        assert.equal(line.closed, 0, `${said} has no head`);
        assert.equal(line.fill, "none", `${said} is not filled`);
        assert.ok(line.width <= THIN, `${said} is ${line.width}px wide`);
    }
}

/** Holds the line the pointer pulls on the declaration `held` shows, in a layout with a
 * mouse: while creatures declared await what they are aimed at, one line from the middle of
 * each to the pointer, in the look of the line the aim will draw, wherever the pointer goes;
 * and none while no creature waits. */
export async function pointerLines(page, held, what) {
    const state = held.prompt?.state;
    if (state?.shape !== "declare") return;
    const { width, height } = page.viewportSize();
    const points = [
        { x: width * 0.42, y: height * 0.3 },
        { x: width * 0.6, y: height * 0.5 },
    ];
    for (const at of points) {
        await page.mouse.move(at.x, at.y);
        const want = state.aiming.length;
        await page.waitForFunction(
            ([arrow, want]) =>
                document.querySelectorAll(
                    `.stage-line-${arrow}:not([data-line])`,
                ).length === want,
            [state.arrow, want],
        );
        const pulled = (await page.evaluate(lines, state.arrow)).filter(
            (line) => line.key === null,
        );
        const middles = await page.evaluate(
            (slots) =>
                slots.map((slot) => {
                    const box = document
                        .querySelector(`[data-stage="${slot}"]`)
                        .getBoundingClientRect();
                    return {
                        x: box.left + box.width / 2,
                        y: box.top + box.height / 2,
                    };
                }),
            state.aiming,
        );
        const near = (a, b) => Math.hypot(a.x - b.x, a.y - b.y) < 1.5;
        for (const line of pulled) {
            const said = `${what}: the line to the pointer at ${at.x}, ${at.y}`;
            assert.ok(near(line.to, at), `${said} ends at ${line.to.x}`);
            assert.ok(
                middles.some((middle) => near(line.from, middle)),
                `${said} starts on no creature that waits`,
            );
            assert.ok(line.width <= THIN, `${said} is ${line.width}px wide`);
        }
    }
    await page.mouse.move(2, 2);
}
