//! Cirrus, the dashboard's design system, as plain functions that return HTML strings.
//!
//! Text arguments are escaped here. Arguments named `body`, `trailing`, `main` and the like
//! hold HTML that another component already rendered. The styles and script that go with these
//! components are in `static/` and served by [`super::assets`].
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use super::assets::Assets;

/// Escapes `&`, `<`, `>`, `"` and `'`.
pub fn esc(value: &str) -> String {
    escape_with(value, true)
}

/// Escapes text for HTML.
pub fn escape(text: &str) -> String {
    esc(text)
}

/// Escapes `&`, `<` and `>` only, for text outside attributes.
pub fn esc_text(value: &str) -> String {
    escape_with(value, false)
}

fn escape_with(value: &str, quote: bool) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if quote => out.push_str("&quot;"),
            '\'' if quote => out.push_str("&#x27;"),
            _ => out.push(c),
        }
    }
    out
}

/// ` name="value"` for each attribute that has a value.
fn attrs(values: &[(&str, Option<&str>)]) -> String {
    values
        .iter()
        .filter_map(|(name, value)| value.map(|value| format!(" {name}=\"{}\"", esc(value))))
        .collect()
}

/// The colors that carry meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Accent,
    Red,
    Orange,
    Amber,
    Green,
    Blue,
    Violet,
    Muted,
}

impl Tone {
    fn name(self) -> &'static str {
        match self {
            Tone::Neutral => "neutral",
            Tone::Accent => "accent",
            Tone::Red => "red",
            Tone::Orange => "orange",
            Tone::Amber => "amber",
            Tone::Green => "green",
            Tone::Blue => "blue",
            Tone::Violet => "violet",
            Tone::Muted => "muted",
        }
    }
}

/// The icons a page links to.
#[derive(Clone, Copy, Debug)]
pub struct Icons {
    pub svg: &'static str,
    pub png: &'static str,
    pub apple: &'static str,
}

/// What the feed tells the script about the page, so it can follow changes.
#[derive(Clone, Debug, Default)]
pub struct Feed {
    pub seq: String,
    /// The tickets whose changes affect this page, when it is not about all of them.
    pub tickets: Option<Vec<String>>,
}

/// A breadcrumb: a label and where it goes, or `None` for the current page.
pub type Crumb = (String, Option<String>);

/// Everything the page frame needs.
#[derive(Clone, Debug, Default)]
pub struct PageOptions {
    pub title: String,
    pub body: String,
    /// The project home, which the navigation is relative to.
    pub root: String,
    /// Where the fingerprinted assets live, when it is not `root`.
    pub asset_root: Option<String>,
    pub assets: Option<Assets>,
    pub icons: Option<Icons>,
    pub crumbs: Vec<Crumb>,
    pub brand: String,
    /// HTML shown at the left of the header in place of `brand`, from the hub's configuration.
    pub wordmark: Option<String>,
    pub description: String,
    pub base: Option<String>,
    pub feed: Option<Feed>,
}

/// The header and main regions, which the script swaps when it navigates or follows a change.
pub fn page_regions(options: &PageOptions) -> String {
    let trail: String = options
        .crumbs
        .iter()
        .map(|(label, href)| match href {
            None => format!(
                "<span class=\"c-crumb\" aria-current=\"page\">{}</span>",
                esc(label)
            ),
            Some(href) => format!(
                "<a class=\"c-crumb\" href=\"{}\">{}</a>",
                esc(href),
                esc(label)
            ),
        })
        .collect();
    let subscription = options.feed.as_ref().map_or(String::new(), |feed| {
        let tickets = feed.tickets.as_ref().map(|tickets| tickets.join(" "));
        attrs(&[
            ("data-feed-seq", Some(&feed.seq)),
            ("data-feed-tickets", tickets.as_deref()),
        ])
    });
    let root = if options.root.is_empty() {
        "./"
    } else {
        &options.root
    };
    format!(
        "<header class=\"c-header\" id=\"c-header\">\n<nav class=\"c-crumbs\" aria-label=\"Breadcrumbs\">\
         <a class=\"c-brand\" href=\"{}\">{}</a>{trail}</nav>\n</header>\n\
         <main class=\"c-main\" id=\"c-main\"{subscription}>\n{}\n</main>",
        esc(root),
        // The wordmark comes from a file in the repository, so it is trusted HTML.
        options
            .wordmark
            .as_deref()
            .map_or_else(|| esc(&options.brand), str::to_owned),
        options.body
    )
}

