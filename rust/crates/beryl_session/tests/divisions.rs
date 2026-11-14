//! Divisions and catalogs answered by clicks alone: combat damage divided among
//! blockers, and the two shapes no card asks yet, an amount divided among recipients and an
//! entry of a catalog, from decisions built here. Each test holds the prompt's state after
//! each click to a text written by hand from what the screen shows, and checks the
//! picks the clicks end in.

use crate::play;

use beryl_cards::names::{
    BEZA_THE_BOUNDING_SPRING, ELITE_SPELLBINDER, ISLAND, JACE_VRYNS_PRODIGY, THUNDERTRAP_TRAINER,
    TRINKET_MAGE,
};
use beryl_engine::card::{Prompt, Purpose};
use beryl_engine::layout::Queue as _;
use beryl_engine::{
    Bitboard, Candidates, Choice, Decision, DecisionKind, Layout, Observation, Opt, Seat, Slot,
    SlotId as _, Small, TurnStep,
};
use beryl_scenario::{P0, P1, Permanent, Scenario};
use beryl_search::{Agent, AgentDecision};
use beryl_session::StopsSetting;
use beryl_session::prompt::{ButtonId, Click, Clicked, Held};
use beryl_session::protocol::ObjectId;
use beryl_session::protocol::{Candidate, Kind, Side, ZoneName};
use beryl_session::replay::Entry;
use beryl_session::save::Kind as SavedKind;
use beryl_session::script::Subject;
use play::{Play, asked, card, cards, position};

const CONFIRM: Click = Click::Button {
    button: ButtonId::Confirm,
};

/// The choices of the log from entry `from` on: the kind of decision each answered and
/// whether it continues the one before.
fn choices(play: &Play, from: usize) -> Vec<(SavedKind, bool)> {
    (play.game.replay().log[from..].iter())
        .filter_map(|entry| match entry {
            Entry::Choice(chosen) => Some((chosen.what.as_ref()?.kind, chosen.continuation)),
            _ => None,
        })
        .collect()
}

/// An opponent that blocks with everything it can, and otherwise takes each decision's first
/// candidate.
struct BlockAll;

impl<L: Layout> Agent<L> for BlockAll {
    fn choose(&mut self, _: Observation<'_, L>, d: &Decision<L>, _: AgentDecision) -> Choice {
        let block = match &d.candidates {
            Candidates::Options(options) => {
                (options.iter()).position(|o| matches!(o, Opt::Block(_)))
            }
            Candidates::Objects(_) => None,
        };
        Choice(block.unwrap_or(0) as u16)
    }
}

/// The human's turn with Beza, the Bounding Spring, a 4/5, to attack into a Trinket Mage, a
/// 2/2, and a Jace, Vryn's Prodigy, a 0/2: four damage is lethal to both.
const DOUBLE_BLOCK: Scenario = Scenario::new("combat damage")
    .at(3, P0, TurnStep::PrecombatMain)
    .permanents(P0, &[Permanent::new(BEZA_THE_BOUNDING_SPRING)])
    .permanents(
        P1,
        &[
            Permanent::new(TRINKET_MAGE),
            Permanent::new(JACE_VRYNS_PRODIGY),
        ],
    )
    .library(P0, &[ISLAND; 6])
    .library(P1, &[ISLAND; 6]);

/// Under full control, attacks with Beza into a double block, to the division of its damage.
fn divided() -> Play {
    let mut play = Play::with(&DOUBLE_BLOCK, Some(Box::new(BlockAll)), StopsSetting::EXACT);
    while play.decision().kind != Kind::DamageDivision {
        let attacks = |c: &Candidate| matches!(c, Candidate::Attack { .. });
        match play.decision().kind {
            Kind::Attacker
                if play
                    .decision()
                    .candidates
                    .iter()
                    .any(|c| attacks(&c.option)) =>
            {
                play.choose(attacks);
            }
            Kind::Attacker => play.choose(|c| *c == Candidate::Done),
            _ => play.choose(|c| *c == Candidate::Pass),
        }
    }
    play
}

