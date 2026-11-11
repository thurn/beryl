//! A moment as text: played headless to its first live decision from each seat ([`reach`]),
//! with the views shown on the way, for `specs-describe`.

use beryl_session::Status;

use super::{Moment, Phase, View, auto, chosen, start_guarded};
use crate::boards;

/// Plays `moment` headless, seen from `view`, to the viewer's first live decision (or the
/// game's end, once every action is taken), and returns what the viewer sees there: an error
/// says what failed on the way.
pub fn reach(moment: &'static Moment, view: View) -> Result<beryl_session::PlayerView, Unreached> {
    trace(moment, view).map(|(view, _)| view)
}

/// Why a moment's game did not reach the viewer's live decision.
#[derive(Clone, Debug)]
pub enum Unreached {
    /// The session's thread panicked: a debug build's script checks (plan K24) or anything
    /// else that broke. The message says where ([`crate::panics`]).
    Panicked(String),
    /// The game went another way: the moment's actions did not fit, or it ended first.
    Failed(String),
}

impl std::fmt::Display for Unreached {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unreached::Panicked(why) | Unreached::Failed(why) => f.write_str(why),
        }
    }
}

impl From<Unreached> for String {
    fn from(unreached: Unreached) -> String {
        unreached.to_string()
    }
}

/// [`reach`], with the views the viewer was shown once the setup was done, each with the
/// events it animates and its script: the first, shown at once, and the last, [`reach`]'s.
pub(super) fn trace(
    moment: &'static Moment,
    view: View,
) -> Result<(beryl_session::PlayerView, Vec<beryl_session::PlayerView>), Unreached> {
    let (sent, received) = std::sync::mpsc::channel();
    let told = sent.clone();
    // The session's thread owns the guard, which tells of a panic there, however early.
    let guard = crate::panics::Guard::new(move |panic| drop(told.send(Err(panic))));
    let (_sessions, progress) = start_guarded(moment, view, Some(guard), move |update, phase| {
        let _ = sent.send(Ok((update, phase)));
    })
    .map_err(Unreached::Failed)?;
    let mut shown = false;
    let mut views = Vec::new();
    loop {
        let (update, phase) = received
            .recv_timeout(std::time::Duration::from_secs(30))
            .map_err(|_| Unreached::Failed("no live decision within 30 s".to_owned()))?
            .map_err(Unreached::Panicked)?;
        if let Some(failure) = boards::lock(&progress).failure.take() {
            return Err(Unreached::Failed(failure));
        }
        shown |= phase != Phase::Setup;
        if shown {
            views.push(update.view.clone());
        }
        match update.view.status {
            // A game won or lost once shown is a moment too: its end.
            Status::Over { .. } if shown && boards::lock(&progress).done() => {
                return Ok((update.view, views));
            }
            Status::Over { .. } | Status::Stopped => {
                return Err(Unreached::Failed(match boards::lock(&progress).stalled() {
                    None => "the game ended before a live decision".to_owned(),
                    Some(why) => format!("the game ended with {why}"),
                }));
            }
            Status::Deciding { .. } if phase == Phase::Live => {
                if !shown {
                    return Err(Unreached::Failed("nothing was shown".to_owned()));
                }
                return Ok((update.view, views));
            }
            _ => {}
        }
    }
}

