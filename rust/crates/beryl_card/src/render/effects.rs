//! One template per [`Effect`] instruction: what it says, as the clauses of a spell or an
//! ability ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c)): "The controller
//! of the spell or ability follows its instructions in the order written."

use super::Ctx;
use super::clauses::{prefix, subject_of};
use super::filters::Kind;
use super::words::{mana_set, number, ordinal, symbol, with_article};
use crate::ability::Behavior;
use crate::characteristics::Types;
use crate::continuous::Duration;
use crate::definition::CardDef;
use crate::effect::{
    Amount, CopyException, Effect, Found, LibraryOrder, LibraryPosition, Objects, Players,
    Recipients, Regeneration, Taken,
};
use crate::ids::Zone;

impl Ctx {
    /// The clauses an effect says, each a sentence once capitalized and closed: "draw three
    /// cards", "put two cards from your hand on top of your library in any order".
    #[allow(clippy::too_many_lines)] // A line or five per entry, in one exhaustive match.
    pub(super) fn effect(&mut self, effect: &Effect) -> Vec<String> {
        match *effect {
            Effect::Sequence(effects) => self.sequence(effects),
            Effect::If {
                condition,
                then,
                otherwise,
            } => {
                let condition = self.condition(condition);
                let mut clauses = self.effect(then);
                prefix(&mut clauses, &format!("if {condition}, "));
                if let Some(otherwise) = otherwise {
                    let mut more = self.effect(otherwise);
                    prefix(&mut more, "otherwise, ");
                    clauses.extend(more);
                }
                clauses
            }
            Effect::May { player, effect } => {
                self.may = true;
                let mut clauses = self.effect(effect);
                if std::mem::take(&mut self.may) {
                    let who = self.player(player);
                    prefix(&mut clauses, &format!("{who} may have "));
                }
                clauses
            }
            Effect::Unless {
                effect,
                player,
                cost,
            } => {
                let mut clauses = self.effect(effect);
                let unless = self.unless(player, cost);
                match clauses.first_mut() {
                    Some(first) => {
                        first.push_str(" unless ");
                        first.push_str(&unless);
                    }
                    None => clauses.push(format!("unless {unless}")),
                }
                clauses
            }
            Effect::ForEachPlayer { players, effect } => self.for_each_player(players, effect),
            Effect::Choose {
                player,
                among,
                optional,
                ..
            } => {
                self.may |= optional;
                let clause = self.clause(player, ("choose", "chooses"), |c| c.one_of(among));
                self.plural = false;
                vec![clause]
            }
            Effect::LookAt { player, cards } => {
                vec![self.clause(player, ("look", "looks"), |c| {
                    format!("at {}", c.objects(cards))
                })]
            }
            Effect::Counter(spells) => vec![self.acts(("counter", "counters"), spells, "")],
            Effect::CounterInto { spells, zone } => {
                let counter = self.acts(("counter", "counters"), spells, "");
                let instead = match zone {
                    Zone::Exile => "exile it".to_owned(),
                    other => format!(
                        "put it into its owner's {}",
                        super::filters::zone_name(other)
                    ),
                };
                vec![
                    counter,
                    format!(
                        "if that spell is countered this way, {instead} instead of putting it \
                         into its owner's graveyard"
                    ),
                ]
            }
            Effect::Destroy {
                objects,
                regeneration,
            } => {
                let destroy = self.acts(("destroy", "destroys"), objects, "");
                match regeneration {
                    Regeneration::Allowed => vec![destroy],
                    Regeneration::Forbidden => vec![destroy, self.not_regenerated(objects)],
                }
            }
            Effect::Attach { objects, to } => {
                let host = self.objects(to);
                vec![self.acts(("attach", "attaches"), objects, &format!(" to {host}"))]
            }
            Effect::Exile(objects) | Effect::ExileLinked(objects) => {
                vec![self.acts(("exile", "exiles"), objects, "")]
            }
            Effect::ExileUntilThisLeaves(objects) => {
                let until = format!(" until {} leaves the battlefield", self.this);
                vec![self.acts(("exile", "exiles"), objects, &until)]
            }
            Effect::Sacrifice(objects) => {
                vec![self.acts(("sacrifice", "sacrifices"), objects, "")]
            }
            Effect::ReturnToHand(objects) => {
                let to = owners(self.is_plural(objects), "hand");
                vec![self.acts(("return", "returns"), objects, &format!(" to {to}"))]
            }
            Effect::PutIntoGraveyard(objects) => {
                let to = owners(self.is_plural(objects), "graveyard");
                vec![self.acts(("put", "puts"), objects, &format!(" into {to}"))]
            }
            Effect::ReturnTransformed(objects) => {
                let control = owners(self.is_plural(objects), "control");
                let rest = format!(" to the battlefield transformed under {control}");
                vec![self.acts(("return", "returns"), objects, &rest)]
            }
            Effect::PutIntoLibrary { objects, position } => {
                self.put_into_library(objects, position)
            }
            Effect::PutBack { player, count } => {
                vec![self.clause(player, ("put", "puts"), |c| {
                    let cards = c.count_of(count, "card", "cards");
                    let hand = c.possessive(player);
                    format!("{cards} from {hand} hand on top of {hand} library in any order")
                })]
            }
            Effect::ShuffleInto { player, cards } => {
                vec![self.clause(player, ("shuffle", "shuffles"), |c| {
                    let cards = c.objects(cards);
                    format!("{cards} into {} library", c.possessive(player))
                })]
            }
            Effect::Dig {
                player,
                look,
                take,
                rest,
            } => self.dig(player, look, take, rest),
            Effect::Fateseal {
                player,
                library,
                count,
            } => self.fateseal(player, library, count),
            Effect::Search {
                player,
                filter,
                count,
                found,
            } => {
                let several = !matches!(count, Amount::Fixed(1));
                vec![self.clause(player, ("search", "searches"), |c| {
                    let noun = c.noun(filter, Kind::Card(None));
                    let (one, many) = (
                        noun.singular(Kind::Card(None)),
                        noun.plural(Kind::Card(None)),
                    );
                    let cards = c.count_of(count, &one, &many);
                    let library = c.possessive(player);
                    let it = if several { "them" } else { "it" };
                    let found = match found {
                        Found::RevealToHand => format!(
                            "{} {it}, {} {it} into {library} hand",
                            c.verb("reveal", "reveals"),
                            c.verb("put", "puts")
                        ),
                        Found::OntoBattlefieldTapped => {
                            format!("{} {it} onto the battlefield tapped", c.verb("put", "puts"))
                        }
                    };
                    let shuffle = c.verb("shuffle", "shuffles");
                    format!("{library} library for {cards}, {found}, then {shuffle}")
                })]
            }
            Effect::Draw { player, count } => vec![self.cards(player, ("draw", "draws"), count)],
            Effect::Discard { player, count } => {
                let clause = self.cards(player, ("discard", "discards"), count);
                self.discarded = Some(self.plural);
                self.those_named = None;
                vec![clause]
            }
            Effect::Mill { player, count } => vec![self.cards(player, ("mill", "mills"), count)],
            Effect::Scry { player, count } => {
                vec![self.clause(player, ("scry", "scries"), |c| c.amount(count))]
            }
            Effect::Explore(objects) => {
                let plural = self.is_plural(objects);
                let who = self.objects(objects);
                vec![format!(
                    "{who} {}",
                    if plural { "explore" } else { "explores" }
                )]
            }
            Effect::ManifestDread(player) => {
                vec![
                    self.clause(player, ("manifest dread", "manifests dread"), |_| {
                        String::new()
                    }),
                ]
            }
            Effect::FlipCoin(player) => {
                vec![self.clause(player, ("flip", "flips"), |_| "a coin".to_owned())]
            }
            Effect::DealDamage { to, amount } => {
                let damage = self.quantity(amount, "damage");
                let to = match to {
                    Recipients::Players(players) => self.player(players),
                    Recipients::Permanents(objects) => self.objects(objects),
                };
                vec![format!("{} deals {damage} to {to}", self.this)]
            }
            Effect::GainLife { player, amount } => {
                vec![self.stated(player, ("gain", "gains"), |c| c.quantity(amount, "life"))]
            }
            Effect::LoseLife { player, amount } => {
                vec![self.stated(player, ("lose", "loses"), |c| c.quantity(amount, "life"))]
            }
            Effect::CreateTokens {
                player,
                token,
                count,
            } => vec![self.clause(player, ("create", "creates"), |c| c.tokens(token, count))],
            Effect::CreateAttackingTokens {
                player,
                token,
                count,
                attacking,
            } => vec![self.clause(player, ("create", "creates"), |c| {
                let tokens = c.tokens(token, count);
                let that = if matches!(count, Amount::Fixed(1)) {
                    "that's"
                } else {
                    "that are"
                };
                let whom = c.player(attacking);
                format!(
                    "{tokens} {that} tapped and attacking {whom} or a planeswalker they control"
                )
            })],
            Effect::CreateCopies {
                player,
                of,
                count,
                except,
            } => vec![self.clause(player, ("create", "creates"), |c| {
                let of = c.objects(of);
                match (except, count) {
                    (CopyException::PowerToughness(p, t), _) => {
                        format!("{} token copy of {of}", with_article(&format!("{p}/{t}")))
                    }
                    (CopyException::None, Amount::Fixed(1)) => {
                        format!("a token that's a copy of {of}")
                    }
                    (CopyException::None, count) => {
                        let tokens = c.count_of(count, "token", "tokens");
                        format!("{tokens} that are copies of {of}")
                    }
                }
            })],
            Effect::AddCounters {
                objects,
                kind,
                count,
            } => {
                let name = kind.name();
                let counters = self.count_of(
                    count,
                    &format!("{name} counter"),
                    &format!("{name} counters"),
                );
                let on = format!(" on {}", self.objects(objects));
                vec![self.clause(Players::You, ("put", "puts"), |_| format!("{counters}{on}"))]
            }
            Effect::RemoveCounters {
                objects,
                kind,
                count,
            } => {
                let name = kind.name();
                let counters = self.count_of(
                    count,
                    &format!("{name} counter"),
                    &format!("{name} counters"),
                );
                let from = format!(" from {}", self.objects(objects));
                vec![self.clause(Players::You, ("remove", "removes"), |_| {
                    format!("{counters}{from}")
                })]
            }
            Effect::AddMana { player, mana } => {
                vec![self.clause(player, ("add", "adds"), |_| mana_set(mana))]
            }
            Effect::AddManaOfKind {
                player,
                kind,
                amount,
            } => vec![self.clause(player, ("add", "adds"), |c| match amount {
                Amount::Fixed(n) => symbol(kind).repeat(usize::from(n)),
                other => format!("an amount of {} equal to {}", symbol(kind), c.amount(other)),
            })],
            Effect::Modify {
                objects,
                modification,
                duration,
            } => {
                let text = self.modify(objects, modification);
                vec![format!("{text}{}", duration_suffix(duration))]
            }
            Effect::ChangeRule { rule, duration } => {
                let prefix = match duration {
                    Duration::EndOfTurn => "until end of turn, ",
                    Duration::UntilYourNextTurn => "until your next turn, ",
                    Duration::Indefinite => "",
                };
                vec![format!("{prefix}{}", self.rule_change(rule))]
            }
            Effect::GainFlashback(cards) => {
                let plural = self.is_plural(cards);
                let what = self.objects(cards);
                let gains = if plural { "gain" } else { "gains" };
                vec![
                    format!("{what} {gains} flashback until end of turn"),
                    "the flashback cost is equal to its mana cost".to_owned(),
                ]
            }
            Effect::PermitCast {
                player,
                cards,
                duration,
                terms,
            } => {
                let what = self.objects(cards);
                let (before, it, after) = match duration {
                    Duration::Indefinite => {
                        let card = if what == "it" {
                            "that card".to_owned()
                        } else {
                            what
                        };
                        (
                            format!("for as long as {card} remains exiled, "),
                            "it".to_owned(),
                            "",
                        )
                    }
                    Duration::EndOfTurn => (String::new(), what, " this turn"),
                    Duration::UntilYourNextTurn => (String::new(), what, " until your next turn"),
                };
                self.may = true;
                let verb = if terms.played {
                    ("play", "plays")
                } else {
                    ("cast", "casts")
                };
                let cast = self.clause(player, verb, |_| format!("{it}{after}"));
                let mut clauses = vec![format!("{before}{cast}")];
                if terms.costs_more > 0 {
                    clauses.push(format!(
                        "a spell cast this way costs {{{}}} more to cast",
                        terms.costs_more
                    ));
                }
                if terms.exiled_instead {
                    let graveyard = self.possessive(player);
                    clauses.push(format!(
                        "if that spell would be put into {graveyard} graveyard, exile it instead"
                    ));
                }
                clauses
            }
            Effect::MayCast { player, card, free } => {
                self.may = true;
                vec![self.clause(player, ("cast", "casts"), |c| {
                    let card = c.objects(card);
                    if free {
                        format!("{card} without paying its mana cost")
                    } else {
                        card
                    }
                })]
            }
            Effect::UntapUpTo {
                player,
                count,
                permanents,
            } => vec![self.clause(player, ("untap", "untaps"), |c| {
                let many = match count {
                    Amount::Fixed(n) => number(n),
                    other => c.amount(other),
                };
                format!("up to {many} {}", c.counted_objects(permanents))
            })],
            Effect::Delay { ability, .. } => {
                let delayed = self
                    .face
                    .and_then(|face| face.abilities.get(ability.index()));
                match delayed {
                    Some(delayed) => self.delayed(delayed),
                    None => vec!["[delayed ability]".to_owned()],
                }
            }
            Effect::GetEmblem { player, emblem } => {
                vec![self.stated(player, ("get", "gets"), |_| {
                    format!("an emblem with \"{}\"", emblem_text(emblem))
                })]
            }
            Effect::WinGame(player) => {
                vec![self.stated(player, ("win", "wins"), |_| "the game".to_owned())]
            }
            Effect::LoseGame(player) => {
                vec![self.stated(player, ("lose", "loses"), |_| "the game".to_owned())]
            }
        }
    }

