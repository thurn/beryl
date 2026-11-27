//! How the clauses of an effect are written and joined: a clause about a player
//! ("draw a card", "each opponent loses 2 life"), an instruction you carry out on objects
//! ("destroy target creature"), and two instructions joined by ", then".

use super::Ctx;
use super::filters::Kind;
use crate::cost::Cost;
use crate::effect::{Amount, Condition, Effect, ExileMatch, LibraryPosition, Objects, Players};
use crate::ids::Zone;
use crate::trigger::{Subject, Trigger};

impl Ctx {
    /// A clause about `player`: "draw a card" for you, "you may draw a card" after a "may",
    /// "each opponent loses 2 life" for another player. Within `rest`, a player's things are
    /// "your" or "their" things when they are the subject's.
    pub(super) fn clause(
        &mut self,
        player: Players,
        (base, third): (&str, &str),
        rest: impl FnOnce(&mut Ctx) -> String,
    ) -> String {
        let may = std::mem::take(&mut self.may);
        let continued = std::mem::take(&mut self.continued);
        let subject = (player != Players::You).then(|| self.player(player));
        self.other_subject |= subject.is_some();
        let saved = self.subject.replace(player);
        let rest = rest(self);
        self.subject = saved;
        let rest = if rest.is_empty() {
            rest
        } else {
            format!(" {rest}")
        };
        match (subject, may) {
            (Some(_), false) if continued => format!("{third}{rest}"),
            (None, false) if self.state_you => format!("you {base}{rest}"),
            (None, false) => format!("{base}{rest}"),
            (None, true) => format!("you may {base}{rest}"),
            (Some(who), false) => format!("{who} {third}{rest}"),
            (Some(who), true) => format!("{who} may {base}{rest}"),
        }
    }

    /// A clause that says "you" where another would say nothing: "you gain 2 life", "you
    /// win the game", as Oracle text words these.
    pub(super) fn stated(
        &mut self,
        player: Players,
        verb: (&str, &str),
        rest: impl FnOnce(&mut Ctx) -> String,
    ) -> String {
        let plain = player == Players::You && !self.may && !self.continued && !self.state_you;
        let clause = self.clause(player, verb, rest);
        if plain {
            format!("you {clause}")
        } else {
            clause
        }
    }

    /// "Each player shuffles ...": the players as the subject of each clause about one of
    /// them, or "for each opponent, ..." when the clauses are about someone else.
    pub(super) fn for_each_player(&mut self, players: Players, effect: &Effect) -> Vec<String> {
        if subject_of(effect) == Some(Players::ThatPlayer) {
            let saved = self.group.replace(players);
            let clauses = self.effect(effect);
            self.group = saved;
            return clauses;
        }
        let each = match players {
            Players::EachOpponent => "opponent".to_owned(),
            Players::EachPlayer => "player".to_owned(),
            other @ (Players::You
            | Players::Target(_)
            | Players::ControllerOf(_)
            | Players::OwnerOf(_)
            | Players::Enchanted
            | Players::ThatPlayer
            | Players::SubgameNonwinners
            | Players::EachOther(_)) => self.player(other),
        };
        let mut clauses = self.effect(effect);
        prefix(&mut clauses, &format!("for each {each}, "));
        clauses
    }

    /// A conditional instruction, followed by its alternative when it has one.
    pub(super) fn conditional(
        &mut self,
        condition: Condition,
        then: &Effect,
        otherwise: Option<&Effect>,
    ) -> Vec<String> {
        if otherwise.is_none()
            && let Condition::Not(inner @ Condition::AttackedThisTurn(_)) = condition
        {
            let unless = self.condition(*inner);
            let mut clauses = self.effect(then);
            match clauses.first_mut() {
                Some(first) => {
                    first.push_str(" unless ");
                    first.push_str(&unless);
                }
                None => clauses.push(format!("unless {unless}")),
            }
            return clauses;
        }
        let cast_trigger = self.trigger.is_some_and(|trigger| trigger.event.is_cast());
        let did = matches!(condition, Condition::Did);
        // After a clause about another player, the outcome of a condition about a card says
        // who acts: "If a card with the chosen name was milled this way, you draw two cards."
        let state_you = self.other_subject && matches!(condition, Condition::HasChosenName(_));
        let condition = self.condition(condition);
        let saved = std::mem::replace(&mut self.state_you, state_you);
        let mut clauses = self.effect(then);
        if did
            && cast_trigger
            && let Effect::MayCast {
                player: Players::ThatPlayer,
                ..
            } = then
            && let Some(clause) = clauses.first_mut()
            && let Some(cast) = clause.strip_prefix("that player ")
        {
            *clause = format!("if the player does, they {cast}");
        } else {
            prefix(&mut clauses, &format!("if {condition}, "));
        }
        if let Some(otherwise) = otherwise {
            let mut more = self.effect(otherwise);
            prefix(&mut more, "otherwise, ");
            clauses.extend(more);
        }
        self.state_you = saved;
        clauses
    }

