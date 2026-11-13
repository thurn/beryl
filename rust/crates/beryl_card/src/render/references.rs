//! What the expressions of an effect say: which players ([`Players`]), which objects
//! ([`Objects`]), how many ([`Amount`]) and whether ([`Condition`]).

use super::filters::{Kind, zone_name};
use super::words::{counted, list, number, plural};
use super::{Attached, Ctx};
use crate::cost::{CastingKind, CostAction};
use crate::effect::{Amount, Comparison, Condition, Objects, Players};
use crate::filter::Filter;
use crate::ids::Zone;
use crate::index::SpecIx;
use crate::target::TargetKind;
use crate::trigger::TriggerEvent;

impl Ctx {
    /// A player as the subject or object of a clause: "you", "each opponent", "target
    /// player" the first time and "that player" after, "its controller".
    pub(super) fn player(&mut self, players: Players) -> String {
        match players {
            Players::You => "you".to_owned(),
            Players::EachOpponent => "each opponent".to_owned(),
            Players::EachPlayer => "each player".to_owned(),
            Players::Target(n) => self.target(n).unwrap_or_else(|| "that player".to_owned()),
            Players::ControllerOf(objects) => {
                format!("{} controller", self.objects_possessive(objects))
            }
            Players::OwnerOf(objects) => format!("{} owner", self.objects_possessive(objects)),
            Players::ThatPlayer => match self.group {
                Some(group) => self.player(group),
                None => "that player".to_owned(),
            },
            Players::EachOther(players) => match *players {
                Players::You => "each other player".to_owned(),
                other => format!("each player other than {}", self.player(other)),
            },
        }
    }

    /// Whose things they are: "your", "their" in a clause about the same player, "target
    /// player's", "its owner's".
    pub(super) fn possessive(&mut self, players: Players) -> String {
        if self.subject == Some(players) {
            return if players == Players::You {
                "your".to_owned()
            } else {
                "their".to_owned()
            };
        }
        match players {
            Players::You => "your".to_owned(),
            Players::ThatPlayer if self.group.is_some() => "their".to_owned(),
            other => format!("{}'s", self.player(other)),
        }
    }

    /// The `n`th target: its phrase the first time it is said, `None` after.
    fn target(&mut self, n: SpecIx) -> Option<String> {
        let spec = *self.targets.get(n.index())?;
        let said = self.mentioned.get_mut(n.index())?;
        if *said {
            return None;
        }
        *said = true;
        Some(self.target_phrase(spec))
    }

    /// Whether the `n`th target may be several objects or players.
    fn target_is_plural(&self, n: SpecIx) -> bool {
        self.targets
            .get(n.index())
            .is_some_and(|spec| spec.count > 1)
    }

    /// Objects as the object of a verb: "target creature" the first time and "it" after,
    /// "this creature", "all creatures", "the cards exiled with this artifact".
    pub(super) fn objects(&mut self, objects: Objects) -> String {
        match objects {
            Objects::This => self.this_word(),
            Objects::Target(n) => self.target(n).unwrap_or_else(|| self.pronoun(n)),
            Objects::AttachedTo => self.attached_to(),
            Objects::Those => if self.plural { "them" } else { "it" }.to_owned(),
            Objects::TriggeredOn => self.triggered_on(),
            Objects::ExiledWithThis => format!("the cards exiled with {}", self.this),
            Objects::All { zone, filter } => {
                let kind = Kind::of_zone(zone);
                let noun = self.noun(filter, kind);
                let place = match (zone, mentions_owner(filter)) {
                    (Zone::Battlefield | Zone::Stack, _) | (_, true) => String::new(),
                    (zone, false) => format!(" in all {}", plural(zone_name(zone))),
                };
                format!("all {}{place}", noun.plural(kind))
            }
            Objects::InZoneOf {
                zone,
                players,
                filter,
            } => {
                let owner = self.possessive(*players);
                if filter == Filter::Any {
                    format!("{owner} {}", zone_name(zone))
                } else {
                    let noun = self.noun(filter, Kind::Card(None));
                    format!(
                        "{} in {owner} {}",
                        noun.plural(Kind::Card(None)),
                        zone_name(zone)
                    )
                }
            }
            Objects::CardsIn { players, zones } => {
                let owner = self.possessive(*players);
                let names: Vec<String> = zones.iter().map(|&z| zone_name(z).to_owned()).collect();
                format!("{owner} {}", list(&names, "and"))
            }
            Objects::Paid(action) => paid(action).to_owned(),
        }
    }

