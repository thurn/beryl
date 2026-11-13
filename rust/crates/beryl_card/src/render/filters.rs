//! What a [`Filter`] says, as a noun phrase: "nonland permanent an opponent controls",
//! "instant or sorcery card in your graveyard", "artifact card with mana value 1 or less";
//! and what a [`TargetSpec`] says: "up to one other target nonland permanent".

use super::Ctx;
use super::words::{article, colors, list, plural, supertypes, types, with_article};
use crate::effect::Players;
use crate::filter::Filter;
use crate::ids::Zone;
use crate::target::{TargetKind, TargetSpec};

/// The kind of thing a filter is read over, which names it when the filter names no type:
/// "permanent", "card", "spell", "player".
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Permanent,
    /// A card in a zone other than the battlefield and the stack, the zone named when known
    /// ("in your graveyard").
    Card(Option<Zone>),
    Spell,
    Player,
}

impl Kind {
    /// The kind of object an instruction over a zone means: permanents on the battlefield,
    /// spells on the stack and cards elsewhere
    /// ([CR 609.2](https://yawgatog.com/resources/magic-rules/#R6092)).
    pub(super) fn of_zone(zone: Zone) -> Kind {
        match zone {
            Zone::Battlefield | Zone::Nowhere => Kind::Permanent,
            Zone::Stack => Kind::Spell,
            Zone::Library | Zone::Hand | Zone::Graveyard | Zone::Exile => Kind::Card(Some(zone)),
        }
    }
}

/// A noun phrase built from a filter: words before the noun, the noun or its alternatives,
/// and words after it.
#[derive(Default)]
pub(super) struct Noun {
    adjectives: Vec<String>,
    /// The card types or subtypes it may be, any one of them: "creature", "enchantment",
    /// "planeswalker". Each subtype among them takes its own article ("a Plains or an
    /// Island").
    kinds: Vec<String>,
    /// Words after the noun: "you control", "in your graveyard", "with mana value 2 or
    /// less".
    suffixes: Vec<String>,
}

impl Ctx {
    /// The noun phrase `filter` says of things of kind `kind`.
    pub(super) fn noun(&mut self, filter: Filter, kind: Kind) -> Noun {
        let mut noun = Noun::default();
        self.add(&mut noun, filter, kind);
        noun
    }

    /// Adds what `filter` says to `noun`: each entry's template.
    fn add(&mut self, noun: &mut Noun, filter: Filter, kind: Kind) {
        match filter {
            Filter::Any => {}
            Filter::Types(set) => noun.kind_or_adjective(types(set)),
            Filter::Supertypes(set) => noun.adjectives.extend(supertypes(set)),
            Filter::Subtype(subtype) => noun.kind_or_adjective(vec![subtype.name().to_owned()]),
            Filter::Colors(set) => noun.adjectives.push(list(&colors(set), "or")),
            Filter::Whose(players) => match kind {
                Kind::Card(Some(zone)) => {
                    let owner = self.zone_owner(*players);
                    noun.suffixes
                        .push(format!("in {owner} {}", zone_name(zone)));
                }
                Kind::Card(None) => {
                    let owner = self.zone_owner(*players);
                    noun.suffixes.push(format!("in {owner} zones"));
                }
                Kind::Permanent | Kind::Spell => {
                    let who = self.controls(*players, true);
                    noun.suffixes.push(who);
                }
                Kind::Player => {
                    if *players == Players::EachOpponent {
                        noun.kinds = vec!["opponent".to_owned()];
                    }
                }
            },
            Filter::Tapped => noun.adjectives.push("tapped".to_owned()),
            Filter::ManaValueAtMost(n) => {
                noun.suffixes.push(format!("with mana value {n} or less"));
            }
            Filter::ManaValueAtMostX => noun.suffixes.push("with mana value X or less".to_owned()),
            Filter::Not(inner) => self.add_not(noun, *inner, kind),
            Filter::All(filters) => {
                for &each in filters {
                    self.add(noun, each, kind);
                }
            }
            Filter::AnyOf(filters) => {
                let simple = filters
                    .iter()
                    .all(|f| matches!(f, Filter::Types(_) | Filter::Subtype(_)));
                if simple {
                    let mut kinds = Vec::new();
                    for &each in filters {
                        let mut one = Noun::default();
                        self.add(&mut one, each, kind);
                        kinds.extend(one.kinds);
                    }
                    noun.kind_or_adjective(kinds);
                } else {
                    let each: Vec<String> = filters
                        .iter()
                        .map(|&f| {
                            let one = self.noun(f, kind);
                            one.singular(kind)
                        })
                        .collect();
                    noun.kinds = vec![list(&each, "or")];
                }
            }
        }
    }

