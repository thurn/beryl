//! The arrows, lines, lifts and mana of the scripts the session builds, and the
//! rule invariants that hold them (K24): a hand-written expectation per verb for both seats,
//! the sequences behind two themes of repeated bug reports, and for each invariant a
//! hand-built script that breaks it: the last two, a step whose cause was not there.

mod common;

use beryl_cards::{CARDS, names};
use beryl_search::Budget;
use beryl_session::protocol::{
    Candidate, Kind, ObjectView, Side, StackItem, TargetRef, Why, ZoneName,
};
use beryl_session::script::{
    Act, Arrow, Follow, Heard, Notice, Open, Screen, Script, Span, Step, Subject, Table, carried,
};
use beryl_session::{CandidateIndex, Game, Prepared, Seat, SessionId, Status, StopsSetting};
use common::{script, scripts, text, views};

/// The steps of `card`'s scenario `name` whose text has `part`, over all its scripts, for the
/// player who goes first and for the other: each with the number of its script.
fn steps(card: &str, name: &str, part: &str) -> [Vec<(usize, String)>; 2] {
    let mut found: [Vec<(usize, String)>; 2] = Default::default();
    for (n, seats) in scripts(card, name).into_iter().enumerate() {
        for (seat, script) in seats.into_iter().enumerate() {
            let lines = script.lines().filter(|line| line.contains(part));
            found[seat].extend(lines.map(|line| (n, line.trim_start_matches("& ").to_owned())));
        }
    }
    found
}

fn lines(found: &[(usize, &str)]) -> Vec<(usize, String)> {
    found
        .iter()
        .map(|&(n, line)| (n, line.to_owned()))
        .collect()
}

const TEFERI_FALLS: &str = "combat damage removes Teferi while other attacks remain";

/// `point`, an attack line: only an attacker at a planeswalker has one (Arena,
/// `UrGalw_f35w 48:56.6`; one at the player has only its chevron). It begins once what it
/// attacks is chosen, and ends before that planeswalker leaves. The same from both seats,
/// but for who the player is.
#[test]
fn an_attack_line_begins_with_its_destination_and_ends_before_that_leaves() {
    let [first, second] = script(names::TEFERI_TIME_RAVELER, TEFERI_FALLS, 7);
    let chosen = text(&[
        "set #0 (Elite Spellbinder) battlefield 3/1 attacking #256",
        "& +point attack #0 > #256",
    ]);
    assert_eq!(first, chosen);
    assert_eq!(second, chosen);
    let [first, second] = script(names::TEFERI_TIME_RAVELER, TEFERI_FALLS, 9);
    let combat = |defender: &str, attacker: &str| {
        text(&[
            "tap #0 (Elite Spellbinder)".to_owned(),
            "& tap #2 (Snapcaster Mage)".to_owned(),
            format!("set turn 3 {attacker} declareBlockers"),
            format!("set turn 3 {attacker} combatDamage"),
            "strike #0 > #256 3".to_owned(),
            format!("& strike #2 > {defender} 2"),
            format!("& count {defender} life 18"),
            "-point attack #0 > #256".to_owned(),
            "vanish #256 (Teferi, Time Raveler) battlefield>graveyard stateBased".to_owned(),
            format!("set turn 3 {attacker} endOfCombat"),
        ])
    };
    assert_eq!(first, combat("opponent", "you"));
    assert_eq!(second, combat("you", "opponent"));
}