    /// One of the objects, with its article: "a card exiled with this artifact", "a nonland
    /// card in that player's hand".
    pub(super) fn one_of(&mut self, objects: Objects) -> String {
        match objects {
            Objects::ExiledWithThis => format!("a card exiled with {}", self.this),
            Objects::All { zone, filter } => {
                let kind = Kind::of_zone(zone);
                self.noun(filter, kind).indefinite(kind)
            }
            Objects::InZoneOf {
                zone,
                players,
                filter,
            } => {
                let owner = self.possessive(*players);
                let noun = self.noun(filter, Kind::Card(None));
                format!(
                    "{} in {owner} {}",
                    noun.indefinite(Kind::Card(None)),
                    zone_name(zone)
                )
            }
            Objects::CardsIn { players, zones } => {
                let owner = self.possessive(*players);
                let names: Vec<String> = zones.iter().map(|&z| zone_name(z).to_owned()).collect();
                format!("a card in {owner} {}", list(&names, "or"))
            }
            Objects::This
            | Objects::Target(_)
            | Objects::AttachedTo
            | Objects::Those
            | Objects::TriggeredOn
            | Objects::Paid(_) => self.objects(objects),
        }
    }

    /// Objects as they are counted: "creatures you control", "cards in your graveyard".
    pub(super) fn counted_objects(&mut self, objects: Objects) -> String {
        match objects {
            Objects::All { zone, filter } => {
                let kind = Kind::of_zone(zone);
                self.noun(filter, kind).plural(kind)
            }
            Objects::InZoneOf {
                zone,
                players,
                filter,
            } => {
                let owner = self.possessive(*players);
                let noun = self.noun(filter, Kind::Card(None));
                format!(
                    "{} in {owner} {}",
                    noun.plural(Kind::Card(None)),
                    zone_name(zone)
                )
            }
            other => self.objects(other),
        }
    }

    /// Whose they are: "its", "this creature's", "target creature's".
    pub(super) fn objects_possessive(&mut self, objects: Objects) -> String {
        match objects {
            Objects::Those => if self.plural { "their" } else { "its" }.to_owned(),
            Objects::TriggeredOn | Objects::Paid(_) => "its".to_owned(),
            Objects::This if self.this_is_it => "its".to_owned(),
            Objects::Target(n) => match self.target(n) {
                Some(phrase) => format!("{phrase}'s"),
                None if self.target_is_plural(n) => "their".to_owned(),
                None => "its".to_owned(),
            },
            other => format!("{}'s", self.objects(other)),
        }
    }

    /// Whether the objects may be several, so a verb about them is plural.
    pub(super) fn is_plural(&self, objects: Objects) -> bool {
        match objects {
            Objects::Target(n) => self.target_is_plural(n),
            Objects::Those => self.plural,
            Objects::All { .. }
            | Objects::InZoneOf { .. }
            | Objects::CardsIn { .. }
            | Objects::ExiledWithThis => true,
            Objects::This | Objects::AttachedTo | Objects::TriggeredOn | Objects::Paid(_) => false,
        }
    }

    /// The object the ability is on, or "it" in its own trigger's condition.
    fn this_word(&self) -> String {
        if self.this_is_it {
            "it".to_owned()
        } else {
            self.this.clone()
        }
    }

