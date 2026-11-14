use std::collections::{BTreeMap, BTreeSet};

use beryl_card_store::facts;

use crate::action::{Adjustment, Destination, Position, Selection};
use crate::assist::{Assist, Line, Proposal, enabled};
use crate::helpers::{card_record, counter, printed_face};
use crate::state::{
    ArrowKind, ArrowTarget, Card, Face, LineVisibility, Object, ObjectId, Phase, Zone,
};
use crate::{Action, Seat, Table};

#[derive(PartialEq, Eq)]
enum Strike {
    Normal,
    First,
    Double,
}

struct Creature {
    card: ObjectId,
    power: i32,
    toughness: i32,
    strike: Strike,
    keywords: Vec<String>,
    marked: i32,
    dead: bool,
}

impl Creature {
    fn new(table: &Table, id: ObjectId) -> Option<Self> {
        let card = battlefield_card(table, id)?;
        let record = card_record(&card.card);
        let (types, printed, printed_keywords) = match &card.card {
            Card::Custom { types, stats, .. } => (types.as_str(), *stats, Vec::new()),
            Card::Named(_) => {
                let record = record.as_ref()?;
                let face = printed_face(record, card.face)?;
                (
                    face.type_line.as_str(),
                    facts::printed_stats(face),
                    facts::printed_keywords(record, face),
                )
            }
            Card::Hidden => return None,
        };
        if !facts::is_creature_type_line(types) {
            return None;
        }
        let (power, toughness) = facts::shown_stats(
            printed,
            card.stats,
            counter(card, "+1/+1"),
            counter(card, "-1/-1"),
        )?;
        let keyword = |name: &str| {
            printed_keywords
                .iter()
                .any(|key| key.eq_ignore_ascii_case(name))
        };

        Some(Self {
            card: id,
            power: power.max(0),
            toughness,
            strike: if keyword("Double strike") {
                Strike::Double
            } else if keyword("First strike") {
                Strike::First
            } else {
                Strike::Normal
            },
            keywords: printed_keywords.into_iter().map(str::to_owned).collect(),
            marked: card.damage,
            dead: false,
        })
    }

    fn strikes(&self, first: bool) -> bool {
        !self.dead
            && self.power > 0
            && match self.strike {
                Strike::Normal => !first,
                Strike::First => first,
                Strike::Double => true,
            }
    }

    fn has(&self, name: &str) -> bool {
        self.keywords
            .iter()
            .any(|keyword| keyword.eq_ignore_ascii_case(name))
    }

    fn lethal_on(&self, blocker: &Self) -> i32 {
        let unmarked = blocker.toughness.saturating_sub(blocker.marked).max(0);
        if self.has("Deathtouch") {
            unmarked.min(1)
        } else {
            unmarked
        }
    }
}

struct Attack {
    attacker: ObjectId,
    target: ArrowTarget,
    blockers: Vec<ObjectId>,
}

impl Attack {
    fn assign(&self, creatures: &BTreeMap<ObjectId, Creature>, first: bool, hits: &mut Vec<Hit>) {
        let attacker = &creatures[&self.attacker];
        if attacker.strikes(first) {
            let mut remaining = attacker.power;
            for blocker in self.blockers.iter().map(|id| &creatures[id]) {
                if !blocker.dead {
                    let assigned = remaining.min(attacker.lethal_on(blocker));
                    Hit::push(hits, attacker, Recipient::Creature(blocker.card), assigned);
                    remaining -= assigned;
                }
            }
            // A creature stays blocked when its blockers die in the first round.
            if self.blockers.is_empty() || attacker.has("Trample") {
                Hit::push(hits, attacker, Recipient::Attacked(self.target), remaining);
            }
        }
        if !attacker.dead {
            for blocker in self.blockers.iter().map(|id| &creatures[id]) {
                if blocker.strikes(first) {
                    Hit::push(
                        hits,
                        blocker,
                        Recipient::Creature(attacker.card),
                        blocker.power,
                    );
                }
            }
        }
    }
}

enum Recipient {
    Creature(ObjectId),
    Attacked(ArrowTarget),
}

struct Hit {
    source: ObjectId,
    recipient: Recipient,
    amount: i32,
}

impl Hit {
    fn push(hits: &mut Vec<Self>, source: &Creature, recipient: Recipient, amount: i32) {
        if amount > 0 {
            hits.push(Self {
                source: source.card,
                recipient,
                amount,
            });
        }
    }
}

struct Combat<'a> {
    table: &'a Table,
    attacks: Vec<Attack>,
    creatures: BTreeMap<ObjectId, Creature>,
    losses: BTreeMap<Seat, i32>,
    gains: BTreeMap<Seat, i32>,
    loyalty_damage: BTreeMap<ObjectId, i32>,
}

