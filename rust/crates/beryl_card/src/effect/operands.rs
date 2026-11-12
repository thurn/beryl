//! The operands of an instruction: the expressions it holds, whichever entry it is.
//!
//! What is asked of every expression of a definition (does it name a target the ability
//! lacks, which filters does it hold, does it read what a cost took) is asked the same way
//! of every entry. [`Effect::operands`] is the one place that says which expressions an
//! entry holds, so a new entry is listed once and each such walk reads it.

use crate::cost::{Chosen, Cost, CostPart};
use crate::effect::{Amount, Effect, Objects, Players, Recipients, Taken};
use crate::filter::Filter;

/// The expressions one instruction holds. An instruction that holds other instructions
/// ([`Effect::Sequence`], [`Effect::If`], [`Effect::May`], [`Effect::Unless`],
/// [`Effect::ForEachPlayer`]) has here only its own: theirs are their own.
#[derive(Clone, Copy, Debug, Default)]
pub struct Operands<'a> {
    /// The players who do it or have it done to them.
    pub players: Option<&'a Players>,
    /// A second set of players, as the players whose library another looks at.
    pub others: Option<&'a Players>,
    pub objects: Option<&'a Objects>,
    pub amount: Option<&'a Amount>,
    pub filter: Option<&'a Filter>,
}

impl<'a> Operands<'a> {
    const NONE: Operands<'static> = Operands {
        players: None,
        others: None,
        objects: None,
        amount: None,
        filter: None,
    };

    const fn of_players(players: &'a Players) -> Self {
        Operands {
            players: Some(players),
            ..Operands::NONE
        }
    }

    const fn of_objects(objects: &'a Objects) -> Self {
        Operands {
            objects: Some(objects),
            ..Operands::NONE
        }
    }

    const fn amount(self, amount: &'a Amount) -> Self {
        Operands {
            amount: Some(amount),
            ..self
        }
    }

    const fn objects(self, objects: &'a Objects) -> Self {
        Operands {
            objects: Some(objects),
            ..self
        }
    }

    const fn filter(self, filter: Option<&'a Filter>) -> Self {
        Operands { filter, ..self }
    }

    /// Whether any of them reads what a cost took ([`Objects::Paid`]).
    pub const fn names_paid(&self) -> bool {
        let players = match self.players {
            Some(players) => players.names_paid(),
            None => false,
        };
        let others = match self.others {
            Some(players) => players.names_paid(),
            None => false,
        };
        let objects = match self.objects {
            Some(objects) => objects.names_paid(),
            None => false,
        };
        let amount = match self.amount {
            Some(amount) => amount.names_paid(),
            None => false,
        };
        players || others || objects || amount
    }
}

/// The filter of the one card a cost is paid with, where it is such a cost: what an
/// [`Effect::Unless`] may have its player discard.
const fn chosen_for(cost: &Cost) -> Option<&Filter> {
    match cost.parts() {
        [CostPart::Chosen(_, Chosen { filter, .. })] => Some(filter),
        _ => None,
    }
}

/// The filter of the cards an [`Effect::Dig`] may take, where it has one.
const fn taken_of(take: &Taken) -> Option<&Filter> {
    match take {
        Taken::Put(_) => None,
        Taken::MayReveal(filter) => Some(filter),
    }
}

impl Effect {
    /// The expressions this instruction holds ([`Operands`]).
    #[allow(clippy::too_many_lines)] // A line or five per entry.
    pub const fn operands(&self) -> Operands<'_> {
        match self {
            // Its rule is the ability's own (`Ability::rule`).
            Effect::Sequence(_) | Effect::If { .. } | Effect::ChangeRule { .. } => Operands::NONE,
            Effect::May { player, .. }
            | Effect::ForEachPlayer {
                players: player, ..
            }
            | Effect::AddMana { player, .. }
            | Effect::GetEmblem { player, .. }
            | Effect::ManifestDread(player)
            | Effect::FlipCoin(player)
            | Effect::WinGame(player)
            | Effect::LoseGame(player) => Operands::of_players(player),
            Effect::Unless { player, cost, .. } => {
                Operands::of_players(player).filter(chosen_for(cost))
            }
            Effect::Counter(objects)
            | Effect::CounterInto {
                spells: objects, ..
            }
            | Effect::Destroy(objects)
            | Effect::Exile(objects)
            | Effect::ExileUntilThisLeaves(objects)
            | Effect::ExileLinked(objects)
            | Effect::Sacrifice(objects)
            | Effect::ReturnToHand(objects)
            | Effect::PutIntoGraveyard(objects)
            | Effect::ReturnTransformed(objects)
            | Effect::GainFlashback(objects)
            | Effect::PutIntoLibrary { objects, .. }
            | Effect::Explore(objects)
            | Effect::Modify { objects, .. } => Operands::of_objects(objects),
            Effect::Draw { player, count }
            | Effect::Discard { player, count }
            | Effect::Mill { player, count }
            | Effect::Scry { player, count }
            | Effect::PutBack { player, count }
            | Effect::GainLife {
                player,
                amount: count,
            }
            | Effect::LoseLife {
                player,
                amount: count,
            }
            | Effect::CreateTokens { player, count, .. }
            | Effect::AddManaOfKind {
                player,
                amount: count,
                ..
            } => Operands::of_players(player).amount(count),
            Effect::CreateAttackingTokens {
                player,
                count,
                attacking,
                ..
            } => Operands {
                others: Some(attacking),
                ..Operands::of_players(player).amount(count)
            },
            Effect::ShuffleInto {
                player,
                cards: objects,
            }
            | Effect::MayCast {
                player,
                card: objects,
                ..
            }
            | Effect::PermitCast {
                player,
                cards: objects,
                ..
            }
            | Effect::LookAt {
                player,
                cards: objects,
            }
            | Effect::Choose {
                player,
                among: objects,
                ..
            } => Operands::of_players(player).objects(objects),
            Effect::Dig {
                player, look, take, ..
            } => Operands::of_players(player)
                .amount(look)
                .filter(taken_of(take)),
            Effect::Fateseal {
                player,
                library,
                count,
            } => Operands {
                others: Some(library),
                ..Operands::of_players(player).amount(count)
            },
            Effect::Search {
                player,
                filter,
                count,
                ..
            } => Operands::of_players(player)
                .amount(count)
                .filter(Some(filter)),
            Effect::DealDamage { to, amount } => match to {
                Recipients::Players(players) => Operands::of_players(players).amount(amount),
                Recipients::Permanents(objects) => Operands::of_objects(objects).amount(amount),
            },
            Effect::CreateCopies {
                player,
                of: objects,
                count,
                ..
            }
            | Effect::UntapUpTo {
                player,
                count,
                permanents: objects,
            } => Operands::of_players(player).objects(objects).amount(count),
            Effect::AddCounters { objects, count, .. }
            | Effect::RemoveCounters { objects, count, .. } => {
                Operands::of_objects(objects).amount(count)
            }
            Effect::Delay { carrying, .. } => Operands::NONE.amount(carrying),
        }
    }
}