/// A division of combat damage is a pick of the blockers that die. The blocker the session
/// would take next is marked as suggested, the others the damage still reaches as eligible,
/// and the button ends the division once the damage left has somewhere to go.
#[test]
fn combat_damage_is_divided_by_picking_the_blockers_that_die() {
    let mut play = divided();
    let beza = play.named(BEZA_THE_BOUNDING_SPRING, ZoneName::Battlefield, Side::You)[0];
    let mage = play.named(TRINKET_MAGE, ZoneName::Battlefield, Side::Opponent)[0];
    let jace = play.named(JACE_VRYNS_PRODIGY, ZoneName::Battlefield, Side::Opponent)[0];
    // Four damage cannot all go on one blocker without killing it, so the division cannot
    // end before a pick. Each takes two; the session would take the Mage, which hits back.
    let open = format!(
        "pick combat-damage: Assign Damage\n\
         picked 0 of 0..2\n\
         eligible #{jace}\n\
         suggested #{mage}\n\
         confirm(Done)\n"
    );
    assert_eq!(play.prompt(), open);
    assert_eq!(play.click(CONFIRM).as_deref(), Some(open.as_str()));
    assert_eq!(play.on(beza).as_deref(), Some(open.as_str()));
    // With the Mage picked the rest may stay on it, or kill Jace too.
    let one = format!(
        "pick combat-damage: Assign Damage\n\
         picked 1 of 0..2: #{mage}\n\
         selected #{mage}\n\
         suggested #{jace}\n\
         confirm[Done]*\n"
    );
    assert_eq!(play.on(mage).as_deref(), Some(one.as_str()));
    let mut both = [mage, jace];
    both.sort_unstable();
    assert_eq!(
        play.on(jace).unwrap(),
        format!(
            "pick combat-damage: Assign Damage\n\
             picked 2 of 0..2: #{mage} #{jace}\n\
             selected #{} #{}\n\
             confirm[Done]*\n",
            both[0], both[1],
        )
    );
    // A second click takes a pick back.
    assert_eq!(
        play.on(mage).unwrap(),
        format!(
            "pick combat-damage: Assign Damage\n\
             picked 1 of 0..2: #{jace}\n\
             selected #{jace}\n\
             suggested #{mage}\n\
             confirm[Done]*\n"
        )
    );
    assert_eq!(play.on(jace).as_deref(), Some(open.as_str()));

    // Both picked, the button ends the division where the two lethal picks do: the engine
    // asks for no end once nothing is left to pick.
    let from = play.game.replay().log.len();
    let mut picked = divided();
    picked.choose(|c| *c == Candidate::Lethal { slot: mage });
    picked.choose(|c| *c == Candidate::Lethal { slot: jace });
    play.on(jace);
    play.on(mage);
    assert_eq!(play.click(CONFIRM), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(
        choices(&play, from)[..2],
        [
            (SavedKind::DamageDivision, false),
            (SavedKind::DamageDivision, true)
        ],
        "one prompt, so one undo"
    );
    let theirs = |play: &Play, name| {
        play.named(name, ZoneName::Battlefield, Side::Opponent)
            .len()
    };
    assert_eq!(theirs(&play, TRINKET_MAGE), 0);
    assert_eq!(theirs(&play, JACE_VRYNS_PRODIGY), 0);

    // The Mage alone, the button says the end the engine then asks for.
    let mut play = divided();
    let mut picked = divided();
    picked.choose(|c| *c == Candidate::Lethal { slot: mage });
    picked.choose(|c| *c == Candidate::Done);
    play.on(mage);
    assert_eq!(play.click(CONFIRM), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(theirs(&play, TRINKET_MAGE), 0);
    assert_eq!(theirs(&play, JACE_VRYNS_PRODIGY), 1);
}

/// A position with three creatures for a division to be among.
const RECIPIENTS: Scenario = Scenario::new("recipients")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[ISLAND])
    .permanents(
        P1,
        &[
            Permanent::new(TRINKET_MAGE),
            Permanent::new(ELITE_SPELLBINDER),
            Permanent::new(THUNDERTRAP_TRAINER),
        ],
    )
    .library(P0, &[ISLAND; 6])
    .library(P1, &[ISLAND; 6]);

/// A decision that offers the first player the numbers `from..=to`, of `prompt`.
fn numbers(from: u8, to: u8, prompt: Prompt) -> Decision {
    let mut offered = <Small as Layout>::Queue::<Opt, 64>::default();
    for n in from..=to {
        offered.push(Opt::Number(n)).unwrap();
    }
    Decision::options(Seat::P0, DecisionKind::ChooseObject, offered, prompt)
}

/// An amount divided among recipients, which no card asks yet, from decisions built here:
/// five damage among three creatures. Each opens with one, a click that names a number sets
/// a share within what is left, and the button confirms once all is given out. Each is marked
/// as a target of the opponent's throughout. The picks are a number for each recipient in turn
/// but the last.
#[test]
fn an_amount_is_divided_among_recipients() {
    let (table, state) = position(&RECIPIENTS);
    let slot = |num: u16| Slot::from_num(num).expect("a slot");
    let among = |slots: &[u16]| {
        (slots.iter()).fold(Bitboard::EMPTY, |all, &num| {
            all | Bitboard::single(slot(num))
        })
    };
    // The decision is about the recipients still to be given a share and offers the shares
    // the first may take: of five among three, one to three.
    let prompt = Prompt::new(Purpose::DivideDamage).count(5).most(5);
    let creatures: Vec<u16> = (state.battlefield().iter())
        .map(|slot| slot.num())
        .collect();
    let d = numbers(1, 3, prompt).about(among(&creatures));
    let view = asked(&table, &state, &d);
    let mut held = Held::opening(&view, &state, &table, Some(&d)).expect("a held prompt");
    let about: Vec<ObjectId> = held.view().about.clone();
    let [a, b, c] = about[..] else {
        panic!("three recipients");
    };
    let state_of = |held: &Held| held.view().to_string();
    // Each recipient is the opponent's, and marked as the target it is.
    let aimed = cards(&[a, b, c]);
    assert_eq!(
        state_of(&held),
        format!(
            "divide divide-damage: Distribute damage.\n\
             divide 5, 2 left: #{a}=1 #{b}=1 #{c}=1\n\
             hostile-target {aimed}\n\
             confirm(Submit)\n"
        )
    );
    let share = |n: u8, on: ObjectId| Click::Number {
        n,
        on: Some(card(on)),
    };
    assert!(
        matches!(held.click(&state, &table, Some(&d), CONFIRM), Clicked::Same),
        "two are left"
    );
    assert!(matches!(
        held.click(&state, &table, Some(&d), share(3, a)),
        Clicked::Changed
    ));
    assert_eq!(
        state_of(&held),
        format!(
            "divide divide-damage: Distribute damage.\n\
             divide 5, 0 left: #{a}=3 #{b}=1 #{c}=1\n\
             hostile-target {aimed}\n\
             confirm[Submit]*\n"
        )
    );
    // Nothing is left for another share to grow by, a share is at least one, and a number
    // that names no recipient sets nothing.
    assert!(matches!(
        held.click(&state, &table, Some(&d), share(2, b)),
        Clicked::Same
    ));
    assert!(matches!(
        held.click(&state, &table, Some(&d), share(0, c)),
        Clicked::Same
    ));
    assert!(matches!(
        held.click(&state, &table, Some(&d), Click::Number { n: 2, on: None }),
        Clicked::Same
    ));
    assert!(matches!(
        held.click(&state, &table, Some(&d), share(2, a)),
        Clicked::Changed
    ));
    assert_eq!(
        state_of(&held),
        format!(
            "divide divide-damage: Distribute damage.\n\
             divide 5, 1 left: #{a}=2 #{b}=1 #{c}=1\n\
             hostile-target {aimed}\n\
             confirm(Submit)\n"
        )
    );
    assert!(matches!(
        held.click(&state, &table, Some(&d), share(2, b)),
        Clicked::Changed
    ));
    assert_eq!(
        state_of(&held),
        format!(
            "divide divide-damage: Distribute damage.\n\
             divide 5, 0 left: #{a}=2 #{b}=2 #{c}=1\n\
             hostile-target {aimed}\n\
             confirm[Submit]*\n"
        )
    );
    assert_eq!(held.view().revision, 3, "a state for each change");

    // The picks: two for the first, then two of the three left for the second. The third's
    // share is what is left, which the engine does not ask.
    let Clicked::Answers(mut run) = held.click(&state, &table, Some(&d), CONFIRM) else {
        panic!("the button answers");
    };
    assert_eq!(run.answer(&state, &d), Some(Choice(1)), "the number 2");
    let second = numbers(1, 2, prompt.most(3)).about(among(&[b.0, c.0]));
    assert_eq!(run.answer(&state, &second), Some(Choice(1)), "the number 2");
    assert_eq!(run.rest(), None);
}

/// An entry of a catalog, which no card asks yet, from a decision built here: a color. The
/// decision offers a number for each entry; the prompt lists the colors by name, a click
/// chooses one and the button confirms it.
#[test]
fn a_color_is_chosen_from_its_catalog() {
    let (table, state) = position(&RECIPIENTS);
    let d = numbers(0, 4, Prompt::new(Purpose::Color));
    let view = asked(&table, &state, &d);
    let mut held = Held::opening(&view, &state, &table, Some(&d)).expect("a held prompt");
    assert_eq!(
        held.view().to_string(),
        "catalog color: Choose Color\n\
         catalog White | Blue | Black | Red | Green\n\
         confirm(Submit 0)\n"
    );
    assert!(
        matches!(held.click(&state, &table, Some(&d), CONFIRM), Clicked::Same),
        "none chosen"
    );
    assert!(matches!(
        held.click(&state, &table, Some(&d), Click::Entry { index: 5 }),
        Clicked::Same
    ));
    assert!(matches!(
        held.click(
            &state,
            &table,
            Some(&d),
            Click::On {
                on: Subject::Player { player: Side::You }
            }
        ),
        Clicked::Same
    ));
    assert!(matches!(
        held.click(&state, &table, Some(&d), Click::Entry { index: 3 }),
        Clicked::Changed
    ));
    assert_eq!(
        held.view().to_string(),
        "catalog color: Choose Color\n\
         catalog White | Blue | Black | Red* | Green\n\
         confirm[Submit 1]*\n"
    );
    // Another entry takes its place.
    assert!(matches!(
        held.click(&state, &table, Some(&d), Click::Entry { index: 1 }),
        Clicked::Changed
    ));
    assert_eq!(
        held.view().to_string(),
        "catalog color: Choose Color\n\
         catalog White | Blue* | Black | Red | Green\n\
         confirm[Submit 1]*\n"
    );
    let Clicked::Answers(mut run) = held.click(&state, &table, Some(&d), CONFIRM) else {
        panic!("the button answers");
    };
    assert_eq!(run.answer(&state, &d), Some(Choice(1)), "blue");
    assert_eq!(run.rest(), None, "one pick");
}
