//! The facts about a position that a scenario's own assertions may leave out.

use beryl_card::{Ability, Behavior, CostPart, Effect, Players};
use beryl_engine::card::{AbilityKind, FaceIx, ObjectAbility, Subtypes, Types};
use beryl_engine::chosen::ANOTHER;
use beryl_engine::event::Event;
use beryl_engine::state::Flag;
use beryl_engine::{Choice, Decision, Layout, Seat, Step, Zone, ZoneKey};
use beryl_scenario_types::{PermanentState, SnapshotRef};

use super::{Runner, answer};
use crate::{Action, Assertion, StackItem, script};

impl<L: Layout> Runner<L> {
    /// Facts about the final public position, while there is an open decision. The oracle
    /// exporters compare these at the positions their harnesses can reach. The snapshot
    /// leaves out the identity of cards drawn in secret and which mana sources paid: it counts
    /// a player's tapped mana sources only where each makes one mana
    /// ([`Assertion::TappedMana`]), so that the count is the same whichever paid.
    pub(super) fn snapshot(&mut self) -> Result<Vec<Assertion>, String> {
        if !matches!(self.position()?, Step::Decide(_)) {
            return Ok(Vec::new());
        }
        let payments_may_differ = self.payments_may_differ();
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
                let names = cards.sorted(|slot| self.name(slot));
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
            if let Some(count) = self
                .tapped_mana_sources(player)
                .filter(|_| !payments_may_differ)
            {
                out.push(Assertion::TappedMana { player, count });
            }
            out.push(Assertion::ManaPool {
                player,
                amounts: s.player(player).mana().amounts(),
            });
        }
        if !s.stack().is_empty() {
            out.push(Assertion::Stack {
                count: count(s.stack().len()),
            });
            out.extend(self.stack_items().rev().enumerate().map(|(depth, item)| {
                Assertion::StackObject {
                    depth: u16::try_from(depth).expect("a stack index fits in u16"),
                    name: item.name,
                    spell: item.spell,
                    controller: item.controller,
                }
            }));
        }
        Ok(out)
    }

    /// The snapshot's facts about the permanents `player` controls.
    fn battlefield_facts(&self, player: Seat, out: &mut Vec<Assertion>) {
        let s = &self.s;
        let count = |n: usize| u8::try_from(n).unwrap_or(u8::MAX);
        let mine: Vec<L::Slot> = s
            .battlefield()
            .iter()
            .copied()
            .filter(|&slot| s.controller(slot) == player)
            .collect();
        out.extend(mine.iter().map(|&slot| Assertion::PermanentState {
            object: self.snapshot_ref(slot),
            state: self.permanent_state(slot),
        }));
        let (down, up): (Vec<L::Slot>, Vec<L::Slot>) = mine
            .into_iter()
            .partition(|&slot| s.object(slot).face() == FaceIx::DOWN);
        let up_names: Vec<&'static str> = up.iter().map(|&slot| self.name(slot)).collect();
        let down_names: Vec<&'static str> = down.iter().map(|&slot| self.name(slot)).collect();
        // A nameless permanent, as a face-down copy, has nothing an oracle could name it by;
        // nor has a name a face-down permanent of the same player's card also has, which one
        // oracle counts with the face-up ones and the other does not
        // ([CR 708.2](https://yawgatog.com/resources/magic-rules/#R7082): a face-down
        // permanent has no name).
        let named_alone = |c: &&str| !c.is_empty() && !down_names.contains(c);
        for card in distinct(&up_names).into_iter().filter(named_alone) {
            let named: Vec<L::Slot> = up
                .iter()
                .copied()
                .filter(|&slot| self.name(slot) == card)
                .collect();
            let flag = |slot: L::Slot, f: Flag| s.object(slot).flags().has(f);
            out.push(Assertion::Controls {
                player,
                card,
                count: count(named.len()),
            });
            let makes_mana = named.iter().any(|&slot| {
                let face = s.copiable(slot).face(&self.t);
                face.abilities.iter().any(|a| a.kind() == AbilityKind::Mana)
            });
            if !makes_mana {
                out.push(Assertion::Tapped {
                    player,
                    card,
                    count: count(
                        named
                            .iter()
                            .filter(|&&slot| flag(slot, Flag::Tapped))
                            .count(),
                    ),
                });
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
        for card in distinct(&down_names) {
            out.push(Assertion::FaceDown {
                player,
                card,
                count: count(down_names.iter().filter(|&&n| n == card).count()),
            });
        }
    }

    /// The permanents `player` controls that have a mana ability, each with whether it pays
    /// as any other of them would: every mana ability it has makes one mana for tapping it
    /// and nothing else, and it is a noncreature permanent whose printed mana abilities no
    /// effect changed. A land creature can be tapped by other means, as by regenerating, and
    /// an oracle may not give a land that Urborg made a Swamp the same mana abilities.
    pub(super) fn mana_sources(&self, player: Seat) -> impl Iterator<Item = (L::Slot, bool)> {
        let s = &self.s;
        s.battlefield()
            .iter()
            .copied()
            .filter(move |&slot| s.controller(slot) == player)
            .filter_map(move |slot| {
                let intrinsic = s.intrinsic_mana(&self.t, slot) != 0;
                let mut abilities = s
                    .abilities(&self.t, slot)
                    .map(|(_, a)| a)
                    .filter(|a| a.kind() == AbilityKind::Mana)
                    .peekable();
                if !intrinsic && abilities.peek().is_none() {
                    return None;
                }
                let plain =
                    !s.derived(slot).types.contains(Types::CREATURE) && s.makes_card_mana(slot);
                Some((slot, plain && abilities.all(makes_one_mana_for_a_tap)))
            })
    }

    /// How many of the permanents `player` controls that have a mana ability are tapped,
    /// where each of them pays as any other would ([`Runner::mana_sources`]): then a cost
    /// paid with any of them taps as many, and an oracle that pays from other sources than
    /// Beryl agrees. None where one makes more, or costs more than a tap, as a Sol Ring or a
    /// Treasure, or is a creature, and none where `player` controls a copy.
    fn tapped_mana_sources(&self, player: Seat) -> Option<u8> {
        // A copy or a merged permanent, as a creature mutated onto Dryad Arbor, may have mana
        // abilities in an oracle that it lacks in Beryl, where its copiable values say.
        let copies = self.s.battlefield().iter().any(|&slot| {
            self.s.controller(slot) == player && self.s.object(slot).flags().has(Flag::Copy)
        });
        if copies {
            return None;
        }
        let mut count = 0u8;
        for (slot, one) in self.mana_sources(player) {
            if !one {
                return None;
            }
            count += u8::from(self.s.object(slot).flags().has(Flag::Tapped));
        }
        Some(count)
    }

    /// The player whose next action activates a mana ability of a permanent at priority, as a
    /// player may whenever they have priority
    /// ([CR 605.3a](https://yawgatog.com/resources/magic-rules/#R6053a)): "A player may
    /// activate an activated mana ability whenever they have priority". The exact profile
    /// offers no such activation, since a payment taps for itself, so the profile that names
    /// this player ([`Profile::taps_mana`]) holds until the action is done. A scenario stops
    /// with that mana in the pool, where the oracles compare its color and amount.
    pub(super) fn activates_mana(&self) -> Option<Seat> {
        let Some(Action::Activate {
            player,
            card,
            ability,
        }) = self.upcoming
        else {
            return None;
        };
        // Read from each permanent's face, not its layer results, which may be stale here.
        let ObjectAbility::Own(i) = ability else {
            return None;
        };
        self.s
            .battlefield()
            .iter()
            .any(|&slot| {
                let face = self.s.definition(slot).face(&self.t);
                face.name == card
                    && (face.abilities.get(i.index()))
                        .is_some_and(|a| a.kind() == AbilityKind::Mana)
            })
            .then_some(player)
    }

    /// Notes, from the events recorded since the last call, each permanent that made mana
    /// ([`Runner::made_mana`]) and whether a copy was tapped ([`Runner::copy_tapped`]).
    pub(super) fn note_mana_sources(&mut self) {
        for event in &self.x.events.events()[self.events_read..] {
            match *event {
                Event::Mana {
                    source: Some(object),
                    ..
                } if !self.made_mana.contains(&object) => self.made_mana.push(object),
                Event::Tap { object } if self.s.is_current(object) => {
                    self.copy_tapped |= self.s.object(object.slot).flags().has(Flag::Copy);
                }
                _ => {}
            }
        }
        self.events_read = self.x.events.events().len();
    }

    /// Whether the payments of the game may have tapped other sources in an oracle, in a way
    /// that the count of tapped mana sources shows: a permanent that made mana has left the
    /// battlefield, or a copy was tapped.
    pub(super) fn payments_may_differ(&self) -> bool {
        self.copy_tapped
            || self
                .made_mana
                .iter()
                .any(|&object| !self.s.is_current(object))
    }

    fn public_name(&self, slot: L::Slot) -> Option<&'static str> {
        (self.s.object(slot).face() != FaceIx::DOWN).then(|| self.name(slot))
    }

    fn matching_permanents(
        &self,
        controller: Seat,
        name: Option<&str>,
    ) -> impl Iterator<Item = L::Slot> {
        self.s.battlefield().iter().copied().filter(move |&slot| {
            self.s.controller(slot) == controller && self.public_name(slot) == name
        })
    }

    fn snapshot_ref(&self, slot: L::Slot) -> SnapshotRef {
        let object = self.s.object(slot);
        let zone = object.zone();
        let name = self.public_name(slot);
        let (player, index) = if zone == Zone::Battlefield {
            let controller = self.s.controller(slot);
            let index = self
                .matching_permanents(controller, name)
                .position(|other| other == slot);
            (controller, index)
        } else {
            debug_assert_eq!(zone, Zone::Graveyard);
            let owner = object.owner();
            let index = self
                .s
                .graveyard(owner)
                .iter()
                .copied()
                .filter(|&other| self.public_name(other) == name)
                .position(|other| other == slot);
            (owner, index)
        };
        SnapshotRef {
            zone,
            player,
            name,
            index: u16::try_from(index.expect("a snapshot object appears in its zone list"))
                .expect("a zone index fits in u16"),
        }
    }

    pub(super) fn snapshot_permanent(&self, object: SnapshotRef) -> Result<L::Slot, String> {
        if object.zone != Zone::Battlefield {
            return Err(format!("not a battlefield reference: {object:?}"));
        }
        self.matching_permanents(object.player, object.name)
            .nth(usize::from(object.index))
            .ok_or_else(|| format!("no permanent matches {object:?}"))
    }

    pub(super) fn permanent_state(&self, slot: L::Slot) -> PermanentState {
        let object = self.s.object(slot);
        let derived = self.s.derived(slot);
        let all_creature_types = derived.subtypes.has_creature_type(ANOTHER);
        let subtypes = derived
            .subtypes
            .iter(self.t.subtypes())
            .filter(|subtype| {
                !all_creature_types
                    || !subtype
                        .card_type()
                        .intersects(Types::CREATURE.union(Types::KINDRED))
            })
            .fold(Subtypes::NONE, Subtypes::with);
        PermanentState {
            colors: derived.colors,
            types: derived.types,
            subtypes,
            all_creature_types,
            keywords: derived.keywords,
            damage: object.damage(),
            attached_to: object.attached_to().map(|host| self.snapshot_ref(host)),
            enchanted_player: object.enchanted_player(),
            pt: derived
                .types
                .contains(Types::CREATURE)
                .then_some((derived.power, derived.toughness)),
        }
    }

    /// The stack in the engine's order, bottom first.
    pub(super) fn stack_items(&self) -> impl DoubleEndedIterator<Item = StackItem> + '_ {
        self.s.stack().iter().map(|entry| StackItem {
            name: self.t.def(entry.card()).face_name(entry.face()),
            spell: entry.spell().is_some(),
            controller: entry.controller(),
        })
    }

    /// The candidates of `d` that an action of the scenario vocabulary takes, each with that
    /// action: one the runner, given the action, would answer `d` with.
    pub(super) fn picks(&self, d: &Decision<L>) -> Vec<(Choice, Action)> {
        (0..d.len() as u16)
            .map(Choice)
            .filter_map(|c| Some((c, script(&self.t, &self.s, d, c)?)))
            .filter(|(c, action)| answer(&self.t, &self.s, d, action) == Some(*c))
            .collect()
    }
}

/// Whether the mana ability `a` adds one mana for its controller, its whole cost a tap of its
/// source, as a basic land's does.
fn makes_one_mana_for_a_tap(a: &Ability) -> bool {
    let cost = a.cost();
    let one = matches!(
        a.body(),
        Behavior::Data(&Effect::AddMana {
            player: Players::You,
            mana,
        }) if mana.amount == 1
    );
    one && cost.mana.is_zero() && cost.parts() == [CostPart::TAP]
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
