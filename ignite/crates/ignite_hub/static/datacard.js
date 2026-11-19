// Cirrus data cards: the hovercard of a mark that carries its facts as JSON in [data-card]
// instead of pointing at a <template>, so a chart with hundreds of marks needs no template
// apiece. cirrus.js shows the card this builds as it shows any hovercard.
//
// The JSON is {group, kicker, title, note, figure, context, rows}, where each row is [label, value]
// and a value is text, {time: iso} (shown in the viewer's clock, to the minute) or
// {code: text}. Every string is set as text, never parsed as HTML.
(() => {
    "use strict";

    const clock = new Intl.DateTimeFormat(undefined, {
        dateStyle: "medium",
        timeStyle: "short",
    });

    function make(tag, className, content) {
        const node = document.createElement(tag);
        if (className) node.className = className;
        if (content != null) node.textContent = content;
        return node;
    }

    function value(dd, given) {
        if (given && typeof given === "object" && given.time) {
            const date = new Date(given.time);
            dd.textContent = Number.isNaN(date.getTime())
                ? given.time
                : clock.format(date);
        } else if (given && typeof given === "object" && given.code) {
            dd.append(make("code", "c-datacard-code", given.code));
        } else {
            dd.textContent = String(given ?? "");
        }
    }

    window.cirrusDataCard = (text) => {
        let data;
        try {
            data = JSON.parse(text);
        } catch {
            return null;
        }
        const card = make("div", "c-hovercard c-datacard");
        const kicker = make("div", "c-datacard-kicker");
        const swatch = make("span", "c-datacard-swatch");
        if (data.group) swatch.setAttribute("data-group", data.group);
        kicker.append(swatch, data.kicker || "");
        const figure = make("div", "c-datacard-figure", data.figure || "");
        if (data.context) figure.append(make("span", "", data.context));
        const rows = make("dl", "c-meta");
        for (const [label, given] of data.rows || []) {
            const item = make("div", "c-meta-item");
            const dd = make("dd");
            value(dd, given);
            item.append(make("dt", "", label), dd);
            rows.append(item);
        }
        card.append(kicker, make("div", "c-hovercard-title", data.title || ""));
        if (data.note) card.append(make("p", "c-datacard-note", data.note));
        card.append(figure, rows);
        return card;
    };
})();