    /// A target said before: "it", or "them" for several, and "that player" for a player.
    fn pronoun(&self, n: SpecIx) -> String {
        match self.targets.get(n.index()) {
            Some(spec) if spec.kind == TargetKind::Player => "that player".to_owned(),
            Some(spec) if spec.count > 1 => "them".to_owned(),
            _ => "it".to_owned(),
        }
    }

    /// "Enchanted creature", "equipped creature"
    /// ([CR 303.4b](https://yawgatog.com/resources/magic-rules/#R3034b)): "The object or
    /// player an Aura is attached to is called enchanted."
    fn attached_to(&self) -> String {
        match self.attached {
            Attached::Aura { creature: true } | Attached::Equipment => match self.attached {
                Attached::Equipment => "equipped creature",
                _ => "enchanted creature",
            },
            Attached::Aura { creature: false } => "enchanted permanent",
            Attached::Neither => "the permanent it's attached to",
        }
        .to_owned()
    }

    /// What the trigger event was about: "that card", "that spell", "that creature"
    /// ([CR 603.2](https://yawgatog.com/resources/magic-rules/#R6032)).
    fn triggered_on(&self) -> String {
        let Some(trigger) = self.trigger else {
            return "it".to_owned();
        };
        match trigger.event {
            TriggerEvent::Discards | TriggerEvent::Draws => "that card",
            TriggerEvent::Casts => "that spell",
            TriggerEvent::CreatesTokens => "those tokens",
            TriggerEvent::Enters
            | TriggerEvent::Dies
            | TriggerEvent::Attacks
            | TriggerEvent::PlayerAttacks
            | TriggerEvent::DealsCombatDamageToPlayer => "that creature",
            TriggerEvent::Step(_) | TriggerEvent::MainPhase => "it",
        }
        .to_owned()
    }

    /// A number as Oracle text says it after "equal to": "3", "X", "the number of creatures
    /// you control", "that spell's mana value".
    pub(super) fn amount(&mut self, amount: Amount) -> String {
        match amount {
            Amount::Fixed(n) => n.to_string(),
            Amount::X => "X".to_owned(),
            Amount::ThatMany => "that many".to_owned(),
            Amount::Carried => "that amount".to_owned(),
            Amount::ThoseOf(players) => {
                let who = self.player(players);
                format!("the number of them {who} controlled")
            }
            Amount::Count(objects) => format!("the number of {}", self.counted_objects(*objects)),
            Amount::Times(n, amount) => format!("{n} times {}", self.amount(*amount)),
            Amount::Power(objects) => format!("{} power", self.objects_possessive(objects)),
            Amount::Toughness(objects) => {
                format!("{} toughness", self.objects_possessive(objects))
            }
            Amount::ManaValue(objects) => {
                format!("{} mana value", self.objects_possessive(objects))
            }
            Amount::Counters(objects, kind) => format!(
                "the number of {} counters on {}",
                kind.name(),
                self.objects(objects)
            ),
            Amount::Life(players) => format!("{} life total", self.possessive(players)),
            Amount::Most(players, amount) => {
                let who = self.player(players);
                format!("the greatest {} among {who}", self.amount(*amount))
            }
        }
    }

    /// A number of things: "a card", "two cards", "X cards", "that many cards", "a card for
    /// each creature you control".
    pub(super) fn count_of(&mut self, amount: Amount, singular: &str, plural: &str) -> String {
        match amount {
            Amount::Fixed(n) => counted(n, singular, plural),
            Amount::X => format!("X {plural}"),
            Amount::ThatMany => format!("that many {plural}"),
            Amount::ThoseOf(players) => {
                let who = match players {
                    Players::ThatPlayer => "they".to_owned(),
                    other if self.subject == Some(other) => "they".to_owned(),
                    other => self.player(other),
                };
                format!(
                    "{} for each of those permanents {who} controlled",
                    super::words::with_article(singular)
                )
            }
            Amount::Count(objects) => format!(
                "{} for each {}",
                super::words::with_article(singular),
                self.one_of_counted(*objects)
            ),
            other => format!("a number of {plural} equal to {}", self.amount(other)),
        }
    }

