//! What the expressions of an effect say: which players ([`Players`]), which objects
//! ([`Objects`]), how many ([`Amount`]).

use super::filters::{Kind, zone_name};
use super::words::{counted, list, plural};
use super::{Attached, Ctx};
use crate::cost::CostAction;
use crate::effect::{Amount, Objects, Players};
use crate::filter::Filter;
use crate::ids::Zone;
use crate::index::SpecIx;
use crate::target::TargetKind;
use crate::trigger::{Subject, TriggerEvent};

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
                other @ (Players::EachOpponent
                | Players::EachPlayer
                | Players::Target(_)
                | Players::ControllerOf(_)
                | Players::OwnerOf(_)
                | Players::ThatPlayer
                | Players::EachOther(_)) => {
                    format!("each player other than {}", self.player(other))
                }
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
            other @ (Players::EachOpponent
            | Players::EachPlayer
            | Players::Target(_)
            | Players::ControllerOf(_)
            | Players::OwnerOf(_)
            | Players::ThatPlayer
            | Players::EachOther(_)) => format!("{}'s", self.player(other)),
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
            Objects::This => {
                // "It" no longer means the discarded card once this object is named.
                self.those_named = self.those_named.or(self.discarded);
                self.this_word()
            }
            Objects::Target(n) => self.target(n).unwrap_or_else(|| self.pronoun(n)),
            Objects::AttachedTo => self.attached_to(),
            Objects::Those => match (self.those_named, self.plural) {
                (Some(true), _) => "the discarded cards",
                (Some(false), _) => "the discarded card",
                (None, true) => "them",
                (None, false) => "it",
            }
            .to_owned(),
            Objects::TriggeredOn => self.triggered_on(),
            Objects::TriggeredOnIn(zone) => {
                format!("{} from {}", self.triggered_on(), owners_zone(zone))
            }
            Objects::ExiledWithThis => format!("the cards exiled with {}", self.this),
            Objects::PutOntoBattlefieldWithThis => "that creature".to_owned(),
            Objects::Carried => "that card".to_owned(),
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
            // "Each nonland card in your graveyard": an effect on the cards of one zone.
            Objects::Everywhere(filter) if zone_of(filter).is_some() => {
                let card = Kind::Card(zone_of(filter));
                format!("each {}", self.noun(filter, card).singular(card))
            }
            Objects::Everywhere(filter) => {
                let card = Kind::Card(None);
                let cards = self.noun(filter, card).plural(card);
                let spells = self.noun(filter, Kind::Spell).plural(Kind::Spell);
                let permanents = self.noun(filter, Kind::Permanent).plural(Kind::Permanent);
                format!("all {cards} that aren't on the battlefield, {spells}, and {permanents}")
            }
            // "Each other creature": one at a time, so the verb after it is singular.
            Objects::Other(&Objects::All { zone, filter }) => {
                let kind = Kind::of_zone(zone);
                format!("each other {}", self.noun(filter, kind).singular(kind))
            }
            Objects::Other(objects) => {
                format!("{} other than {}", self.objects(*objects), self.this)
            }
        }
    }

    /// One of the objects, with its article: "a card exiled with this artifact", "a nonland
    /// card in that player's hand".
    pub(super) fn one_of(&mut self, objects: Objects) -> String {
        match objects {
            Objects::ExiledWithThis => format!("a card exiled with {}", self.this),
            Objects::PutOntoBattlefieldWithThis => "that creature".to_owned(),
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
            Objects::Other(&Objects::All { zone, filter }) => {
                let kind = Kind::of_zone(zone);
                format!("another {}", self.noun(filter, kind).singular(kind))
            }
            Objects::Other(_)
            | Objects::Everywhere(_)
            | Objects::This
            | Objects::Target(_)
            | Objects::AttachedTo
            | Objects::Those
            | Objects::TriggeredOn
            | Objects::TriggeredOnIn(_)
            | Objects::Carried
            | Objects::Paid(_) => self.objects(objects),
        }
    }

    /// Objects as they are counted: "creatures you control", "cards in your graveyard".
    pub(super) fn counted_objects(&mut self, objects: Objects) -> String {
        match objects {
            Objects::All { zone, filter } => {
                let kind = Kind::of_zone(zone);
                let noun = self.noun(filter, kind).plural(kind);
                // "Cards in all graveyards": a zone each player has one of, with no player
                // named by the filter.
                match zone {
                    Zone::Library | Zone::Hand | Zone::Graveyard if !mentions_owner(filter) => {
                        format!("{noun} in all {}s", zone_name(zone))
                    }
                    Zone::Library
                    | Zone::Hand
                    | Zone::Battlefield
                    | Zone::Graveyard
                    | Zone::Exile
                    | Zone::Stack
                    | Zone::Nowhere
                    | Zone::Merged => noun,
                }
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
            Objects::Other(&Objects::All { zone, filter }) => {
                let kind = Kind::of_zone(zone);
                format!("other {}", self.noun(filter, kind).plural(kind))
            }
            other @ (Objects::This
            | Objects::Target(_)
            | Objects::AttachedTo
            | Objects::Those
            | Objects::TriggeredOn
            | Objects::TriggeredOnIn(_)
            | Objects::ExiledWithThis
            | Objects::PutOntoBattlefieldWithThis
            | Objects::Carried
            | Objects::CardsIn { .. }
            | Objects::Everywhere(_)
            | Objects::Paid(_)
            | Objects::Other(_)) => self.objects(other),
        }
    }

    /// Whose they are: "its", "this creature's", "target creature's".
    pub(super) fn objects_possessive(&mut self, objects: Objects) -> String {
        match objects {
            Objects::Those => if self.plural { "their" } else { "its" }.to_owned(),
            Objects::TriggeredOn | Objects::TriggeredOnIn(_) | Objects::Paid(_) => "its".to_owned(),
            Objects::This if self.this_is_it => "its".to_owned(),
            Objects::Target(n) => match self.target(n) {
                Some(phrase) => format!("{phrase}'s"),
                None if self.target_is_plural(n) => "their".to_owned(),
                None => "its".to_owned(),
            },
            other @ (Objects::This
            | Objects::AttachedTo
            | Objects::ExiledWithThis
            | Objects::PutOntoBattlefieldWithThis
            | Objects::Carried
            | Objects::All { .. }
            | Objects::InZoneOf { .. }
            | Objects::CardsIn { .. }
            | Objects::Everywhere(_)
            | Objects::Other(_)) => format!("{}'s", self.objects(other)),
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
            // "Each nonland card in your graveyard has": one at a time.
            Objects::Everywhere(filter) => zone_of(filter).is_none(),
            // "Each other creature is": one at a time.
            Objects::PutOntoBattlefieldWithThis
            | Objects::This
            | Objects::AttachedTo
            | Objects::TriggeredOn
            | Objects::TriggeredOnIn(_)
            | Objects::Carried
            | Objects::Paid(_)
            | Objects::Other(_) => false,
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
            Attached::Aura { creature: true, .. } | Attached::Equipment => match self.attached {
                Attached::Equipment => "equipped creature",
                Attached::Aura { .. } | Attached::Neither => "enchanted creature",
            },
            Attached::Aura {
                creature: false, ..
            } => "enchanted permanent",
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
            // "When this creature becomes the target of a spell or ability, sacrifice it."
            TriggerEvent::BecomesTarget if matches!(trigger.subject, Subject::This) => "it",
            TriggerEvent::Discards | TriggerEvent::Draws => "that card",
            TriggerEvent::Casts => "that spell",
            TriggerEvent::CreatesTokens => "those tokens",
            TriggerEvent::Leaves | TriggerEvent::BecomesTarget => "that permanent",
            TriggerEvent::Enters
            | TriggerEvent::Dies
            | TriggerEvent::Attacks
            | TriggerEvent::Mutates
            | TriggerEvent::PlayerAttacks
            | TriggerEvent::DealsDamage
            | TriggerEvent::TurnedFaceUp
            | TriggerEvent::DealsCombatDamageToPlayer => "that creature",
            TriggerEvent::Step(_) | TriggerEvent::MainPhase | TriggerEvent::State(_) => "it",
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
            Amount::SpellsCastBefore => {
                "the number of other spells cast before it this turn".to_owned()
            }
            Amount::ThoseOf(players) => {
                let who = self.player(players);
                format!("the number of them {who} controlled")
            }
            Amount::Count(objects) => format!("the number of {}", self.counted_objects(*objects)),
            Amount::Times(n, amount) => format!("{n} times {}", self.amount(*amount)),
            Amount::Plus(n, amount) => format!("{} plus {n}", self.amount(*amount)),
            Amount::CardTypesAmong(objects) => format!(
                "the number of card types among {}",
                self.counted_objects(*objects)
            ),
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
            Amount::SpellsCast(players) => {
                format!(
                    "the number of spells {} cast this turn",
                    self.player(players)
                )
            }
            Amount::CardsDrawn(players) => {
                format!(
                    "the number of cards {} drew this turn",
                    self.player(players)
                )
            }
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
                    other @ (Players::You
                    | Players::EachOpponent
                    | Players::EachPlayer
                    | Players::Target(_)
                    | Players::ControllerOf(_)
                    | Players::OwnerOf(_)
                    | Players::EachOther(_)) => {
                        if self.subject == Some(other) {
                            "they".to_owned()
                        } else {
                            self.player(other)
                        }
                    }
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
            other @ (Amount::Carried
            | Amount::Times(..)
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
            | Amount::SpellsCastBefore) => {
                format!("a number of {plural} equal to {}", self.amount(other))
            }
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
            other @ (Amount::ThoseOf(_)
            | Amount::Times(..)
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
            | Amount::SpellsCastBefore) => format!("{unit} equal to {}", self.amount(other)),
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
            other @ (Objects::This
            | Objects::Target(_)
            | Objects::AttachedTo
            | Objects::Those
            | Objects::TriggeredOn
            | Objects::TriggeredOnIn(_)
            | Objects::ExiledWithThis
            | Objects::PutOntoBattlefieldWithThis
            | Objects::Carried
            | Objects::CardsIn { .. }
            | Objects::Everywhere(_)
            | Objects::Paid(_)
            | Objects::Other(_)) => self.objects(other),
        }
    }
}

