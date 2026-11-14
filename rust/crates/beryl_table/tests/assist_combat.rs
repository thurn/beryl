use beryl_table::action::{SeenObject, Selection, Target};
use beryl_table::assist::{Assist, Proposal, apply_proposal, combat::proposal};
use beryl_table::state::{Card, Object, ObjectId, Phase, Zone};
use beryl_table::{Action, Chance, Change, Seat, Table, apply};

const HOST: Seat = Seat(0);
const GUEST: Seat = Seat(1);

struct CombatTable {
    table: Table,
    chance: Chance,
}

impl CombatTable {
    fn new() -> Self {
        let mut combat = Self {
            table: Table::new(2),
            chance: Chance::fresh(1),
        };
        combat.act(
            HOST,
            &Action::Phase {
                phase: Phase::DeclareAttackers,
            },
        );
        combat.switch(GUEST, true);
        combat
    }

    fn act(&mut self, by: Seat, action: &Action) {
        apply(&mut self.table, by, action, &mut self.chance).unwrap();
    }

    fn switch(&mut self, by: Seat, on: bool) {
        self.act(
            by,
            &Action::SetAssist {
                assist: Assist::Combat,
                on,
            },
        );
    }

    fn put(&mut self, owner: Seat, name: &str, stats: Option<(i32, i32)>) -> SeenObject {
        let id = ObjectId(u32::try_from(self.table.battlefield.len()).unwrap() + 1);
        let zone = Zone::Battlefield;
        let mut object = Object::new(id, Card::Named(name.into()), owner, zone);
        object.stats = stats;
        object.known_to = vec![HOST, GUEST];
        Change::Create {
            object,
            index: self.table.battlefield.len(),
        }
        .perform(&mut self.table);
        SeenObject { id, zone }
    }

    fn card(&self, card: SeenObject) -> &Object {
        &self.table.objects[&card.id]
    }

    fn arrow(&mut self, by: Seat, from: SeenObject, to: Target) {
        self.act(
            by,
            &Action::Arrow {
                selection: Selection {
                    zone: from.zone,
                    objects: vec![from.id],
                },
                to,
            },
        );
    }

    fn offer(&self) -> Proposal {
        proposal(&self.table).unwrap()
    }

    fn accept(&mut self) {
        self.act(GUEST, &apply_proposal(&self.offer()));
    }
}

#[test]
fn the_worked_example_kills_both_blockers_before_they_strike() {
    let mut combat = CombatTable::new();
    let attacker = combat.put(HOST, "Akroma, Angel of Wrath", Some((3, 3)));
    let first = combat.put(GUEST, "Grizzly Bears", None);
    let second = combat.put(GUEST, "Llanowar Elves", None);
    combat.arrow(HOST, attacker, Target::Player(GUEST));
    combat.arrow(GUEST, first, Target::Object(attacker));
    combat.arrow(GUEST, second, Target::Object(attacker));

    let offered = combat.offer();
    assert_eq!(offered.assist, Assist::Combat);
    let (phase, results) = offered.lines.split_last().unwrap();
    assert_eq!(
        results
            .iter()
            .map(|line| (line.text.as_str(), line.whose, line.on))
            .collect::<Vec<_>>(),
        [
            ("Grizzly Bears dies", GUEST, true),
            ("Llanowar Elves dies", GUEST, true)
        ]
    );
    assert_eq!(phase.text, "Go to end of combat");
    assert_eq!(phase.whose, HOST);
    assert_eq!(combat.card(first).zone, Zone::Battlefield);
    combat.accept();

    assert_eq!(combat.card(attacker).damage, 0);
    assert_eq!(combat.table.zones[1].graveyard, [second.id, first.id]);
    assert_eq!(combat.table.players[1].life, 20);
    assert_eq!(combat.table.phase, Phase::EndOfCombat);
    assert_eq!(proposal(&combat.table), None);
}

