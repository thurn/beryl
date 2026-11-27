//! Templates for instructions that move objects between zones by naming where they go: into
//! a library, onto the battlefield, and whose zone that is (CR 400.3).

use super::Ctx;
use super::filters::{Kind, zone_name};
use super::words::{number, ordinal, with_article};
use crate::effect::{
    Amount, Effect, ExileMatch, LibraryOrder, LibraryPosition, LibraryRest, Objects, Players,
    PutOnto, Taken,
};
use crate::ids::{Zone, ZoneSet};

impl Ctx {
    /// An instruction that ends the turn or restarts the game with carried cards in exile.
    pub(super) fn end_or_restart(&mut self, effect: &Effect) -> Vec<String> {
        let Effect::RestartGame { filter } = *effect else {
            return vec!["end the turn".to_owned()];
        };
        let kind = Kind::Card(Some(Zone::Exile));
        let cards = self.noun(filter, kind).plural(kind);
        vec![
            format!(
                "restart the game, leaving in exile all {cards} exiled with {}",
                self.this
            ),
            "then put those cards onto the battlefield under your control".to_owned(),
        ]
    }

    /// "Look at the top four cards of your library. Put two of them into your hand and the
    /// rest on the bottom of your library in any order."
    pub(super) fn dig(
        &mut self,
        player: Players,
        look: Amount,
        take: Taken,
        top: u8,
        rest: LibraryRest,
    ) -> Vec<String> {
        if matches!(take, Taken::None) && top == 0 && rest == LibraryRest::Top {
            let looks = self.clause(player, ("look", "looks"), |c| {
                let put = c.verb("put", "puts");
                format!(
                    "at {}, then {put} them back in any order",
                    c.top_cards(player, look)
                )
            });
            return vec![looks];
        }
        let looks = self.clause(player, ("look", "looks"), |c| {
            format!("at {}", c.top_cards(player, look))
        });
        let rest_place = |library: &str| match rest {
            LibraryRest::Bottom(LibraryOrder::Any) => {
                format!("on the bottom of {library} library in any order")
            }
            LibraryRest::Bottom(LibraryOrder::Random) => {
                format!("on the bottom of {library} library in a random order")
            }
            LibraryRest::Top => format!("on top of {library} library in any order"),
        };
        let takes = match take {
            Taken::None => {
                let puts = self.clause(player, ("put", "puts"), |c| {
                    let library = c.possessive(player);
                    let tail = rest_place(&library);
                    if top > 0 {
                        format!(
                            "{} of them on top of {library} library and the rest {tail}",
                            number(top)
                        )
                    } else {
                        format!("them {tail}")
                    }
                });
                return vec![looks, puts];
            }
            // One sentence puts every pile.
            Taken::Put(n) => {
                let both = self.clause(player, ("put", "puts"), |c| {
                    let library = c.possessive(player);
                    if top > 0 {
                        if let Amount::Fixed(total) = look
                            && matches!(rest, LibraryRest::Bottom(_))
                            && u16::from(n) + u16::from(top) + 1 == u16::from(total)
                        {
                            format!(
                                "{} of those cards into {library} hand, {} on top of {library} \
                                 library, and one on the bottom of {library} library",
                                number(n),
                                number(top)
                            )
                        } else {
                            let tail = rest_place(&library);
                            format!(
                                "{} of those cards into {library} hand, {} on top of {library} \
                                 library, and the rest {tail}",
                                number(n),
                                number(top)
                            )
                        }
                    } else {
                        let tail = rest_place(&library);
                        format!(
                            "{} of them into {library} hand and the rest {tail}",
                            number(n)
                        )
                    }
                });
                return vec![looks, both];
            }
            Taken::MayReveal(filter) => {
                self.may = true;
                self.clause(player, ("reveal", "reveals"), |c| {
                    let card = c
                        .noun(filter, Kind::Card(None))
                        .indefinite(Kind::Card(None));
                    let put = c.verb("put", "puts");
                    format!(
                        "{card} from among them and {put} it into {} hand",
                        c.possessive(player)
                    )
                })
            }
        };
        let rest_clause = self.clause(player, ("put", "puts"), |c| {
            let library = c.possessive(player);
            let tail = rest_place(&library);
            if top > 0 {
                format!(
                    "{} of the rest on top of {library} library and the rest {tail}",
                    number(top)
                )
            } else {
                format!("the rest {tail}")
            }
        });
        vec![looks, takes, rest_clause]
    }

