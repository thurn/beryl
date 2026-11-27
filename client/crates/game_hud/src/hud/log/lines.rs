//! The lines of the game log, ported from the web interface's `logLines` (`ui/src/game/log.ts`)
//! with its function names in snake case and its words exactly: what a view's events say
//! happened, as short sentences naming cards in full ("Opponent cast <card> targeting <card>",
//! "<card> was countered"). The routine (steps beginning, the draw step's draw, tapping for
//! mana) is left out, and each turn opens with a heading.

use std::collections::BTreeMap;

use beryl_protocol::protocol::{
    Counter, GameEvent, LossReason, ObjectId, ObjectView, OutcomeView, PlayerState, PlayerView,
    Side, StackItem, Status, Step, TargetRef, Why, ZoneName,
};

/// A turn's heading, something countered, or anything else that happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogKind {
    Turn,
    Counter,
    Action,
}

/// One line of the log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogLine {
    pub kind: LogKind,
    /// The player it is about, for its marker.
    pub side: Side,
    pub text: String,
    /// The public result of a library choice, also shown near the player.
    pub library_choice: Option<String>,
    pub scry: bool,
}

/// The names of the cards seen so far, by slot: a card that has gone where the viewer can't see
/// it (a library, the opponent's hand) is still named by what it was.
pub type Names = BTreeMap<ObjectId, String>;

/// Adds to `names` the cards `view` shows.
pub fn learn_names(names: &mut Names, view: &PlayerView) {
    for object in &view.objects {
        // A name already known is kept, so that a view of a large board copies only the names
        // it changes. An empty name (a face-down copy's) names nothing, as the web's
        // `if (o.name)` has it, so the card is still "a card".
        if let Some(name) = &object.name
            && !name.is_empty()
            && names.get(&object.slot) != Some(name)
        {
            names.insert(object.slot, name.clone());
        }
    }
}

const fn who(side: Side) -> &'static str {
    match side {
        Side::You => "You",
        Side::Opponent => "Opponent",
    }
}

/// A player as the object of a sentence.
const fn whom(side: Side) -> &'static str {
    match side {
        Side::You => "you",
        Side::Opponent => "Opponent",
    }
}

const fn whose(side: Side) -> &'static str {
    match side {
        Side::You => "your",
        Side::Opponent => "Opponent's",
    }
}

fn player(view: &PlayerView, side: Side) -> &PlayerState {
    match side {
        Side::You => &view.players.you,
        Side::Opponent => &view.players.opponent,
    }
}

