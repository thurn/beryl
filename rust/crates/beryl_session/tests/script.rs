//! A script's text form, its wire form, and its lists against the design language's data.

use std::path::PathBuf;

use beryl_session::protocol::{
    CardHandling, CardType, Combat, Counter, CounterView, Facing, Mana, ManaPool, ObjectView,
    PaidCost, PowerToughness, Side, StackItem, Step as TurnStep, TargetRef, Why, ZoneName,
};
use beryl_session::script::{
    Act, Arrow, Fact, Mark, Notice, Reset, Script, Span, Step, Stream, Subject, Tally, Verb,
};

/// A card the viewer knows, lying in `zone`.
fn card(slot: u16, name: &str, zone: ZoneName) -> ObjectView {
    ObjectView {
        slot,
        zone,
        owner: Side::You,
        controller: Side::You,
        name: Some(name.to_owned()),
        facing: Facing::Up,
        token: false,
        tapped: false,
        summoning_sick: false,
        damage: 0,
        counters: Vec::new(),
        types: Vec::new(),
        pt: None,
        keywords: Vec::new(),
        attached_to: None,
        exiled_by: None,
        exiled_with: None,
        combat: None,
    }
}

/// A card the viewer has not seen.
fn hidden(slot: u16, zone: ZoneName) -> ObjectView {
    ObjectView {
        name: None,
        owner: Side::Opponent,
        controller: Side::Opponent,
        ..card(slot, "", zone)
    }
}

fn creature(slot: u16, name: &str, power: i8, toughness: i8) -> ObjectView {
    ObjectView {
        types: vec![CardType::Creature],
        pt: Some(PowerToughness { power, toughness }),
        ..card(slot, name, ZoneName::Battlefield)
    }
}

fn object(slot: u16) -> Subject {
    Subject::Object { slot }
}

fn player(player: Side) -> Subject {
    Subject::Player { player }
}

fn step(act: Act) -> Step {
    Step { act, cause: None }
}

fn caused(act: Act, cause: Subject) -> Step {
    Step {
        act,
        cause: Some(cause),
    }
}

fn spell(slot: u16, targets: Vec<TargetRef>) -> StackItem {
    StackItem::Spell {
        slot,
        targets,
        x: 0,
        paid: Vec::new(),
    }
}

fn trigger(source: Option<u16>, name: &str, targets: Vec<TargetRef>) -> StackItem {
    StackItem::Ability {
        source,
        ability: 0,
        granted: false,
        name: name.to_owned(),
        triggered: true,
        taps_to_pay: None,
        nth: 0,
        text: None,
        controller: Side::Opponent,
        targets,
        x: 0,
    }
}

fn line(act: Act) -> String {
    step(act).to_string()
}

#[test]
fn a_script_reads_one_step_a_line_with_beats_joined() {
    let you = player(Side::You);
    let island = ObjectView {
        tapped: true,
        ..card(4, "Island", ZoneName::Battlefield)
    };
    let draw = |slot, name| Act::Fly {
        from: ZoneName::Library,
        why: Why::Draw,
        object: card(slot, name, ZoneName::Hand),
        at: None,
        item: None,
    };
    let script = Script {
        reset: None,
        beats: vec![
            vec![caused(
                Act::Fly {
                    from: ZoneName::Hand,
                    why: Why::Cast,
                    object: card(7, "Brainstorm", ZoneName::Stack),
                    at: None,
                    item: Some(spell(7, Vec::new())),
                },
                you,
            )],
            vec![
                caused(Act::Tap { object: island }, you),
                caused(
                    Act::Streak {
                        from: Some(object(4)),
                        to: object(7),
                        stream: Stream::Mana { color: Mana::Blue },
                        amount: 1,
                        object: None,
                    },
                    you,
                ),
            ],
            vec![
                caused(draw(31, "Counterspell"), object(7)),
                caused(draw(32, "Plains"), object(7)),
            ],
            vec![step(Act::Vanish {
                subject: object(7),
                from: ZoneName::Stack,
                why: Why::Resolve,
                object: Some(card(7, "Brainstorm", ZoneName::Graveyard)),
                at: None,
            })],
        ],
    };
    assert_eq!(
        script.to_string(),
        "fly #7 (Brainstorm) hand>stack cast <- you\n\
         tap #4 (Island) <- you\n\
         & streak #4 > #7 blue 1 <- you\n\
         fly #31 (Counterspell) library>hand draw <- #7\n\
         & fly #32 (Plains) library>hand draw <- #7\n\
         vanish #7 (Brainstorm) stack>graveyard resolve\n"
    );
}

