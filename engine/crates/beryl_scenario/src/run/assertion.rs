//! Checks the facts a scenario asserts at an open decision or at the end of the game.

use beryl_engine::card::{FaceIx, Types};
use beryl_engine::{Layout, Outcome, Step, Zone, ZoneKey, view};

use super::Runner;
use crate::{Assertion, Ending, StackItem};

impl<L: Layout> Runner<L> {
    /// The actual and expected values of an assertion about permanents.
    fn permanent_fact(&self, assertion: Assertion) -> Result<(String, String), String> {
        let s = &self.s;
        let tally = |found: usize, expected: u8| (found.to_string(), expected.to_string());
        Ok(match assertion {
            Assertion::Tapped {
                player,
                card,
                count,
            } => tally(
                self.permanents(player, card)
                    .filter(|&slot| s.object(slot).is_tapped())
                    .count(),
                count,
            ),
            Assertion::Controls {
                player,
                card,
                count,
            } => tally(self.permanents(player, card).count(), count),
            Assertion::FaceDown {
                player,
                card,
                count,
            } => tally(
                self.permanents(player, card)
                    .filter(|&slot| s.object(slot).face() == FaceIx::DOWN)
                    .count(),
                count,
            ),
            Assertion::Attacking {
                player,
                card,
                count,
            } => tally(
                self.permanents(player, card)
                    .filter(|&slot| s.object(slot).is_attacking())
                    .count(),
                count,
            ),
            Assertion::Pt {
                player,
                card,
                token,
                power,
                toughness,
            } => {
                let slot = self
                    .permanents(player, card)
                    .find(|&slot| s.object(slot).is_token() == token)
                    .ok_or_else(|| {
                        let what = if token { "token" } else { "card" };
                        format!("player {player} controls no {card} {what}")
                    })?;
                let d = s.derived(slot);
                (
                    format!("{}/{}", d.power, d.toughness),
                    format!("{power}/{toughness}"),
                )
            }
            Assertion::Creature {
                player,
                card,
                creature,
            } => {
                let d = s.derived(self.permanent(player, card)?);
                let is = d.types.contains(Types::CREATURE);
                (is.to_string(), creature.to_string())
            }
            Assertion::Subtype {
                player,
                card,
                subtype,
                has,
            } => {
                let mut named = self.permanents(player, card).peekable();
                if named.peek().is_none() {
                    return Err(format!("player {player} controls no {card}"));
                }
                let is = named.any(|slot| s.has_subtype(&self.t, slot, subtype));
                (is.to_string(), has.to_string())
            }
            Assertion::Counters {
                player,
                card,
                kind,
                count,
            } => {
                let slot = self.permanent(player, card)?;
                let n = s.counters_of(slot, kind);
                (n.to_string(), count.to_string())
            }
            Assertion::PermanentState { object, state } => {
                let actual = self.permanent_state(self.snapshot_permanent(object)?);
                (format!("{actual:?}"), format!("{state:?}"))
            }
            Assertion::ManaValue { object, value } => {
                let slot = self.snapshot_permanent(object)?;
                (s.mana_value(&self.t, slot).to_string(), value.to_string())
            }
            _ => unreachable!("an assertion about permanents"),
        })
    }

    /// What an assertion about a card's characteristics in a zone reads, and what it expects:
    /// its colors, or its power and toughness.
    fn characteristic_in(
        &self,
        position: &Step<L>,
        assertion: Assertion,
    ) -> Result<(String, String), String> {
        let s = &self.s;
        if let Step::Over(o) = position {
            return Err(format!(
                "no characteristics to read: the game is over ({o:?})"
            ));
        }
        Ok(match assertion {
            Assertion::Colors {
                player,
                zone,
                card,
                colors,
            } => {
                let slot = self.object_in(player, zone, card)?;
                (
                    format!("{:?}", s.colors(&self.t, slot)),
                    format!("{colors:?}"),
                )
            }
            Assertion::PtIn {
                player,
                zone,
                card,
                power,
                toughness,
            } => {
                let slot = self.object_in(player, zone, card)?;
                let (p, t) = (s.power(&self.t, slot), s.toughness(&self.t, slot));
                (format!("{p}/{t}"), format!("{power}/{toughness}"))
            }
            _ => unreachable!("an assertion about characteristics in a zone"),
        })
    }