/// The theme of five reports (by-y3sp, by-kq8i, by-2wsl, by-m0wf, by-fm8): the combat line
/// that went away and came back, pointed at the player before the planeswalker, or swung
/// back to the player when its planeswalker died. Over a whole combat each line begins once,
/// not before its destination is chosen, and ends at most once.
#[test]
fn a_combat_line_is_begun_once_and_never_swings() {
    let [first, second] = steps(names::TEFERI_TIME_RAVELER, TEFERI_FALLS, "point attack");
    assert_eq!(
        first,
        lines(&[
            (7, "+point attack #0 > #256"),
            (9, "-point attack #0 > #256"),
        ])
    );
    assert_eq!(
        second,
        lines(&[
            (7, "+point attack #0 > #256"),
            (9, "-point attack #0 > #256"),
        ])
    );
    // The last attacker declared, sent at a planeswalker, begins its line as it taps, not
    // after combat (by-w8gv.21.57).
    let name = "combat damage removes Teferi while the other planeswalker is attacked";
    let [first, second] = steps(names::TEFERI_TIME_RAVELER, name, "point attack");
    let both = lines(&[
        (6, "+point attack #0 > #256"),
        (8, "+point attack #2 > #259"),
        (8, "-point attack #0 > #256"),
    ]);
    assert_eq!(first, both);
    assert_eq!(second, both);
    // An attacker's line stays through its attack trigger and through the choice of what
    // the token the trigger makes is attacking (by-y3sp).
    let name = "Adeline's Human attacks a planeswalker";
    let [first, second] = steps(names::ADELINE_RESPLENDENT_CATHAR, name, "point attack");
    assert_eq!(first, lines(&[(5, "+point attack #512 > #256")]));
    assert_eq!(second, lines(&[(5, "+point attack #512 > #256")]));
}

/// `point`, a block line: it begins as the blocker is declared and ends, once combat damage
/// is done, before the creatures that damage destroys leave. The dies trigger lifts out of
/// its source first, and its line to that source ends with the block line.
#[test]
fn a_block_line_lasts_until_combat_damage_is_done() {
    let name = "Sphinx dies as a card, then as a token";
    let [first, second] = script(names::SPHINX_OF_FALSE_CONCLUSIONS, name, 7);
    let declared = text(&[
        "set #1 (Sphinx of False Conclusions) battlefield tapped 4/2 attacking",
        "& set #256 (Elite Spellbinder) battlefield 3/1 blocking #1",
        "& +point block #256 > #1",
    ]);
    assert_eq!(first, declared);
    assert_eq!(second, declared);
    let [first, second] = script(names::SPHINX_OF_FALSE_CONCLUSIONS, name, 8);
    let damage = |who: &str| {
        let who = who.to_owned();
        let turn = |step: &str| format!("set turn 1 {who} {step}");
        text(&[
            turn("combatDamage"),
            "strike #1 > #256 4".to_owned(),
            "& strike #256 > #1 3".to_owned(),
            format!("emerge a2 (Sphinx of False Conclusions, triggered) {who} from #1 <- {who}"),
            "& +point source a2 > #1".to_owned(),
            "-point block #256 > #1".to_owned(),
            "& -point source a2 > #1".to_owned(),
            "vanish #1 (Sphinx of False Conclusions) battlefield>graveyard stateBased".to_owned(),
            "& vanish #256 (Elite Spellbinder) battlefield>graveyard stateBased".to_owned(),
        ])
    };
    assert_eq!(first, damage("you"));
    assert_eq!(second, damage("opponent"));
}