#[test]
fn a_reset_names_the_abilities_on_the_stack_and_shows_only_spans() {
    let script = Script {
        reset: Some(Reset {
            abilities: vec![4, 5],
        }),
        beats: vec![vec![
            step(Act::Point {
                span: Span::Begin,
                arrow: Arrow::Source,
                from: Subject::Ability { id: 5 },
                to: object(12),
            }),
            step(Act::Mark {
                span: Span::Begin,
                mark: Mark::Eligible,
                on: object(9),
            }),
        ]],
    };
    assert_eq!(
        script.to_string(),
        "reset a4 a5\n+point source a5 > #12\n& +mark eligible #9\n"
    );
    assert_eq!(
        Script {
            reset: Some(Reset::default()),
            beats: Vec::new(),
        }
        .to_string(),
        "reset\n"
    );
}

#[test]
fn cards_fly_to_where_their_records_put_them() {
    let bolt = spell(
        9,
        vec![
            TargetRef::Object { slot: 12 },
            TargetRef::Player {
                player: Side::Opponent,
            },
        ],
    );
    let cast = |item| Act::Fly {
        from: ZoneName::Hand,
        why: Why::Cast,
        object: card(9, "Fireball", ZoneName::Stack),
        at: None,
        item: Some(item),
    };
    assert_eq!(
        line(cast(bolt)),
        "fly #9 (Fireball) hand>stack cast at #12, opponent"
    );
    let kicked = StackItem::Spell {
        slot: 9,
        targets: Vec::new(),
        x: 3,
        paid: vec![PaidCost::Kicker],
    };
    assert_eq!(
        line(cast(kicked)),
        "fly #9 (Fireball) hand>stack cast x=3 kicker"
    );
    let to = |object: ObjectView, from, at| Act::Fly {
        from,
        why: Why::Effect,
        object,
        at,
        item: None,
    };
    // A card the viewer cannot see has no name; a library says where in it a card goes.
    assert_eq!(
        line(to(hidden(40, ZoneName::Hand), ZoneName::Library, None)),
        "fly #40 library>hand effect"
    );
    assert_eq!(
        line(to(hidden(40, ZoneName::Library), ZoneName::Hand, None)),
        "fly #40 hand>library top effect"
    );
    assert_eq!(
        line(to(
            hidden(40, ZoneName::Library),
            ZoneName::Library,
            Some(0)
        )),
        "fly #40 library>library bottom effect"
    );
    assert_eq!(
        line(to(
            hidden(40, ZoneName::Library),
            ZoneName::Library,
            Some(52)
        )),
        "fly #40 library>library @52 effect"
    );
    // An aura lands on its permanent; a stolen permanent crosses the battlefield.
    let aura = ObjectView {
        attached_to: Some(12),
        ..card(9, "Pacifism", ZoneName::Battlefield)
    };
    assert_eq!(
        line(to(aura, ZoneName::Stack, None)),
        "fly #9 (Pacifism) stack>battlefield on #12 effect"
    );
    let stolen = ObjectView {
        owner: Side::Opponent,
        ..creature(12, "Grizzly Bears", 2, 2)
    };
    assert_eq!(
        line(to(stolen, ZoneName::Battlefield, None)),
        "fly #12 (Grizzly Bears) battlefield>battlefield you effect"
    );
}

#[test]
fn cards_appear_and_vanish_with_no_flight() {
    let token = ObjectView {
        token: true,
        ..creature(200, "Soldier", 1, 1)
    };
    assert_eq!(
        line(Act::Appear {
            from: ZoneName::Nowhere,
            why: Why::Created,
            object: token,
            at: None,
            item: None,
        }),
        "appear #200 (Soldier) nowhere>battlefield created"
    );
    let gone = |subject, from, why, object| Act::Vanish {
        subject,
        from,
        why,
        object,
        at: None,
    };
    assert_eq!(
        line(gone(
            object(12),
            ZoneName::Battlefield,
            Why::Destroy,
            Some(card(12, "Grizzly Bears", ZoneName::Graveyard)),
        )),
        "vanish #12 (Grizzly Bears) battlefield>graveyard destroy"
    );
    assert_eq!(
        line(gone(
            object(200),
            ZoneName::Battlefield,
            Why::Sacrifice,
            None
        )),
        "vanish #200 battlefield>gone sacrifice"
    );
    assert_eq!(
        line(gone(
            Subject::Ability { id: 3 },
            ZoneName::Stack,
            Why::Resolve,
            None
        )),
        "vanish a3 stack>gone resolve"
    );
}