/// "A", "A and B", "A, B and C".
#[must_use]
pub fn listed(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

const NUMBERS: [&str; 8] = [
    "Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven",
];

/// A kind's name in the rules: "time", "first strike", "+1/+2" (`PlusOnePlusTwo`). The web
/// reads its camel-case wire name; the variant's name is the same with a capital first letter.
#[must_use]
pub fn counter_name(kind: Counter) -> String {
    match kind {
        Counter::PlusOne => return "+1/+1".to_owned(),
        Counter::MinusOne => return "\u{2212}1/\u{2212}1".to_owned(),
        _ => {}
    }
    let variant = format!("{kind:?}");
    // The words of the name, split before each capital.
    let mut words: Vec<String> = Vec::new();
    for c in variant.chars() {
        if c.is_ascii_uppercase() || words.is_empty() {
            words.push(String::new());
        }
        if let Some(word) = words.last_mut() {
            word.push(c);
        }
    }
    // A power and toughness kind: "Plus", a number, "Plus" or "Minus", a number.
    if let [sign, power, sign_again, toughness] = words.as_slice()
        && [sign, sign_again]
            .iter()
            .all(|s| *s == "Plus" || *s == "Minus")
        && let (Some(p), Some(t)) = (
            NUMBERS.iter().position(|n| n == power),
            NUMBERS.iter().position(|n| n == toughness),
        )
    {
        let part =
            |sign: &str, n: usize| format!("{}{n}", if sign == "Plus" { "+" } else { "\u{2212}" });
        return format!("{}/{}", part(sign, p), part(sign_again, t));
    }
    let lower: Vec<String> = words.iter().map(|w| w.to_ascii_lowercase()).collect();
    lower.join(" ")
}

/// How a player lost, after "lost the game".
fn loss(reason: LossReason, life: i32) -> String {
    match reason {
        LossReason::Life => format!(" at {life} life"),
        LossReason::Library => " drawing from an empty library".to_owned(),
        LossReason::Effect => " to a card's effect".to_owned(),
        LossReason::Concede => " by conceding".to_owned(),
    }
}

fn counters(n: u8, kind: Counter) -> String {
    let count = if n == 1 {
        "a".to_owned()
    } else {
        n.to_string()
    };
    let plural = if n == 1 { "" } else { "s" };
    format!("{count} {} counter{plural}", counter_name(kind))
}

/// A loyalty ability's cost as its card prints it: "+1", "0", "−3".
fn loyalty_cost(n: i8) -> String {
    match n {
        n if n > 0 => format!("+{n}"),
        n if n < 0 => format!("\u{2212}{}", -i16::from(n)),
        _ => "0".to_owned(),
    }
}

/// The indices of `events`' loyalty counters paid to activate an ability, which the
/// activation's line says: the last such change to its source before it.
fn loyalty_paid(events: &[GameEvent]) -> Vec<usize> {
    let mut paid = Vec::new();
    for (i, event) in events.iter().enumerate() {
        let GameEvent::Ability {
            item:
                StackItem::Ability {
                    source: Some(source),
                    ..
                },
            loyalty: Some(loyalty),
        } = event
        else {
            continue;
        };
        if *loyalty == 0 {
            continue;
        }
        let amount = loyalty.unsigned_abs();
        let found = events[..i].iter().rposition(|change| match change {
            GameEvent::CountersAdded {
                slot,
                counter: Counter::Loyalty,
                amount: n,
            } if *loyalty > 0 => slot == source && *n == amount,
            GameEvent::CountersRemoved {
                slot,
                counter: Counter::Loyalty,
                amount: n,
            } if *loyalty < 0 => slot == source && *n == amount,
            _ => false,
        });
        paid.extend(found);
    }
    paid
}

/// What `view`'s events say happened, oldest first, as the web's `logLines`. `names` names the
/// cards it no longer shows (`learn_names` of the views before, and of this one).
#[must_use]
pub fn log_lines(view: &PlayerView, names: &Names) -> Vec<LogLine> {
    let mut lines = Lines {
        view,
        names,
        out: Vec::new(),
    };
    let paid = loyalty_paid(&view.events);
    for (i, event) in view.events.iter().enumerate() {
        if !paid.contains(&i) {
            lines.line(event);
        }
    }
    lines.out
}

/// The lines of one view as they are made.
struct Lines<'a> {
    view: &'a PlayerView,
    names: &'a Names,
    out: Vec<LogLine>,
}