/// The same theme through the session itself (by-m0wf, by-fm8), whose views say which
/// attackers still await what they attack: a lone attacker is sent at a planeswalker, and
/// its attack trigger's token after it. Neither line points at the player first, though each
/// creature is attacking with nothing chosen for a view, and the first step of the script
/// that follows the choice comes before the step that says what was chosen.
#[test]
fn a_line_at_a_planeswalker_never_points_at_the_player_first() {
    let (_, scenarios) = beryl_cards::scenarios()
        .iter()
        .find(|&&(card, _)| card == names::ADELINE_RESPLENDENT_CATHAR)
        .unwrap();
    let scenario = scenarios
        .iter()
        .find(|s| s.name == "Adeline's Human attacks a planeswalker")
        .unwrap();
    let (table, state) = beryl_scenario::position(&CARDS, scenario).unwrap();
    let random = beryl_search::agent("random").unwrap();
    let mut game = Game::prepared(
        Prepared {
            opponent: (random.new)(&table, 1, Budget::DEFAULT),
            stand_in: (random.new)(&table, 2, Budget::DEFAULT),
            scripted: false,
            hand_over: None,
            table,
            state,
            human: Seat::P0,
            stops: StopsSetting::SMART,
        },
        SessionId(1),
    );
    let mut lines = Vec::new();
    let mut pending = Vec::new();
    while let Some(view) = game.next_view() {
        if view.turn.number > 1 {
            break;
        }
        let script = view.script.as_ref().expect("a view's script").to_string();
        let attack = script.lines().filter(|line| line.contains("point attack"));
        lines.extend(attack.map(|line| line.trim_start_matches("& ").to_owned()));
        pending.extend(view.pending_attackers.clone().unwrap_or_default());
        let Status::Deciding { decision } = &view.status else {
            continue;
        };
        // Attack with everything, at the planeswalker; otherwise the first candidate, which
        // passes priority.
        let wanted = |option: &Candidate| match decision.kind {
            Kind::Attacker => matches!(option, Candidate::Attack { .. }),
            Kind::AttackTarget => matches!(
                option,
                Candidate::Defender {
                    planeswalker: Some(_)
                }
            ),
            _ => false,
        };
        let candidates = &decision.candidates;
        let index = candidates.iter().position(|c| wanted(&c.option));
        let index = CandidateIndex(u16::try_from(index.unwrap_or(0)).unwrap());
        game.choose(game.answer(decision.id, index).unwrap());
    }
    // Both creatures awaited their choice in a view of their own.
    assert_eq!(pending, [0, 512]);
    // Each line is begun once, at the planeswalker, and ended once, as combat ends.
    assert_eq!(
        lines,
        [
            "+point attack #0 > #256",
            "+point attack #512 > #256",
            "-point attack #0 > #256",
            "-point attack #512 > #256",
        ]
    );
}

const HOLE: &str = "Portable Hole's target gone before the trigger resolves";

/// `point`, the lines of a combat that begins and ends inside one script: the seat's screen is
/// shown no view from the attack to the deaths combat damage deals, as a seat is that has
/// nothing to decide meanwhile. The block line begins with the block's event and ends before
/// the creatures leave, though the view's end has no line, as when each part had a view.
#[test]
fn a_combat_inside_one_script_shows_its_lines() {
    let name = "Sphinx dies as a card, then as a token";
    let views = common::views_heard(names::SPHINX_OF_FALSE_CONCLUSIONS, name, |_, _, _| {});
    // The second combat: its attack is view 18's, its block and damage view 21's.
    let (shown, combat) = views.split_at(17);
    let combat = &combat[..5];
    let mut screen = Screen::default();
    for seats in shown {
        let (view, heard) = &seats[0];
        screen.script(view, heard);
    }
    let mut merged = Heard::default();
    let mut events = Vec::new();
    for seats in combat {
        let (view, heard) = &seats[0];
        let before = events.len();
        let moves = heard.moves.iter().cloned();
        merged.moves.extend(moves.map(|(at, m)| (at + before, m)));
        merged.causes.extend(&heard.causes);
        merged.together.extend(&heard.together);
        events.extend(view.events.iter().cloned());
        merged.stack.clone_from(&heard.stack);
    }
    let mut view = combat[4][0].0.clone();
    view.events = events;
    let script = screen.script(&view, &merged).to_string();
    let lines: Vec<&str> = script
        .lines()
        .map(|line| line.trim_start_matches("& "))
        .filter(|line| {
            ["point block", "strike", "vanish #"]
                .iter()
                .any(|part| line.contains(part))
        })
        .collect();
    assert_eq!(
        lines,
        [
            "+point block #258 > #512",
            "strike #258 > #512 3",
            "strike #512 > #258 4",
            "-point block #258 > #512",
            "vanish #258 (Elite Spellbinder) battlefield>graveyard stateBased",
            "vanish #512 battlefield>gone stateBased",
        ]
    );
}

