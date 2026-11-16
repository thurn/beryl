/** What a phone screen (`?phone`) must not show, as DESIGN.md of the phone prototype sets it:
 * UI text under 12px (the P/T slash may be smaller, and a card's own printing, drawn where its
 * scan has not loaded, is the card's: a scan's is texture); a control smaller than about 40×40,
 * too small for a finger; the page scrolling sideways; and a card or control reaching outside
 * the window. The hand can extend past the edge, and a scrollable panel can contain controls
 * below its fold if scrolling can bring them fully into view. */
export async function phoneFaults(page) {
    return page.evaluate(() => {
        const faults = [];
        const shown = (e) => {
            if (!e.getClientRects().length) return false;
            if (getComputedStyle(e).visibility !== "visible") return false;
            for (let a = e; a; a = a.parentElement) {
                if (parseFloat(getComputedStyle(a).opacity) <= 0) return false;
            }
            return true;
        };
        const name = (e) =>
            e.getAttribute("aria-label") ||
            e.textContent?.trim().slice(0, 30) ||
            e.className ||
            e.tagName.toLowerCase();
        for (const el of document.querySelectorAll("#root *")) {
            const text = [...el.childNodes]
                .filter((n) => n.nodeType === Node.TEXT_NODE)
                .map((n) => n.textContent.trim())
                .join(" ")
                .trim();
            if (!text || el.closest(".card-rim, .card-pt i") || !shown(el))
                continue;
            const size = parseFloat(getComputedStyle(el).fontSize);
            if (size < 11.95) {
                faults.push(`"${text.slice(0, 40)}" at ${size.toFixed(1)}px`);
            }
        }
        const controls = document.querySelectorAll(
            "button, [role=button], a[href], input, select",
        );
        for (const e of controls) {
            if (!shown(e)) continue;
            const b = e.getBoundingClientRect();
            if (b.width < 39.5 || b.height < 39.5) {
                faults.push(
                    `${name(e)} is ${Math.round(b.width)}×${Math.round(b.height)}`,
                );
            }
        }
        const page = document.documentElement;
        if (page.scrollWidth > innerWidth + 1) {
            faults.push(`the page scrolls sideways to ${page.scrollWidth}px`);
        }
        const canScrollIntoWindow = (e, box) => {
            for (
                let a = e.parentElement;
                a && a !== page;
                a = a.parentElement
            ) {
                const overflow = getComputedStyle(a).overflowY;
                if (overflow === "visible") continue;
                const top = a.getBoundingClientRect().top + a.clientTop;
                const bottom = top + a.clientHeight;
                if (overflow === "auto" || overflow === "scroll") {
                    const maxScroll = a.scrollHeight - a.clientHeight;
                    if (maxScroll > 0) {
                        return (
                            top >= -1 &&
                            bottom <= innerHeight + 1 &&
                            box.height <= a.clientHeight + 1 &&
                            box.top + a.scrollTop >= top - 1 &&
                            box.bottom + a.scrollTop <= bottom + maxScroll + 1
                        );
                    }
                }
                if (box.top < top - 1 || box.bottom > bottom + 1) return false;
            }
            return false;
        };
        for (const e of document.querySelectorAll(".card-el, button")) {
            if (!shown(e) || e.closest("[data-zone=hand]")) continue;
            const b = e.getBoundingClientRect();
            if (
                b.left < -1 ||
                b.right > innerWidth + 1 ||
                ((b.top < -1 || b.bottom > innerHeight + 1) &&
                    !canScrollIntoWindow(e, b))
            ) {
                faults.push(
                    `${e.dataset.card ?? name(e)} reaches outside the window`,
                );
            }
        }
        return faults;
    });
}