impl<'a> Combat<'a> {
    fn new(table: &'a Table) -> Self {
        let mut combat = Self {
            table,
            attacks: Vec::new(),
            creatures: BTreeMap::new(),
            losses: BTreeMap::new(),
            gains: BTreeMap::new(),
            loyalty_damage: BTreeMap::new(),
        };
        let mut attack_indices = BTreeMap::new();
        for arrow in table
            .arrows
            .iter()
            .filter(|arrow| arrow.kind == ArrowKind::Attack)
        {
            // Manual arrows can be repeated. Each creature has one attack target.
            if attack_indices.contains_key(&arrow.from) || !valid_target(table, arrow.to) {
                continue;
            }
            if let Some(attacker) = Creature::new(table, arrow.from) {
                combat.creatures.insert(arrow.from, attacker);
                attack_indices.insert(arrow.from, combat.attacks.len());
                combat.attacks.push(Attack {
                    attacker: arrow.from,
                    target: arrow.to,
                    blockers: Vec::new(),
                });
            }
        }
        for arrow in table
            .arrows
            .iter()
            .filter(|arrow| arrow.kind == ArrowKind::Block)
        {
            if let ArrowTarget::Object(attacker) = arrow.to
                && let Some(&index) = attack_indices.get(&attacker)
                && !combat.creatures.contains_key(&arrow.from)
                && let Some(blocker) = Creature::new(table, arrow.from)
            {
                combat.creatures.insert(arrow.from, blocker);
                combat.attacks[index].blockers.push(arrow.from);
            }
        }
        combat
    }

    fn round(&mut self, first: bool) {
        let mut hits = Vec::new();
        for attack in &self.attacks {
            attack.assign(&self.creatures, first, &mut hits);
        }

        let mut touched_by_deathtouch = BTreeSet::new();
        for hit in hits {
            let source = &self.creatures[&hit.source];
            if source.has("Lifelink") {
                let seat = self.table.objects[&hit.source].controller;
                add(&mut self.gains, seat, hit.amount);
            }
            match hit.recipient {
                Recipient::Creature(id) => {
                    if source.has("Deathtouch") {
                        touched_by_deathtouch.insert(id);
                    }
                    let creature = self.creatures.get_mut(&id).expect("combatant");
                    creature.marked = creature.marked.saturating_add(hit.amount);
                }
                Recipient::Attacked(ArrowTarget::Player(seat)) => {
                    add(&mut self.losses, seat, hit.amount);
                }
                Recipient::Attacked(ArrowTarget::Object(id)) => {
                    add(&mut self.loyalty_damage, id, hit.amount);
                }
            }
        }
        for (id, creature) in &mut self.creatures {
            if creature.toughness <= 0
                || (!creature.has("Indestructible")
                    && (creature.marked >= creature.toughness
                        || touched_by_deathtouch.contains(id)))
            {
                creature.dead = true;
            }
        }
    }

    fn lines(&self) -> Vec<Line> {
        let mut lines = Vec::new();
        for (&id, &damage) in &self.loyalty_damage {
            let card = &self.table.objects[&id];
            let lost = damage.min(counter(card, "loyalty"));
            if lost > 0 {
                lines.push(line(
                    format!("{} loses {lost} loyalty", name(card)),
                    card.controller,
                    LineVisibility::KnownObject(card.id),
                    Action::Counter {
                        selection: select(id),
                        name: "loyalty".into(),
                        adjustment: Adjustment::Add(-lost),
                    },
                ));
            }
        }

        for attack in &self.attacks {
            for &id in std::iter::once(&attack.attacker).chain(&attack.blockers) {
                if let Some(line) = self.creature_line(&self.creatures[&id]) {
                    lines.push(line);
                }
            }
        }
        for (&seat, &loss) in &self.losses {
            lines.push(life_line(seat, "loses", loss, -loss));
        }
        for (&seat, &gain) in &self.gains {
            lines.push(life_line(seat, "gains", gain, gain));
        }
        lines.push(line(
            "Go to end of combat".into(),
            self.table.active,
            LineVisibility::Public,
            Action::Phase {
                phase: Phase::EndOfCombat,
            },
        ));
        lines
    }

    fn creature_line(&self, creature: &Creature) -> Option<Line> {
        let card = &self.table.objects[&creature.card];
        let selection = select(card.id);
        let (text, action) = if creature.dead {
            (
                format!("{} dies", name(card)),
                Action::Move {
                    selection,
                    to: Destination::Zone(Zone::Graveyard(card.owner)),
                    position: Position::Top,
                    face: None,
                    tapped: None,
                },
            )
        } else {
            let damage = creature.marked.saturating_sub(card.damage);
            if damage == 0 {
                return None;
            }
            (
                format!("{} takes {damage}", name(card)),
                Action::Damage {
                    selection,
                    damage: creature.marked,
                },
            )
        };
        Some(line(
            text,
            card.controller,
            LineVisibility::KnownObject(card.id),
            action,
        ))
    }
}

fn battlefield_card(table: &Table, id: ObjectId) -> Option<&Object> {
    table
        .objects
        .get(&id)
        .filter(|card| card.zone == Zone::Battlefield && card.face != Face::Down)
}

fn valid_target(table: &Table, target: ArrowTarget) -> bool {
    match target {
        ArrowTarget::Player(seat) => table.players.get(usize::from(seat.0)).is_some(),
        ArrowTarget::Object(id) => {
            battlefield_card(table, id).is_some_and(|card| match &card.card {
                Card::Custom { types, .. } => facts::is_planeswalker_type_line(types),
                Card::Named(_) => card_record(&card.card)
                    .as_ref()
                    .and_then(|record| printed_face(record, card.face))
                    .is_some_and(|face| facts::is_planeswalker_type_line(&face.type_line)),
                Card::Hidden => false,
            })
        }
    }
}

