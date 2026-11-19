//! The efficiency section of a ticket's page: two tall strips, the time its sessions spent on
//! the left and the dollars on the right, each a column of coloured segments that show a card
//! when hovered. The data is [`super::spend::Spend`].
//!
//! The page holds only a placeholder ([`placeholder`]); the strips arrive as a fragment
//! (`?fragment=spend`) once the page has shown, so a large epic never slows the page itself.
//! The views (timeline, by kind and, for an epic, merged) are all in the fragment, and radio
//! buttons switch between them with no script.
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use std::collections::HashMap;

use serde_json::{Value, json};

use super::components::esc;
use super::references::{display_id, ticket_url};
use super::spend::{Band, Block, GROUPS, Item, Spend, Strip, label};

/// The smallest share of a strip whose segment prints its name beside it.
const LABEL_SHARE: f64 = 0.035;

/// The smallest share of a strip whose band prints its ticket beside it.
const BAND_LABEL_SHARE: f64 = 0.04;

/// The section's body on the page: a skeleton of the two strips that the fragment at `url`
/// replaces.
pub fn placeholder(url: &str) -> String {
    format!(
        "<div class=\"c-include c-spend-include\" data-include=\"{}\" \
         data-missing=\"The efficiency data could not be loaded.\">\
         <div class=\"c-spend-skeleton\" aria-busy=\"true\" aria-label=\"Loading\">\
         <span class=\"c-skeleton-line\"></span><span class=\"c-skeleton-line\"></span>\
         </div></div>",
        esc(url)
    )
}

/// A duration: `2h 05m`, `4m 12s`, `38s`.
pub fn duration(seconds: f64) -> String {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let whole = seconds.max(0.0).round() as u64;
    match whole {
        0..60 => format!("{whole}s"),
        60..600 => format!("{}m {:02}s", whole / 60, whole % 60),
        600..3600 => format!("{}m", whole / 60),
        _ => format!("{}h {:02}m", whole / 3600, whole % 3600 / 60),
    }
}

/// Dollars: `$123`, `$4.20`, `<$0.01`.
pub fn money(dollars: f64) -> String {
    if dollars >= 100.0 {
        format!("${dollars:.0}")
    } else if dollars >= 0.005 {
        format!("${dollars:.2}")
    } else if dollars > 0.0 {
        "<$0.01".to_owned()
    } else {
        "$0".to_owned()
    }
}

/// A count of tokens: `2.1M`, `340K`, `512`.
pub fn tokens(count: f64) -> String {
    if count >= 1e6 {
        format!("{:.1}M", count / 1e6)
    } else if count >= 1e3 {
        format!("{:.0}K", count / 1e3)
    } else {
        format!("{count:.0}")
    }
}

fn percent(share: f64) -> String {
    if share > 0.0 && share < 0.01 {
        "<1%".to_owned()
    } else {
        format!("{:.0}%", share * 100.0)
    }
}

fn group_label(group: &str) -> &str {
    GROUPS
        .iter()
        .find(|(name, _)| *name == group)
        .map_or(group, |(_, shown)| shown)
}

/// What every strip of one fragment shares: how to name tickets and whether to show commands.
struct Context<'a> {
    root: &'a str,
    private: bool,
    titles: HashMap<&'a str, &'a str>,
    epic: bool,
}

impl Context<'_> {
    fn ticket_line(&self, ticket: &str) -> Option<Value> {
        if !self.epic || ticket.is_empty() {
            return None;
        }
        let title = self.titles.get(ticket).copied().unwrap_or("");
        Some(json!(["Ticket", format!("{} {title}", display_id(ticket))]))
    }
}