    /// "Look at the top card of target player's library. You may put that card on the
    /// bottom of that player's library" (CR 701.29a).
    pub(super) fn fateseal(
        &mut self,
        player: Players,
        library: Players,
        count: Amount,
    ) -> Vec<String> {
        let looks = self.clause(player, ("look", "looks"), |c| {
            format!("at {}", c.top_cards(library, count))
        });
        self.may = true;
        let them = if matches!(count, Amount::Fixed(1)) {
            "that card"
        } else {
            "any number of them"
        };
        let puts = self.clause(player, ("put", "puts"), |c| {
            format!("{them} on the bottom of {} library", c.possessive(library))
        });
        vec![looks, puts]
    }

    /// Exile objects, with the affected player stated for a cast trigger or a library top.
    pub(super) fn exile_clauses(&mut self, objects: Objects) -> Vec<String> {
        let clause = if let Objects::LibraryTop { players, .. } = objects {
            self.clause(*players, ("exile", "exiles"), |c| c.objects(objects))
        } else if objects == Objects::TriggeredOnIn(Zone::Stack)
            && self.trigger.is_some_and(|trigger| trigger.event.is_cast())
        {
            "that player exiles it".to_owned()
        } else {
            self.acts(("exile", "exiles"), objects, "")
        };
        self.done_this_way = Some("exiled");
        vec![clause]
    }

    /// Exile from the top of a library and describe what happens to the matching card.
    pub(super) fn exile_until_clauses(
        &mut self,
        player: Players,
        matching: ExileMatch,
        when_found: &Effect,
    ) -> Vec<String> {
        let exile = self.clause(player, ("exile", "exiles"), |c| {
            let library = c.possessive(player);
            match matching {
                ExileMatch::ManaValueBelow { filter, below } => {
                    let kind = Kind::Card(None);
                    let matched = with_article(&c.noun(filter, kind).singular(kind));
                    let limit = c.amount(below);
                    format!("cards from the top of {library} library until you exile {matched} whose mana value is less than {limit}")
                }
                ExileMatch::SharesTypeWithTriggeringSpell => {
                    format!("it, then exiles cards from the top of {library} library until they exile a card that shares a card type with it")
                }
            }
        });
        self.plural = false;
        let mut clauses = vec![exile];
        let mut found = self.effect(when_found);
        if matches!(matching, ExileMatch::SharesTypeWithTriggeringSpell)
            && let Some(clause) = found.first_mut()
        {
            *clause = clause.replacen("cast it", "cast that card", 1);
        }
        clauses.extend(found);
        clauses
    }

    /// "The top card of your library", "the top four cards of target player's library".
    pub(super) fn top_cards(&mut self, player: Players, count: Amount) -> String {
        let cards = match count {
            Amount::Fixed(1) => "card".to_owned(),
            Amount::Fixed(n) => format!("{} cards", number(n)),
            Amount::X => "X cards".to_owned(),
            other @ (Amount::ThatMany
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
            | Amount::SpellsCastBefore) => format!("{} cards", self.amount(other)),
        };
        let whose = self.possessive(player);
        format!("the top {cards} of {whose} library")
    }

    /// A fixed number of cards goes on top, or any number goes on the bottom.
    pub(super) fn put_from_hand(&mut self, player: Players, count: Option<Amount>) -> Vec<String> {
        vec![self.clause(player, ("put", "puts"), |c| {
            let whose = c.possessive(player);
            if let Some(count) = count {
                let cards = c.count_of(count, "card", "cards");
                format!("{cards} from {whose} hand on top of {whose} library in any order")
            } else {
                format!("any number of cards from {whose} hand on the bottom of {whose} library")
            }
        })]
    }

    /// "Put [objects] into their owners' libraries [position]" (CR 400.3).
    pub(super) fn put_into_library(
        &mut self,
        objects: Objects,
        position: LibraryPosition,
    ) -> Vec<String> {
        let plural = self.is_plural(objects);
        // "Put target instant or sorcery card from your graveyard on top of your library": a
        // card in a graveyard is in its owner's (CR 404.1), so its owner is "you".
        let library = if self.own_graveyard_target(objects) {
            "your library".to_owned()
        } else {
            owners(plural, "library")
        };
        match position {
            LibraryPosition::Top => {
                vec![self.acts(("put", "puts"), objects, &format!(" on top of {library}"))]
            }
            LibraryPosition::Bottom => vec![self.acts(
                ("put", "puts"),
                objects,
                &format!(" on the bottom of {library}"),
            )],
            LibraryPosition::BottomRandom => vec![self.acts(
                ("put", "puts"),
                objects,
                &format!(" on the bottom of {library} in a random order"),
            )],
            LibraryPosition::FromTop(n) => vec![self.acts(
                ("put", "puts"),
                objects,
                &format!(" into {library} {} from the top", ordinal(n)),
            )],
            LibraryPosition::TopOrBottom => {
                // "Choose up to one target creature spell. Its owner puts it on their choice
                // of the top or bottom of their library": the target is said first.
                let mut clauses = Vec::new();
                let what = self.objects(objects);
                let (whose, it) = if matches!(what.as_str(), "it" | "them") {
                    (if plural { "their owners" } else { "its owner" }, what)
                } else if matches!(objects, Objects::Target(_)) {
                    clauses.push(format!("choose {what}"));
                    ("its owner", "it".to_owned())
                } else {
                    (if plural { "their owners" } else { "its owner" }, what)
                };
                let puts = if plural { "put" } else { "puts" };
                clauses.push(format!(
                    "{whose} {puts} {it} on their choice of the top or bottom of their library"
                ));
                self.plural = plural;
                clauses
            }
        }
    }