    fn stack_fact(&self, assertion: Assertion) -> (String, String) {
        match assertion {
            Assertion::StackObject {
                depth,
                name,
                spell,
                controller,
            } => {
                let item = self.stack_items().rev().nth(usize::from(depth));
                let expected = StackItem {
                    name,
                    spell,
                    controller,
                };
                (format!("{item:?}"), format!("{:?}", Some(expected)))
            }
            Assertion::Stack { count } => (self.s.stack().len().to_string(), count.to_string()),
            _ => unreachable!("an assertion about the stack"),
        }
    }

    pub(super) fn check(&mut self, position: &Step<L>, assertion: Assertion) -> Result<(), String> {
        let s = &self.s;
        let (actual, expected) = match assertion {
            Assertion::Life { player, life } => {
                (s.player(player).life().to_string(), life.to_string())
            }
            Assertion::Count {
                player,
                zone,
                card,
                count,
            } => {
                if matches!(zone, Zone::Stack | Zone::Nowhere) {
                    return Err(format!("no owned {zone:?} to count"));
                }
                // A cast offer can follow a move inside the enclosing resolution.
                // Count cards where they are at the question the script will answer.
                let shown = view(s, &self.t, &mut self.x);
                let n = shown
                    .zone(ZoneKey::Of(player, zone))
                    .filter(|slot| card.is_none_or(|c| shown.name(&self.t, slot) == c))
                    .count();
                (n.to_string(), count.to_string())
            }
            Assertion::Tapped { .. }
            | Assertion::Controls { .. }
            | Assertion::FaceDown { .. }
            | Assertion::Attacking { .. }
            | Assertion::Pt { .. }
            | Assertion::Creature { .. }
            | Assertion::Subtype { .. }
            | Assertion::Counters { .. }
            | Assertion::PermanentState { .. }
            | Assertion::ManaValue { .. } => {
                // Control and characteristics are layer results, up to date only at an open
                // decision.
                if let Step::Over(o) = position {
                    return Err(format!("no permanents to read: the game is over ({o:?})"));
                }
                self.permanent_fact(assertion)?
            }
            Assertion::Colors { .. } | Assertion::PtIn { .. } => {
                self.characteristic_in(position, assertion)?
            }
            Assertion::Stack { .. } | Assertion::StackObject { .. } => self.stack_fact(assertion),
            Assertion::Library { player, top } => {
                let library = s.library(player).iter().rev().take(top.len());
                let actual: Vec<&str> = library.map(|&slot| self.name(slot)).collect();
                (format!("{actual:?}"), format!("{top:?}"))
            }
            Assertion::ManaPool { player, amounts } => {
                let actual = s.player(player).mana().amounts();
                (format!("{actual:?}"), format!("{amounts:?}"))
            }
            Assertion::Mana {
                player: p,
                color,
                amount,
            } => (s.player(p).mana()[color].to_string(), amount.to_string()),
            Assertion::Decision {
                player,
                kind,
                candidates,
            } => {
                let actual = match position {
                    Step::Decide(d) => self.describe(d),
                    Step::Over(o) => format!("the game over ({o:?})"),
                };
                let matches = matches!(position, Step::Decide(d)
                    if d.decider == player && d.kind == kind && d.len() == usize::from(candidates));
                if matches {
                    return Ok(());
                }
                (
                    actual,
                    format!("player {player} {kind:?} among {candidates} candidates"),
                )
            }
            Assertion::Outcome { outcome } => {
                let actual = match position {
                    Step::Decide(_) => Ending::InProgress,
                    Step::Over(Outcome::Win(player)) => Ending::Win { player: *player },
                    Step::Over(Outcome::Draw) => Ending::Draw,
                    Step::Over(o @ Outcome::Stalled(_)) => {
                        return Err(format!("the game stalled: {o:?}"));
                    }
                };
                (format!("{actual:?}"), format!("{outcome:?}"))
            }
        };
        if actual == expected {
            Ok(())
        } else {
            Err(format!(
                "{assertion:?}: expected {expected}, found {actual}"
            ))
        }
    }
}