#[test]
fn double_strike_deals_damage_twice_and_blocks_stay_in_effect() {
    let mut combat = CombatTable::new();
    let unblocked = combat.put(HOST, "Boros Swiftblade", None);
    let second_attacker = combat.put(HOST, "Boros Swiftblade", None);
    let blocker = combat.put(GUEST, "Llanowar Elves", None);
    combat.arrow(HOST, unblocked, Target::Player(GUEST));
    combat.arrow(HOST, second_attacker, Target::Player(GUEST));
    combat.arrow(GUEST, blocker, Target::Object(second_attacker));
    combat.accept();

    assert_eq!(combat.table.players[1].life, 18);
    assert_eq!(combat.card(blocker).zone, Zone::Graveyard(GUEST));
    assert_eq!(combat.card(second_attacker).damage, 0);
}

#[test]
fn deathtouch_destroys_after_damage_and_lifelink_counts_only_damage_dealt() {
    let mut combat = CombatTable::new();
    let attacker = combat.put(HOST, "Vampire Nighthawk", Some((1, 2)));
    let blocker = combat.put(GUEST, "Grizzly Bears", Some((2, 5)));
    combat.arrow(HOST, attacker, Target::Player(GUEST));
    combat.arrow(GUEST, blocker, Target::Object(attacker));
    assert!(
        combat
            .offer()
            .lines
            .iter()
            .any(|line| line.text == "Seat 1 gains 1")
    );
    combat.accept();

    assert_eq!(combat.card(attacker).zone, Zone::Graveyard(HOST));
    assert_eq!(combat.card(blocker).zone, Zone::Graveyard(GUEST));
    assert_eq!(combat.table.players[0].life, 21);
    assert_eq!(combat.table.players[1].life, 20);
}

#[test]
fn indestructible_survives_lethal_damage_and_deathtouch_with_damage_marked() {
    let mut combat = CombatTable::new();
    let attacker = combat.put(HOST, "Vampire Nighthawk", None);
    let blocker = combat.put(HOST, "Darksteel Myr", Some((0, 2)));
    combat
        .table
        .objects
        .get_mut(&blocker.id)
        .unwrap()
        .controller = GUEST;
    combat.table.objects.get_mut(&blocker.id).unwrap().damage = 1;
    combat.arrow(HOST, attacker, Target::Player(GUEST));
    combat.arrow(GUEST, blocker, Target::Object(attacker));

    let offered = combat.offer();
    assert_eq!(offered.lines[0].text, "Darksteel Myr takes 1");
    assert_eq!(offered.lines[0].whose, GUEST);
    combat.accept();

    assert_eq!(combat.card(blocker).zone, Zone::Battlefield);
    assert_eq!(combat.card(blocker).damage, 2);
}

#[test]
fn trample_uses_shown_stats_and_existing_damage_before_hitting_a_planeswalker() {
    let mut combat = CombatTable::new();
    let attacker = combat.put(HOST, "Ball Lightning", Some((3, 3)));
    let blocker = combat.put(GUEST, "Grizzly Bears", None);
    let walker = combat.put(GUEST, "Ajani Goldmane", None);
    combat.table.objects.get_mut(&attacker.id).unwrap().counters = vec![("+1/+1".into(), 1)];
    combat.table.objects.get_mut(&blocker.id).unwrap().damage = 1;
    combat.table.objects.get_mut(&walker.id).unwrap().counters = vec![("loyalty".into(), 4)];
    combat.arrow(HOST, attacker, Target::Object(walker));
    combat.arrow(GUEST, blocker, Target::Object(attacker));

    let offered = combat.offer();
    let loyalty = offered
        .lines
        .iter()
        .find(|line| line.text == "Ajani Goldmane loses 3 loyalty")
        .unwrap();
    assert_eq!(loyalty.whose, GUEST);
    combat.accept();

    assert_eq!(combat.card(walker).counters, [("loyalty".into(), 1)]);
    assert_eq!(combat.card(blocker).zone, Zone::Graveyard(GUEST));
    assert_eq!(combat.card(attacker).damage, 2);
    assert_eq!(combat.table.players[1].life, 20);
}