/// The theme of six reports (by-yfw, by-ad8, by-8uw among them): an arrow left on a card that
/// moved, or drawn at it in its graveyard or hand. A trigger aims at a permanent that is
/// returned to its owner's hand in response: the trigger's arrow ends before the permanent
/// goes, in the same beat as the arrow of the spell that returns it, and is not drawn again
/// though the trigger stays on the stack still naming the card, now in a hand.
#[test]
fn an_arrow_ends_before_its_target_moves_and_does_not_follow_it() {
    let [first, second] = script(names::PORTABLE_HOLE, HOLE, 5);
    let returned = text(&[
        "-point hostile-target a1 > #256",
        "& -point hostile-target #4 > #256",
        "vanish #256 (Snapcaster Mage) battlefield>hand effect <- #4",
        "vanish #4 (Sink into Stupor) stack>graveyard resolve <- #4",
    ]);
    assert_eq!(first, returned);
    assert_eq!(second, returned);
    let aimed = lines(&[
        (2, "+point hostile-target a1 > #256"),
        (5, "-point hostile-target a1 > #256"),
    ]);
    let [first, second] = steps(names::PORTABLE_HOLE, HOLE, "target a1");
    assert_eq!(first, aimed);
    assert_eq!(second, aimed);
    // What the UI drew the arrow from: the view after still has the trigger on the stack
    // aimed at the card's slot, and the card in its owner's hand.
    let view = &views(names::PORTABLE_HOLE, HOLE, |_, _, _| {})[6][0];
    assert!(matches!(
        view.stack.as_slice(),
        [StackItem::Ability { targets, .. }] if *targets == [TargetRef::Object { slot: 256 }]
    ));
    let creature = view.objects.iter().find(|o| o.slot == 256).unwrap();
    assert_eq!(creature.zone, ZoneName::Hand);
    // A spell returned to its owner's hand takes no arrow there with it (by-8uw): the
    // arrow at it, and its own at what it would have countered, end before it goes.
    let name = "Reprieve on a counterspell saves the spell it targets";
    let [first, _] = script(names::REPRIEVE, name, 6);
    assert_eq!(
        first,
        text(&[
            "-point hostile-target #259 > #0",
            "& -point hostile-target #3 > #259",
            "vanish #259 (Counterspell) stack>hand effect <- #3",
            "fly #4 (Island) library>hand draw <- #3",
            "vanish #3 (Reprieve) stack>graveyard resolve <- #3",
        ])
    );
    let [first, _] = steps(names::REPRIEVE, name, "target #3 > #259");
    assert_eq!(
        first,
        lines(&[
            (5, "+point hostile-target #3 > #259"),
            (6, "-point hostile-target #3 > #259"),
        ])
    );
}