    /// Adds what "not [filter]" says: "nonland", "untapped", "you don't control".
    fn add_not(&mut self, noun: &mut Noun, filter: Filter, kind: Kind) {
        match filter {
            Filter::Types(set) => noun
                .adjectives
                .extend(types(set).into_iter().map(|t| format!("non{t}"))),
            Filter::Supertypes(set) => noun
                .adjectives
                .extend(supertypes(set).into_iter().map(|t| format!("non{t}"))),
            Filter::Subtype(subtype) => noun.adjectives.push(format!("non-{}", subtype.name())),
            Filter::Colors(set) => noun
                .adjectives
                .extend(colors(set).into_iter().map(|c| format!("non{c}"))),
            Filter::Tapped => noun.adjectives.push("untapped".to_owned()),
            Filter::Whose(players) => {
                let who = self.controls(*players, false);
                noun.suffixes.push(who);
            }
            Filter::Any
            | Filter::ManaValueAtMost(_)
            | Filter::ManaValueAtMostX
            | Filter::Not(_)
            | Filter::All(_)
            | Filter::AnyOf(_) => {
                let inner = self.noun(filter, kind);
                noun.suffixes
                    .push(format!("that isn't {}", inner.indefinite(kind)));
            }
        }
    }

    /// "You control", "an opponent controls", and with `does` false, "you don't control".
    fn controls(&mut self, players: Players, does: bool) -> String {
        let (who, third) = match players {
            Players::You => ("you".to_owned(), false),
            Players::EachOpponent => ("an opponent".to_owned(), true),
            Players::EachPlayer => ("a player".to_owned(), true),
            other => (self.player(other), true),
        };
        match (does, third) {
            (true, false) => format!("{who} control"),
            (true, true) => format!("{who} controls"),
            (false, false) => format!("{who} don't control"),
            (false, true) => format!("{who} doesn't control"),
        }
    }

    /// What one instance of the word "target" says, such as "target creature", "up to one
    /// other target nonland permanent" or "two target players"
    /// ([CR 115.1](https://yawgatog.com/resources/magic-rules/#R1151)).
    pub(super) fn target_phrase(&mut self, spec: TargetSpec) -> String {
        let (kind, prefix) = match spec.kind {
            TargetKind::Permanent => (Kind::Permanent, ""),
            TargetKind::Spell => (Kind::Spell, ""),
            TargetKind::SpellOrPermanent => (Kind::Permanent, "spell or "),
            TargetKind::GraveyardCard => (Kind::Card(Some(Zone::Graveyard)), ""),
            TargetKind::Player => (Kind::Player, ""),
        };
        let noun = self.noun(spec.filter, kind);
        let several = spec.count > 1;
        let named = if several {
            noun.plural(kind)
        } else {
            noun.singular(kind)
        };
        let other = if spec.other {
            if spec.count == 1 && !spec.optional {
                "another "
            } else {
                "other "
            }
        } else {
            ""
        };
        let count = match (spec.optional, spec.count) {
            (true, n) => format!("up to {} ", super::words::number(n)),
            (false, 1) => String::new(),
            (false, n) => format!("{} ", super::words::number(n)),
        };
        format!("{count}{other}target {prefix}{named}")
    }