#[test]
fn block_arrow_order_decides_who_takes_damage() {
    let mut combat = CombatTable::new();
    let attacker = combat.put(HOST, "Grizzly Bears", Some((2, 4)));
    let second = combat.put(GUEST, "Llanowar Elves", None);
    let first = combat.put(HOST, "Grizzly Bears", None);
    combat.table.objects.get_mut(&first.id).unwrap().controller = GUEST;
    combat.arrow(HOST, attacker, Target::Player(GUEST));
    combat.arrow(GUEST, first, Target::Object(attacker));
    combat.arrow(GUEST, second, Target::Object(attacker));
    combat.accept();

    assert_eq!(combat.card(first).zone, Zone::Graveyard(HOST));
    assert_eq!(combat.card(second).zone, Zone::Battlefield);
    assert_eq!(combat.card(second).damage, 0);
    assert_eq!(combat.card(attacker).damage, 3);
}

#[test]
fn either_seat_can_enable_the_proposal_and_unchecked_lines_are_skipped() {
    let mut combat = CombatTable::new();
    let attacker = combat.put(HOST, "Grizzly Bears", None);
    assert_eq!(proposal(&combat.table), None);
    combat.switch(GUEST, false);
    combat.arrow(HOST, attacker, Target::Player(GUEST));
    assert_eq!(proposal(&combat.table), None);
    combat.switch(HOST, true);
    let mut offered = combat.offer();
    assert_eq!(offered.lines[0].text, "Seat 2 loses 2");
    assert_eq!(offered.lines[0].whose, GUEST);
    offered.lines[0].on = false;
    combat.act(HOST, &apply_proposal(&offered));

    assert_eq!(combat.table.players[1].life, 20);
    assert_eq!(combat.table.phase, Phase::EndOfCombat);
    assert_eq!(proposal(&combat.table), None);
}

#[test]
fn double_strike_lifelink_counts_damage_in_both_rounds() {
    let mut combat = CombatTable::new();
    let attacker = combat.put(HOST, "Drogskol Reaver", None);
    combat.arrow(HOST, attacker, Target::Player(GUEST));
    combat.accept();

    assert_eq!(combat.table.players[0].life, 26);
    assert_eq!(combat.table.players[1].life, 14);
}

#[test]
fn a_blocker_does_not_strike_after_its_attacker_dies() {
    let mut combat = CombatTable::new();
    let attacker = combat.put(HOST, "Grizzly Bears", None);
    let first = combat.put(GUEST, "Akroma, Angel of Wrath", Some((2, 2)));
    let late = combat.put(GUEST, "Vampire Nighthawk", None);
    combat.arrow(HOST, attacker, Target::Player(GUEST));
    combat.arrow(GUEST, first, Target::Object(attacker));
    combat.arrow(GUEST, late, Target::Object(attacker));
    combat.accept();

    assert_eq!(combat.card(attacker).zone, Zone::Graveyard(HOST));
    assert_eq!(combat.card(first).damage, 0);
    assert_eq!(combat.table.players[1].life, 20);
}

#[test]
fn combat_reads_the_active_faces_printed_keywords() {
    let mut combat = CombatTable::new();
    let attacker = combat.put(HOST, "Kruin Outlaw // Terror of Kruin Pass", None);
    let blocker = combat.put(GUEST, "Grizzly Bears", Some((1, 4)));
    combat.arrow(HOST, attacker, Target::Player(GUEST));
    combat.arrow(GUEST, blocker, Target::Object(attacker));
    combat.accept();

    assert_eq!(combat.card(blocker).zone, Zone::Battlefield);
    assert_eq!(combat.card(blocker).damage, 2);
    assert_eq!(combat.card(attacker).damage, 1);
}

#[test]
fn mentioned_keywords_do_not_grant_combat_abilities() {
    let mut combat = CombatTable::new();
    let attacker = combat.put(HOST, "Odric, Blood-Cursed", None);
    combat.arrow(HOST, attacker, Target::Player(GUEST));
    combat.accept();

    assert_eq!(combat.table.players[0].life, 20);
    assert_eq!(combat.table.players[1].life, 17);
}

#[test]
fn indestructible_does_not_keep_a_zero_toughness_creature_alive() {
    let mut combat = CombatTable::new();
    let attacker = combat.put(HOST, "Grizzly Bears", None);
    let blocker = combat.put(GUEST, "Darksteel Myr", Some((0, 0)));
    combat.arrow(HOST, attacker, Target::Player(GUEST));
    combat.arrow(GUEST, blocker, Target::Object(attacker));
    combat.accept();

    assert_eq!(combat.card(blocker).zone, Zone::Graveyard(GUEST));
}
