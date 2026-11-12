// What a game screen says went wrong, for the checks that open one (shots.mjs, specs.mjs,
// playthrough.mjs).

/** What the game screen tells its player went wrong (src/screens/Game.tsx's toast): a command
 * the session, or a recording, refused. A screen a check opens shows none: a click a recording
 * holds no answer to is refused so, and the shot of what it was to open would show the table
 * as it was. */
export async function toldErrors(page) {
    return page.evaluate(() =>
        [...document.querySelectorAll(".game-error")].map((e) =>
            e.textContent.trim(),
        ),
    );
}