    /// What the word "target" may choose, without the word: "creature or Vehicle", as an
    /// Aura's "Enchant creature or Vehicle" says it
    /// ([CR 702.5a](https://yawgatog.com/resources/magic-rules/#R7025a)).
    pub(super) fn target_noun(&mut self, spec: TargetSpec) -> String {
        let kind = match spec.kind {
            TargetKind::Spell => Kind::Spell,
            TargetKind::GraveyardCard => Kind::Card(Some(Zone::Graveyard)),
            TargetKind::Player => Kind::Player,
            TargetKind::Permanent | TargetKind::SpellOrPermanent => Kind::Permanent,
        };
        let noun = self.noun(spec.filter, kind);
        noun.singular(kind)
    }

    /// Whose zone it is, as the owner of the cards in it: "your", "an opponent's".
    pub(super) fn zone_owner(&mut self, players: Players) -> String {
        match players {
            Players::EachOpponent => "an opponent's".to_owned(),
            Players::EachPlayer => "a".to_owned(),
            other => self.possessive(other),
        }
    }
}

/// The name of a zone each player has one of: "library", "hand"
/// ([CR 400.1](https://yawgatog.com/resources/magic-rules/#R4001)).
pub(super) fn zone_name(zone: Zone) -> &'static str {
    match zone {
        Zone::Library => "library",
        Zone::Hand => "hand",
        Zone::Battlefield => "battlefield",
        Zone::Graveyard => "graveyard",
        Zone::Exile => "exile",
        Zone::Stack => "stack",
        Zone::Nowhere => "nowhere",
    }
}

impl Noun {
    /// Sets the kinds, or when they are set, says these as words before them ("artifact
    /// creature").
    fn kind_or_adjective(&mut self, names: Vec<String>) {
        if self.kinds.is_empty() {
            self.kinds = names;
        } else {
            self.adjectives.push(list(&names, "or"));
        }
    }

    /// The noun the kind of thing gives when the filter names none, or after the types it
    /// names ("artifact card").
    fn head(kind: Kind) -> Option<&'static str> {
        match kind {
            Kind::Permanent => None,
            Kind::Card(_) => Some("card"),
            Kind::Spell => Some("spell"),
            Kind::Player => Some("player"),
        }
    }

    /// The phrase, one thing or several.
    fn phrase(&self, kind: Kind, several: bool) -> String {
        let form = |noun: &String| if several { plural(noun) } else { noun.clone() };
        let kinds: Vec<String> = self.kinds.iter().map(form).collect();
        let named = match (kinds.as_slice(), Noun::head(kind)) {
            ([], None) => form(&"permanent".to_owned()),
            ([], Some(head)) => form(&head.to_owned()),
            (_, None) => list(&kinds, "or"),
            (_, Some(_)) if kind == Kind::Player => list(&kinds, "or"),
            (_, Some(head)) => format!("{} {}", list(&self.kinds, "or"), form(&head.to_owned())),
        };
        // "Noncreature, nonland card": negated types are listed with commas.
        let mut words: Vec<String> = Vec::new();
        for adjective in &self.adjectives {
            match words.last_mut() {
                Some(last) if last.starts_with("non") && adjective.starts_with("non") => {
                    last.push_str(", ");
                    last.push_str(adjective);
                }
                _ => words.push(adjective.clone()),
            }
        }
        words.push(named);
        words.extend(self.suffixes.iter().cloned());
        words.join(" ")
    }

    /// One of them: "nonland permanent an opponent controls".
    pub(super) fn singular(&self, kind: Kind) -> String {
        self.phrase(kind, false)
    }

    /// Several of them: "creatures you control".
    pub(super) fn plural(&self, kind: Kind) -> String {
        self.phrase(kind, true)
    }

    /// One of them with its article: "a creature", "an artifact card", and "a Plains or an
    /// Island" for subtypes, which each take their own.
    pub(super) fn indefinite(&self, kind: Kind) -> String {
        let subtypes = self.kinds.len() > 1
            && self.adjectives.is_empty()
            && Noun::head(kind).is_none()
            && self.kinds.iter().all(|k| k.starts_with(char::is_uppercase));
        if subtypes {
            let each: Vec<String> = self.kinds.iter().map(|k| with_article(k)).collect();
            let mut words = vec![list(&each, "or")];
            words.extend(self.suffixes.iter().cloned());
            return words.join(" ");
        }
        let phrase = self.singular(kind);
        format!("{} {phrase}", article(&phrase))
    }
}
