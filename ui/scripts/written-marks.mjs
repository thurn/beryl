// The marks of the verb suite, as a script written by hand: the session's scripts draw no
// mark (rust/crates/beryl_session/src/script/mod.rs, Spans: all six are a prompt's, which the
// session's prompt state will give), so the suite's check of `mark` (verbs.mjs), the stepped
// capture (stepped.mjs) and the films for the motion review (verb-film.mjs) all play this one
// over the table their first moment leaves. It marks as three prompts would, each ending its
// marks before the next begins: everything of yours on the battlefield and in your hand as
// eligible, at once, then one permanent picked, in place of its eligible outline; a
// permanent as what the offered default would use, then as picked to pay; and the targets
// of a spell alone, one of each relationship, a player and a permanent among them. Each beat
// is played as a script of its own, the stage standing idle between them with the marks
// drawn, as a prompt's marks come and go.

/** In the page: the beats of the marks for the table of `view`, and what they are on:
 * `picked` and `paying`, the slots of the two permanents, whether `paying` `turns` (it is
 * untapped), the slots marked `eligible` and the `targets`, each a mark and the slot or
 * player it is on. */
export function writtenMarks(view) {
    const field = view.objects.filter((o) => o.zone === "battlefield");
    const yours = field.filter((o) => o.controller === "you");
    const hand = view.objects.filter(
        (o) => o.zone === "hand" && o.owner === "you",
    );
    // One to pick and another to pay with, untapped if any is: permanents that lie in no
    // pile of their like, where only the top card's frame shows. And one of the opponent's
    // to aim at.
    const alone = (o) => field.filter((p) => p.name === o.name).length === 1;
    const picked = yours.findLast(alone) ?? yours.at(-1);
    const others = yours.filter((o) => o !== picked);
    const paying =
        others.find((o) => !o.tapped && alone(o)) ??
        others.find((o) => !o.tapped) ??
        others[0];
    const aimed =
        field.find((o) => o.controller === "opponent" && alone(o)) ??
        field.find((o) => o.controller === "opponent");
    const object = ({ slot }) => ({ kind: "object", slot });
    const player = (side) => ({ kind: "player", player: side });
    const mark = (span, name, on) => ({
        act: { verb: "mark", span, mark: name, on },
    });
    const eligible = [...yours, ...hand].map(object);
    const targets = [
        ["friendly-target", player("you")],
        ["hostile-target", player("opponent")],
        ...(aimed ? [["hostile-target", object(aimed)]] : []),
    ];
    const unpicked = eligible.filter((on) => on.slot !== picked.slot);
    return {
        beats: [
            eligible.map((on) => mark("begin", "eligible", on)),
            [
                mark("end", "eligible", object(picked)),
                mark("begin", "selected", object(picked)),
            ],
            [
                ...unpicked.map((on) => mark("end", "eligible", on)),
                mark("end", "selected", object(picked)),
            ],
            [mark("begin", "suggested", object(paying))],
            [
                mark("end", "suggested", object(paying)),
                mark("begin", "paying", object(paying)),
            ],
            [mark("end", "paying", object(paying))],
            targets.map(([name, on]) => mark("begin", name, on)),
            targets.map(([name, on]) => mark("end", name, on)),
        ],
        picked: picked.slot,
        paying: paying.slot,
        turns: !paying.tapped,
        eligible: eligible.map((on) => on.slot),
        targets: targets.map(([name, on]) => [
            name,
            on.kind === "player" ? on.player : on.slot,
        ]),
    };
}

/** Gives `page` the marks to write (`window.berylWrittenMarks`), for a walk played in it. */
export const install = (page) =>
    page.evaluate(`window.berylWrittenMarks = ${writtenMarks}`);