    /// An amount of something measured: "3 life", "X damage", "that much life", "2 damage
    /// for each card in your hand", "damage equal to its power".
    pub(super) fn quantity(&mut self, amount: Amount, unit: &str) -> String {
        match amount {
            Amount::Fixed(n) => format!("{n} {unit}"),
            Amount::X => format!("X {unit}"),
            Amount::ThatMany | Amount::Carried => format!("that much {unit}"),
            Amount::Count(objects) => {
                format!("1 {unit} for each {}", self.one_of_counted(*objects))
            }
            Amount::Times(n, Amount::Count(objects)) => {
                format!("{n} {unit} for each {}", self.one_of_counted(**objects))
            }
            other => format!("{unit} equal to {}", self.amount(other)),
        }
    }

    /// One of the objects counted, without its article: "creature you control".
    fn one_of_counted(&mut self, objects: Objects) -> String {
        match objects {
            Objects::All { zone, filter } => {
                let kind = Kind::of_zone(zone);
                self.noun(filter, kind).singular(kind)
            }
            Objects::InZoneOf {
                zone,
                players,
                filter,
            } => {
                let owner = self.possessive(*players);
                let noun = self.noun(filter, Kind::Card(None));
                format!(
                    "{} in {owner} {}",
                    noun.singular(Kind::Card(None)),
                    zone_name(zone)
                )
            }
            other => self.objects(other),
        }
    }

    /// A condition, as it follows "if" or "as long as": "you do", "it was kicked", "an
    /// opponent controls more lands than you", "there are five or more cards in your
    /// graveyard".
    pub(super) fn condition(&mut self, condition: Condition) -> String {
        match condition {
            Condition::Did => "you do".to_owned(),
            Condition::Paid(kind) => self.paid_condition(kind, true),
            Condition::Compare(left, comparison, right) => self.compare(left, comparison, right),
            Condition::Is(objects, filter) => {
                let what = self.objects(objects);
                let noun = self
                    .noun(filter, Kind::Card(None))
                    .indefinite(Kind::Card(None));
                format!("{} {noun}", be(&what, self.is_plural(objects), true))
            }
            Condition::EnteredThisTurn(objects) => {
                format!("{} entered this turn", self.objects(objects))
            }
            Condition::Token(objects) => {
                let what = self.objects(objects);
                format!("{} a token", be(&what, false, true))
            }
            Condition::WonFlip => "you win the flip".to_owned(),
            Condition::Not(inner) => self.negated(*inner),
            Condition::All(conditions) => {
                let each: Vec<String> = conditions.iter().map(|&c| self.condition(c)).collect();
                list(&each, "and")
            }
            Condition::Any(conditions) => {
                let each: Vec<String> = conditions.iter().map(|&c| self.condition(c)).collect();
                list(&each, "or")
            }
        }
    }

    /// "If it isn't a token", "if you lose the flip", "if you don't".
    fn negated(&mut self, condition: Condition) -> String {
        match condition {
            Condition::Did => "you don't".to_owned(),
            Condition::Paid(kind) => self.paid_condition(kind, false),
            Condition::Is(objects, filter) => {
                let what = self.objects(objects);
                let noun = self
                    .noun(filter, Kind::Card(None))
                    .indefinite(Kind::Card(None));
                format!("{} {noun}", be(&what, self.is_plural(objects), false))
            }
            Condition::EnteredThisTurn(objects) => {
                format!("{} didn't enter this turn", self.objects(objects))
            }
            Condition::Token(objects) => {
                let what = self.objects(objects);
                format!("{} a token", be(&what, false, false))
            }
            Condition::WonFlip => "you lose the flip".to_owned(),
            Condition::Not(inner) => self.condition(*inner),
            Condition::Compare(..) | Condition::All(_) | Condition::Any(_) => {
                format!("it's not true that {}", self.condition(condition))
            }
        }
    }