/// The fragment: figures, the views, the legend and view switch, and a table. `private` shows
/// commands in the cards; `key` makes the radio buttons' name unique on the page.
pub fn fragment(spend: &Spend, root: &str, private: bool, key: &str) -> String {
    let mut ctx = Context {
        root,
        private,
        titles: spend
            .tickets
            .iter()
            .map(|(id, title)| (id.as_str(), title.as_str()))
            .collect(),
        epic: spend.tickets.len() > 1,
    };
    let name = format!(
        "spend-view-{}",
        key.replace(|c: char| !c.is_ascii_alphanumeric(), "-")
    );
    let mut views: Vec<(&str, &str, String)> = vec![
        (
            "timeline",
            "Timeline",
            pair(&spend.timeline, ctx.epic, &[], &mut ctx),
        ),
        (
            "kind",
            "By kind",
            pair(&spend.by_kind, ctx.epic, &spend.breaks, &mut ctx),
        ),
    ];
    if let Some(merged) = &spend.merged {
        views.push((
            "merged",
            "Merged",
            pair(merged, false, &spend.breaks, &mut ctx),
        ));
    }
    let radios: String = views
        .iter()
        .enumerate()
        .map(|(index, (value, _, _))| {
            format!(
                "<input class=\"c-spend-choice\" type=\"radio\" name=\"{name}\" id=\"{name}-{value}\" \
                 value=\"{value}\"{}>",
                if index == 0 { " checked" } else { "" }
            )
        })
        .collect();
    let bodies: String = views
        .iter()
        .map(|(value, _, body)| {
            format!("<div class=\"c-spend-view\" data-view=\"{value}\">{body}</div>")
        })
        .collect();
    let switch: String = views
        .iter()
        .map(|(value, shown, _)| format!("<label for=\"{name}-{value}\">{shown}</label>"))
        .collect();
    format!(
        "<div class=\"c-spend\">{}{radios}{bodies}<div class=\"c-spend-foot\">{}\
         <div class=\"c-segmented\" role=\"group\" aria-label=\"View\">{switch}</div></div>{}</div>",
        figures(spend),
        legend(spend),
        table(spend),
    )
}

fn figures(spend: &Spend) -> String {
    let totals = &spend.totals;
    let mut items = vec![
        ("Agent time", duration(totals.agent_s)),
        ("Dollars", money(totals.dollars)),
        ("Requests", totals.requests.to_string()),
        ("Sessions", totals.sessions.to_string()),
    ];
    if totals.person_s >= 1.0 {
        items.push(("Waited on a person", duration(totals.person_s)));
    }
    let body: String = items
        .iter()
        .map(|(name, value)| {
            format!(
                "<div class=\"c-spend-figure\"><span class=\"c-spend-figure-label\">{name}</span>\
                 <span class=\"c-spend-figure-value\">{}</span></div>",
                esc(value)
            )
        })
        .collect();
    format!("<div class=\"c-spend-figures\">{body}</div>")
}

/// The groups that appear anywhere, in the legend's fixed order.
fn legend(spend: &Spend) -> String {
    let mut present: Vec<&str> = Vec::new();
    let mut strips = vec![
        &spend.timeline.0,
        &spend.timeline.1,
        &spend.by_kind.0,
        &spend.by_kind.1,
    ];
    if let Some((time, dollars)) = &spend.merged {
        strips.extend([time, dollars]);
    }
    for strip in strips {
        for band in &strip.bands {
            for item in &band.items {
                let group = item.block().group.as_str();
                if !present.contains(&group) {
                    present.push(group);
                }
            }
        }
    }
    if !spend.breaks.is_empty() && !present.contains(&"person") {
        present.push("person");
    }
    let items: String = GROUPS
        .iter()
        .filter(|(group, _)| present.contains(group))
        .map(|(group, shown)| {
            format!("<li><span class=\"c-spend-swatch\" data-group=\"{group}\"></span>{shown}</li>")
        })
        .collect();
    format!("<ul class=\"c-spend-legend\" aria-label=\"Legend\">{items}</ul>")
}

/// The time and dollar strips side by side, with the breaks listed under them when the view
/// does not draw them.
fn pair(
    (time, dollars): &(Strip, Strip),
    bands: bool,
    breaks: &[(String, f64)],
    ctx: &mut Context,
) -> String {
    let left = strip("Time", &duration(time.total()), time, false, bands, ctx);
    let right = strip(
        "Dollars",
        &money(dollars.total()),
        dollars,
        true,
        bands,
        ctx,
    );
    let beside = if breaks.is_empty() {
        String::new()
    } else {
        let items: String = breaks
            .iter()
            .map(|(name, seconds)| {
                format!(
                    "<li><span class=\"c-spend-swatch c-spend-hatch\" data-group=\"person\"></span>{} \
                     <span class=\"c-spend-amount\">{}</span></li>",
                    esc(name),
                    duration(*seconds)
                )
            })
            .collect();
        format!("<div class=\"c-spend-beside\"><span>Not agent time</span><ul>{items}</ul></div>")
    };
    format!("<div class=\"c-spend-pair\">{left}{right}</div>{beside}")
}