impl Lines<'_> {
    fn object(&self, slot: ObjectId) -> Option<&ObjectView> {
        self.view.objects.iter().find(|object| object.slot == slot)
    }

    fn name(&self, slot: ObjectId) -> String {
        (self.names.get(&slot)).map_or_else(|| "a card".to_owned(), Clone::clone)
    }

    fn named(&self, slot: ObjectId) -> bool {
        self.names.contains_key(&slot)
    }

    fn owner(&self, slot: ObjectId) -> Side {
        self.object(slot)
            .map_or(Side::Opponent, |object| object.owner)
    }

    fn controller(&self, slot: ObjectId) -> Side {
        self.object(slot)
            .map_or_else(|| self.owner(slot), |object| object.controller)
    }

    /// An ability on the stack is named by the face it is printed on.
    fn ability(&self, place: u16) -> String {
        match self.view.stack.get(usize::from(place)) {
            Some(StackItem::Ability { name, .. }) => format!("{name}'s ability"),
            _ => "an ability".to_owned(),
        }
    }

    fn target(&self, target: &TargetRef) -> String {
        match target {
            TargetRef::Player { player } => whom(*player).to_owned(),
            TargetRef::Object { slot } => self.name(*slot),
            TargetRef::Ability { place } => self.ability(*place),
        }
    }

    fn targets(&self, targets: &[TargetRef]) -> String {
        let targets: Vec<String> = targets.iter().map(|t| self.target(t)).collect();
        if targets.is_empty() {
            String::new()
        } else {
            format!(" targeting {}", listed(&targets))
        }
    }

    fn say(&mut self, side: Side, text: String) {
        self.say_as(side, text, LogKind::Action);
    }

    fn say_as(&mut self, side: Side, text: String, kind: LogKind) {
        self.out.push(LogLine {
            kind,
            side,
            text,
            library_choice: None,
            scry: false,
        });
    }

    fn line(&mut self, event: &GameEvent) {
        let view = self.view;
        match event {
            GameEvent::StepBegins { step, active } => {
                if *step == Step::Untap {
                    let text = format!("Turn {}: {} turn", view.turn.own_number, whose(*active));
                    self.say_as(*active, text, LogKind::Turn);
                }
            }
            GameEvent::Cast {
                slot, controller, ..
            } => {
                let spell = view.stack.iter().find_map(|item| match item {
                    StackItem::Spell {
                        slot: on, targets, ..
                    } if on == slot => Some(targets.as_slice()),
                    _ => None,
                });
                let aimed = self.targets(spell.unwrap_or_default());
                let text = format!("{} cast {}{aimed}", who(*controller), self.name(*slot));
                self.say(*controller, text);
            }
            GameEvent::ZoneChange {
                slot,
                from,
                to,
                reason,
                library_top,
            } => self.zone_change(*slot, (*from, *to), *reason, *library_top),
            GameEvent::Scry {
                player,
                top,
                bottom,
            } => self.out.push(LogLine {
                kind: LogKind::Action,
                side: *player,
                text: format!("{} put {top} on top and {bottom} on bottom", who(*player)),
                library_choice: Some(format!("Scry \u{b7} {top} top \u{b7} {bottom} bottom")),
                scry: true,
            }),
            GameEvent::Draw { player, cards, .. } => {
                // The draw step's draw goes without saying.
                if view.turn.step == Step::Draw && *player == view.turn.active {
                    return;
                }
                let drew = if *cards == 1 {
                    "a card".to_owned()
                } else {
                    format!("{cards} cards")
                };
                self.say(*player, format!("{} drew {drew}", who(*player)));
            }
            GameEvent::Damage {
                source, to, amount, ..
            } => {
                let text = format!(
                    "{} dealt {amount} damage to {}",
                    self.name(*source),
                    self.target(to)
                );
                self.say(self.controller(*source), text);
            }
            GameEvent::LifeGain { player, amount } => {
                self.say(*player, format!("{} gained {amount} life", who(*player)));
            }
            GameEvent::LifeLoss { player, amount } => {
                self.say(*player, format!("{} lost {amount} life", who(*player)));
            }
            GameEvent::CountersAdded {
                slot,
                counter,
                amount,
            } => {
                let text = format!("{} got {}", self.name(*slot), counters(*amount, *counter));
                self.say(self.controller(*slot), text);
            }
            GameEvent::CountersRemoved {
                slot,
                counter,
                amount,
            } => {
                let text = format!("{} lost {}", self.name(*slot), counters(*amount, *counter));
                self.say(self.controller(*slot), text);
            }
            _ => self.later_line(event),
        }
    }

    /// The lines of the events `line` leaves to it: tokens, abilities, attacks, control, coins
    /// and the game's end.
    fn later_line(&mut self, event: &GameEvent) {
        let view = self.view;
        match event {
            GameEvent::TokensCreated { controller, tokens } => self.created(*controller, tokens),
            GameEvent::Ability { item, loyalty } => self.activated(item, *loyalty),
            GameEvent::Attack { player, attackers } => {
                let names: Vec<String> = attackers.iter().map(|&slot| self.name(slot)).collect();
                let text = format!("{} attacked with {}", who(*player), listed(&names));
                self.say(*player, text);
            }
            GameEvent::GainControl { slot, player } => {
                let text = format!("{} gained control of {}", who(*player), self.name(*slot));
                self.say(*player, text);
            }
            GameEvent::CoinFlip {
                player,
                won,
                source,
            } => {
                let result = if *won { "won" } else { "lost" };
                let text = format!(
                    "{} {result} {}'s coin flip",
                    who(*player),
                    self.name(*source)
                );
                self.say(*player, text);
            }
            // A word, color, type or card name a player chose, so that a player who missed the
            // announcement can still read what was chosen: a text change's two words come as
            // the word replaced, then its replacement.
            GameEvent::ValueChosen { player, value } => {
                self.say(*player, format!("{} chose {value}", who(*player)));
            }
            GameEvent::Lose { player } => {
                let how = match &view.status {
                    Status::Over {
                        outcome: OutcomeView::Win { reason, .. },
                    } => loss(*reason, player_life(view, *player)),
                    _ => String::new(),
                };
                self.say(*player, format!("{} lost the game{how}", who(*player)));
            }
            GameEvent::Win { player } => {
                self.say(*player, format!("{} won the game", who(*player)));
            }
            // The web's switch says nothing of the rest.
            _ => {}
        }
    }

    /// The line of tokens `controller` created, by kind: "You created a Treasure Token".
    fn created(&mut self, controller: Side, tokens: &[ObjectId]) {
        let mut kinds: Vec<(String, usize)> = Vec::new();
        for &token in tokens {
            let name = self.name(token);
            match kinds.iter_mut().find(|(n, _)| *n == name) {
                Some((_, count)) => *count += 1,
                None => kinds.push((name, 1)),
            }
        }
        let made: Vec<String> = (kinds.iter())
            .map(|(n, count)| {
                if *count == 1 {
                    format!("a {n}")
                } else {
                    format!("{count} {n}s")
                }
            })
            .collect();
        let text = format!("{} created {}", who(controller), listed(&made));
        self.say(controller, text);
    }

    /// The line of an ability activated, or a triggered one put on the stack.
    fn activated(&mut self, item: &StackItem, loyalty: Option<i8>) {
        let StackItem::Ability {
            name,
            triggered,
            targets,
            controller,
            ..
        } = item
        else {
            return;
        };
        let aimed = self.targets(targets);
        if *triggered {
            self.say(*controller, format!("{name} triggered{aimed}"));
            return;
        }
        let cost = loyalty.map_or_else(String::new, |n| format!(" {}", loyalty_cost(n)));
        let text = format!("{} activated {name}{cost}{aimed}", who(*controller));
        self.say(*controller, text);
    }

    fn zone_change(
        &mut self,
        slot: ObjectId,
        (from, to): (ZoneName, ZoneName),
        reason: Why,
        library_top: Option<bool>,
    ) {
        let card = self.name(slot);
        let side = self.owner(slot);
        match reason {
            // Said by the cast, the draw and the tokens' creation.
            Why::Cast | Why::Draw | Why::Created => return,
            Why::Play => return self.say(side, format!("{} played {card}", who(side))),
            Why::Resolve => return self.say(self.controller(slot), format!("{card} resolved")),
            Why::Counter => {
                return self.say_as(side, format!("{card} was countered"), LogKind::Counter);
            }
            Why::Destroy => return self.say(side, format!("{card} was destroyed")),
            Why::Sacrifice => return self.say(side, format!("{} sacrificed {card}", who(side))),
            Why::Discard => return self.say(side, format!("{} discarded {card}", who(side))),
            Why::StateBased | Why::Exile | Why::Return | Why::Effect => {}
        }
        // Otherwise, by where it went. A card the viewer never saw goes unsaid.
        if !self.named(slot) || from == to {
            return;
        }
        match to {
            ZoneName::Hand => {
                let text = if from == ZoneName::Battlefield {
                    format!("{card} returned to {} hand", whose(side))
                } else {
                    format!("{card} was put into {} hand", whose(side))
                };
                self.say(side, text);
            }
            ZoneName::Battlefield => {
                self.say(
                    self.controller(slot),
                    format!("{card} entered the battlefield"),
                );
            }
            ZoneName::Graveyard => {
                let text = if from == ZoneName::Battlefield {
                    format!("{card} went to the graveyard")
                } else {
                    format!("{card} was put into {} graveyard", whose(side))
                };
                self.say(side, text);
            }
            ZoneName::Exile => self.say(side, format!("{card} was exiled")),
            ZoneName::Library => self.library_line(slot, from, side, library_top),
            ZoneName::Stack | ZoneName::Nowhere | ZoneName::Merged | ZoneName::Sideboard => {}
        }
    }

    fn library_line(&mut self, slot: ObjectId, from: ZoneName, side: Side, top: Option<bool>) {
        let card = self.name(slot);
        let library = &player(self.view, side).library;
        // Older recordings omit the choice. Infer it only when the ends differ.
        let inferred = if from == ZoneName::Stack && library.len() > 1 {
            if library.last() == Some(&slot) {
                Some(true)
            } else if library.first() == Some(&slot) {
                Some(false)
            } else {
                None
            }
        } else {
            None
        };
        match top.or(inferred) {
            None => self.say(side, format!("{card} was put into {} library", whose(side))),
            Some(top) => {
                let end = if top { "top" } else { "bottom" };
                let theirs = if side == Side::You { "your" } else { "their" };
                self.out.push(LogLine {
                    kind: LogKind::Action,
                    side,
                    text: format!("{} put {card} on the {end} of {theirs} library", who(side)),
                    library_choice: Some(
                        (if top {
                            "Top of Library"
                        } else {
                            "Bottom of Library"
                        })
                        .to_owned(),
                    ),
                    scry: false,
                });
            }
        }
    }
}

fn player_life(view: &PlayerView, side: Side) -> i32 {
    player(view, side).life
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chosen_words_are_logged() {
        let mut view = beryl_protocol::testing::ViewBuilder::new().build();
        view.events = ["Island", "Plains"]
            .map(|value| GameEvent::ValueChosen {
                player: Side::Opponent,
                value: value.to_owned(),
            })
            .to_vec();
        let text: Vec<String> = log_lines(&view, &Names::new())
            .into_iter()
            .map(|line| line.text)
            .collect();
        assert_eq!(text, ["Opponent chose Island", "Opponent chose Plains"]);
    }
}