    /// "Each player may put an artifact card from their hand onto the battlefield": each
    /// player's hidden pick of a card of their zone (CR 101.4a), and then the cards picked put
    /// onto the battlefield under their owners' control, said as one instruction. One
    /// player's pick, not inside [`Effect::ForEachPlayer`], says "that opponent may put ...".
    /// `None` for any other pair.
    pub(super) fn hidden_put(&mut self, effect: &Effect, next: &Effect) -> Option<String> {
        let (players, choose) = if let Effect::ForEachPlayer { players, effect } = *effect {
            (Some(players), effect)
        } else {
            (None, effect)
        };
        let (
            &Effect::Choose {
                player,
                among:
                    Objects::InZoneOf {
                        zone,
                        players: whose,
                        filter,
                    },
                hidden: true,
                ..
            },
            &Effect::PutOntoBattlefield(&PutOnto {
                objects: Objects::Those,
                under: None,
                tapped,
                linked: false,
            }),
        ) = (choose, next)
        else {
            return None;
        };
        let saved = match players {
            Some(players) => self.group.replace(players),
            None => self.group,
        };
        self.may = true;
        let tapped = if tapped { " tapped" } else { "" };
        let clause = self.clause(player, ("put", "puts"), |c| {
            let owner = c.possessive(*whose);
            let noun = c
                .noun(filter, Kind::Card(None))
                .indefinite(Kind::Card(None));
            format!(
                "{noun} from {owner} {} onto the battlefield{tapped}",
                zone_name(zone)
            )
        });
        self.group = saved;
        Some(clause)
    }

    /// "Return enchanted creature card to the battlefield under your control", for a
    /// [`PutOnto`]. A card "returns" to the battlefield from a graveyard or from exile.
    pub(super) fn put_onto(&mut self, onto: &PutOnto) -> String {
        let tapped = if onto.tapped { " tapped" } else { "" };
        if onto.objects == Objects::This
            && onto.under.is_none()
            && let Some(ability) = self.ability
        {
            let from = match ability.zones() {
                ZoneSet::GRAVEYARD => Some("your graveyard"),
                zones if zones == ZoneSet::GRAVEYARD.union(ZoneSet::EXILE) => {
                    Some("your graveyard or from exile")
                }
                _ => None,
            };
            if let Some(from) = from {
                return format!("return this card from {from} to the battlefield{tapped}");
            }
        }
        let plural = self.is_plural(onto.objects);
        let control = match onto.under {
            None => owners(plural, "control"),
            Some(player) => {
                let who = self.player(player);
                if who == "you" {
                    "your control".to_owned()
                } else {
                    format!("{} control", super::abilities::possessive(&who))
                }
            }
        };
        let mut what = self.moved_card(onto.objects);
        let from_graveyard = matches!(onto.objects, Objects::Target(spec) if
            self.targets.get(spec.index()).is_some_and(|target|
                target.kind == crate::TargetKind::GraveyardCard));
        let (verbs, destination) = if from_graveyard && onto.under.is_some() {
            if what.ends_with(" graveyard") {
                what = what.replacen(" in ", " from ", 1);
            } else {
                what.push_str(" from a graveyard");
            }
            (("put", "puts"), "onto")
        } else {
            (("return", "returns"), "to")
        };
        let rest = format!(" {destination} the battlefield{tapped} under {control}");
        self.discarded = None;
        self.those_named = None;
        let clause = self.clause(Players::You, verbs, |_| format!("{what}{rest}"));
        self.plural = plural;
        clause
    }
}

/// "Its owner's hand", or "their owners' hands" for several (CR 400.3).
pub(super) fn owners(plural: bool, zone: &str) -> String {
    if plural {
        let zones = if zone == "control" {
            "control".to_owned()
        } else {
            super::words::plural(zone)
        };
        format!("their owners' {zones}")
    } else {
        format!("its owner's {zone}")
    }
}