fn strip(
    title: &str,
    total_text: &str,
    strip: &Strip,
    dollars: bool,
    bands: bool,
    ctx: &mut Context,
) -> String {
    let total = strip.total();
    let side = if dollars { "right" } else { "left" };
    let body: String = strip
        .bands
        .iter()
        .map(|band| band_html(band, total, dollars, bands, ctx))
        .collect();
    format!(
        "<div class=\"c-spend-strip\" data-side=\"{side}\"><div class=\"c-spend-strip-head\">\
         <span class=\"c-spend-strip-title\">{title}</span>\
         <span class=\"c-spend-strip-total\">{}</span></div>\
         <div class=\"c-spend-column\" role=\"list\" aria-label=\"{title}\">{body}</div></div>",
        esc(total_text)
    )
}

fn band_html(band: &Band, total: f64, dollars: bool, bands: bool, ctx: &mut Context) -> String {
    let weight = band.weight();
    let share = if total > 0.0 { weight / total } else { 0.0 };
    let breaks = band
        .items
        .iter()
        .filter(|item| matches!(item, Item::Break(_)))
        .count();
    let rows: String = band
        .items
        .iter()
        .map(|item| row(item, &band.ticket, weight, total, dollars, bands, ctx))
        .collect();
    let label = if bands && band.ticket.is_empty() && share >= BAND_LABEL_SHARE {
        format!(
            "<span class=\"c-spend-band-label\">More tickets <span class=\"c-spend-amount\">{}</span></span>",
            if dollars {
                money(weight)
            } else {
                duration(weight)
            }
        )
    } else if bands && share >= BAND_LABEL_SHARE {
        let amount = if dollars {
            money(weight)
        } else {
            duration(weight)
        };
        format!(
            "<a class=\"c-spend-band-label\" href=\"{}\"><span class=\"c-mono\">{}</span> \
             <span class=\"c-spend-amount\">{amount}</span></a>",
            esc(&ticket_url(ctx.root, &band.ticket)),
            esc(&display_id(&band.ticket))
        )
    } else {
        String::new()
    };
    format!(
        "<div class=\"c-spend-band\" style=\"flex:{:.4} 1 calc({breaks} * var(--c-spend-break))\">{label}{rows}</div>",
        share * 1000.0
    )
}

fn row(
    item: &Item,
    ticket: &str,
    band_weight: f64,
    total: f64,
    dollars: bool,
    bands: bool,
    ctx: &mut Context,
) -> String {
    let block = item.block();
    let is_break = matches!(item, Item::Break(_));
    let share = if total > 0.0 && !is_break {
        block.weight / total
    } else {
        0.0
    };
    let amount = if dollars {
        money(block.weight)
    } else {
        duration(block.weight)
    };
    let name = label(block);
    let card = esc(&card(block, ticket, share, dollars, is_break, ctx).to_string());
    let shown = is_break || (!bands && share >= LABEL_SHARE);
    let text = if shown {
        format!(
            "<span class=\"c-spend-name\">{}</span> <span class=\"c-spend-amount\">{amount}</span>",
            esc(&name)
        )
    } else {
        String::new()
    };
    let aria = format!("{name}, {amount}");
    let bar = if bands && !ticket.is_empty() {
        format!(
            "<a class=\"c-spend-bar{hatch}\" href=\"{}\" data-group=\"{}\" data-card=\"{card}\" \
             data-hover-side aria-label=\"{}\"></a>",
            esc(&ticket_url(ctx.root, ticket)),
            esc(&block.group),
            esc(&aria),
            hatch = if is_break { " c-spend-hatch" } else { "" }
        )
    } else {
        format!(
            "<span class=\"c-spend-bar{hatch}\" data-group=\"{}\" data-card=\"{card}\" \
             data-hover-side tabindex=\"-1\" aria-label=\"{}\"></span>",
            esc(&block.group),
            esc(&aria),
            hatch = if is_break { " c-spend-hatch" } else { "" }
        )
    };
    let style = if is_break {
        String::new()
    } else {
        let within = if band_weight > 0.0 {
            block.weight / band_weight
        } else {
            0.0
        };
        format!(" style=\"flex-grow:{:.5}\"", within * 1000.0)
    };
    format!(
        "<div class=\"c-spend-row{}\" role=\"listitem\"{style}><span class=\"c-spend-label\">{text}</span>{bar}</div>",
        if is_break { " c-spend-break" } else { "" }
    )
}