    /// "Each player shuffles ...": the players as the subject of each clause about one of
    /// them, or "for each opponent, ..." when the clauses are about someone else.
    fn for_each_player(&mut self, players: Players, effect: &Effect) -> Vec<String> {
        if subject_of(effect) == Some(Players::ThatPlayer) {
            let saved = self.group.replace(players);
            let clauses = self.effect(effect);
            self.group = saved;
            return clauses;
        }
        let each = match players {
            Players::EachOpponent => "opponent".to_owned(),
            Players::EachPlayer => "player".to_owned(),
            other => self.player(other),
        };
        let mut clauses = self.effect(effect);
        prefix(&mut clauses, &format!("for each {each}, "));
        clauses
    }

    /// "Put [objects] into their owners' libraries [position]" (CR 400.3).
    fn put_into_library(&mut self, objects: Objects, position: LibraryPosition) -> Vec<String> {
        let plural = self.is_plural(objects);
        let library = owners(plural, "library");
        match position {
            LibraryPosition::Top => {
                vec![self.acts(("put", "puts"), objects, &format!(" on top of {library}"))]
            }
            LibraryPosition::Bottom => vec![self.acts(
                ("put", "puts"),
                objects,
                &format!(" on the bottom of {library}"),
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

    /// "Look at the top four cards of your library. Put two of them into your hand and the
    /// rest on the bottom of your library in any order."
    fn dig(
        &mut self,
        player: Players,
        look: Amount,
        take: Taken,
        rest: LibraryOrder,
    ) -> Vec<String> {
        let looks = self.clause(player, ("look", "looks"), |c| {
            format!("at {}", c.top_cards(player, look))
        });
        let order = match rest {
            LibraryOrder::Any => "any order",
            LibraryOrder::Random => "a random order",
        };
        let takes = match take {
            // One sentence puts both piles: "Put two of them into your hand and the rest on
            // the bottom of your library in any order."
            Taken::Put(n) => {
                let both = self.clause(player, ("put", "puts"), |c| {
                    let library = c.possessive(player);
                    format!(
                        "{} of them into {library} hand and the rest on the bottom of \
                         {library} library in {order}",
                        number(n)
                    )
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
        let rest = self.clause(player, ("put", "puts"), |c| {
            format!(
                "the rest on the bottom of {} library in {order}",
                c.possessive(player)
            )
        });
        vec![looks, takes, rest]
    }

    /// "Look at the top card of target player's library. You may put that card on the
    /// bottom of that player's library" (CR 701.29a).
    fn fateseal(&mut self, player: Players, library: Players, count: Amount) -> Vec<String> {
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

    /// "The top card of your library", "the top four cards of target player's library".
    fn top_cards(&mut self, player: Players, count: Amount) -> String {
        let cards = match count {
            Amount::Fixed(1) => "card".to_owned(),
            Amount::Fixed(n) => format!("{} cards", number(n)),
            Amount::X => "X cards".to_owned(),
            other => format!("{} cards", self.amount(other)),
        };
        let whose = self.possessive(player);
        format!("the top {cards} of {whose} library")
    }

    /// "Two 2/1 white Insect creature tokens with flying", "a Treasure token"
    /// ([CR 111.10](https://yawgatog.com/resources/magic-rules/#R11110)): a predefined token
    /// is named by its subtype alone.
    fn tokens(&mut self, token: &'static CardDef, count: Amount) -> String {
        let Some(face) = token.faces.first() else {
            return self.count_of(count, "token", "tokens");
        };
        let subtypes: Vec<&str> = face.subtypes.iter().map(crate::Subtype::name).collect();
        let (one, many) = if let (Some(power), Some(toughness)) = (face.power, face.toughness) {
            let colors = super::words::colors(face.colors);
            let colors = if colors.is_empty() {
                "colorless".to_owned()
            } else {
                super::words::list(&colors, "and")
            };
            let types = super::words::types(face.types.minus(Types::KINDRED)).join(" ");
            let keywords = super::words::keywords(face.keywords);
            let with = if keywords.is_empty() {
                String::new()
            } else {
                format!(" with {}", super::words::list(&keywords, "and"))
            };
            let named = format!(
                "{power}/{toughness} {colors} {} {types}",
                subtypes.join(" ")
            );
            (
                format!("{named} token{with}"),
                format!("{named} tokens{with}"),
            )
        } else {
            let named = subtypes.join(" ");
            (format!("{named} token"), format!("{named} tokens"))
        };
        self.count_of(count, &one, &many)
    }

    /// A delayed triggered ability made in the middle of an effect: "at the beginning of the
    /// next end step, untap up to two lands" (CR 603.7a).
    fn delayed(&mut self, ability: &'static crate::Ability) -> Vec<String> {
        let saved = (
            self.targets,
            std::mem::take(&mut self.mentioned),
            self.trigger,
            self.subject,
            self.group,
        );
        self.begin(ability.targets(), ability.trigger());
        let clauses = self.triggered(ability);
        (
            self.targets,
            self.mentioned,
            self.trigger,
            self.subject,
            self.group,
        ) = saved;
        clauses
    }
}

/// The text of an emblem's abilities, as the quoted text of "an emblem with "..."".
fn emblem_text(emblem: &'static CardDef) -> String {
    let Some(face) = emblem.faces.first() else {
        return String::new();
    };
    let mut ctx = Ctx::of_face(face, "this emblem");
    face.abilities
        .iter()
        .map(|ability| ctx.paragraph(ability))
        .collect::<Vec<_>>()
        .join(" ")
}

/// "Its owner's hand", or "their owners' hands" for several (CR 400.3).
fn owners(plural: bool, zone: &str) -> String {
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

/// How long a continuous effect from a resolving spell lasts, after what it does
/// ([CR 611.2a](https://yawgatog.com/resources/magic-rules/#R6112a)): "until end of turn".
pub(super) fn duration_suffix(duration: Duration) -> &'static str {
    match duration {
        Duration::EndOfTurn => " until end of turn",
        Duration::UntilYourNextTurn => " until your next turn",
        Duration::Indefinite => "",
    }
}

/// Whether a behavior is effect data that says nothing: an Aura's spell, which only
/// targets.
pub(super) fn says_nothing(body: Behavior) -> bool {
    matches!(body, Behavior::Data(Effect::Sequence([])))
}

impl Ctx {
    /// "It can't be regenerated", or "They can't be regenerated" after several
    /// ([CR 701.19c](https://yawgatog.com/resources/magic-rules/#R70119c)). An overload card's
    /// text serves both of its spells, so it names what was destroyed instead of using a
    /// pronoun that "each" would make wrong: "A creature destroyed this way can't be
    /// regenerated."
    fn not_regenerated(&mut self, objects: Objects) -> String {
        let overloaded = self.face.is_some_and(|face| {
            face.casting
                .iter()
                .any(|option| matches!(option, crate::CastingOption::Overload { .. }))
        });
        if overloaded
            && let Objects::Target(n) = objects
            && let Some(&spec) = self.targets.get(n.index())
        {
            let noun = self.target_noun(spec);
            return format!(
                "{} destroyed this way can't be regenerated",
                with_article(&noun)
            );
        }
        let they = if self.is_plural(objects) {
            "they"
        } else {
            "it"
        };
        format!("{they} can't be regenerated")
    }
}