/// A whole document around `regions`.
pub fn document(options: &PageOptions, regions: &str) -> String {
    let asset_root = options.asset_root.as_deref().unwrap_or(&options.root);
    let assets = options.assets.clone().unwrap_or_else(Assets::current);
    let description = if options.description.is_empty() {
        &options.title
    } else {
        &options.description
    };
    let base = options.base.as_ref().map_or(String::new(), |base| {
        format!("<base href=\"{}\">", esc(base))
    });
    let icons = options.icons.map_or(String::new(), |icons| {
        let name = if options.brand.is_empty() {
            &options.title
        } else {
            &options.brand
        };
        format!(
            "\n<link rel=\"icon\" type=\"image/png\" sizes=\"32x32\" href=\"{}\">\n\
             <link rel=\"icon\" type=\"image/svg+xml\" sizes=\"any\" href=\"{}\">\n\
             <link rel=\"apple-touch-icon\" sizes=\"180x180\" href=\"{}\">\n\
             <meta name=\"apple-mobile-web-app-title\" content=\"{}\">",
            esc(icons.png),
            esc(icons.svg),
            esc(icons.apple),
            esc(name)
        )
    });
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n{base}\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1, viewport-fit=cover\">\n\
         <title>{}</title>\n<meta name=\"description\" content=\"{}\">\n\
         <meta name=\"cirrus-root\" content=\"{}\">{icons}\n\
         <link rel=\"stylesheet\" href=\"{}\">\n<script src=\"{}\" defer></script>\n</head>\n<body>\n\
         <div class=\"c-progress-bar\" aria-hidden=\"true\"></div>\n{regions}\n</body>\n</html>\n",
        esc(&options.title),
        esc(description),
        esc(&options.root),
        esc(&format!("{asset_root}{}", assets.css)),
        esc(&format!("{asset_root}{}", assets.js)),
    )
}

/// Wraps a body in the frame with no navigation.
pub fn page(title: &str, body: &str) -> String {
    let options = PageOptions {
        title: title.to_owned(),
        body: body.to_owned(),
        root: "/".to_owned(),
        ..PageOptions::default()
    };
    document(&options, &page_regions(&options))
}

pub fn title(text: &str, eyebrow: &str, sub: &str) -> String {
    let above = if eyebrow.is_empty() {
        String::new()
    } else {
        format!("<div class=\"c-eyebrow\">{eyebrow}</div>")
    };
    let below = if sub.is_empty() {
        String::new()
    } else {
        format!("<p class=\"c-subtitle\">{sub}</p>")
    };
    format!(
        "<div class=\"c-titleblock\">{above}<h1 class=\"c-title\">{}</h1>{below}</div>",
        esc(text)
    )
}

fn tip_attrs(tip: Option<&str>) -> String {
    // An empty tip is written but makes the element no tab stop.
    attrs(&[
        ("data-tip", tip),
        ("tabindex", tip.filter(|tip| !tip.is_empty()).map(|_| "0")),
    ])
}

pub fn chip(text: &str, tone: Tone, tip: Option<&str>) -> String {
    format!(
        "<span class=\"c-chip c-tone-{}\"{}>{}</span>",
        tone.name(),
        tip_attrs(tip),
        esc(text)
    )
}

pub fn dot(tone: Tone, label: &str, tip: Option<&str>) -> String {
    format!(
        "<span class=\"c-dot c-tone-{}\"{}>{}</span>",
        tone.name(),
        tip_attrs(tip),
        esc(label)
    )
}

pub fn mono(text: &str) -> String {
    format!("<span class=\"c-mono\">{}</span>", esc(text))
}

pub fn muted(body: &str) -> String {
    format!("<span class=\"c-muted\">{body}</span>")
}

/// Reveals a tooltip on hover, focus or tap.
pub fn tip(body: &str, text: &str) -> String {
    format!(
        "<span class=\"c-tip-target\" tabindex=\"0\"{}>{body}</span>",
        attrs(&[("data-tip", Some(text))])
    )
}

/// The script shows relative time; without it the date shows.
pub fn time(iso: Option<&str>, fallback: &str) -> String {
    let Some(iso) = iso.filter(|iso| !iso.is_empty()) else {
        return esc(fallback);
    };
    let stamp = esc(iso);
    let date: String = iso.chars().take(10).collect();
    format!(
        "<time class=\"c-time\" datetime=\"{stamp}\" tabindex=\"0\" data-tip=\"{stamp}\">{}</time>",
        esc(&date)
    )
}