/// A view's events in a line, each its kind and fields, objects by name: "turn 5 your
/// postcombatMain: zoneChange Island exile->graveyard (effect); tokensCreated you [Treasure]".
/// None if it has none but steps beginning, which every view of a turn passing has.
pub(super) fn events_line(view: &beryl_session::PlayerView) -> Option<String> {
    let name = |slot: &serde_json::Value| {
        slot.as_u64()
            .and_then(|s| view.objects.iter().find(|o| u64::from(o.slot) == s))
            .and_then(|o| o.name.clone())
            .unwrap_or_else(|| format!("#{slot}"))
    };
    let text = |v: &serde_json::Value| match v {
        serde_json::Value::String(s) => s.clone(),
        v => v.to_string(),
    };
    let events: Vec<String> = view
        .events
        .iter()
        .filter(|e| !matches!(e, beryl_session::protocol::GameEvent::StepBegins { .. }))
        .map(|e| {
            let serde_json::Value::Object(fields) = serde_json::to_value(e).unwrap_or_default()
            else {
                return format!("{e:?}");
            };
            let mut parts = vec![fields.get("kind").map(text).unwrap_or_default()];
            if let (Some(from), Some(to)) = (fields.get("from"), fields.get("to"))
                && fields.contains_key("slot")
            {
                parts.push(format!(
                    "{} {}->{}",
                    name(&fields["slot"]),
                    text(from),
                    text(to)
                ));
                if let Some(why) = fields.get("reason") {
                    parts.push(format!("({})", text(why)));
                }
                return parts.join(" ");
            }
            for (k, v) in &fields {
                match k.as_str() {
                    "kind" => {}
                    "slot" | "source" => parts.push(name(v)),
                    // A stack item by its name: "Currency Converter ability 2".
                    "item" => {
                        parts.push(v.get("name").map(text).unwrap_or_default());
                        if let Some(k) = v.get("ability") {
                            parts.push(format!("ability {}", text(k)));
                        }
                    }
                    "loyalty" if v.is_null() => {}
                    "tokens" | "attackers" => {
                        let names: Vec<String> =
                            v.as_array().into_iter().flatten().map(name).collect();
                        parts.push(format!("[{}]", names.join(", ")));
                    }
                    _ => parts.push(format!("{k}={}", text(v))),
                }
            }
            parts.join(" ")
        })
        .collect();
    (!events.is_empty()).then(|| format!("{}: {}", turn_line(view), events.join("; ")))
}

/// A view's turn and step as a line begins with them: "turn 5 your postcombatMain".
pub(super) fn turn_line(view: &beryl_session::PlayerView) -> String {
    let turn = &view.turn;
    let whose = if turn.active == beryl_session::protocol::Side::You {
        "your"
    } else {
        "their"
    };
    let step = match serde_json::to_value(turn.step) {
        Ok(serde_json::Value::String(step)) => step,
        _ => String::new(),
    };
    format!("turn {} {whose} {step}", turn.number)
}

/// One view as `specs-describe --views` prints it: its turn and events, what the viewer is
/// asked (or the game's status), the stack bottom first, and each side's battlefield, hand,
/// graveyard and exile by name (a card the viewer cannot see as `?`, tapped ones marked), and
/// their library's size.
fn view_lines(view: &beryl_session::PlayerView) -> Vec<String> {
    let v = serde_json::to_value(view).unwrap_or_default();
    let objects = v["objects"].as_array().cloned().unwrap_or_default();
    let named = |slot: &serde_json::Value| {
        objects
            .iter()
            .find(|o| o["slot"] == *slot)
            .and_then(|o| o["name"].as_str())
            .unwrap_or("?")
            .to_owned()
    };
    let mut lines = vec![events_line(view).unwrap_or_else(|| {
        format!(
            "turn {} {:?} {:?}: no events",
            view.turn.number, view.turn.active, view.turn.step
        )
    })];
    lines.push(match &v["status"] {
        s if s["kind"] == "deciding" => format!(
            "asks   {} ({} candidates)",
            s["decision"]["kind"].as_str().unwrap_or("?"),
            s["decision"]["candidates"].as_array().map_or(0, Vec::len)
        ),
        s => format!("status {}", s["kind"].as_str().unwrap_or("?")),
    });
    let stack: Vec<String> = v["stack"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| match item["kind"].as_str() {
            Some("spell") => named(&item["slot"]),
            _ => item["name"].as_str().map_or_else(
                || format!("{} ability", named(&item["source"])),
                str::to_owned,
            ),
        })
        .collect();
    if !stack.is_empty() {
        lines.push(format!("stack  {}", stack.join(" < ")));
    }
    for side in ["you", "opponent"] {
        let mut zones = Vec::new();
        for zone in ["battlefield", "hand", "graveyard", "exile"] {
            // A permanent is on its controller's side; a card elsewhere, its owner's.
            let whose = if zone == "battlefield" {
                "controller"
            } else {
                "owner"
            };
            let cards: Vec<String> = objects
                .iter()
                .filter(|o| o["zone"] == zone && o[whose] == side)
                .map(|o| {
                    let name = o["name"].as_str().unwrap_or("?");
                    // A permanent's status, power and toughness, counters and keywords, as
                    // the view gives them: what its frame shows beyond its card.
                    let mut marks = Vec::new();
                    if o["tapped"] == true {
                        marks.push("tapped".to_owned());
                    }
                    if let (Some(p), Some(t)) =
                        (o["pt"]["power"].as_i64(), o["pt"]["toughness"].as_i64())
                    {
                        marks.push(format!("{p}/{t}"));
                    }
                    for c in o["counters"].as_array().into_iter().flatten() {
                        if let (Some(kind), Some(n)) = (c["kind"].as_str(), c["count"].as_u64()) {
                            marks.push(format!("{n} {kind}"));
                        }
                    }
                    for k in o["keywords"].as_array().into_iter().flatten() {
                        marks.extend(k.as_str().map(str::to_owned));
                    }
                    if zone == "battlefield" && !marks.is_empty() {
                        format!("{name} ({})", marks.join(", "))
                    } else if o["tapped"] == true {
                        format!("{name} (tapped)")
                    } else {
                        name.to_owned()
                    }
                })
                .collect();
            if !cards.is_empty() {
                zones.push(format!("{zone}: {}", cards.join(", ")));
            }
        }
        let library = v["players"][side]["library"].as_array().map_or(0, Vec::len);
        zones.push(format!("library: {library}"));
        lines.push(format!("{side:<6} {}", zones.join(" | ")));
    }
    lines
}