/// `count` of a pool and `streak` of mana from one: mana an effect adds with nothing to pay
/// for counts in its player's pool, and mana a pool has lost by the time a spell is cast
/// streaks from the player into the spell.
#[test]
fn mana_counts_in_a_pool_and_streaks_out_of_it() {
    let name = "Mana Drain: the countered spell's mana value at the caster's next main phase";
    let [first, second] = steps(names::MANA_DRAIN, name, "pool");
    assert_eq!(first, lines(&[(7, "count you pool CCC <- a1")]));
    assert_eq!(second, lines(&[(7, "count opponent pool CCC <- a1")]));
    // No scenario casts a spell with floating mana, so the view before a cast is given one
    // blue mana in the caster's pool, which the view after the cast no longer has.
    let name = "Counterspell counters a spell into its owner's graveyard";
    let floating = |n: usize, seat: u8, view: &mut beryl_session::protocol::PlayerView| {
        if n == 1 {
            let caster = if seat == 0 {
                &mut view.players.you
            } else {
                &mut view.players.opponent
            };
            caster.mana.blue = 1;
        }
    };
    let [first, second] = views(names::COUNTERSPELL, name, floating).swap_remove(2);
    let paid = |who: &str, dwell: bool| {
        let mut lines = vec![
            format!("fly #1 (Counterspell) hand>stack cast at #257 <- {who}"),
            "& +point hostile-target #1 > #257".to_owned(),
            format!("tap #0 (Island) <- {who}"),
            format!("& tap #2 (Island) <- {who}"),
            "& streak #0 > #1 blue 1 <- #0".to_owned(),
            "& streak #2 > #1 blue 1 <- #2".to_owned(),
            // The cast is announced after its lands are tapped: what the pool paid follows.
            format!("streak {who} > #1 blue 1 <- {who}"),
            format!("& count {who} pool 0 <- {who}"),
        ];
        if dwell {
            lines.push("dwell #1".to_owned());
        }
        lines.extend(
            [
                "-point hostile-target #1 > #257",
                "vanish #257 (Portable Hole) stack>graveyard counter <- #1",
                "& vanish #1 (Counterspell) stack>graveyard resolve <- #1",
            ]
            .map(str::to_owned),
        );
        text(&lines)
    };
    assert_eq!(first.script.unwrap().to_string(), paid("you", false));
    assert_eq!(second.script.unwrap().to_string(), paid("opponent", true));
}

/// `streak` of mana from a source sacrificed for it: the Treasure's mana leaves where the
/// Treasure lay, as it is taken from sight, after the mana of the land tapped with it: all in
/// one beat, the payment being one event of the rules.
#[test]
fn a_sacrificed_sources_mana_streaks_from_where_it_lay() {
    let name = "Beza's Treasure pays for Counterspell";
    let [first, second] = scripts(names::BEZA_THE_BOUNDING_SPRING, name).swap_remove(9);
    let paid = |who: &str| {
        text(&[
            format!("fly #1 (Counterspell) hand>stack cast at #258 <- {who}"),
            "& +point hostile-target #1 > #258".to_owned(),
            format!("tap #0 (Island) <- {who}"),
            format!("& tap #512 (Treasure Token) <- {who}"),
            "& streak #0 > #1 blue 1 <- #0".to_owned(),
            format!("& vanish #512 battlefield>gone sacrifice <- {who}"),
            "& streak #512 > #1 blue 1 <- #512".to_owned(),
        ])
    };
    assert_eq!(first, paid("you"));
    assert_eq!(second, paid("opponent"));
}

/// `dwell` at the game's end: the view that ends the game leaves its table to be seen, in a
/// last beat of its own, before the result covers it. No scenario plays a game out, so the
/// view after combat damage is told that the damage won the first player the game.
#[test]
fn the_games_last_play_is_left_to_be_seen() {
    use beryl_session::protocol::{LossReason, OutcomeView, Side};
    let name = "Mana Crypt's flip lost: 3 damage to its controller";
    let over = |n: usize, seat: u8, view: &mut beryl_session::protocol::PlayerView| {
        if n == 2 {
            view.status = beryl_session::protocol::Status::Over {
                outcome: OutcomeView::Win {
                    winner: if seat == 0 { Side::Opponent } else { Side::You },
                    reason: LossReason::Life,
                    by: None,
                },
            };
        }
    };
    let [first, second] = views(names::MANA_CRYPT, name, over).swap_remove(2);
    let last = |view: beryl_session::protocol::PlayerView| {
        let script = view.script.expect("a view's script").to_string();
        script.lines().last().map(str::to_owned)
    };
    assert_eq!(last(first).as_deref(), Some("dwell you"));
    assert_eq!(last(second).as_deref(), Some("dwell opponent"));
}

