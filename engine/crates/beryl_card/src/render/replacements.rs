//! The template of a replacement effect's clauses ([`Replacement`]), such as "this land
//! enters tapped".

use super::Ctx;
use super::filters::{Kind, zone_name};
use super::words::{number, with_article};
use crate::ability::Ability;
use crate::continuous::{ChosenKind, Draws, Replaced, Replacement, Rewrite};
use crate::effect::Players;
use crate::filter::Filter;
use crate::ids::Zone;

impl Ctx {
    /// What "enters tapped unless you control" asks for. Oracle text says "other" of a count
    /// of two or more and "a" of one: "three or more other Islands", "an Island".
    fn controlled(&mut self, n: u8, filter: Filter) -> String {
        let kind = Kind::Permanent;
        let noun = self.noun(filter, kind);
        if n == 1 {
            with_article(&noun.singular(kind))
        } else {
            format!("{} or more other {}", number(n), noun.plural(kind))
        }
    }

    /// A replacement effect's clauses, such as "this land enters tapped" or "if you would
    /// draw a card while your library has no cards in it, you win the game instead"
    /// ([CR 614.1](https://yawgatog.com/resources/magic-rules/#R6141)).
    pub(super) fn replacement(&mut self, ability: &Ability) -> Vec<String> {
        let this = self.this.clone();
        let &Replacement { replaces, rewrite } =
            ability.replacement().unwrap_or(&Replacement::NONE);
        let mut clauses = match (replaces, rewrite) {
            (_, Rewrite::EntersTapped) => vec![format!("{this} enters tapped")],
            (_, Rewrite::EntersPrepared) => vec![format!("{this} enters prepared")],
            (_, Rewrite::EntersTappedUnlessFewLands(n)) => vec![format!(
                "{this} enters tapped unless you control {} or fewer other lands",
                number(n)
            )],
            (_, Rewrite::EntersTappedUnlessYouControl(n, filter)) => {
                vec![format!(
                    "{this} enters tapped unless you control {}",
                    self.controlled(n, *filter)
                )]
            }
            (_, Rewrite::EntersWithCounters(kind, n)) => vec![format!(
                "{this} enters with {} {} counter{} on it",
                number(n),
                kind.name(),
                if n == 1 { "" } else { "s" }
            )],
            (_, Rewrite::EntersTappedUnlessPayLife(n)) => vec![
                format!("as {this} enters, you may pay {n} life"),
                "if you don't, it enters tapped".to_owned(),
            ],
            (_, Rewrite::Choose(kind)) => {
                vec![format!("as {this} enters, choose {}", chosen_kind(kind))]
            }
            (_, Rewrite::EnterAsCopy { of, except }) => vec![self.enter_as_copy(*of, except)],
            (Replaced::PutInto(&objects, zone), Rewrite::ToZone(instead)) => {
                let what = if objects == Filter::Any {
                    "a card or token".to_owned()
                } else {
                    let kind = Kind::Card(None);
                    with_article(&self.noun(objects, kind).singular(kind))
                };
                let place = match zone {
                    Zone::Exile => "exile".to_owned(),
                    zone @ (Zone::Library
                    | Zone::Hand
                    | Zone::Battlefield
                    | Zone::Graveyard
                    | Zone::Stack
                    | Zone::Nowhere
                    | Zone::Merged) => with_article(zone_name(zone)),
                };
                let instead = replacement_destination(instead);
                vec![format!(
                    "if {what} would be put into {place} from anywhere, {instead}"
                )]
            }
            (Replaced::Enters { objects, not_cast }, Rewrite::ToZone(instead)) => {
                let what = with_article(
                    &self
                        .noun(*objects, Kind::Permanent)
                        .singular(Kind::Permanent),
                );
                let cast = if not_cast { " and it wasn't cast" } else { "" };
                let instead = replacement_destination(instead);
                vec![format!("if {what} would enter{cast}, {instead}")]
            }
            (Replaced::Draw(players, draws), Rewrite::Instead) => {
                return self.replaced_draw(ability, players, draws);
            }
            (Replaced::BeginTurn, Rewrite::MaySkipTurnToUntap) => vec![format!(
                "if you would begin your turn while {this} is tapped, you may skip that turn instead. If you do, untap {this}"
            )],
            (Replaced::Damage, Rewrite::Multiply(n)) => vec![super::prevention::multiplied(n)],
            (Replaced::Tokens { players }, Rewrite::Multiply(n)) => {
                vec![self.replaced_tokens(players, TokenCount::Multiply(n))]
            }
            (Replaced::Tokens { players }, Rewrite::Plus(n)) => {
                vec![self.replaced_tokens(players, TokenCount::Plus(n))]
            }
            (
                Replaced::None
                | Replaced::ThisEnters
                | Replaced::Draw(..)
                | Replaced::PutInto(..)
                | Replaced::Enters { .. }
                | Replaced::BeginTurn
                | Replaced::Damage
                | Replaced::Tokens { .. },
                Rewrite::None
                | Rewrite::ToZone(_)
                | Rewrite::MaySkipTurnToUntap
                | Rewrite::Instead
                | Rewrite::Dredge(_)
                | Rewrite::Multiply(_)
                | Rewrite::Plus(_),
            ) => self.body(ability.body()),
        };
        if let Some(&condition) = ability.condition() {
            let condition = self.condition(condition);
            if let Some(first) = clauses.first_mut() {
                *first = format!("if {condition}, {first}");
            }
        }
        clauses
    }

