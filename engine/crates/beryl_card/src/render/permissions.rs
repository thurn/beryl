//! Templates for instructions that let a player cast or play a card, or change a rule of the
//! game for a while: "you may cast it this turn", "it gains flashback until end of turn",
//! "until end of turn, you may play lands from your graveyard".

use super::Ctx;
use crate::continuous::{Duration, Governed, RuleChange};
use crate::effect::{Amount, CastTerms, Objects, Players};

impl Ctx {
    /// A rule change from a resolving spell or ability, with how long it lasts: "until end of
    /// turn, ..." ahead of it, or "this turn" after a prevention of all damage.
    pub(super) fn change_rule(&mut self, rule: &RuleChange, duration: Duration) -> Vec<String> {
        if matches!(rule.what, Governed::PreventDamage) && duration == Duration::EndOfTurn {
            return vec![format!("{} this turn", self.rule_change(rule))];
        }
        let prefix = match duration {
            Duration::EndOfTurn => "until end of turn, ",
            Duration::UntilYourNextTurn => "until your next turn, ",
            Duration::Indefinite => "",
        };
        vec![format!("{prefix}{}", self.rule_change(rule))]
    }

    /// "Target instant card in your graveyard gains flashback until end of turn. The
    /// flashback cost is equal to its mana cost."
    pub(super) fn gain_flashback(&mut self, cards: Objects) -> Vec<String> {
        let plural = self.is_plural(cards);
        let what = self.objects(cards);
        let gains = if plural { "gain" } else { "gains" };
        vec![
            format!("{what} {gains} flashback until end of turn"),
            "the flashback cost is equal to its mana cost".to_owned(),
        ]
    }

    /// "You may cast that card this turn", "for as long as that card remains exiled, you may
    /// play it", and the terms a spell cast this way is cast on.
    pub(super) fn permit_cast(
        &mut self,
        player: Players,
        cards: Objects,
        duration: Duration,
        terms: CastTerms,
    ) -> Vec<String> {
        let what = self.taken_from(cards);
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

    /// "You may cast that card without paying its mana cost", or with `copy`, "copy that
    /// card. You may cast the copy without paying its mana cost" (see CR 707.12).
    pub(super) fn may_cast(
        &mut self,
        player: Players,
        card: Objects,
        free: bool,
        free_below: Option<Amount>,
        copy: bool,
    ) -> Vec<String> {
        let mut clauses = self.cast_offer(player, card, free, free_below);
        if copy {
            let named = self.objects(card);
            for clause in &mut clauses {
                *clause = clause.replacen(&format!(" {named}"), " the copy", 1);
            }
            // A new sentence names a card an instruction before moved as "that card".
            let copied = if named == "it" { "that card" } else { &named };
            clauses.insert(0, format!("copy {copied}"));
        }
        clauses
    }
}
