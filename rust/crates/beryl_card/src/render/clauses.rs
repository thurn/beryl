//! How the clauses of an effect are written and joined: a clause about a player
//! ("draw a card", "each opponent loses 2 life"), an instruction you carry out on objects
//! ("destroy target creature"), and two instructions joined by ", then".

use super::Ctx;
use crate::cost::Cost;
use crate::effect::{Amount, Effect, Objects, Players};

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
        let plain = player == Players::You && !self.may && !self.continued;
        let clause = self.clause(player, verb, rest);
        if plain {
            format!("you {clause}")
        } else {
            clause
        }
    }

    /// Each instruction's clauses in turn, two that Oracle text joins with ", then" joined
    /// so: "draw a card, then discard a card".
    pub(super) fn sequence(&mut self, effects: &[Effect]) -> Vec<String> {
        let mut clauses: Vec<String> = Vec::new();
        let mut previous: Option<&Effect> = None;
        for effect in effects {
            let then = previous.is_some_and(|before| joined_by_then(before, effect));
            self.continued = then && previous.and_then(subject_of) == subject_of(effect);
            let mut more = self.effect(effect);
            self.continued = false;
            if then
                && let Some(last) = clauses.last_mut()
                && !more.is_empty()
            {
                last.push_str(", then ");
                last.push_str(&more.remove(0));
            }
            clauses.extend(more);
            previous = Some(effect);
        }
        clauses
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
            Objects::InZoneOf {
                zone,
                players,
                filter: crate::Filter::Any,
            } => format!(
                "all cards from {} {}",
                self.possessive(*players),
                super::filters::zone_name(zone)
            ),
            other => self.objects(other),
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
        ) | (Effect::ShuffleInto { .. }, Effect::Draw { .. })
            | (
                Effect::Exile(Objects::InZoneOf { .. }),
                Effect::ShuffleInto { .. }
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
        | Effect::LookAt { player, .. }
        | Effect::PutBack { player, .. }
        | Effect::ShuffleInto { player, .. }
        | Effect::Dig { player, .. }
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
        | Effect::PermitCast { player, .. }
        | Effect::MayCast { player, .. }
        | Effect::UntapUpTo { player, .. }
        | Effect::GetEmblem { player, .. }
        | Effect::ManifestDread(player)
        | Effect::FlipCoin(player)
        | Effect::WinGame(player)
        | Effect::LoseGame(player) => Some(player),
        Effect::ForEachPlayer { .. }
        | Effect::Counter(_)
        | Effect::CounterInto { .. }
        | Effect::Destroy { .. }
        | Effect::Regenerate(_)
        | Effect::Exile(_)
        | Effect::ExileUntilThisLeaves(_)
        | Effect::ExileLinked(_)
        | Effect::Sacrifice(_)
        | Effect::ReturnToHand(_)
        | Effect::PutIntoGraveyard(_)
        | Effect::ReturnToBattlefield(_)
        | Effect::ReturnTransformed(_)
        | Effect::PutIntoLibrary { .. }
        | Effect::Explore(_)
        | Effect::DealDamage { .. }
        | Effect::AddCounters { .. }
        | Effect::RemoveCounters { .. }
        | Effect::Modify { .. }
        | Effect::ExchangeControl(..)
        | Effect::Attach { .. }
        | Effect::ChangeRule { .. }
        | Effect::GainFlashback(_)
        | Effect::Delay { .. } => Some(Players::You),
    }
}