/// A link. The hovercard names a template on the page.
pub fn link(href: &str, body: &str, hovercard: Option<&str>, class: &str) -> String {
    let class = (!class.is_empty()).then_some(class);
    format!(
        "<a href=\"{}\"{}>{body}</a>",
        esc(href),
        attrs(&[("class", class), ("data-hovercard", hovercard)])
    )
}

pub fn template(id: &str, body: &str) -> String {
    format!("<template id=\"{}\">{body}</template>", esc(id))
}

fn count_label(count: Option<usize>) -> String {
    count.map_or(String::new(), |count| {
        format!("<span class=\"c-count\">{count}</span>")
    })
}

/// A top-level division containing sections that use heading level 3.
pub fn part(heading: &str, body: &str, id: &str, count: Option<usize>) -> String {
    format!(
        "<section class=\"c-part\"{}><h2 class=\"c-part-title\">{}{}</h2>{body}</section>",
        attrs(&[("id", Some(id))]),
        esc(heading),
        count_label(count)
    )
}

pub fn section(
    heading: &str,
    body: &str,
    count: Option<usize>,
    id: Option<&str>,
    level: u8,
) -> String {
    let title = if heading.is_empty() {
        String::new()
    } else {
        format!(
            "<h{level} class=\"c-section-title\">{}{}</h{level}>",
            esc(heading),
            count_label(count)
        )
    };
    format!(
        "<section class=\"c-section\"{}>{title}{body}</section>",
        attrs(&[("id", id)])
    )
}

pub fn subsection(heading: &str, body: &str, count: Option<usize>, level: u8) -> String {
    format!(
        "<div class=\"c-subsection\"><h{level} class=\"c-subsection-title\">{}{}</h{level}>{body}</div>",
        esc(heading),
        count_label(count)
    )
}

pub fn grid<I: IntoIterator<Item = String>>(cards: I) -> String {
    format!(
        "<div class=\"c-grid\">{}</div>",
        cards.into_iter().collect::<String>()
    )
}

/// A whole-card link. The script prefetches the target as the card scrolls into view.
pub fn card(href: &str, body: &str, hovercard: Option<&str>) -> String {
    format!(
        "<a class=\"c-card\" href=\"{}\" data-prefetch{}>{body}</a>",
        esc(href),
        attrs(&[("data-hovercard", hovercard)])
    )
}

pub fn rows<I: IntoIterator<Item = String>>(items: I) -> String {
    let items: String = items
        .into_iter()
        .map(|item| format!("<li>{item}</li>"))
        .collect();
    format!("<ul class=\"c-rows\">{items}</ul>")
}

pub fn row(main: &str, trailing: &str, href: Option<&str>) -> String {
    let inner = format!(
        "<span class=\"c-row-main\">{main}</span><span class=\"c-row-trailing\">{trailing}</span>"
    );
    match href {
        None => format!("<div class=\"c-row\">{inner}</div>"),
        Some(href) => format!(
            "<a class=\"c-row\" href=\"{}\" data-prefetch>{inner}</a>",
            esc(href)
        ),
    }
}

/// Label and HTML value pairs for a property list.
pub fn meta(items: &[(String, String)]) -> String {
    let body: String = items
        .iter()
        .map(|(label, value)| {
            format!(
                "<div class=\"c-meta-item\"><dt>{}</dt><dd>{value}</dd></div>",
                esc(label)
            )
        })
        .collect();
    format!("<dl class=\"c-meta\">{body}</dl>")
}

/// A bar of `done` in `total`. The share rounds exact halves to the even integer, as the
/// Python original did.
pub fn progress(done: u64, total: u64, tip: Option<&str>) -> String {
    let percentage = if total == 0 {
        0.0
    } else {
        100.0 * done as f64 / total as f64
    };
    let lower = percentage.floor();
    let half = ((percentage - lower) - 0.5).abs() < f64::EPSILON;
    let share = if half && lower % 2.0 == 0.0 {
        lower
    } else {
        percentage.round()
    };
    format!(
        "<span class=\"c-progress\"{}><span class=\"c-progress-track\"><span class=\"c-progress-fill\" \
         style=\"width:{share}%\"></span></span><span class=\"c-progress-label\">{done}/{total}</span></span>",
        tip_attrs(tip)
    )
}

