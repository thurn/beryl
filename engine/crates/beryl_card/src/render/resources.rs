//! Templates for what a player gets, loses or counts out: cards drawn, discarded and milled,
//! life, mana, counters, tokens, and permanents untapped.

use super::Ctx;
use super::filters::Kind;
use super::words::{mana_set, number, symbol, with_article};
use crate::characteristics::Types;
use crate::counter::CounterKind;
use crate::definition::CardDef;
use crate::effect::{Amount, Objects, Players};
use crate::mana::{ManaColor, Term};

impl Ctx {
    /// "Discard two cards", or "discard your hand".
    pub(super) fn discard(&mut self, player: Players, count: Amount) -> Vec<String> {
        let hand = crate::effect::build::hand_of(&Players::You);
        let clause =
            if player == Players::You && matches!(count, Amount::Count(cards) if *cards == hand) {
                self.clause(player, ("discard", "discards"), |_| "your hand".to_owned())
            } else {
                self.cards(player, ("discard", "discards"), count)
            };
        self.discarded = Some(self.plural);
        self.those_named = None;
        vec![clause]
    }

    /// "Create a 1/1 white Soldier creature token that's tapped and attacking that player or
    /// a planeswalker they control".
    pub(super) fn create_attacking_tokens(
        &mut self,
        player: Players,
        token: &'static CardDef,
        count: Amount,
        attacking: Players,
    ) -> Vec<String> {
        vec![self.clause(player, ("create", "creates"), |c| {
            let tokens = c.tokens(token, count);
            let that = if matches!(count, Amount::Fixed(1)) {
                "that's"
            } else {
                "that are"
            };
            let whom = c.player(attacking);
            format!("{tokens} {that} tapped and attacking {whom} or a planeswalker they control")
        })]
    }

    /// "Put a +1/+1 counter on target creature", or with `on` false, "remove two charge
    /// counters from this artifact".
    pub(super) fn counters(
        &mut self,
        objects: Objects,
        kind: CounterKind,
        count: Amount,
        on: bool,
    ) -> Vec<String> {
        let name = kind.name();
        let counters = self.count_of(
            count,
            &format!("{name} counter"),
            &format!("{name} counters"),
        );
        let (verb, word) = if on {
            (("put", "puts"), "on")
        } else {
            (("remove", "removes"), "from")
        };
        let rest = format!("{counters} {word} {}", self.objects(objects));
        vec![self.clause(Players::You, verb, |_| rest.clone())]
    }

    /// "Remove all counters from it".
    pub(super) fn remove_all_counters(&mut self, objects: Objects) -> Vec<String> {
        let from = format!("all counters from {}", self.objects(objects));
        vec![self.clause(Players::You, ("remove", "removes"), |_| from.clone())]
    }

    /// "Double the number of +1/+1 counters on each creature you control".
    pub(super) fn double_counters(&mut self, objects: Objects, kind: CounterKind) -> Vec<String> {
        let target = if let Objects::All { zone, filter } = objects {
            let kind = Kind::of_zone(zone);
            format!("each {}", self.noun(filter, kind).singular(kind))
        } else {
            self.objects(objects)
        };
        let on = format!("the number of {} counters on {target}", kind.name());
        vec![self.clause(Players::You, ("double", "doubles"), |_| on.clone())]
    }

    /// "Add {G}{G}", or "add an amount of {G} equal to its power".
    pub(super) fn add_mana_of_kind(
        &mut self,
        player: Players,
        kind: ManaColor,
        amount: Amount,
    ) -> Vec<String> {
        vec![self.clause(player, ("add", "adds"), |c| match amount {
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
        })]
    }

    /// "Add one mana of any color", the kinds a mana ability's activator chooses from.
    pub(super) fn add_mana(&mut self, player: Players, mana: crate::ManaSet) -> Vec<String> {
        vec![self.clause(player, ("add", "adds"), |_| mana_set(mana))]
    }

