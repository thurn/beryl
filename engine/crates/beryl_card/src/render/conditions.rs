//! Templates for conditions, including the phrases that compare two amounts.

use super::Ctx;
use super::filters::{Kind, zone_name};
use super::words::{list, number};
use crate::effect::{Amount, Comparison, Condition, Objects, Players};
use crate::filter::Filter;
use crate::ids::Zone;
use crate::target::TargetKind;

impl Ctx {
    /// A condition, as it follows "if" or "as long as": "you do", "it was kicked", "an
    /// opponent controls more lands than you", "there are five or more cards in your
    /// graveyard".
    pub(super) fn condition(&mut self, condition: Condition) -> String {
        match condition {
            Condition::Did => "you do".to_owned(),
            Condition::Paid(kind) => self.paid_condition(kind, true),
            Condition::Compare(left, comparison, right) => self.compare(left, comparison, right),
            // A permanent target said before is "that artifact" in a condition about it:
            // "If that artifact is an Equipment". A card is "it": "If it's a land card".
            Condition::Is(objects, filter) => {
                let permanent = match objects {
                    Objects::Target(n) => self
                        .targets
                        .get(n.index())
                        .copied()
                        .filter(|spec| spec.kind == TargetKind::Permanent),
                    Objects::This
                    | Objects::AttachedTo
                    | Objects::Those
                    | Objects::TriggeredOn
                    | Objects::TriggeredOnIn(_)
                    | Objects::ExiledWithThis
                    | Objects::LibraryTop { .. }
                    | Objects::ExceptThose(_)
                    | Objects::PutOntoBattlefieldWithThis
                    | Objects::Carried
                    | Objects::All { .. }
                    | Objects::InZoneOf { .. }
                    | Objects::CardsIn { .. }
                    | Objects::Everywhere(_)
                    | Objects::Paid(_)
                    | Objects::Other(_) => None,
                };
                let (what, kind) = match permanent {
                    Some(spec) => {
                        let head = self.noun(kind_of(spec.filter), Kind::Permanent);
                        (
                            format!("that {}", head.singular(Kind::Permanent)),
                            Kind::Permanent,
                        )
                    }
                    None => (self.objects(objects), Kind::Card(None)),
                };
                let noun = self.noun(filter, kind);
                let noun = noun
                    .adjectives_only()
                    .unwrap_or_else(|| noun.indefinite(kind));
                format!("{} {noun}", be(&what, self.is_plural(objects), true))
            }
            Condition::EnteredThisTurn(objects) => {
                format!("{} entered this turn", self.objects(objects))
            }
            Condition::OnBattlefield(objects) => {
                let what = self.condition_subject(objects);
                format!(
                    "{} on the battlefield",
                    be(&what, self.is_plural(objects), true)
                )
            }
            Condition::Token(objects) => {
                let what = self.objects(objects);
                format!("{} a token", be(&what, false, true))
            }
            Condition::WonFlip => "you win the flip".to_owned(),
            Condition::DrewInDrawStep(players) => format!(
                "{} drew a card in their draw step this turn",
                self.player(players)
            ),
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

    /// What a condition about whether objects are somewhere is about: "it" for the object the
    /// ability is on, as the trigger has named it already ("When this Aura enters, if it's on
    /// the battlefield").
    fn condition_subject(&mut self, objects: Objects) -> String {
        match objects {
            Objects::This => "it".to_owned(),
            objects @ (Objects::Target(_)
            | Objects::AttachedTo
            | Objects::Those
            | Objects::TriggeredOn
            | Objects::TriggeredOnIn(_)
            | Objects::ExiledWithThis
            | Objects::LibraryTop { .. }
            | Objects::ExceptThose(_)
            | Objects::PutOntoBattlefieldWithThis
            | Objects::Carried
            | Objects::All { .. }
            | Objects::InZoneOf { .. }
            | Objects::CardsIn { .. }
            | Objects::Everywhere(_)
            | Objects::Paid(_)
            | Objects::Other(_)) => self.objects(objects),
        }
    }

    /// "If it isn't a token", "if you lose the flip", "if you don't".
    fn negated(&mut self, condition: Condition) -> String {
        match condition {
            Condition::Did => "you don't".to_owned(),
            Condition::Paid(kind) => self.paid_condition(kind, false),
            Condition::Is(objects, filter) => {
                let what = self.objects(objects);
                let noun = self.noun(filter, Kind::Card(None));
                let noun =
                    (noun.adjectives_only()).unwrap_or_else(|| noun.indefinite(Kind::Card(None)));
                format!("{} {noun}", be(&what, self.is_plural(objects), false))
            }
            Condition::EnteredThisTurn(objects) => {
                format!("{} didn't enter this turn", self.objects(objects))
            }
            Condition::OnBattlefield(objects) => {
                let what = self.condition_subject(objects);
                format!(
                    "{} on the battlefield",
                    be(&what, self.is_plural(objects), false)
                )
            }
            Condition::Token(objects) => {
                let what = self.objects(objects);
                format!("{} a token", be(&what, false, false))
            }
            Condition::WonFlip => "you lose the flip".to_owned(),
            Condition::DrewInDrawStep(players) => format!(
                "{} didn't draw a card in their draw step this turn",
                self.player(players)
            ),
            Condition::Not(inner) => self.condition(*inner),
            Condition::Compare(..) | Condition::All(_) | Condition::Any(_) => {
                format!("it's not true that {}", self.condition(condition))
            }
        }
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
                other @ (Players::You
                | Players::Target(_)
                | Players::ControllerOf(_)
                | Players::OwnerOf(_)
                | Players::Enchanted
                | Players::ThatPlayer
                | Players::SubgameNonwinners
                | Players::EachOther(_)) => self.player(other),
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
                        let be = if n == 1 && comparison == Comparison::AtLeast {
                            "is"
                        } else {
                            "are"
                        };
                        format!("there {be} {counters} on {on}")
                    };
                }
                Amount::Count(objects) => {
                    if let Some((players, zone)) = whole_zone(*objects)
                        && n <= 1
                        && how_many(comparison, n) == "no"
                    {
                        if players == Players::You && zone == Zone::Hand {
                            return "you have no cards in hand".to_owned();
                        }
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
                Amount::Fixed(_)
                | Amount::X
                | Amount::ThatMany
                | Amount::Carried
                | Amount::ThoseOf(_)
                | Amount::Times(..)
                | Amount::Half { .. }
                | Amount::Plus(..)
                | Amount::CardTypesAmong(_)
                | Amount::Power(_)
                | Amount::Toughness(_)
                | Amount::ManaValue(_)
                | Amount::Life(_)
                | Amount::Most(..)
                | Amount::SpellsCast(_)
                | Amount::CardsDrawn(_)
                | Amount::SpellsCastBefore => {}
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
            Amount::Fixed(_)
            | Amount::X
            | Amount::ThatMany
            | Amount::Carried
            | Amount::ThoseOf(_)
            | Amount::Count(_)
            | Amount::Times(..)
            | Amount::Half { .. }
            | Amount::Plus(..)
            | Amount::CardTypesAmong(_)
            | Amount::Power(_)
            | Amount::Toughness(_)
            | Amount::ManaValue(_)
            | Amount::Counters(..)
            | Amount::Life(_)
            | Amount::Most(..)
            | Amount::SpellsCast(_)
            | Amount::CardsDrawn(_)
            | Amount::SpellsCastBefore => None,
        }
    }
}

/// The part of a target's filter that names what kind of object it is, for "that artifact":
/// its card types, without "with mana value X or less" or who controls it.
fn kind_of(filter: Filter) -> Filter {
    filter.conjunction().map_or(filter, |parts| {
        parts
            .iter()
            .copied()
            .find(|part| matches!(part, Filter::Types(_)))
            .unwrap_or(Filter::Any)
    })
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
        Objects::This
        | Objects::Target(_)
        | Objects::AttachedTo
        | Objects::Those
        | Objects::TriggeredOn
        | Objects::TriggeredOnIn(_)
        | Objects::ExiledWithThis
        | Objects::LibraryTop { .. }
        | Objects::ExceptThose(_)
        | Objects::PutOntoBattlefieldWithThis
        | Objects::Carried
        | Objects::All { .. }
        | Objects::InZoneOf { .. }
        | Objects::CardsIn { .. }
        | Objects::Everywhere(_)
        | Objects::Paid(_)
        | Objects::Other(_) => None,
    }
}

/// The filter without its "you control", when it has one.
fn without_you(filter: Filter) -> Option<Filter> {
    match filter {
        Filter::Whose(Players::You) => Some(Filter::Any),
        Filter::All([first, Filter::Whose(Players::You)]) => Some(*first),
        Filter::All([Filter::Whose(Players::You), second]) => Some(*second),
        Filter::Any
        | Filter::Types(_)
        | Filter::Supertypes(_)
        | Filter::SubtypeWord(_)
        | Filter::ColorWords(_)
        | Filter::Whose(_)
        | Filter::Owned(_)
        | Filter::AttackedThisTurn
        | Filter::Tapped
        | Filter::Token
        | Filter::ManaValueAtMost(_)
        | Filter::PowerAtMost(_)
        | Filter::ManaValueAtMostX
        | Filter::Not(_)
        | Filter::All(_)
        | Filter::AnyOf(_)
        | Filter::InZone(_) => None,
    }
}