#[test]
fn abilities_emerge_and_cards_turn() {
    let from_source = trigger(Some(12), "Mulldrifter", vec![TargetRef::Object { slot: 5 }]);
    assert_eq!(
        line(Act::Emerge {
            id: 3,
            item: from_source,
        }),
        "emerge a3 (Mulldrifter, triggered) opponent from #12 at #5"
    );
    let mut emblem = trigger(None, "Teferi, Hero of Dominaria", Vec::new());
    if let StackItem::Ability { triggered, .. } = &mut emblem {
        *triggered = false;
    }
    assert_eq!(
        line(Act::Emerge {
            id: 4,
            item: emblem
        }),
        "emerge a4 (Teferi, Hero of Dominaria, activated) opponent"
    );
    let face_down = |name: Option<&str>| ObjectView {
        name: name.map(str::to_owned),
        facing: Facing::Down,
        ..creature(31, "", 2, 2)
    };
    assert_eq!(
        line(Act::Flip {
            object: face_down(None)
        }),
        "flip #31 (face down)"
    );
    assert_eq!(
        line(Act::Flip {
            object: face_down(Some("Island"))
        }),
        "flip #31 (Island, face down)"
    );
    assert_eq!(
        line(Act::Showcase {
            object: card(31, "Island", ZoneName::Library)
        }),
        "showcase #31 (Island)"
    );
    let island = card(4, "Island", ZoneName::Battlefield);
    assert_eq!(
        line(Act::Tap {
            object: island.clone()
        }),
        "untap #4 (Island)"
    );
    let tapped = ObjectView {
        tapped: true,
        ..island
    };
    assert_eq!(line(Act::Tap { object: tapped }), "tap #4 (Island)");
}

#[test]
fn damage_strikes_or_streaks_and_numbers_count() {
    let hurt = ObjectView {
        damage: 2,
        ..creature(9, "Hill Giant", 3, 3)
    };
    assert_eq!(
        line(Act::Strike {
            source: 5,
            to: object(9),
            amount: 2,
            object: Some(hurt.clone()),
        }),
        "strike #5 > #9 2"
    );
    assert_eq!(
        line(Act::Strike {
            source: 5,
            to: player(Side::Opponent),
            amount: 2,
            object: None,
        }),
        "strike #5 > opponent 2"
    );
    assert_eq!(
        line(Act::Streak {
            from: Some(Subject::Ability { id: 3 }),
            to: object(9),
            stream: Stream::Damage,
            amount: 2,
            object: Some(hurt),
        }),
        "streak a3 > #9 damage 2"
    );
    assert_eq!(
        line(Act::Streak {
            from: None,
            to: Subject::Ability { id: 3 },
            stream: Stream::Mana { color: Mana::Red },
            amount: 2,
            object: None,
        }),
        "streak nowhere > a3 red 2"
    );
    let count = |tally| line(Act::Count { tally });
    assert_eq!(
        count(Tally::Life {
            player: Side::You,
            life: 17
        }),
        "count you life 17"
    );
    let pool = |blue, green| ManaPool {
        white: 0,
        blue,
        black: 0,
        red: 0,
        green,
        colorless: 0,
    };
    assert_eq!(
        count(Tally::Pool {
            player: Side::Opponent,
            mana: pool(2, 1)
        }),
        "count opponent pool UUG"
    );
    assert_eq!(
        count(Tally::Pool {
            player: Side::You,
            mana: pool(0, 0)
        }),
        "count you pool 0"
    );
    let grown = ObjectView {
        counters: vec![CounterView {
            kind: Counter::PlusOne,
            count: 2,
        }],
        ..creature(12, "Grizzly Bears", 4, 4)
    };
    assert_eq!(
        count(Tally::Counters {
            counter: Counter::PlusOne,
            object: grown.clone()
        }),
        "count #12 (Grizzly Bears) plusOne 2"
    );
    assert_eq!(
        count(Tally::Counters {
            counter: Counter::Loyalty,
            object: grown.clone()
        }),
        "count #12 (Grizzly Bears) loyalty 0"
    );
    assert_eq!(
        count(Tally::Stats { object: grown }),
        "count #12 (Grizzly Bears) pt 4/4"
    );
}