    /// "Add {W}{U}", "add {B} for each Swamp you control" or "add {W}{W}, {W}{U}, or {U}{U}":
    /// the terms of the mana ability being written ([`crate::Ability::product`]).
    pub(super) fn add_product(&mut self, player: Players) -> Vec<String> {
        let terms = self.ability.map_or(&[][..], |a| a.product());
        let mut parts: Vec<String> = Vec::new();
        for term in terms {
            let text = self.term(*term);
            // Fixed symbols run together, as "{W}{U}" does.
            match parts.last_mut() {
                Some(last)
                    if last.ends_with('}') && text.starts_with('{') && !text.contains(' ') =>
                {
                    last.push_str(&text);
                }
                _ => parts.push(text),
            }
        }
        let text = parts.join(" and ");
        vec![self.clause(player, ("add", "adds"), |_| text.clone())]
    }

    /// One term of a mana ability's product ([`Term`]).
    fn term(&mut self, term: Term) -> String {
        let bits = match term.kinds {
            crate::mana::Kinds::Fixed(bits) => bits,
            crate::mana::Kinds::CouldProduce(filter) => {
                let kind = super::filters::Kind::Permanent;
                let what = self.noun(*filter, kind).indefinite(kind);
                let count = if let Amount::Fixed(n) = term.amount {
                    number(n)
                } else {
                    self.amount(term.amount)
                };
                return format!("{count} mana of any type that {what} could produce");
            }
        };
        let kinds: Vec<ManaColor> = ManaColor::ALL
            .into_iter()
            .filter(|&color| bits & (1 << color.index()) != 0)
            .collect();
        match (term.amount, kinds.as_slice()) {
            (Amount::Fixed(n), [kind]) => symbol(*kind).repeat(usize::from(n)),
            (Amount::Fixed(n), _) if term.uniform => mana_set(crate::ManaSet {
                colors: bits,
                amount: n,
            }),
            // Each unit of its own kind: every choice, as "{W}{W}, {W}{U}, or {U}{U}" lists.
            (Amount::Fixed(n), _) => {
                let each: Vec<String> = choices(&kinds, usize::from(n))
                    .iter()
                    .map(|choice| choice.iter().map(|&k| symbol(k)).collect::<String>())
                    .collect();
                super::words::list(&each, "or")
            }
            (Amount::Count(objects), [kind]) => {
                format!(
                    "{} for each {}",
                    symbol(*kind),
                    self.one_of_counted(*objects)
                )
            }
            (other, _) => format!(
                "an amount of {} equal to {}",
                kinds
                    .iter()
                    .map(|&k| symbol(k))
                    .collect::<Vec<_>>()
                    .join(" or "),
                self.amount(other)
            ),
        }
    }

    /// "Untap up to two lands".
    pub(super) fn untap_up_to(
        &mut self,
        player: Players,
        count: Amount,
        permanents: Objects,
    ) -> Vec<String> {
        vec![self.clause(player, ("untap", "untaps"), |c| {
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
        })]
    }

    /// "Two 2/1 white Insect creature tokens with flying", "a Treasure token"
    /// ([CR 111.10](https://yawgatog.com/resources/magic-rules/#R11110)): a predefined token
    /// is named by its subtype alone.
    pub(super) fn tokens(&mut self, token: &'static CardDef, count: Amount) -> String {
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
            // CR 111.3: "The spell or ability that creates a token may define the values of
            // any number of characteristics for the token."
            let mut keywords = super::words::keywords(face.keywords);
            keywords.extend(face.abilities.iter().map(|ability| self.quoted(ability)));
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
}

/// Every way to choose `n` of `kinds` with repeats, each in the order of `kinds` and the
/// choices in that order too: {W}{W}, {W}{U}, {U}{U}.
fn choices(kinds: &[ManaColor], n: usize) -> Vec<Vec<ManaColor>> {
    if n == 0 {
        return vec![Vec::new()];
    }
    let mut out = Vec::new();
    for (i, &kind) in kinds.iter().enumerate() {
        for mut rest in choices(&kinds[i..], n - 1) {
            rest.insert(0, kind);
            out.push(rest);
        }
    }
    out
}
