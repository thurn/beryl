// What the verb suite reads of the stage's arrows, lines and marks at one tick, in the page
// (verbs.mjs `walk`; src/stage/spans.ts, src/stage/dom.ts): each line and mark the stage holds
// with the element that draws it, and how the page differs from the model. The page is held
// to the model here, apart from the model's own geometry: one path a line, with the model's
// shape, its ends on the edges or faces of the two things it is between as the model's poses
// and the page's life totals have them; one element a mark, as far in as the model says and
// drawn about the whole of its card. It lives apart from `walk` only for that script's length.

/** In the page: what reads the spans of `stage` at each tick, for a walk that began at stage
 * time `origin` on a `clock` it holds and draws in `layer`, and tells its `faults`. */
export function spanSampler({ stage, clock, origin, layer, faults }) {
    const SIDES = ["you", "opponent"];
    const near = (a, b, by) => Math.abs(a - b) <= by;
    /** The box in stage pixels of what the stage names `on`, worked out apart from the
     * stage's own lines: a card as its pose and its record say it lies, leaning as its pose
     * leans; a player's life total as the page draws it. */
    const boxOf = (on) => {
        if (SIDES.includes(on)) {
            const table = layer.getBoundingClientRect();
            const by = table.width / layer.offsetWidth;
            const orb = document
                .querySelector(`[data-life="${on}"]`)
                .getBoundingClientRect();
            return {
                x: (orb.left - table.left) / by,
                y: (orb.top - table.top) / by,
                width: orb.width / by,
                height: orb.height / by,
                card: orb.height / by,
                round: true,
            };
        }
        const pose = stage.pose(on);
        const resting = stage.shown.resting.get(on);
        if (!pose || !resting) return null;
        const lies =
            resting.dress.zone === "battlefield" && resting.object?.tapped;
        const long = (pose.width * 88) / 63;
        const [width, height] = lies ? [long, pose.width] : [pose.width, long];
        const lean = Math.tan((pose.skew * Math.PI) / 180) * height;
        return {
            x: pose.x + Math.min(0, lean),
            y: pose.y,
            width: width + Math.abs(lean),
            height,
            card: pose.width,
            round: false,
        };
    };
    /** Why `point`, an end of a line, is not on the edge of `box`: a life total's is the
     * oval within its box; a card's is its frame, or for an attacker the tip of the chevron
     * it wears, a sixth of its width past its frame. With `onto`, why it is not on `box` at
     * all, as the head of an arrow at a target is: on its target's face, or at its edge
     * where the arrow meets the strip of a stack item under its own. */
    const offEdge = (point, box, onto = false) => {
        if (!box) return "ends on nothing";
        if (box.round) {
            const out = Math.hypot(
                (point.x - box.x - box.width / 2) / (box.width / 2),
                (point.y - box.y - box.height / 2) / (box.height / 2),
            );
            const on = onto ? out > 0.3 && out < 1.12 : near(out, 1, 0.12);
            return on ? null : `ends ${out} of the way out`;
        }
        const across = [point.x - box.x, box.x + box.width - point.x];
        const down = [point.y - box.y, box.y + box.height - point.y];
        const by = 0.2 * box.card;
        const within = [...across, ...down].every((d) => d >= -by);
        const edge =
            onto || [...across, ...down].some((d) => Math.abs(d) <= by);
        return within && edge
            ? null
            : `ends at ${point.x}, ${point.y}, off ${JSON.stringify(box)}`;
    };
    /** The zone the stage draws what it names `on` in; none for a player. */
    const zoneOf = (on) => stage.shown.resting.get(on)?.dress.zone ?? null;
    /** A number that names an element for as long as it is on the page. */
    const numbered = new WeakMap();
    let made = 0;
    const numberOf = (element) => {
        if (!numbered.has(element)) numbered.set(element, ++made);
        return numbered.get(element);
    };
    /** How a mark's element is drawn, whatever the size of what it is on: its colors, its
     * border and what it carries. */
    const lookOf = (element) => {
        const style = getComputedStyle(element);
        const colors = (said) => said.match(/rgba?\([^)]*\)/g) ?? [];
        return JSON.stringify([
            colors(style.boxShadow),
            style.borderTopStyle,
            colors(style.borderTopColor),
            getComputedStyle(element, "::after").content,
        ]);
    };
    /** The arrows, lines and marks the stage holds now, each with the element that draws it
     * (spans.ts, dom.ts), where its ends lie in the model, and how the page differs: one path
     * a line, with the model's shape, its ends on the edges of the two things it is between
     * as the model has them; one element a mark, as far in as the model says. */
    return () => {
        const at = clock.now - origin;
        const fault = (why) => faults.push(`at ${at} ms ${why}`);
        const held = stage.lines();
        const paths = [...layer.querySelectorAll(".stage-line")];
        if (paths.length !== held.length) {
            fault(`the page has ${paths.length} lines for ${held.length}`);
        }
        const lines = held.map((line) => {
            const { key, arrow, from, to, shown, hidden, shape } = line;
            const drawn = paths.filter((path) => path.dataset.line === key);
            const d = drawn[0]?.getAttribute("d") ?? null;
            if (drawn.length !== 1) {
                fault(`the line ${key} has ${drawn.length} elements`);
            } else if (d !== (shape?.d ?? "")) {
                fault(`the line ${key} is drawn as ${d}`);
            } else if (!drawn[0].matches(`.stage-line-${arrow}`)) {
                fault(`the line ${key} is drawn as ${drawn[0].className}`);
            }
            const boxes = [boxOf(from), boxOf(to)];
            if (shape) {
                const off =
                    offEdge(shape.from, boxes[0]) ??
                    offEdge(shape.to, boxes[1], arrow.endsWith("-target"));
                if (off) fault(`the line ${key} ${off}`);
            }
            return {
                key,
                arrow,
                from,
                to,
                shown,
                hidden,
                drawn: shape !== null,
                element: drawn[0] ? numberOf(drawn[0]) : null,
                ends: [from, to].map((on) => stage.pose(on)),
                zones: [from, to].map(zoneOf),
            };
        });
        const marked = stage.marks();
        // Not those of the copy a raised hand card is drawn by (src/stage/raised.ts).
        const elements = document.querySelectorAll(
            ".stage-mark:not(.stage-raised-copy *)",
        ).length;
        if (elements !== marked.length) {
            fault(`the page has ${elements} marks for ${marked.length}`);
        }
        const marks = marked.map(({ mark, on, shown }) => {
            const node = SIDES.includes(on)
                ? document.querySelector(`[data-life="${on}"]`)
                : layer.querySelector(`[data-stage="${on}"]`);
            const drawn = [
                ...(node?.querySelectorAll(`:scope > .stage-mark-${mark}`) ??
                    []),
            ];
            const through = Number(
                drawn[0]?.style.getPropertyValue("--shown") || NaN,
            );
            if (drawn.length !== 1) {
                fault(`the mark ${mark} on ${on} has ${drawn.length} elements`);
            } else if (!near(through, shown, 0.0001)) {
                fault(`the mark ${mark} on ${on} is drawn ${through} in`);
            } else if (
                !near(Number(getComputedStyle(drawn[0]).opacity), shown, 0.001)
            ) {
                fault(`the mark ${mark} on ${on} does not show ${shown}`);
            } else if (
                !SIDES.includes(on) &&
                getComputedStyle(drawn[0]).display !== "none"
            ) {
                // It is drawn about the whole of its card, standing off it by a tenth of its
                // width at most.
                const [about, card] = [drawn[0], node].map((element) =>
                    element.getBoundingClientRect(),
                );
                const off = ["left", "top", "right", "bottom"].find(
                    (edge) => !near(about[edge], card[edge], 0.1 * card.width),
                );
                if (off) fault(`the mark ${mark} on ${on} is off its ${off}`);
            }
            return {
                mark,
                on,
                shown,
                element: drawn[0] ? numberOf(drawn[0]) : null,
                look: drawn[0] && shown === 1 ? lookOf(drawn[0]) : null,
                turn: SIDES.includes(on) ? 0 : stage.pose(on).turn,
            };
        });
        return { at, lines, marks };
    };
}

/** Gives `page` the sampler (`window.berylSpanSampler`), for a walk played in it. */
export const install = (page) =>
    page.evaluate(`window.berylSpanSampler = ${spanSampler}`);