/// Prints each moment of the specs for `cards` (by name or slug; all of them if none), or the
/// one `moment` names: its page, the actions it plays once shown, and from each seat the
/// events shown on the way and what holds at its first live decision, abilities by their cost.
/// With `views`, each view shown is printed whole instead: its events, what it asks, the stack
/// and each side's zones ([`view_lines`]).
pub fn describe(cards: &[String], moment: Option<&str>, views: bool) -> Result<(), String> {
    let mut found = false;
    for spec in chosen(cards)? {
        let def = spec.def;
        let labels = auto::ability_labels(def);
        // An ability by its cost rather than its index: "offers Currency Converter {T}".
        let label = |c: &str| {
            labels.iter().fold(c.to_owned(), |c, (k, l)| {
                c.replace(
                    &format!("{} ability {k}", spec.card),
                    &format!("{} {l}", spec.card),
                )
            })
        };
        for m in spec
            .moments()
            .iter()
            .filter(|m| moment.is_none_or(|n| n == m.name))
        {
            found = true;
            let card = &spec.slug;
            println!("== {card} / {}  \"{}\"", m.name, m.title);
            if !m.group.is_empty() {
                println!("scenario: {}", m.group);
            }
            println!("page: /specs/{card}/{}", m.name);
            for a in m.play {
                println!("plays: {a:?}");
            }
            for view in View::ALL {
                let (shown, views_shown) = trace(m, view)?;
                println!("{}:", view.name());
                // The first view shown animates nothing (`specs-serve` clears its events).
                for (n, v) in views_shown.iter().skip(1).enumerate() {
                    if views {
                        println!("  view {}", n + 1);
                        for line in view_lines(v) {
                            println!("    {}", label(&line));
                        }
                    } else if let Some(line) = events_line(v) {
                        println!("  shown  {}", label(&line));
                    }
                }
                let holds: Vec<String> = auto::snapshot(&shown, def)
                    .iter()
                    .map(|c| label(c))
                    .collect();
                println!("  holds  {}", holds.join(", "));
            }
            println!();
        }
    }
    if found {
        Ok(())
    } else {
        Err(format!(
            "no moment {cards:?} {}",
            moment.unwrap_or_default()
        ))
    }
}