    /// A player casts a spell, optionally specifying its previous zone.
    pub(super) fn cast_trigger_text(
        &mut self,
        trigger: &Trigger,
        cast: &str,
        from: Option<Zone>,
    ) -> String {
        use super::words::ordinal;
        use crate::trigger::CastNumber;

        if let Some(number) = trigger.cast_number {
            return match number {
                CastNumber::InTurn(n) => {
                    format!("whenever the {} spell of a turn is cast", ordinal(n))
                }
                CastNumber::ByCaster(n) => {
                    format!("whenever {cast} their {} spell each turn", ordinal(n))
                }
            };
        }

        let spell = match trigger.subject {
            Subject::A(filter) => self.noun(filter, Kind::Spell).indefinite(Kind::Spell),
            // Only the player who casts a spell controls its cast trigger, so Oracle text says
            // "When you cast this spell".
            Subject::This if from.is_none() => return "when you cast this spell".to_owned(),
            Subject::This => self.this.clone(),
        };
        if let Some(zone) = from {
            let name = super::filters::zone_name(zone);
            if zone == Zone::Hand {
                let whose = if trigger.whose == Players::You {
                    "your"
                } else {
                    "their"
                };
                format!("whenever {cast} {spell} from {whose} {name}")
            } else {
                format!("whenever {cast} {spell} from {name}")
            }
        } else {
            format!("whenever {cast} {spell}")
        }
    }