/// The table the first player's screen shows before a counterspell is cast at a spell on the
/// stack, and their script of the cast and its resolution: an arrow that begins as the
/// counterspell goes on the stack, the mana, the arrow's end in a beat of its own, and both
/// spells leaving. The rule invariants hold it, and each test below breaks one.
fn counterspell() -> (Table, Script) {
    let name = "Counterspell counters a spell into its owner's graveyard";
    let views = views(names::COUNTERSPELL, name, |_, _, _| {});
    let script = views[2][0].script.clone().unwrap();
    assert_eq!(script.beats.len(), 4);
    assert_eq!(
        carried(Table::of(&views[1][0]), &[], &script),
        Ok(Vec::new())
    );
    (Table::of(&views[1][0]), script)
}

/// The counterspell's arrow at the spell it counters.
const ARROW: Open = Open::Point {
    arrow: Arrow::HostileTarget,
    from: Subject::Object { slot: 1 },
    to: Subject::Object { slot: 257 },
};

fn step(act: Act) -> Step {
    Step { act, cause: None }
}

fn broken(table: &Table, open: &[Open], script: &Script) -> String {
    carried(table.clone(), open, script).expect_err("the script breaks a rule")
}

/// No arrow points at a thing that is moving or has left.
#[test]
fn an_arrow_at_a_thing_that_is_moving_or_gone_breaks_a_rule() {
    let (table, whole) = counterspell();
    // The arrow begins in the beat that takes its target off the stack, after it has gone.
    let mut late = whole.clone();
    late.beats[0].truncate(1);
    late.beats.remove(2);
    late.beats[2].insert(1, step(ARROW.act(Span::Begin)));
    assert_eq!(
        broken(&table, &[], &late),
        "`+point hostile-target #1 > #257`: #257 is moving"
    );
    // A line from an ability that is not on the stack.
    let source = Open::Point {
        arrow: Arrow::Source,
        from: Subject::Ability { id: 9 },
        to: Subject::Object { slot: 257 },
    };
    let gone = Script {
        reset: None,
        beats: vec![vec![step(source.act(Span::Begin))]],
    };
    assert_eq!(
        broken(&table, &[], &gone),
        "`+point source a9 > #257`: a9 has left the table"
    );
}

/// An arrow is ended before its target moves: in an earlier beat.
#[test]
fn an_arrow_not_ended_before_its_target_moves_breaks_a_rule() {
    let (table, whole) = counterspell();
    // As the UI drew it, from each view afresh: the arrow still there as the card left.
    let mut kept = whole.clone();
    kept.beats.remove(2);
    assert_eq!(
        broken(&table, &[], &kept),
        "`vanish #257 (Portable Hole) stack>graveyard counter <- #1`: \
         `+point hostile-target #1 > #257` is not ended before #257 moves"
    );
    // Ended in the beat that moves its target is not before.
    let mut together = whole.clone();
    let end = together.beats.remove(2);
    together.beats[2].splice(0..0, end);
    assert_eq!(
        broken(&table, &[], &together),
        "`vanish #257 (Portable Hole) stack>graveyard counter <- #1`: \
         `+point hostile-target #1 > #257` ends in the beat that moves #257, not before it"
    );
}

/// One visual per thing.
#[test]
fn two_visuals_of_one_thing_break_a_rule() {
    let (table, whole) = counterspell();
    // The same arrow twice.
    let mut doubled = whole.clone();
    doubled.beats[0].push(step(ARROW.act(Span::Begin)));
    assert_eq!(
        broken(&table, &[], &doubled),
        "`+point hostile-target #1 > #257`: it is begun already: one visual per thing"
    );
    // A card flying to two places in one beat.
    let mut twice = whole.clone();
    let fly = twice.beats[0][0].clone();
    twice.beats[0].push(fly);
    assert_eq!(
        broken(&table, &[], &twice),
        "`fly #1 (Counterspell) hand>stack cast at #257 <- you`: \
         #1 moves twice in one beat: one visual per thing"
    );
    // A card on the table coming to be again.
    let crypt: ObjectView = table.objects[&257].clone();
    let again = Script {
        reset: None,
        beats: vec![vec![step(Act::Appear {
            from: ZoneName::Nowhere,
            why: Why::Created,
            object: crypt,
            at: None,
            item: None,
        })]],
    };
    assert_eq!(
        broken(&table, &[], &again),
        "`appear #257 (Portable Hole) nowhere>stack created`: \
         #257 is on the table already: one visual per thing"
    );
}

