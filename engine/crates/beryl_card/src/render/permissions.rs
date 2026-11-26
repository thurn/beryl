//! Templates for instructions that let a player cast or play a card, or change a rule of the
//! game for a while: "you may cast it this turn", "it gains flashback until end of turn",
//! "until end of turn, you may play lands from your graveyard".

use super::Ctx;
use super::filters::Kind;
use crate::continuous::{Added, Duration, Governed, ManaChange, RuleChange, Tapped};
use crate::effect::{Amount, CastFace, CastTerms, Objects, Players};

impl Ctx {
    /// A rule change from a resolving spell or ability, with how long it lasts: "until end of
    /// turn, ..." ahead of it, or "this turn" after a prevention of all damage.
    pub(super) fn change_rule(&mut self, rule: &RuleChange, duration: Duration) -> Vec<String> {
        if matches!(rule.what, Governed::PreventDamage) && duration == Duration::EndOfTurn {
            return vec![format!("{} this turn", self.rule_change(rule))];
        }
        // "Target creature must be blocked this turn if able."
        if let (Governed::MustBeBlocked(objects), Duration::EndOfTurn) = (rule.what, duration) {
            let attackers = self.objects(objects);
            return vec![format!("{attackers} must be blocked this turn if able")];
        }
        let prefix = match duration {
            Duration::EndOfTurn => "until end of turn, ",
            Duration::UntilYourNextTurn => "until your next turn, ",
            Duration::Indefinite => "",
        };
        vec![format!("{prefix}{}", self.rule_change(rule))]
    }

    /// A mana change from a resolving spell or ability, with how long it lasts:
    /// "until end of turn, whenever a player taps an Island for mana, that player adds an
    /// additional {U}".
    pub(super) fn change_mana(&mut self, change: &ManaChange, duration: Duration) -> Vec<String> {
        let prefix = match duration {
            Duration::EndOfTurn => "until end of turn, ",
            Duration::UntilYourNextTurn => "until your next turn, ",
            Duration::Indefinite => "",
        };
        let body = match *change {
            ManaChange::Adds { tapped, added } => {
                let what = match tapped {
                    Tapped::A(filter) => self
                        .noun(filter, Kind::Permanent)
                        .indefinite(Kind::Permanent),
                    Tapped::AttachedTo => "enchanted permanent".to_owned(),
                };
                let mana = match added {
                    Added::Of(set) => super::words::mana_set(set),
                    Added::TypeProduced => {
                        "one mana of any type that permanent produced".to_owned()
                    }
                };
                format!(
                    "whenever a player taps {what} for mana, that player adds an additional {mana}"
                )
            }
            ManaChange::Multiplies { tapped, by } => {
                let what = self
                    .noun(tapped, Kind::Permanent)
                    .indefinite(Kind::Permanent);
                let times = match by {
                    2 => "twice".to_owned(),
                    3 => "three times".to_owned(),
                    n => format!("{n} times"),
                };
                format!(
                    "if you tap {what} for mana, it produces {times} as much of that mana instead"
                )
            }
        };
        vec![format!("{prefix}{body}")]
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
    #[allow(clippy::too_many_arguments)]
    pub(super) fn may_cast(
        &mut self,
        player: Players,
        card: Objects,
        free: bool,
        free_below: Option<Amount>,
        copy: bool,
        face: CastFace,
        within_x: bool,
    ) -> Vec<String> {
        if within_x || face != CastFace::Up {
            return self.cast_face_down(player, card, free, face, within_x);
        }
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

    /// "You may choose a creature card in your hand whose mana cost could be paid by some
    /// amount of, or all of, the mana you spent on {X}. If you do, you may cast that card face
    /// down as a 2/2 creature spell without paying its mana cost", and the replacement effect
    /// of the permanent that spell becomes.
    fn cast_face_down(
        &mut self,
        player: Players,
        card: Objects,
        free: bool,
        face: CastFace,
        within_x: bool,
    ) -> Vec<String> {
        let mut clauses = Vec::new();
        let it = if within_x {
            let what = self.one_of(card);
            let whose = format!(
                "{what} whose mana cost could be paid by some amount of, or all of, the mana \
                 you spent on {{X}}"
            );
            self.may = true;
            clauses.push(self.clause(player, ("choose", "chooses"), |_| whose));
            let they = if self.player(player) == "you" {
                "you"
            } else {
                "they"
            };
            format!("if {they} do, {they} may cast that card")
        } else {
            self.may = true;
            let what = self.objects(card);
            self.clause(player, ("cast", "casts"), |_| what)
        };
        let down = match face {
            CastFace::Up => "",
            CastFace::DownTurnedUpWhenUsed => " face down as a 2/2 creature spell",
        };
        let free = if free {
            " without paying its mana cost"
        } else {
            ""
        };
        clauses.push(format!("{it}{down}{free}"));
        if face == CastFace::DownTurnedUpWhenUsed {
            clauses.push(
                "if the creature that spell becomes as it resolves has not been turned face up \
                 and would assign or deal damage, be dealt damage, or become tapped, instead \
                 it's turned face up and assigns or deals damage, is dealt damage, or becomes \
                 tapped"
                    .to_owned(),
            );
        }
        clauses
    }
}