/// A segment's hovercard as the data that `cirrus.js` builds a data card from: a kicker with
/// the colour group, a title, a figure with its context, and rows of a label and a value. A
/// value is text, `{"time": <iso>}` for a moment shown relative to now, or `{"code": <text>}`.
fn card(
    block: &Block,
    ticket: &str,
    share: f64,
    dollars: bool,
    is_break: bool,
    ctx: &Context,
) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    if let Some(at) = block.at {
        rows.push(json!(["Started", {"time": at.to_string()}]));
    }
    if block.runs > 1 {
        rows.push(json!(["Runs", block.runs.to_string()]));
    }
    if dollars {
        rows.push(json!(["Requests", block.requests.to_string()]));
        if let Some(model) = &block.model {
            rows.push(json!(["Model", model]));
        }
        for (index, class) in ["Input", "Cache write", "Cache read", "Output"]
            .iter()
            .enumerate()
        {
            if block.tokens[index] > 0.0 {
                rows.push(json!([
                    class,
                    format!(
                        "{} · {} tokens",
                        money(block.class_dollars[index]),
                        tokens(block.tokens[index])
                    )
                ]));
            }
        }
    } else if block.calls > 0 {
        rows.push(json!(["Tool calls", block.calls.to_string()]));
    }
    if !block.mix.is_empty() {
        let total: f64 = block.weight.max(f64::MIN_POSITIVE);
        let mix: Vec<String> = block
            .mix
            .iter()
            .map(|(name, weight)| format!("{name} {}", percent(weight / total)))
            .collect();
        rows.push(json!(["Mostly", mix.join(", ")]));
    }
    if let Some(line) = ctx.ticket_line(ticket) {
        rows.push(line);
    }
    if ctx.private
        && let Some(command) = &block.command
    {
        rows.push(json!(["Command", {"code": command}]));
    }
    let figure = if dollars {
        money(block.weight)
    } else {
        duration(block.weight)
    };
    let context = if is_break {
        "not agent time".to_owned()
    } else {
        format!(
            "{} of {}",
            percent(share),
            if dollars { "dollars" } else { "agent time" }
        )
    };
    json!({
        "group": block.group,
        "kicker": group_label(&block.group),
        "title": label(block),
        "figure": figure,
        "context": context,
        "rows": rows,
    })
}

/// A table of the merged view (or the single ticket's kinds): each kind's time and dollars,
/// for reading the figures without hovering.
fn table(spend: &Spend) -> String {
    let (time, dollars) = spend.merged.as_ref().unwrap_or(&spend.by_kind);
    let mut rows: Vec<(String, String, f64, f64)> = Vec::new();
    let mut add = |block: &Block, seconds: f64, cost: f64| {
        let name = label(&Block {
            part: String::new(),
            ..block.clone()
        });
        match rows.iter_mut().find(|row| row.0 == name) {
            Some(row) => {
                row.2 += seconds;
                row.3 += cost;
            }
            None => rows.push((name, block.group.clone(), seconds, cost)),
        }
    };
    for band in &time.bands {
        for item in &band.items {
            add(item.block(), item.block().weight, 0.0);
        }
    }
    for band in &dollars.bands {
        for item in &band.items {
            add(item.block(), 0.0, item.block().weight);
        }
    }
    // Largest first by the two shares together, so a kind that cost much and took little time
    // ranks beside one that took much time.
    let (seconds_total, dollars_total) = (time.total().max(1e-9), dollars.total().max(1e-9));
    let rank = |row: &(String, String, f64, f64)| row.2 / seconds_total + row.3 / dollars_total;
    rows.sort_by(|left, right| rank(right).total_cmp(&rank(left)));
    let body: String = rows
        .iter()
        .map(|(name, group, seconds, cost)| {
            format!(
                "<tr><td><span class=\"c-spend-swatch\" data-group=\"{}\"></span>{}</td>\
                 <td>{}</td><td>{}</td></tr>",
                esc(group),
                esc(name),
                if *seconds > 0.0 {
                    duration(*seconds)
                } else {
                    String::new()
                },
                if *cost > 0.0 {
                    money(*cost)
                } else {
                    String::new()
                }
            )
        })
        .collect();
    format!(
        "<details class=\"c-disclosure c-spend-table\"><summary>Table</summary>\
         <div class=\"c-disclosure-body\"><table><thead><tr><th>Kind</th><th>Time</th>\
         <th>Dollars</th></tr></thead><tbody>{body}</tbody></table></div></details>"
    )
}