fn name(card: &Object) -> &str {
    match &card.card {
        Card::Named(name) | Card::Custom { name, .. } => name,
        Card::Hidden => "A card",
    }
}

fn add<Key: Ord>(totals: &mut BTreeMap<Key, i32>, key: Key, amount: i32) {
    let total = totals.entry(key).or_default();
    *total = total.saturating_add(amount);
}

fn select(id: ObjectId) -> Selection {
    Selection {
        objects: vec![id],
        zone: Zone::Battlefield,
    }
}

fn line(text: String, whose: Seat, visibility: LineVisibility, action: Action) -> Line {
    Line {
        text,
        visibility,
        whose,
        on: true,
        actions: vec![action],
    }
}

fn life_line(seat: Seat, verb: &str, amount: i32, change: i32) -> Line {
    line(
        format!("Seat {} {verb} {amount}", u16::from(seat.0) + 1),
        seat,
        LineVisibility::Public,
        Action::Life {
            seat,
            adjustment: Adjustment::Add(change),
        },
    )
}

/// Computes the manual combat preview for seats that have enabled the assist.
/// Damage is assigned in arrow order and capped at lethal on each blocker.
/// Each round deals its assignments together ([CR 510.2], “dealt simultaneously”).
/// First and double strike add “a second combat damage step” ([CR 510.4]).
/// Deathtouch makes any nonzero assignment “lethal damage” ([CR 702.2c]) and
/// can destroy a creature that’s “been dealt damage” by a deathtouch source
/// ([CR 702.2b]). Indestructible
/// permanents “aren't destroyed by lethal damage” ([CR 702.12b]). A creature
/// with “toughness 0 or less” still goes to the graveyard ([CR 704.5f]). Trample
/// gets “any excess damage” past the blockers ([CR 702.19b]). Lifelink causes
/// the source's controller “to gain that much life” ([CR 702.15b]).
///
/// [CR 510.2]: https://yawgatog.com/resources/magic-rules/#R5102
/// [CR 510.4]: https://yawgatog.com/resources/magic-rules/#R5104
/// [CR 702.2b]: https://yawgatog.com/resources/magic-rules/#R7022b
/// [CR 702.2c]: https://yawgatog.com/resources/magic-rules/#R7022c
/// [CR 702.12b]: https://yawgatog.com/resources/magic-rules/#R70212b
/// [CR 702.15b]: https://yawgatog.com/resources/magic-rules/#R70215b
/// [CR 702.19b]: https://yawgatog.com/resources/magic-rules/#R70219b
/// [CR 704.5f]: https://yawgatog.com/resources/magic-rules/#R7045f
#[must_use]
pub fn proposal(table: &Table) -> Option<Proposal> {
    // Phase changes leave the arrows on the table until the next turn.
    if !matches!(
        table.phase,
        Phase::BeginningOfCombat
            | Phase::DeclareAttackers
            | Phase::DeclareBlockers
            | Phase::CombatDamage
    ) || !table
        .seats()
        .any(|seat| enabled(table, seat, Assist::Combat))
    {
        return None;
    }

    let mut combat = Combat::new(table);
    if combat.attacks.is_empty() {
        return None;
    }
    if combat
        .creatures
        .values()
        .any(|creature| creature.strike != Strike::Normal)
    {
        combat.round(true);
    }
    combat.round(false);

    Some(Proposal {
        assist: Assist::Combat,
        lines: combat.lines(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deathtouch_trample_assigns_one_to_each_blocker() {
        let attacker = Creature {
            card: ObjectId(1),
            power: 5,
            toughness: 5,
            strike: Strike::Normal,
            keywords: vec!["Deathtouch".into(), "Trample".into()],
            marked: 0,
            dead: false,
        };
        let blockers = [ObjectId(2), ObjectId(3)];
        let mut creatures = BTreeMap::from([(attacker.card, attacker)]);
        for card in blockers {
            creatures.insert(
                card,
                Creature {
                    card,
                    power: 0,
                    toughness: 4,
                    strike: Strike::Normal,
                    keywords: Vec::new(),
                    marked: 0,
                    dead: false,
                },
            );
        }
        let mut hits = Vec::new();
        Attack {
            attacker: ObjectId(1),
            target: ArrowTarget::Player(Seat(1)),
            blockers: blockers.to_vec(),
        }
        .assign(&creatures, false, &mut hits);
        assert!(matches!(
            hits.as_slice(),
            [
                Hit {
                    recipient: Recipient::Creature(ObjectId(2)),
                    amount: 1,
                    ..
                },
                Hit {
                    recipient: Recipient::Creature(ObjectId(3)),
                    amount: 1,
                    ..
                },
                Hit {
                    recipient: Recipient::Attacked(ArrowTarget::Player(Seat(1))),
                    amount: 3,
                    ..
                },
            ]
        ));
    }
}