/// Nothing leaves the stack that the table does not hold there: a stage that started over
/// from a view leaving out a resolving spell or ability would have nothing to take away.
#[test]
fn taking_off_the_stack_what_it_does_not_hold_breaks_a_rule() {
    let (mut table, whole) = counterspell();
    // The spell countered, its card on the stack, but no entry for it on the table's stack.
    table
        .stack
        .retain(|e| !matches!(e.item, StackItem::Spell { slot: 257, .. }));
    assert_eq!(
        broken(&table, &[], &whole),
        "`vanish #257 (Portable Hole) stack>graveyard counter <- #1`: \
         #257 leaves the stack, which does not hold it"
    );
    // An ability that was never on the stack, finishing.
    let finish = Script {
        reset: None,
        beats: vec![vec![step(Act::Vanish {
            subject: Subject::Ability { id: 9 },
            from: ZoneName::Stack,
            why: Why::Resolve,
            object: None,
            at: None,
        })]],
    };
    assert_eq!(
        broken(&table, &[], &finish),
        "`vanish a9 stack>gone resolve`: a9 leaves the stack, which does not hold it"
    );
}

/// A coin's flip, as a step that names the ability `a1` its cause, and what a script says that
/// has the step where no such ability is.
fn flip() -> (Step, &'static str) {
    let notice = Notice::Coin {
        player: Side::You,
        won: false,
        source: 0,
    };
    let cause = Some(Subject::Ability { id: 1 });
    let act = Act::Announce { notice };
    let broken = "`announce coin you lost by #0 <- a1`: its cause a1 is not on the table as it runs, \
                  and did not leave it in this beat";
    (Step { act, cause }, broken)
}

/// The script that puts Adeline's trigger on the stack and the script that resolves it, with
/// the table and the spans each plays over.
fn trigger() -> [(Table, Vec<Open>, Script); 2] {
    let name = "Adeline's Human arrives attacking";
    let views = views(names::ADELINE_RESPLENDENT_CATHAR, name, |_, _, _| {});
    let has = |s: &Script, lifts: bool| {
        (s.beats.iter().flatten()).any(|step| match step.act {
            Act::Emerge { id: 1, .. } => lifts,
            Act::Vanish { subject, .. } => !lifts && subject == Subject::Ability { id: 1 },
            _ => false,
        })
    };
    // What the seat's screen shows and has open before each of its scripts.
    let mut shown = (Table::of(&views[0][0]), Vec::new());
    let mut found = Vec::new();
    for seats in &views {
        let script = seats[0].script.clone().unwrap();
        if has(&script, found.is_empty()) {
            found.push((shown.0.clone(), shown.1.clone(), script.clone()));
        }
        let open = carried(shown.0.clone(), &shown.1, &script).unwrap();
        shown = (
            beryl_session::script::replay(shown.0, &script).unwrap(),
            open,
        );
    }
    found
        .try_into()
        .expect("a script that lifts it and one that resolves it")
}

/// A step names a cause that existed when it ran: an ability that has yet to go on the stack
/// made nothing happen.
#[test]
fn a_cause_yet_to_come_breaks_a_rule() {
    let [(table, open, lift), _] = trigger();
    assert!(carried(table.clone(), &open, &lift).is_ok());
    let (flip, broke) = flip();
    // In a beat before the one that puts the ability on the stack.
    let mut early = lift.clone();
    early.beats.insert(0, vec![flip.clone()]);
    assert_eq!(broken(&table, &open, &early), broke);
    // Once it is there, it may.
    let mut after = lift;
    after.beats.push(vec![flip]);
    assert!(carried(table, &open, &after).is_ok());
}