#[test]
fn the_table_sets_and_the_stage_dwells_and_announces() {
    let set = |fact| line(Act::Set { fact });
    assert_eq!(
        set(Fact::Turn {
            number: 3,
            active: Side::Opponent,
            step: TurnStep::PrecombatMain,
        }),
        "set turn 3 opponent precombatMain"
    );
    let attacker = ObjectView {
        tapped: true,
        summoning_sick: true,
        damage: 1,
        combat: Some(Combat::Attacking {
            planeswalker: Some(20),
            target_departed: None,
            blocked: false,
        }),
        ..creature(12, "Grizzly Bears", 2, 2)
    };
    assert_eq!(
        set(Fact::Object { object: attacker }),
        "set #12 (Grizzly Bears) battlefield tapped sick 2/2 damage 1 attacking #20"
    );
    let blocker = ObjectView {
        combat: Some(Combat::Blocking { attacker: Some(12) }),
        ..creature(9, "Hill Giant", 3, 3)
    };
    assert_eq!(
        set(Fact::Object { object: blocker }),
        "set #9 (Hill Giant) battlefield 3/3 blocking #12"
    );
    assert_eq!(
        set(Fact::Library {
            player: Side::You,
            order: vec![3, 1, 2],
        }),
        "set you library 3"
    );
    assert_eq!(
        set(Fact::Item {
            subject: Subject::Ability { id: 3 },
            item: trigger(Some(12), "Mulldrifter", vec![TargetRef::Object { slot: 6 }]),
        }),
        "set a3 at #6"
    );
    assert_eq!(line(Act::Dwell { subject: object(7) }), "dwell #7");
    let announce = |notice| line(Act::Announce { notice });
    assert_eq!(
        announce(Notice::Turn {
            active: Side::Opponent
        }),
        "announce turn opponent"
    );
    assert_eq!(
        announce(Notice::Scry {
            player: Side::Opponent,
            top: 1,
            bottom: 2
        }),
        "announce scry opponent 1 top 2 bottom"
    );
    assert_eq!(
        announce(Notice::Coin {
            player: Side::You,
            won: false,
            source: 4
        }),
        "announce coin you lost by #4"
    );
}

#[test]
fn arrows_marks_and_lifts_begin_and_end() {
    let point = |span, arrow| {
        line(Act::Point {
            span,
            arrow,
            from: Subject::Ability { id: 3 },
            to: player(Side::You),
        })
    };
    assert_eq!(
        point(Span::Begin, Arrow::HostileTarget),
        "+point hostile-target a3 > you"
    );
    assert_eq!(
        point(Span::End, Arrow::HostileTarget),
        "-point hostile-target a3 > you"
    );
    assert_eq!(
        line(Act::Mark {
            span: Span::End,
            mark: Mark::FriendlyTarget,
            on: object(12),
        }),
        "-mark friendly-target #12"
    );
    let lift = |span, handling| line(Act::Lift { span, handling });
    let library = |count, scry| CardHandling::Library {
        side: Side::Opponent,
        count,
        scry,
    };
    assert_eq!(
        lift(Span::Begin, library(Some(2), Some(true))),
        "+lift opponent library 2 scry"
    );
    assert_eq!(
        lift(Span::End, library(None, None)),
        "-lift opponent library all"
    );
    assert_eq!(
        lift(
            Span::Begin,
            CardHandling::Hand {
                side: Side::Opponent
            }
        ),
        "+lift opponent hand"
    );
}

#[test]
fn a_script_survives_the_wire_and_leaves_out_what_it_lacks() {
    let script = Script {
        reset: None,
        beats: vec![vec![caused(
            Act::Tap {
                object: card(4, "Island", ZoneName::Battlefield),
            },
            player(Side::You),
        )]],
    };
    let json = serde_json::to_value(&script).unwrap();
    assert_eq!(json["beats"][0][0]["act"]["verb"], "tap");
    assert_eq!(json["beats"][0][0]["cause"]["player"], "you");
    assert!(json.get("reset").is_none());
    assert_eq!(serde_json::from_value::<Script>(json).unwrap(), script);
    // A step with no cause, and a recording with no reset, read back as none.
    let bare: Script = serde_json::from_str(
        r#"{"beats":[[{"act":{"verb":"dwell","subject":{"kind":"ability","id":3}}}]]}"#,
    )
    .unwrap();
    assert_eq!(bare.to_string(), "dwell a3\n");
}

/// The names under `key` of `data/ui-conventions.json`, in its order.
fn data_names(key: &str) -> Vec<String> {
    // Read at run time: a test binary from the shared target directory may have been built in
    // a worktree that no longer exists.
    let path = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
    )
    .join("../../../data/ui-conventions.json");
    let data: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    data[key]
        .as_array()
        .unwrap_or_else(|| panic!("data/ui-conventions.json has no list `{key}`"))
        .iter()
        .map(|entry| entry["name"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn the_design_language_lists_the_same_verbs_and_marks() {
    let verbs: Vec<_> = Verb::ALL.iter().map(|v| v.name()).collect();
    assert_eq!(
        data_names("verbs"),
        verbs,
        "data/ui-conventions.json's verbs and script::Verb must list the same names in order"
    );
    let marks: Vec<_> = Mark::ALL.iter().map(|m| m.name()).collect();
    assert_eq!(
        data_names("marks"),
        marks,
        "data/ui-conventions.json's marks and script::Mark must list the same names in order"
    );
    // A name is the wire's too.
    for &verb in Verb::ALL {
        assert_eq!(serde_json::to_value(verb).unwrap(), verb.name());
    }
    for &mark in Mark::ALL {
        assert_eq!(serde_json::to_value(mark).unwrap(), mark.name());
    }
    for &arrow in Arrow::ALL {
        assert_eq!(serde_json::to_value(arrow).unwrap(), arrow.name());
    }
}