pub fn disclosure(summary: &str, body: &str, open: bool) -> String {
    let open = if open { " open" } else { "" };
    format!(
        "<details class=\"c-disclosure\"{open}><summary>{summary}</summary>\
         <div class=\"c-disclosure-body\">{body}</div></details>"
    )
}

pub fn skeleton(lines: usize) -> String {
    const WIDTHS: [u32; 6] = [92, 78, 85, 64, 88, 70];
    let bars: String = (0..lines)
        .map(|index| {
            format!(
                "<span class=\"c-skeleton-line\" style=\"width:{}%\"></span>",
                WIDTHS[index % WIDTHS.len()]
            )
        })
        .collect();
    format!("<div class=\"c-skeleton\" aria-busy=\"true\" aria-label=\"Loading\">{bars}</div>")
}

/// Fills a fragment after the page shows, with a skeleton in the meantime.
pub fn include(url: &str, missing: &str, lines: usize) -> String {
    format!(
        "<div class=\"c-include\"{}>{}</div>",
        attrs(&[("data-include", Some(url)), ("data-missing", Some(missing))]),
        skeleton(lines)
    )
}

pub fn empty(text: &str) -> String {
    format!("<p class=\"c-empty\">{}</p>", esc(text))
}

pub fn prose(html: &str) -> String {
    format!("<div class=\"c-prose\">{html}</div>")
}

/// A reading column with a property rail that moves above it on narrow screens.
pub fn layout(main: &str, aside: &str) -> String {
    format!(
        "<div class=\"c-layout\"><div class=\"c-layout-main\">{main}</div><aside class=\"c-layout-aside\">{aside}</aside></div>"
    )
}

pub fn timeline<I: IntoIterator<Item = String>>(entries: I) -> String {
    format!(
        "<ol class=\"c-timeline\">{}</ol>",
        entries.into_iter().collect::<String>()
    )
}

pub fn timeline_entry(body: &str, when: &str, who: &str, quiet: bool) -> String {
    let class = if quiet {
        "c-timeline-entry c-quiet"
    } else {
        "c-timeline-entry"
    };
    let actor = if who.is_empty() {
        String::new()
    } else {
        format!("<span class=\"c-timeline-actor\">{who}</span>")
    };
    format!(
        "<li class=\"{class}\"><div class=\"c-timeline-body\">{actor}{body}</div><div class=\"c-timeline-when\">{when}</div></li>"
    )
}

/// A comment. The first character of the full author name is the avatar; `shown` is HTML.
pub fn comment(author: &str, shown: &str, when: &str, body: &str) -> String {
    let initial: String = author
        .trim()
        .chars()
        .next()
        .map_or_else(|| "?".to_owned(), |c| c.to_uppercase().collect());
    format!(
        "<article class=\"c-comment\"><div class=\"c-avatar\" aria-hidden=\"true\">{}</div>\
         <div class=\"c-comment-main\"><header class=\"c-comment-head\"><span class=\"c-comment-author\">{shown}</span>{when}</header>\
         <div class=\"c-prose\">{body}</div></div></article>",
        esc(&initial)
    )
}

/// One line of a table of contents: label, anchor, count and depth (0 a part, 1 a section).
pub type TocEntry = (String, String, Option<usize>, u8);

pub fn toc(entries: &[TocEntry]) -> String {
    let items: String = entries
        .iter()
        .map(|(label, anchor, count, depth)| {
            let count = count.map_or(String::new(), |count| {
                format!("<span class=\"c-toc-count\">{count}</span>")
            });
            format!(
                "<li class=\"c-toc-item c-toc-depth-{depth}\"><a class=\"c-toc-link\" href=\"#{}\">\
                 <span class=\"c-toc-label\">{}</span>{count}</a></li>",
                esc(anchor),
                esc(label)
            )
        })
        .collect();
    format!("<nav class=\"c-toc\" aria-label=\"On this page\"><ul>{items}</ul></nav>")
}

/// The sticky contents sit beside the page, or above it on a narrow screen.
pub fn with_toc(nav: &str, body: &str) -> String {
    format!(
        "<div class=\"c-toc-layout\"><div class=\"c-toc-rail\">{nav}</div><div class=\"c-toc-main\">{body}</div></div>"
    )
}