    /// An optional cast from the cards the previous instruction made available.
    pub(super) fn cast_offer(
        &mut self,
        player: Players,
        card: Objects,
        free: bool,
        free_below: Option<Amount>,
    ) -> Vec<String> {
        self.may = true;
        vec![self.clause(player, ("cast", "casts"), |c| {
            let name = c.objects(card);
            let mut cast = if matches!(card, Objects::ExceptThose(&Objects::ExiledWithThis)) {
                format!("a spell from among {name}")
            } else {
                name
            };
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

    /// Each instruction's clauses in turn, two that Oracle text joins with ", then" joined
    /// so: "draw a card, then discard a card".
    pub(super) fn sequence(&mut self, effects: &[Effect]) -> Vec<String> {
        if let [
            Effect::ExileUntil {
                player: Players::ThatPlayer,
                matching: ExileMatch::SharesTypeWithTriggeringSpell,
                when_found,
            },
            Effect::PutIntoLibrary {
                objects: Objects::Those,
                position: LibraryPosition::BottomRandom,
            },
        ] = effects
        {
            let mut clauses = self.exile_until_clauses(
                Players::ThatPlayer,
                ExileMatch::SharesTypeWithTriggeringSpell,
                when_found,
            );
            clauses.push(format!(
                "then they put all cards exiled with {} on the bottom of their library in a random order",
                self.this,
            ));
            return clauses;
        }
        if let Some(clause) = self.chosen_sacrifice(effects) {
            return vec![clause];
        }
        let mut clauses: Vec<String> = Vec::new();
        let mut previous: Option<&Effect> = None;
        let mut effects = effects.iter().peekable();
        while let Some(effect) = effects.next() {
            if let Some(clause) = effects.peek().and_then(|&next| {
                self.hidden_put(effect, next)
                    .or_else(|| self.chosen_exile(effect, next))
                    .or_else(|| self.chosen_return_to_hand(effect, next))
                    .or_else(|| self.chosen_at_random(effect, next))
            }) {
                effects.next();
                clauses.push(clause);
                previous = None;
                continue;
            }
            let then = previous.is_some_and(|before| joined_by_then(before, effect));
            let and = previous.is_some_and(|before| joined_by_and(before, effect));
            // "Draw three cards. Then discard two cards unless you discard a basic land
            // card": an instruction Oracle would join with ", then" starts a sentence of its
            // own with "Then" when an "unless" follows it.
            let then_unless = previous.is_some_and(|before| {
                matches!(
                    effect,
                    Effect::Unless { effect: inner, .. }
                        | Effect::If {
                            condition: Condition::Not(Condition::AttackedThisTurn(_)),
                            then: inner,
                            otherwise: None,
                        }
                        if joined_by_then(before, inner)
                )
            });
            self.continued = then && previous.and_then(subject_of) == subject_of(effect);
            let mut more = self.effect(effect);
            self.continued = false;
            if then
                && let Some(last) = clauses.last_mut()
                && !more.is_empty()
            {
                last.push_str(", then ");
                last.push_str(&more.remove(0));
            } else if and
                && let Some(last) = clauses.last_mut()
                && !more.is_empty()
            {
                last.push_str(" and ");
                last.push_str(&more.remove(0));
            }
            if then_unless && let Some(first) = more.first_mut() {
                *first = format!("then {first}");
            }
            clauses.extend(more);
            previous = Some(effect);
        }
        clauses
    }

    /// A card chosen at random from a zone and returned to its owner's hand.
    fn chosen_at_random(&mut self, choose: &Effect, next: &Effect) -> Option<String> {
        let &Effect::ChooseAtRandom(Objects::InZoneOf {
            zone,
            players,
            filter,
        }) = choose
        else {
            return None;
        };
        let &Effect::ReturnToHand(Objects::Those) = next else {
            return None;
        };
        self.discarded = None;
        self.those_named = None;
        self.plural = false;
        Some(self.clause(Players::You, ("return", "returns"), |c| {
            let kind = super::filters::Kind::Card(None);
            let card = c.noun(filter, kind).indefinite(kind);
            let from = c.possessive(*players);
            let to = if *players == Players::You {
                "your hand"
            } else {
                "its owner's hand"
            };
            format!(
                "{card} at random from {from} {} to {to}",
                super::filters::zone_name(zone)
            )
        }))
    }

    /// A player chooses a card from their own hand to exile.
    fn chosen_exile(&mut self, choose: &Effect, exile: &Effect) -> Option<String> {
        let Effect::Choose {
            player,
            among:
                Objects::InZoneOf {
                    zone: crate::Zone::Hand,
                    players,
                    filter,
                },
            optional: false,
            hidden: false,
            ..
        } = choose
        else {
            return None;
        };
        if player != *players
            || !matches!(
                exile,
                Effect::Exile(Objects::Those) | Effect::ExileLinked(Objects::Those)
            )
        {
            return None;
        }
        self.discarded = None;
        self.those_named = None;
        self.plural = false;
        Some(self.clause(*player, ("exile", "exiles"), |ctx| {
            let kind = super::filters::Kind::Card(Some(crate::Zone::Hand));
            let card = ctx.noun(*filter, kind).indefinite(kind);
            format!("{card} from {} hand", ctx.possessive(*player))
        }))
    }

    /// A player chooses a permanent they control, then returns it to its owner's hand.
    fn chosen_return_to_hand(&mut self, choose: &Effect, bounce: &Effect) -> Option<String> {
        let Effect::Choose {
            player,
            among:
                Objects::InZoneOf {
                    zone: crate::Zone::Battlefield,
                    players,
                    filter,
                },
            optional: false,
            hidden: false,
            ..
        } = choose
        else {
            return None;
        };
        if player != *players || !matches!(bounce, Effect::ReturnToHand(Objects::Those)) {
            return None;
        }
        self.discarded = None;
        self.those_named = None;
        self.plural = false;
        Some(self.clause(*player, ("return", "returns"), |ctx| {
            let kind = super::filters::Kind::Permanent;
            let perm = ctx.noun(*filter, kind).indefinite(kind);
            let control = if *player == Players::You {
                "you control"
            } else {
                "they control"
            };
            format!("{perm} {control} to its owner's hand")
        }))
    }

    /// A player chooses a permanent they control, then sacrifices the chosen permanent.
    fn chosen_sacrifice(&mut self, effects: &[Effect]) -> Option<String> {
        let [
            Effect::ForEachPlayer {
                players,
                effect:
                    Effect::Choose {
                        player: Players::ThatPlayer,
                        among:
                            Objects::InZoneOf {
                                zone: crate::Zone::Battlefield,
                                players: Players::ThatPlayer,
                                filter,
                            },
                        optional: false,
                        hidden: false,
                        ..
                    },
            },
            Effect::Sacrifice(Objects::Those),
        ] = effects
        else {
            return None;
        };
        let kind = super::filters::Kind::Permanent;
        self.plural = true;
        Some(self.clause(*players, ("sacrifice", "sacrifices"), |ctx| {
            format!(
                "{} of their choice",
                ctx.noun(*filter, kind).indefinite(kind)
            )
        }))
    }

    /// The verb for the clause being written: `base` for you or after "may", `third`
    /// otherwise.
    pub(super) fn verb<'a>(&self, base: &'a str, third: &'a str) -> &'a str {
        match self.subject {
            Some(Players::You) | None => base,
            Some(_) => third,
        }
    }

    /// "Destroy target creature", an instruction you carry out on objects, with `rest` after
    /// them. What it acted on is what "it" or "them" means next.
    pub(super) fn acts(&mut self, verb: (&str, &str), objects: Objects, rest: &str) -> String {
        // What this instruction acts on is what "those" means next, not a discarded card.
        self.discarded = None;
        self.those_named = None;
        let plural = self.is_plural(objects);
        let clause = self.clause(Players::You, verb, |c| {
            format!("{}{rest}", c.moved(objects))
        });
        self.plural = plural;
        clause
    }

    /// Objects an instruction moves: a whole zone is "all cards from target player's
    /// library".
    pub(super) fn moved(&mut self, objects: Objects) -> String {
        match objects {
            // "Exile target player's graveyard": a graveyard is named whole.
            Objects::InZoneOf {
                zone: crate::Zone::Graveyard,
                players,
                filter: crate::Filter::Any,
            } => format!("{} graveyard", self.possessive(*players)),
            Objects::InZoneOf {
                zone,
                players,
                filter: crate::Filter::Any,
            } => format!(
                "all cards from {} {}",
                self.possessive(*players),
                super::filters::zone_name(zone)
            ),
            other @ (Objects::This
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
            | Objects::Other(_)) => self.taken_from(other),
        }
    }

    /// Objects an instruction takes from where they are: a target card in your own
    /// graveyard is "target creature card from your graveyard", as in "exile target instant
    /// card from your graveyard" or "you may cast target sorcery card from your graveyard".
    pub(super) fn taken_from(&mut self, objects: Objects) -> String {
        let own = self.own_graveyard_target(objects);
        let said = self.objects(objects);
        if own {
            said.replacen(" in your graveyard", " from your graveyard", 1)
        } else {
            said
        }
    }

    /// "Draw two cards", "target player mills two cards".
    pub(super) fn cards(&mut self, player: Players, verb: (&str, &str), count: Amount) -> String {
        self.discarded = None;
        self.those_named = None;
        let clause = self.clause(player, verb, |c| c.count_of(count, "card", "cards"));
        self.plural = !matches!(count, Amount::Fixed(1));
        clause
    }

    /// What follows "unless" in "[effect] unless [player] pays [cost]", such as "you discard
    /// a basic land card" or "its controller pays {3}" (CR 118.12a).
    pub(super) fn unless(&mut self, player: Players, cost: Cost) -> String {
        let who = self.player(player);
        let you = player == Players::You;
        if cost.parts().is_empty() {
            let pays = if you { "pay" } else { "pays" };
            return format!("{who} {pays} {}", super::words::mana_text(cost.mana));
        }
        let actions = self.cost_actions(cost);
        let actions = if you {
            actions
        } else {
            actions
                .iter()
                .map(|action| third_person(action))
                .collect::<Vec<_>>()
        };
        format!("{who} {}", super::words::list(&actions, "and"))
    }
}

/// Puts `words` before the first clause.
pub(super) fn prefix(clauses: &mut [String], words: &str) {
    if let Some(first) = clauses.first_mut() {
        *first = format!("{words}{}", super::words::uncapitalized(first));
    }
}

/// An action said of another player: "discard a card" becomes "discards a card".
fn third_person(action: &str) -> String {
    match action.split_once(' ') {
        Some((verb, rest)) => format!("{verb}s {rest}"),
        None => format!("{action}s"),
    }
}

/// Whether Oracle text joins two instructions with ", then": "draw a card, then discard a
/// card", "shuffles ... into their library, then draws seven cards".
fn joined_by_then(before: &Effect, after: &Effect) -> bool {
    matches!(
        (before, after),
        (
            Effect::Draw { .. },
            Effect::Discard { .. } | Effect::PutBack { .. }
        ) | (
            Effect::Discard { .. } | Effect::ShuffleInto { .. } | Effect::PutAnyOnBottom { .. },
            Effect::Draw { .. }
        ) | (
            Effect::Exile(Objects::InZoneOf { .. }),
            Effect::ShuffleInto { .. }
        ) | (
            // "Choose a card name, then target player mills a card".
            Effect::ChooseValue { .. },
            Effect::Mill { .. }
        ) | (
            // "Exile Jace, then return him to the battlefield transformed".
            Effect::Exile(Objects::This),
            Effect::ReturnTransformed(Objects::This | Objects::Those)
        )
    )
}

/// Whether Oracle text joins two instructions with "and": a player losing life and you
/// gaining it, "target player loses 2 life and you gain 2 life".
fn joined_by_and(before: &Effect, after: &Effect) -> bool {
    matches!(
        (before, after),
        (
            Effect::LoseLife { .. },
            Effect::GainLife {
                player: Players::You,
                ..
            }
        )
    )
}

/// The player an instruction is about, when it is one a player carries out.
pub(super) fn subject_of(effect: &Effect) -> Option<Players> {
    match *effect {
        Effect::Sequence(effects) => effects.first().and_then(subject_of),
        Effect::May { effect, .. } | Effect::Unless { effect, .. } => subject_of(effect),
        Effect::If { then, .. } => subject_of(then),
        Effect::Choose { player, .. }
        | Effect::ChooseValue { player, .. }
        | Effect::LookAt { player, .. }
        | Effect::PutBack { player, .. }
        | Effect::PutAnyOnBottom { player }
        | Effect::ShuffleInto { player, .. }
        | Effect::Dig { player, .. }
        | Effect::ExileUntil { player, .. }
        | Effect::Fateseal { player, .. }
        | Effect::Search { player, .. }
        | Effect::Draw { player, .. }
        | Effect::Discard { player, .. }
        | Effect::Mill { player, .. }
        | Effect::Scry { player, .. }
        | Effect::GainLife { player, .. }
        | Effect::LoseLife { player, .. }
        | Effect::CreateTokens { player, .. }
        | Effect::CreateAttackingTokens { player, .. }
        | Effect::CreateCopies { player, .. }
        | Effect::AddMana { player, .. }
        | Effect::AddManaOfKind { player, .. }
        | Effect::AddProduct { player, .. }
        | Effect::PermitCast { player, .. }
        | Effect::MayCast { player, .. }
        | Effect::UntapUpTo { player, .. }
        | Effect::GetEmblem { player, .. }
        | Effect::ManifestDread(player)
        | Effect::FlipCoin(player)
        | Effect::WinGame(player)
        | Effect::LoseGame(player)
        | Effect::ExtraTurn(player, _) => Some(player),
        Effect::Subgame
        | Effect::EndTurn
        | Effect::AdditionalStep(_)
        | Effect::AdditionalPhases { .. }
        | Effect::RestartGame { .. } => None,
        Effect::ForEachPlayer { .. }
        | Effect::ChooseAtRandom(_)
        | Effect::Counter(_)
        | Effect::CopySpell { .. }
        | Effect::CounterInto { .. }
        | Effect::Destroy { .. }
        | Effect::Regenerate(_)
        | Effect::Tap(_)
        | Effect::Untap(_)
        | Effect::PhaseOut(_)
        | Effect::SetPrepared { .. }
        | Effect::Exile(_)
        | Effect::ExileUntilThisLeaves(_)
        | Effect::ExileLinked(_)
        | Effect::Sacrifice(_)
        | Effect::ReturnToHand(_)
        | Effect::PutIntoGraveyard(_)
        | Effect::ReturnToBattlefield(_)
        | Effect::PutOntoBattlefield(_)
        | Effect::ReturnTransformed(_)
        | Effect::Transform(_)
        | Effect::Meld(_)
        | Effect::Flip(_)
        | Effect::Reveal(_)
        | Effect::PutIntoLibrary { .. }
        | Effect::Explore(_)
        | Effect::Mutate(_)
        | Effect::DealDamage { .. }
        | Effect::PreventDamage { .. }
        | Effect::AddCounters { .. }
        | Effect::RemoveCounters { .. }
        | Effect::RemoveAllCounters(_)
        | Effect::DoubleCounters { .. }
        | Effect::Modify { .. }
        | Effect::ChangeText { .. }
        | Effect::ExchangeControl(..)
        | Effect::BecomeCopy { .. }
        | Effect::Attach { .. }
        | Effect::ChangeRule { .. }
        | Effect::ChangeMana { .. }
        | Effect::GainFlashback(_)
        | Effect::ControlPlayer(_)
        | Effect::Wish { .. }
        | Effect::Delay { .. } => Some(Players::You),
    }
}
