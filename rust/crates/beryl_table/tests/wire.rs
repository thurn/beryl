use beryl_table::Action;
use serde_json::Value;

#[test]
fn action_examples_round_trip() {
    for example in [
        r#"{ "move": { "objects": [12, 15], "in": { "hand": 0 },
            "to": { "graveyard": 0 }, "face": "front" } }"#,
        r#"{ "counter": { "objects": [41], "in": "battlefield",
               "name": "+1/+1", "add": 1 } }"#,
        r#"{"move":{"objects":[1],"in":{"hand":0},"to":"play"}}"#,
        r#"{"move":{"objects":[1],"in":"stack","to":"resolve"}}"#,
        r#"{"batch":{"actions":[{"next_turn":{}},{"life":{"seat":0,"set":23}}]}}"#,
    ] {
        let value: Value = serde_json::from_str(example).unwrap();
        let action: Action = serde_json::from_str(example).unwrap();
        assert_eq!(serde_json::to_value(&action).unwrap(), value);
    }
}

#[test]
fn every_action_has_an_externally_tagged_wire_form() {
    use serde_json::json;

    for value in [
        json!({"move":{"objects":[1],"in":{"library":0},"to":{"exile":0},"position":{"nth":3}}}),
        json!({"reorder":{"objects":[1],"in":{"hand":0},"position":"bottom"}}),
        json!({"pin":{"objects":[1],"in":"battlefield","row":"creatures"}}),
        json!({"give":{"objects":[1],"in":"battlefield","to":1}}),
        json!({"tap":{"objects":[1],"in":"battlefield","tapped":true}}),
        json!({"face":{"objects":[1],"in":"battlefield","face":"down"}}),
        json!({"flag":{"objects":[1],"in":"battlefield","doesnt_untap":true}}),
        json!({"peek":{"objects":[1],"in":"battlefield"}}),
        json!({"counter":{"objects":[1],"in":"battlefield","name":"charge","set":3}}),
        json!({"stats":{"objects":[1],"in":"battlefield","stats":[3,5]}}),
        json!({"damage":{"objects":[1],"in":"battlefield","damage":2}}),
        json!({"note":{"objects":[1],"in":"battlefield","note":null}}),
        json!({"attach":{"objects":[1],"in":"battlefield","host":{"id":2,"in":"battlefield"}}}),
        json!({"unattach":{"objects":[1],"in":"battlefield"}}),
        json!({"clone":{"objects":[1],"in":"battlefield"}}),
        json!({"token":{"card":{"custom":{"name":"example","types":"Creature","colors":["G"],"stats":[1,1]}},"count":1,"attached_to":{"id":2,"in":"battlefield"},"note":null}}),
        json!({"marker":{"objects":[1],"in":"battlefield","text":"example ability"}}),
        json!({"remove":{"objects":[1],"in":"stack"}}),
        json!({"draw":{"seat":0,"count":1}}),
        json!({"mulligan":{"seat":0,"count":7}}),
        json!({"shuffle":{"seat":0,"portion":{"bottom":2}}}),
        json!({"look":{"seat":0,"portion":{"top":2}}}),
        json!({"reveal":{"in":{"hand":0},"to":[1]}}),
        json!({"lend":{"in":{"library":0},"to":1}}),
        json!({"always_reveal_top":{"seat":0,"on":true}}),
        json!({"always_look_at_top":{"seat":0,"on":true}}),
        json!({"sort_hand":{"seat":0,"by":"mana_value"}}),
        json!({"life":{"seat":0,"add":-1}}),
        json!({"player_counter":{"seat":0,"name":"energy","set":3}}),
        json!({"phase":{"phase":"combat_damage"}}),
        json!({"next_turn":{}}),
        json!({"roll":{"count":2,"sides":6}}),
        json!({"arrow":{"objects":[1],"in":"battlefield","to":{"player":1}}}),
        json!({"clear_arrows":{"from":null,"to":null}}),
        json!({"load_deck":{"seat":0,"main":[["example card",4]],"side":[]}}),
        json!({"sideboard":{"seat":0,"into_main":[["example card",1]],"into_side":[]}}),
        json!({"ready":{"seat":0,"ready":true}}),
        json!({"start":{"first":0}}),
        json!({"next_game":{"winner":null}}),
        json!({"concede":{}}),
        json!({"set_assist":{"assist":"turn_upkeep","on":true}}),
        json!({"flow":{"seat":0,"flow":"surveil","count":2}}),
        json!({"undo":{}}),
        json!({"rewind":{"to":1}}),
        json!({"batch":{"actions":[]}}),
    ] {
        let action: Action = serde_json::from_value(value.clone())
            .unwrap_or_else(|error| panic!("{error} decoding {value}"));
        assert_eq!(serde_json::to_value(action).unwrap(), value);
    }
}

#[test]
fn applies_a_move_decoded_from_json() {
    use beryl_table::state::{Card, Object, ObjectId, Zone};
    use beryl_table::{Chance, Change, Seat, Table, apply};

    let owner = Seat(0);
    let mut table = Table::new(2);
    for (index, id) in [12, 15].into_iter().enumerate() {
        let mut object = Object::new(
            ObjectId(id),
            Card::Named(format!("test card {id}")),
            owner,
            Zone::Hand(owner),
        );
        object.known_to = vec![owner];
        Change::Create { object, index }.perform(&mut table);
    }
    let action = serde_json::from_str(
        r#"{ "move": { "objects": [12, 15], "in": { "hand": 0 },
            "to": { "graveyard": 0 }, "face": "front" } }"#,
    )
    .unwrap();
    let entry = apply(&mut table, owner, &action, &mut Chance::fresh(0)).unwrap();

    assert_eq!(entry.action, action);
    assert_eq!(
        table.zone(Zone::Graveyard(owner)).unwrap(),
        [ObjectId(12), ObjectId(15)]
    );
    assert!(table.zone(Zone::Hand(owner)).unwrap().is_empty());
    assert!(
        table
            .objects
            .values()
            .all(|object| object.known_to == [Seat(0), Seat(1)])
    );
}

#[test]
fn applies_lobby_and_roll_actions_decoded_from_json() {
    use beryl_table::state::Zone;
    use beryl_table::{Chance, Change, Seat, Table, apply};

    let seat = Seat(0);
    let mut table = Table::new(1);
    let mut chance = Chance::replaying(vec![0, 5]);
    for json in [
        r#"{"load_deck":{"seat":0,"main":[["Island",2]],"side":[]}}"#,
        r#"{"start":{"first":0}}"#,
    ] {
        let action = serde_json::from_str(json).unwrap();
        apply(&mut table, seat, &action, &mut chance).unwrap();
    }
    assert_eq!(table.zone(Zone::Hand(seat)).unwrap().len(), 2);
    let action = serde_json::from_str(r#"{"roll":{"count":1,"sides":6}}"#).unwrap();
    let entry = apply(&mut table, seat, &action, &mut chance).unwrap();
    assert_eq!(entry.changes, [Change::Random { outcomes: vec![5] }]);
}