/// And one that left the stack in a beat before made nothing happen either; in the beat it
/// leaves in, it still may: the last of its effects, as it resolves.
#[test]
fn a_cause_that_is_gone_breaks_a_rule() {
    let [_, (table, open, resolve)] = trigger();
    let (flip, broke) = flip();
    let leaves = |beat: &Vec<Step>| {
        let gone = Subject::Ability { id: 1 };
        (beat.iter()).any(|s| matches!(s.act, Act::Vanish { subject, .. } if subject == gone))
    };
    let at = resolve.beats.iter().position(leaves).unwrap();
    let mut late = resolve.clone();
    late.beats.insert(at + 1, vec![flip.clone()]);
    assert_eq!(broken(&table, &open, &late), broke);
    let mut last = resolve;
    last.beats[at].push(flip);
    assert!(carried(table, &open, &last).is_ok());
}

/// A view shown first that is not its seat's first starts over with a reset that names what
/// its stage holds as the scripts before it named it: the ability on the stack by the number
/// its `emerge` gave it, and its line to its source begun at once. The seat's scripts after it
/// then play over that reset.
#[test]
fn a_view_shown_first_starts_over_where_its_scripts_left_the_stack() {
    let name = "Thieving Skydiver kicked steals Mana Crypt over a Map";
    let views = views(names::THIEVING_SKYDIVER, name, |_, _, _| {});
    for seat in 0..2 {
        let mut follow = Follow::default();
        for seats in &views[..8] {
            follow
                .play(&seats[seat])
                .expect("the session's scripts play");
        }
        let mut first = views[7][seat].clone();
        let restart = follow.restart(&first).expect("a restart");
        assert_eq!(
            restart.to_string(),
            "reset a1\n+point source a1 > #0\n",
            "seat {seat}"
        );
        first.script = Some(restart);
        let mut shown = Follow::default();
        shown.play(&first).expect("the restart plays");
        for later in &views[8..] {
            shown.play(&later[seat]).expect("the scripts after it play");
        }
    }
}

/// Every beginning has an end, in its script or carried to a later one.
#[test]
fn a_span_is_ended_by_its_script_or_carried_to_the_next() {
    let (table, whole) = counterspell();
    // The script cut after the cast leaves the arrow open, for the next script to end.
    let mut cast = whole.clone();
    let rest = Script {
        reset: None,
        beats: cast.beats.split_off(2),
    };
    assert_eq!(carried(table.clone(), &[], &cast), Ok(vec![ARROW]));
    let shown = beryl_session::script::replay(table.clone(), &cast).unwrap();
    assert_eq!(carried(shown.clone(), &[ARROW], &rest), Ok(Vec::new()));
    // An end with no beginning: the screen was not carrying the arrow.
    assert_eq!(
        broken(&shown, &[], &rest),
        "`-point hostile-target #1 > #257`: it ends what was never begun"
    );
    // A reset drops what was carried: the stage starts over with the spans it begins.
    let over = Script {
        reset: Some(beryl_session::script::Reset::default()),
        beats: Vec::new(),
    };
    assert_eq!(carried(shown, &[ARROW], &over), Ok(Vec::new()));
}

/// A span left open is carried by the session into its seat's next script: an ability's
/// arrow and its line to its source begin in one script and end in a later one, each before
/// what it is on moves.
#[test]
fn the_session_carries_an_open_span_to_the_script_that_ends_it() {
    let name = "Thieving Skydiver kicked steals Mana Crypt over a Map";
    let [first, second] = steps(names::THIEVING_SKYDIVER, name, " a1 > ");
    let spans = lines(&[
        (7, "+point source a1 > #0"),
        (9, "+point hostile-target a1 > #256"),
        (10, "-point hostile-target a1 > #256"),
        (10, "-point source a1 > #0"),
    ]);
    assert_eq!(first, spans);
    assert_eq!(second, spans);
}