    /// "It was kicked", "its impending cost was paid" (CR 702.33d).
    fn paid_condition(&mut self, kind: CastingKind, paid: bool) -> String {
        let it = if self.this_is_it || self.trigger.is_some() {
            "it".to_owned()
        } else {
            self.this.clone()
        };
        let name = match kind {
            CastingKind::Kicker => {
                return format!("{it} {} kicked", if paid { "was" } else { "wasn't" });
            }
            CastingKind::Offspring => "offspring",
            CastingKind::Impending => "impending",
            CastingKind::Overload => "overload",
            CastingKind::Evoke => "evoke",
        };
        let whose = if it == "it" {
            "its".to_owned()
        } else {
            format!("{it}'s")
        };
        format!(
            "{whose} {name} cost {} paid",
            if paid { "was" } else { "wasn't" }
        )
    }

    /// One amount held against another, in the words a card uses for the common shapes.
    fn compare(&mut self, left: Amount, comparison: Comparison, right: Amount) -> String {
        // "An opponent controls more lands than you": the most any of them has against
        // yours.
        if let Amount::Most(players, inner) = left
            && *inner == right
            && let Some((verb, noun)) = self.holding(right)
        {
            let who = match players {
                Players::EachOpponent => "an opponent".to_owned(),
                Players::EachPlayer => "a player".to_owned(),
                other => self.player(other),
            };
            let than = match comparison {
                Comparison::MoreThan | Comparison::AtLeast => "more",
                Comparison::LessThan | Comparison::AtMost => "fewer",
                Comparison::Equal => "as many",
            };
            let word = if comparison == Comparison::Equal {
                "as"
            } else {
                "than"
            };
            let verb = if verb == "have" { "has" } else { "controls" };
            return format!("{who} {verb} {than} {noun} {word} you");
        }
        if let Amount::Fixed(n) = right {
            let many = how_many(comparison, n);
            match left {
                Amount::Counters(objects, kind) => {
                    let on = self.objects(objects);
                    return if n == 0 && matches!(comparison, Comparison::Equal | Comparison::AtMost)
                    {
                        format!("there are no {} counters on {on}", kind.name())
                    } else {
                        let counters = if n == 1 && comparison == Comparison::AtLeast {
                            format!("a {} counter", kind.name())
                        } else {
                            format!("{many} {} counters", kind.name())
                        };
                        format!("{on} has {counters} on it")
                    };
                }
                Amount::Count(objects) => {
                    if let Some((players, zone)) = whole_zone(*objects)
                        && n <= 1
                        && how_many(comparison, n) == "no"
                    {
                        let owner = self.possessive(players);
                        return format!("{owner} {} has no cards in it", zone_name(zone));
                    }
                    if let Some((verb, noun)) = self.holding(left) {
                        return format!("you {verb} {many} {noun}");
                    }
                    let things = self.counted_objects(*objects);
                    return format!("there are {many} {things}");
                }
                Amount::Life(Players::You) => return format!("you have {many} life"),
                _ => {}
            }
        }
        let left = self.amount(left);
        let right = self.amount(right);
        let relation = match comparison {
            Comparison::AtLeast => "at least",
            Comparison::AtMost => "at most",
            Comparison::Equal => "equal to",
            Comparison::MoreThan => "greater than",
            Comparison::LessThan => "less than",
        };
        format!("{left} is {relation} {right}")
    }

