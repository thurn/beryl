//! The facts about a position that a scenario's own assertions may leave out.

use beryl_engine::card::{AbilityKind, FaceIx, Types};
use beryl_engine::state::ObjectFlags;
use beryl_engine::{Choice, Decision, DecisionKind, Seat, Slot, Step, Zone, ZoneKey};

use super::{Runner, answer};
use crate::{Action, Assertion, script};

impl Runner {
    /// Facts about the current position that a scenario's own assertions may leave out, so
    /// that an oracle compares the whole public position (§17.2): both life totals; the size
    /// of each hand, graveyard, exile and library, how many of each card the graveyards and
    /// exiles hold, and how many of each card everyone knows a hand holds; how many
    /// permanents of each name each player controls face up, and how many of them that make
    /// no mana are tapped; the power and toughness of a creature alone of its name, card or
    /// token, under its controller, and the counters on a permanent alone of its name; and
    /// how many permanents of each card each player controls face down.
    ///
    /// Only a quiet position has a snapshot, a priority decision over an empty stack, which
    /// every oracle stops at as Beryl does. Some facts are left out as choices or chance
    /// rather than rules: which mana sources paid, which an oracle chooses its own way, and
    /// cards drawn after a shuffle, which an oracle shuffles its own way.
    pub(super) fn snapshot(&mut self) -> Result<Vec<Assertion>, String> {
        let quiet = matches!(self.position()?, Step::Decide(d) if d.kind == DecisionKind::Priority)
            && self.s.stack().is_empty();
        if !quiet {
            return Ok(Vec::new());
        }
        let s = &self.s;
        let mut out = Vec::new();
        let count = |n: usize| u8::try_from(n).unwrap_or(u8::MAX);
        for player in Seat::BOTH {
            out.push(Assertion::Life {
                player,
                life: s.player(player).life(),
            });
        }
        for player in Seat::BOTH {
            for zone in [Zone::Hand, Zone::Graveyard, Zone::Exile, Zone::Library] {
                let cards = s.zone(ZoneKey::Of(player, zone));
                // Counts by name, true in any order: the candidate order only arranges them.
                let names = s.in_candidate_order(cards).map(|slot| self.name(slot));
                let names: Vec<&'static str> = names.collect();
                out.push(Assertion::Count {
                    player,
                    zone,
                    card: None,
                    count: count(names.len()),
                });
                if zone == Zone::Library {
                    continue;
                }
                for card in distinct(&names) {
                    // A card only its owner knows was drawn or searched for, and an oracle may
                    // have found another.
                    let known = |slot| Seat::BOTH.into_iter().all(|p| s.knows(p, slot));
                    let public = cards.all(|slot| self.name(slot) != card || known(slot));
                    if !public || card.is_empty() {
                        continue;
                    }
                    out.push(Assertion::Count {
                        player,
                        zone,
                        card: Some(card),
                        count: count(names.iter().filter(|&&n| n == card).count()),
                    });
                }
            }
        }
        for player in Seat::BOTH {
            self.battlefield_facts(player, &mut out);
        }
        Ok(out)
    }

    /// The snapshot's facts about the permanents `player` controls.
    fn battlefield_facts(&self, player: Seat, out: &mut Vec<Assertion>) {
        let s = &self.s;
        let count = |n: usize| u8::try_from(n).unwrap_or(u8::MAX);
        let mine: Vec<Slot> = s
            .battlefield()
            .iter()
            .copied()
            .filter(|&slot| s.controller(slot) == player)
            .collect();
        let (down, up): (Vec<Slot>, Vec<Slot>) = mine
            .into_iter()
            .partition(|&slot| s.object(slot).face() == FaceIx::DOWN);
        let up_names: Vec<&'static str> = up.iter().map(|&slot| self.name(slot)).collect();
        // A nameless permanent, as a face-down copy, has nothing an oracle could name it by.
        for card in distinct(&up_names).into_iter().filter(|c| !c.is_empty()) {
            let named: Vec<Slot> = up
                .iter()
                .copied()
                .filter(|&slot| self.name(slot) == card)
                .collect();
            let flag = |slot: Slot, f: u32| s.object(slot).flags().has(f);
            out.push(Assertion::Controls {
                player,
                card,
                count: count(named.len()),
            });
            let makes_mana = named.iter().any(|&slot| {
                let face = s.copiable(slot).face(&self.t);
                face.abilities.iter().any(|a| a.kind == AbilityKind::Mana)
            });
            if !makes_mana {
                out.push(Assertion::Tapped {
                    player,
                    card,
                    count: count(
                        named
                            .iter()
                            .filter(|&&slot| flag(slot, ObjectFlags::TAPPED))
                            .count(),
                    ),
                });
            }
            for token in [false, true] {
                let kind: Vec<Slot> = named
                    .iter()
                    .copied()
                    .filter(|&slot| flag(slot, ObjectFlags::TOKEN) == token)
                    .collect();
                if let [slot] = kind[..] {
                    let d = s.derived(slot);
                    if d.types.contains(Types::CREATURE) {
                        out.push(Assertion::Pt {
                            player,
                            card,
                            token,
                            power: d.power,
                            toughness: d.toughness,
                        });
                    }
                }
            }
            if let [slot] = named[..] {
                // Each kind it has a counter of, in the order of the vocabulary's list.
                for (kind, count) in s.counters(slot).iter() {
                    out.push(Assertion::Counters {
                        player,
                        card,
                        kind,
                        count,
                    });
                }
            }
        }
        let down_names: Vec<&'static str> = down.iter().map(|&slot| self.name(slot)).collect();
        for card in distinct(&down_names) {
            out.push(Assertion::FaceDown {
                player,
                card,
                count: count(down_names.iter().filter(|&&n| n == card).count()),
            });
        }
    }

    /// The candidates of `d` that an action of the scenario vocabulary takes, each with that
    /// action: one the runner, given the action, would answer `d` with.
    pub(super) fn picks(&self, d: &Decision) -> Vec<(Choice, Action)> {
        (0..d.len() as u16)
            .map(Choice)
            .filter_map(|c| Some((c, script(&self.t, &self.s, d, c)?)))
            .filter(|(c, action)| answer(&self.t, &self.s, d, action) == Some(*c))
            .collect()
    }
}

/// The distinct names among `names`, in their first order.
fn distinct(names: &[&'static str]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for &n in names {
        if !out.contains(&n) {
            out.push(n);
        }
    }
    out
}
