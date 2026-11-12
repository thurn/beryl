// What the screen draws of a prompt, read in the page for the shape suite (shapes.mjs), which
// holds it to what the session sent.

/** In the page: what the screen draws of a prompt now. The title, and the symbols a cost in
 * it is drawn as, which its words would read the same without; the dock's buttons where each
 * stands (`forward` in the turn button's place, `passOn` the round one, `other` stacked
 * over), every mark the page and the stage's model hold, each as `<mark> <on>`, and what is
 * held up (`heldUp`): the cards lying over everything at rest and under what flies, those in
 * a pile's box by that box, and the triggers listed; how much of a cost it says is left; and
 * the lines of `arrow`, a declaration's, that the page and the model hold; and with
 * `blocks`, the combat damage it says is on its way to the player. */
export function drawn({ dock, title, captions, arrow, blocks }) {
    // A label as the session wrote it: a mana symbol is drawn from its `{...}`.
    const words = (element) => {
        const copy = element.cloneNode(true);
        for (const symbol of copy.querySelectorAll(
            ".mana-symbol[aria-label]",
        )) {
            symbol.textContent = symbol.getAttribute("aria-label");
        }
        for (const tip of copy.querySelectorAll(".tip, kbd, .prompt-left")) {
            if (tip !== copy) tip.remove();
        }
        return copy.textContent.trim();
    };
    const all = (selector) => [...document.querySelectorAll(selector)];
    const left = document.querySelector(`:is(${title}) .prompt-left`);
    const ours = (keys) =>
        keys.filter((key) => arrow && key?.startsWith(`${arrow} `)).sort();
    const buttons = all(dock).flatMap((part) =>
        [...part.querySelectorAll("button:not(.prompt-step)")].map((button) => {
            // The round button and a way to use a card are named, not worded.
            const passOn = button.classList.contains("pass-on-button");
            const named = passOn || button.classList.contains("stage-way");
            const forward =
                !passOn &&
                (button.closest(".turn-action") !== null ||
                    button.closest(".phone-dock") !== null);
            const caption = button
                .closest(".turn-action")
                ?.querySelector(".turn-caption");
            return {
                label: named
                    ? button.getAttribute("aria-label")
                    : words(button),
                enabled: !button.disabled,
                where: passOn ? "passOn" : forward ? "forward" : "other",
                caption:
                    captions && forward
                        ? caption
                            ? words(caption)
                            : ""
                        : undefined,
            };
        }),
    );
    const on = (mark) => {
        const node = mark.parentElement;
        return node.dataset.stage ?? node.dataset.life ?? "?";
    };
    // Not those of the copy a raised hand card is drawn by (src/stage/raised.ts).
    const page = [
        ...document.querySelectorAll(".stage-mark:not(.stage-raised-copy *)"),
    ].map((mark) => {
        const name = [...mark.classList]
            .find((c) => c.startsWith("stage-mark-"))
            .slice("stage-mark-".length);
        return `${name} ${on(mark)}`;
    });
    const stage = window.berylStage;
    const heading = document.querySelector(title);
    const nodes = [...document.querySelectorAll("[data-stage]")];
    const lifted = nodes.filter((node) => {
        const depth = Number(node.style.zIndex);
        return depth >= 850 && depth < 900;
    });
    const slotOf = (node) => Number(node.dataset.stage);
    const middle = (node) => {
        const box = node.getBoundingClientRect();
        return { x: box.left + box.width / 2, y: box.top + box.height / 2 };
    };
    const boxed = new Set();
    const piles = [...document.querySelectorAll(".stage-tray-pile")].map(
        (pile) => {
            const box = pile.getBoundingClientRect();
            const within = lifted.filter((node) => {
                const { x, y } = middle(node);
                return (
                    x >= box.left &&
                    x <= box.right &&
                    y >= box.top &&
                    y <= box.bottom
                );
            });
            for (const node of within) boxed.add(node);
            return {
                label: pile.textContent.trim(),
                cards: within
                    .sort((a, b) => middle(a).x - middle(b).x)
                    .map(slotOf),
            };
        },
    );
    const up = {
        piles,
        loose: lifted
            .filter((node) => !boxed.has(node))
            .map(slotOf)
            .sort((a, b) => a - b),
        order: [...document.querySelectorAll(".stage-order-item")].map((item) =>
            Number(item.dataset.item),
        ),
        backs: lifted.filter((node) => node.dataset.card === undefined).length,
        twice: nodes.length - new Set(nodes.map((n) => n.dataset.stage)).size,
    };
    return {
        title: heading ? words(heading) : "",
        symbols: heading
            ? [...heading.querySelectorAll(".mana-symbol[aria-label]")].map(
                  (symbol) => symbol.getAttribute("aria-label"),
              )
            : [],
        buttons,
        marks: page.sort(),
        model: stage
            .marks()
            .map(({ mark, on }) => `${mark} ${on}`)
            .sort(),
        // A mark at rest is whole.
        coming: [
            ...document.querySelectorAll(
                ".stage-mark:not(.stage-raised-copy *)",
            ),
        ].filter((mark) => mark.style.getPropertyValue("--shown") !== "1")
            .length,
        busy: stage.shown.busy,
        up,
        left: left ? words(left).replace(/^\(|\)$/g, "") : "",
        panel: {
            value:
                document.querySelector(".prompt-stepper")?.dataset.value ??
                null,
            entries: all(".stage-entry").map((entry) =>
                entry.getAttribute("aria-pressed") === "true"
                    ? `[${entry.textContent}]`
                    : entry.textContent,
            ),
            shares: all(".stage-share")
                .map(
                    (s) =>
                        `${s.dataset.share}=${s.querySelector("b").textContent}`,
                )
                .sort(),
        },
        lines: {
            page: ours(
                [...document.querySelectorAll(".stage-line")].map(
                    (line) => line.dataset.line,
                ),
            ),
            model: ours(stage.lines().map((line) => line.key)),
        },
        damage: blocks
            ? Number(
                  document.querySelector('[data-life="you"] .life-incoming')
                      ?.dataset.incoming ?? NaN,
              )
            : null,
    };
}