    /// "If [player] would draw a card", including any restriction on which draws it replaces.
    fn replaced_draw(&mut self, ability: &Ability, players: Players, draws: Draws) -> Vec<String> {
        let except = match draws {
            Draws::Each => "",
            Draws::ExceptFirstInDrawStep => {
                " except the first one they draw in each of their draw steps"
            }
        };
        let condition = ability
            .condition()
            .map(|&c| format!(" while {}", self.condition(c)))
            .unwrap_or_default();
        // One player at a time would draw: "a player", "an opponent".
        let who = match players {
            Players::EachPlayer => "a player".to_owned(),
            Players::EachOpponent => "an opponent".to_owned(),
            players @ (Players::You
            | Players::Target(_)
            | Players::ControllerOf(_)
            | Players::OwnerOf(_)
            | Players::Enchanted
            | Players::ThatPlayer
            | Players::SubgameNonwinners
            | Players::EachOther(_)) => self.player(players),
        };
        let mut instead = self.body(ability.body());
        if let Some(first) = instead.first_mut() {
            let you = if first.starts_with("you ") || first.starts_with("that player ") {
                ""
            } else {
                "you "
            };
            *first = format!("if {who} would draw a card{except}{condition}, {you}{first} instead");
        }
        instead
    }

    /// "If an effect would create one or more tokens under [players'] control, it creates
    /// [count] instead."
    fn replaced_tokens(&mut self, players: Players, count: TokenCount) -> String {
        let under = match players {
            Players::You => "your".to_owned(),
            Players::EachOpponent => "an opponent's".to_owned(),
            Players::EachPlayer => "a player's".to_owned(),
            players @ (Players::Target(_)
            | Players::ControllerOf(_)
            | Players::OwnerOf(_)
            | Players::Enchanted
            | Players::ThatPlayer
            | Players::SubgameNonwinners
            | Players::EachOther(_)) => format!("{}'s", self.player(players)),
        };
        let many = match count {
            TokenCount::Multiply(2) => "twice that many of those tokens".to_owned(),
            TokenCount::Multiply(n) => format!("{} times that many of those tokens", number(n)),
            TokenCount::Plus(1) => "those tokens plus an additional token".to_owned(),
            TokenCount::Plus(n) => format!("that many tokens plus {}", number(n)),
        };
        format!(
            "if an effect would create one or more tokens under {under} control, it creates {many} \
             instead"
        )
    }
}

/// How a token-creation replacement rewrites the count.
#[derive(Clone, Copy)]
enum TokenCount {
    Multiply(u8),
    Plus(u8),
}

/// What a replacement does when it changes where an object goes.
fn replacement_destination(zone: Zone) -> String {
    match zone {
        Zone::Exile => "exile it instead".to_owned(),
        zone @ (Zone::Library
        | Zone::Hand
        | Zone::Battlefield
        | Zone::Graveyard
        | Zone::Stack
        | Zone::Nowhere
        | Zone::Merged) => {
            format!("put it into its owner's {} instead", zone_name(zone))
        }
    }
}

/// What a player chooses a value of `kind` as: "a color", "a card name".
pub(super) const fn chosen_kind(kind: ChosenKind) -> &'static str {
    match kind {
        ChosenKind::Color => "a color",
        ChosenKind::CreatureType => "a creature type",
        ChosenKind::CardName => "a card name",
        ChosenKind::Word => "a color word or a basic land type",
    }
}
