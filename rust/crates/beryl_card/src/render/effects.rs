//! One template per [`Effect`] instruction: what it says, as the clauses of a spell or an
//! ability ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c)): "The controller
//! of the spell or ability follows its instructions in the order written."

use super::Ctx;
use super::clauses::prefix;
use super::filters::Kind;
use super::words::{mana_set, number, symbol, with_article};
use super::zones::owners;
use crate::Recipients;
use crate::ability::Behavior;
use crate::characteristics::Types;
use crate::continuous::Duration;
use crate::definition::CardDef;
use crate::effect::{Amount, Effect, Found, Objects, Players, Regeneration};
use crate::filter::Filter;
use crate::ids::Zone;
use crate::trigger::TriggerEvent;

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
            Effect::CopySpell {
                spells,
                count,
                except,
                new_targets,
            } => self.copy_spell(spells, count, except, new_targets),
            Effect::CounterInto { spells, zone } => {
                let counter = self.acts(("counter", "counters"), spells, "");
                let instead = match zone {
                    Zone::Exile => "exile it".to_owned(),
                    other @ (Zone::Library
                    | Zone::Hand
                    | Zone::Battlefield
                    | Zone::Graveyard
                    | Zone::Stack
                    | Zone::Nowhere
                    | Zone::Merged) => format!(
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
            Effect::Regenerate(objects) => {
                vec![self.acts(("regenerate", "regenerates"), objects, "")]
            }
            Effect::Tap(objects) => vec![format!("tap {}", self.objects(objects))],
            Effect::PhaseOut(objects) => vec![self.phase_out(objects)],
            Effect::Attach { objects, to } => {
                let host = self.objects(to);
                vec![self.acts(("attach", "attaches"), objects, &format!(" to {host}"))]
            }
            Effect::BecomeCopy {
                objects,
                of,
                except,
            } => vec![self.become_copy(objects, of, except)],
            Effect::ExchangeControl(first, second) => {
                let (first, second) = (self.objects(first), self.objects(second));
                vec![format!("exchange control of {first} and {second}")]
            }
            Effect::Exile(objects) | Effect::ExileLinked(objects) => {
                vec![self.acts(("exile", "exiles"), objects, "")]
            }
            Effect::ExileUntilThisLeaves(objects) => {
                let until = format!(" until {} leaves the battlefield", self.this);
                vec![self.acts(("exile", "exiles"), objects, &until)]
            }
            // "That permanent's controller sacrifices it": a delayed ability's permanent may be
            // another player's, who sacrifices it. CR 701.21a: "To sacrifice a permanent, its
            // controller moves it from the battlefield directly to its owner's graveyard."
            Effect::Sacrifice(Objects::Carried) => {
                vec!["that permanent's controller sacrifices it".to_owned()]
            }
            Effect::Sacrifice(objects) => {
                vec![self.acts(("sacrifice", "sacrifices"), objects, "")]
            }
            Effect::ReturnToHand(objects) => {
                // "Return target land card from your graveyard to your hand": the cards a
                // target in its controller's own graveyard names are theirs.
                if self.own_graveyard_target(objects) {
                    let clause = self.acts(("return", "returns"), objects, " to your hand");
                    return vec![clause.replacen(" in your graveyard", " from your graveyard", 1)];
                }
                let to = owners(self.is_plural(objects), "hand");
                vec![self.acts(("return", "returns"), objects, &format!(" to {to}"))]
            }
            Effect::PutIntoGraveyard(objects) => {
                let to = owners(self.is_plural(objects), "graveyard");
                vec![self.acts(("put", "puts"), objects, &format!(" into {to}"))]
            }
            // "Return this card from your graveyard to the battlefield": a card goes to
            // its owner's graveyard (CR 404.1), and a triggered ability of an object with
            // no controller is its owner's (CR 113.8), so "your" names the card's owner,
            // under whose control it returns.
            Effect::ReturnToBattlefield(Objects::This) if self.in_graveyard => {
                let rest = " from your graveyard to the battlefield";
                vec![self.acts(("return", "returns"), Objects::This, rest)]
            }
            Effect::ReturnToBattlefield(objects) => {
                let control = owners(self.is_plural(objects), "control");
                let rest = format!(" to the battlefield under {control}");
                vec![self.acts(("return", "returns"), objects, &rest)]
            }
            Effect::PutOntoBattlefield(onto) => vec![self.put_onto(onto)],
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
            Effect::ExileUntil {
                player,
                filter,
                below,
                when_found,
            } => {
                let exile = self.clause(player, ("exile", "exiles"), |c| {
                    let kind = Kind::Card(None);
                    let matched = with_article(&c.noun(filter, kind).singular(kind));
                    let library = c.possessive(player);
                    let limit = c.amount(below);
                    format!("cards from the top of {library} library until you exile {matched} whose mana value is less than {limit}")
                });
                let mut clauses = vec![exile];
                clauses.extend(self.effect(when_found));
                clauses
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
            Effect::Wish { filter } => {
                self.may = true;
                vec![self.clause(Players::You, ("reveal", "reveals"), |c| {
                    let noun = c.noun(filter, Kind::Card(None)).singular(Kind::Card(None));
                    format!(
                        "{} you own from outside the game and put it into your hand",
                        with_article(&noun)
                    )
                })]
            }
            Effect::Draw { player, count } => vec![self.cards(player, ("draw", "draws"), count)],
            Effect::Discard { player, count } => {
                let clause = if player == Players::You
                    && matches!(count, Amount::Count(cards) if *cards == crate::effect::build::hand_of(&Players::You))
                {
                    self.clause(player, ("discard", "discards"), |_| "your hand".to_owned())
                } else {
                    self.cards(player, ("discard", "discards"), count)
                };
                self.discarded = Some(self.plural);
                self.those_named = None;
                vec![clause]
            }
            Effect::Mill { player, count } => vec![self.cards(player, ("mill", "mills"), count)],
            Effect::Scry { player, count } => {
                vec![self.clause(player, ("scry", "scries"), |c| c.amount(count))]
            }
            // The reminder text of mutate, which no card prints as a line of its own.
            Effect::Mutate(objects) => vec![format!(
                "put this spell over or under {}. They mutate into the creature on top plus all \
                 abilities from under it",
                self.objects(objects)
            )],
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
            Effect::Subgame => vec![
                "players play a Magic subgame, using their libraries as their decks".to_owned(),
            ],
            Effect::FlipCoin(player) => {
                vec![self.clause(player, ("flip", "flips"), |_| "a coin".to_owned())]
            }
            Effect::DealDamage { to, amount } => {
                let damage = self.quantity(amount, "damage");
                if matches!(to, Recipients::DividedTwo(_)) {
                    return vec![format!(
                        "{} deals {damage} divided as you choose among one or two targets",
                        self.this
                    )];
                }
                let to = self.recipients(to);
                vec![format!("{} deals {damage} to {to}", self.this)]
            }
            Effect::PreventDamage {
                to,
                prevention,
                from,
            } => vec![self.prevention(to, prevention, from)],
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
            } => vec![self.create_copies(player, of, count, except)],
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
                other @ (Amount::X
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
                | Amount::SpellsCastBefore) => {
                    format!("an amount of {} equal to {}", symbol(kind), c.amount(other))
                }
            })],
            Effect::Modify {
                objects,
                modifications,
                duration,
            } => {
                let text = self.modify(objects, modifications);
                vec![format!("{text}{}", duration_suffix(duration))]
            }
            Effect::ChangeText { objects, duration } => vec![format!(
                "change the text of {} by replacing all instances of one color word with \
                 another or one basic land type with another{}",
                self.objects(objects),
                duration_suffix(duration)
            )],
            Effect::ChangeRule { rule, duration } => {
                if matches!(rule.what, crate::Governed::PreventDamage)
                    && duration == Duration::EndOfTurn
                {
                    return vec![format!("{} this turn", self.rule_change(rule))];
                }
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
            Effect::MayCast {
                player,
                card,
                free,
                free_below,
            } => {
                self.may = true;
                vec![self.clause(player, ("cast", "casts"), |c| {
                    let card = c.objects(card);
                    let mut cast = card;
                    if free {
                        cast.push_str(" without paying its mana cost");
                    }
                    if let Some(below) = free_below {
                        let limit = c.amount(below);
                        format!("{cast} if the resulting spell's mana value is less than {limit}")
                    } else {
                        cast
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
                    other @ (Amount::X
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
                    | Amount::SpellsCastBefore) => c.amount(other),
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
            Effect::AdditionalStep(step) => vec![format!(
                "that player gets an additional {} after this step",
                super::words::step_noun(step),
            )],
            Effect::EndTurn | Effect::RestartGame { .. } => self.end_or_restart(effect),
            Effect::ExtraTurn(player, ability) => {
                let take_turn = self.clause(player, ("take", "takes"), |_| {
                    "an extra turn after this one".to_owned()
                });
                let mut clauses = vec![take_turn];
                if let Some(ability) = ability {
                    let trigger = self
                        .face
                        .and_then(|face| face.abilities.get(ability.index()));
                    if let Some(trigger) = trigger {
                        clauses.extend(self.delayed(trigger));
                    }
                }
                clauses
            }
            Effect::ControlPlayer(player) => vec![format!(
                "you control {} during that player's next turn",
                self.player(player)
            )],
        }
    }

    /// "Two 2/1 white Insect creature tokens with flying", "a Treasure token"
    /// ([CR 111.10](https://yawgatog.com/resources/magic-rules/#R11110)): a predefined token
    /// is named by its subtype alone.
    fn tokens(&mut self, token: &'static CardDef, count: Amount) -> String {
        let Some(face) = token.faces.first() else {
            return self.count_of(count, "token", "tokens");
        };
        // CR 612.4: "A token’s subtypes and rules text are defined by the spell or ability
        // that created the token."
        let subtypes: Vec<&str> = face
            .subtypes
            .read(&self.words)
            .iter()
            .map(crate::Subtype::name)
            .collect();
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
            let mut supertypes = super::words::supertypes(face.supertypes).join(" ");
            if !supertypes.is_empty() {
                supertypes.push(' ');
            }
            let named = format!(
                "{supertypes}{power}/{toughness} {colors} {} {types}",
                subtypes.join(" ")
            );
            // A token the effect names, as "create Marit Lage, a legendary 20/20 black Avatar
            // creature token" does. One the effect doesn't name is named by its subtypes and
            // the word "Token" (CR 111.4), which the text doesn't say.
            if token.name != format!("{} Token", subtypes.join(" ")) && count == Amount::Fixed(1) {
                return format!(
                    "{}, {}",
                    token.name,
                    with_article(&format!("{named} token{with}"))
                );
            }
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
        let clauses = match (ability.trigger(), ability.body()) {
            (Some(trigger), Behavior::Data(effect))
                if names_carried(effect) && matches!(trigger.event, TriggerEvent::Step(_)) =>
            {
                let mut clauses = self.body(ability.body());
                let lead = self.trigger_text(trigger);
                if let Some(last) = clauses.last_mut() {
                    *last = format!("{last} {}", super::words::uncapitalized(&lead));
                }
                clauses
            }
            (Some(_) | None, Behavior::Data(_) | Behavior::Custom(_)) => self.triggered(ability),
        };
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

/// Whether a delayed ability's one instruction acts on the object it carries, which Oracle
/// text says before a step's trigger: "Return that card to the battlefield under its owner's
/// control at the beginning of the next end step."
fn names_carried(effect: &Effect) -> bool {
    effect.operands().objects == Some(&Objects::Carried)
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

impl Ctx {
    /// Whether `objects` is a target that must be a card in its chooser's own graveyard.
    fn own_graveyard_target(&self, objects: Objects) -> bool {
        let Objects::Target(spec) = objects else {
            return false;
        };
        let yours = |filter: &Filter| matches!(filter, Filter::Whose(Players::You));
        self.targets.get(spec.index()).is_some_and(|t| {
            t.kind == crate::TargetKind::GraveyardCard
                && t.filter
                    .conjunction()
                    .map_or_else(|| yours(&t.filter), |parts| parts.iter().any(yours))
        })
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