/// "Your graveyard": a zone of the controller's own, where a card they own is.
fn owners_zone(zone: Zone) -> String {
    format!("your {}", zone_name(zone))
}

/// Whether the filter says whose zone a card is in, so the zone is said with it.
pub(super) fn mentions_owner(filter: Filter) -> bool {
    match filter {
        Filter::Whose(_) => true,
        Filter::All(filters) => filters.iter().any(|&f| mentions_owner(f)),
        Filter::Any
        | Filter::Types(_)
        | Filter::Supertypes(_)
        | Filter::SubtypeWord(_)
        | Filter::ColorWords(_)
        | Filter::Owned(_)
        | Filter::Tapped
        | Filter::Token
        | Filter::ManaValueAtMost(_)
        | Filter::PowerAtMost(_)
        | Filter::ManaValueAtMostX
        | Filter::Not(_)
        | Filter::AnyOf(_)
        | Filter::InZone(_) => false,
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

/// The zone `filter` names its objects in ([`Filter::InZone`]), directly or among the filters
/// it requires all of.
fn zone_of(filter: Filter) -> Option<Zone> {
    match filter {
        Filter::InZone(zone) => Some(zone),
        Filter::All(filters) => filters.iter().find_map(|&f| zone_of(f)),
        Filter::Any
        | Filter::Types(_)
        | Filter::Supertypes(_)
        | Filter::SubtypeWord(_)
        | Filter::ColorWords(_)
        | Filter::Whose(_)
        | Filter::Owned(_)
        | Filter::Tapped
        | Filter::Token
        | Filter::ManaValueAtMost(_)
        | Filter::PowerAtMost(_)
        | Filter::ManaValueAtMostX
        | Filter::Not(_)
        | Filter::AnyOf(_) => None,
    }
}