    /// What a player has, as "[player] [controls] [lands]" says it, for an amount about
    /// you: the permanents you control, your life, the cards in your hand.
    fn holding(&mut self, amount: Amount) -> Option<(&'static str, String)> {
        match amount {
            Amount::Count(Objects::All {
                zone: Zone::Battlefield,
                filter,
            }) => {
                let rest = without_you(*filter)?;
                Some((
                    "control",
                    self.noun(rest, Kind::Permanent).plural(Kind::Permanent),
                ))
            }
            Amount::Count(Objects::InZoneOf {
                zone,
                players: Players::You,
                filter,
            }) => {
                let noun = self
                    .noun(*filter, Kind::Card(None))
                    .plural(Kind::Card(None));
                Some(("have", format!("{noun} in {}", zone_name(*zone))))
            }
            Amount::Count(Objects::All {
                zone: Zone::Hand,
                filter,
            }) => {
                let rest = without_you(*filter)?;
                let noun = self.noun(rest, Kind::Card(None)).plural(Kind::Card(None));
                Some(("have", format!("{noun} in hand")))
            }
            Amount::Life(Players::You) => Some(("have", "life".to_owned())),
            _ => None,
        }
    }
}

/// "Is", "isn't", "are", "aren't" after `what`, with "it is" as "it's".
fn be(what: &str, plural: bool, positive: bool) -> String {
    match (what, plural, positive) {
        ("it", _, true) => "it's".to_owned(),
        ("it", _, false) => "it isn't".to_owned(),
        (_, true, true) => format!("{what} are"),
        (_, true, false) => format!("{what} aren't"),
        (_, false, true) => format!("{what} is"),
        (_, false, false) => format!("{what} isn't"),
    }
}

/// "Five or more", "two or fewer", "exactly three", "no".
fn how_many(comparison: Comparison, n: u8) -> String {
    let word = number(n);
    match comparison {
        Comparison::LessThan if n == 1 => "no".to_owned(),
        Comparison::AtLeast => format!("{word} or more"),
        Comparison::AtMost | Comparison::Equal if n == 0 => "no".to_owned(),
        Comparison::AtMost => format!("{word} or fewer"),
        Comparison::Equal => format!("exactly {word}"),
        Comparison::MoreThan => format!("more than {word}"),
        Comparison::LessThan => format!("fewer than {word}"),
    }
}

/// The player and zone of objects that are a whole zone of one player's: "your library".
fn whole_zone(objects: Objects) -> Option<(Players, Zone)> {
    match objects {
        Objects::InZoneOf {
            zone,
            players,
            filter: Filter::Any,
        }
        | Objects::All {
            zone: zone @ (Zone::Library | Zone::Hand | Zone::Graveyard | Zone::Exile),
            filter: Filter::Whose(players),
        } => Some((*players, zone)),
        _ => None,
    }
}

/// The filter without its "you control", when it has one.
fn without_you(filter: Filter) -> Option<Filter> {
    match filter {
        Filter::Whose(Players::You) => Some(Filter::Any),
        Filter::All([first, Filter::Whose(Players::You)]) => Some(*first),
        Filter::All([Filter::Whose(Players::You), second]) => Some(*second),
        _ => None,
    }
}

/// Whether the filter says whose zone a card is in, so the zone is said with it.
fn mentions_owner(filter: Filter) -> bool {
    match filter {
        Filter::Whose(_) => true,
        Filter::All(filters) => filters.iter().any(|&f| mentions_owner(f)),
        _ => false,
    }
}

/// What a cost did it to, such as "the sacrificed permanent" or "the discarded card"
/// ([CR 608.2k](https://yawgatog.com/resources/magic-rules/#R6082k)).
fn paid(action: CostAction) -> &'static str {
    match action {
        CostAction::Tap => "the tapped permanent",
        CostAction::Sacrifice => "the sacrificed permanent",
        CostAction::Discard => "the discarded card",
        CostAction::Exile => "the exiled card",
        CostAction::Reveal => "the revealed card",
        CostAction::RemoveCounter(_) => "it",
    }
}
