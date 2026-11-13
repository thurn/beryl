//! The yes-or-no shape's prompts answered by clicks alone: each purpose a card
//! reaches, and a plain "may" from a decision built here. Each test holds the prompt's state
//! to a text written by hand from what the screen shows, and checks that the click
//! ends where the pick a UI holding the prompt's state itself would send ends.

#[path = "common/play.rs"]
mod play;

use beryl_cards::names::{
    COUNTERSPELL, CURRENCY_CONVERTER, ELITE_SPELLBINDER, GET_LOST, ISLAND, JACE_THE_MIND_SCULPTOR,
    MANA_CRYPT, MANA_TITHE, PLAINS, SINK_INTO_STUPOR, SUBTLETY, THIRST_FOR_DISCOVERY,
    THUNDERTRAP_TRAINER, TRINKET_MAGE,
};
use beryl_engine::card::{Prompt, Purpose};
use beryl_engine::{Choice, Decision, DecisionKind, Layout, Opt, Seat, Small, TurnStep};
use beryl_scenario::{P0, P1, Permanent, Scenario};
use beryl_session::StopsSetting;
use beryl_session::prompt::{ButtonId, Click, Clicked, Held};
use beryl_session::protocol::{Candidate, Kind, Side, ZoneName};
use play::{Casts, Play, asked, position};

/// The button of the answer at `index` of the open decision.
const fn option(index: u16) -> ButtonId {
    ButtonId::Option { index }
}

/// Passes the human's priority until another decision is open.
fn pass(play: &mut Play) {
    while play.decision().kind == Kind::Priority {
        play.choose(|c| *c == Candidate::Pass);
    }
}

/// Answers the open decision yes or no by its candidate, as a UI holding the prompt does.
fn say(play: &mut Play, yes: bool) {
    let wanted = if yes { Candidate::Yes } else { Candidate::No };
    play.choose(|c| *c == wanted);
}

/// The one card the open prompt is about.
fn about(play: &Play) -> u16 {
    match play.decision().prompt.about[..] {
        [slot] => slot,
        ref cards => panic!("the prompt is about {cards:?}"),
    }
}

/// The human's main phase with Sink into Stupor to play as its land, Soporific Springs, an
/// Island beside it and a Counterspell in hand that the two together would pay for.
const SPRINGS_FOR_A_SPELL: Scenario = Scenario::new("pay life for a spell")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[SINK_INTO_STUPOR, COUNTERSPELL])
    .permanents(P0, &[Permanent::new(ISLAND)])
    .library(P0, &[PLAINS; 6])
    .library(P1, &[PLAINS; 6]);

/// As [`SPRINGS_FOR_A_SPELL`], with nothing to cast.
const SPRINGS: Scenario = Scenario::new("pay life")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[SINK_INTO_STUPOR])
    .permanents(P0, &[Permanent::new(ISLAND)])
    .library(P0, &[PLAINS; 6])
    .library(P1, &[PLAINS; 6]);

/// Whether to pay life for a land to enter untapped is two buttons about the land. Paying is
/// put forward when the land's mana would pay for a card in hand, and entering tapped
/// otherwise.
#[test]
fn life_for_a_land_is_put_forward_when_its_mana_pays_for_a_card() {
    let mut play = Play::scenario(&SPRINGS_FOR_A_SPELL);
    let land = play.named(SINK_INTO_STUPOR, ZoneName::Hand, Side::You)[0];
    assert_eq!(play.on(land), None, "the land is played by a click");
    assert_eq!(
        play.prompt(),
        format!(
            "yesNo pay-life: Pay 3 life?\n\
             about #{land}\n\
             0[Pay 3 Life]* 1[Enters Tapped]\n"
        )
    );
    // A click on a card answers nothing: the buttons do.
    let open = play.prompt();
    assert_eq!(play.on(land).as_deref(), Some(open.as_str()));
    let mut picked = Play::scenario(&SPRINGS_FOR_A_SPELL);
    picked.choose(|c| matches!(c, Candidate::PlayLand { .. }));
    say(&mut picked, true);
    assert_eq!(play.press(option(0)), None, "paying answers");
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);
    assert_eq!(play.view.players.you.life, 17);

    // With nothing the mana would pay for, the land entering tapped is put forward.
    let mut play = Play::scenario(&SPRINGS);
    let land = play.named(SINK_INTO_STUPOR, ZoneName::Hand, Side::You)[0];
    play.on(land);
    assert_eq!(
        play.prompt(),
        format!(
            "yesNo pay-life: Pay 3 life?\n\
             about #{land}\n\
             0[Pay 3 Life] 1[Enters Tapped]*\n"
        )
    );
    let mut picked = Play::scenario(&SPRINGS);
    picked.choose(|c| matches!(c, Candidate::PlayLand { .. }));
    say(&mut picked, false);
    assert_eq!(play.press(option(1)), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.view.players.you.life, 20);
}

/// P0 casts a Mana Crypt with two Islands untapped, and P1 answers it with Mana Tithe.
const TITHED: Scenario = Scenario::new("unless pays")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[MANA_CRYPT])
    .permanents(P0, &[Permanent::new(ISLAND), Permanent::new(ISLAND)])
    .hand(P1, &[MANA_TITHE])
    .permanents(P1, &[Permanent::new(PLAINS)])
    .library(P0, &[PLAINS; 6])
    .library(P1, &[PLAINS; 6]);

/// Casts the Mana Crypt and lets the Mana Tithe that answers it resolve to its question.
fn tithed() -> Play {
    let mut play = Play::against(&TITHED, Some(Box::new(Casts)));
    play.choose(|c| matches!(c, Candidate::Cast { .. }));
    pass(&mut play);
    assert!(play.decision().prompt.is(Purpose::UnlessPays));
    play
}

/// Whether to pay so that a spell is not countered: the cost in the title and on the button
/// that pays, which is put forward, with the land the payment would tap marked as suggested.
#[test]
fn a_cost_to_stop_an_effect_shows_what_paying_taps() {
    let mut play = tithed();
    let islands = play.named(ISLAND, ZoneName::Battlefield, Side::You);
    let taps = play.decision().candidates[0].payments[0].taps.clone();
    assert!(matches!(taps[..], [land] if islands.contains(&land)));
    assert_eq!(
        play.prompt(),
        format!(
            "yesNo unless-pays: Pay {{1}}.\n\
             suggested #{}\n\
             0[Pay {{1}}]* 1[Decline]\n",
            taps[0]
        )
    );
    let mut picked = tithed();
    say(&mut picked, true);
    assert_eq!(play.press(option(0)), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);
    assert_eq!(
        play.named(MANA_CRYPT, ZoneName::Battlefield, Side::You)
            .len(),
        1,
        "the Mana Crypt was paid for"
    );

    // Declining lets the Mana Tithe counter it.
    let mut play = tithed();
    assert_eq!(play.press(option(1)), None);
    assert_eq!(
        play.named(MANA_CRYPT, ZoneName::Graveyard, Side::You).len(),
        1
    );
}

/// The human's main phase with Get Lost and two creatures of their own: one to destroy for
/// its Maps, the other to explore into the Counterspell on their library.
const LOST: Scenario = Scenario::new("explore")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[GET_LOST])
    .permanents(
        P0,
        &[
            Permanent::new(PLAINS),
            Permanent::new(PLAINS),
            Permanent::new(PLAINS),
            Permanent::new(ELITE_SPELLBINDER),
            Permanent::new(THUNDERTRAP_TRAINER),
        ],
    )
    .library(P0, &[COUNTERSPELL, PLAINS, PLAINS, PLAINS])
    .library(P1, &[PLAINS; 6]);

/// Destroys the Elite Spellbinder with Get Lost and sacrifices one of its Maps for the
/// Thundertrap Trainer to explore.
fn exploring() -> Play {
    let mut play = Play::scenario(&LOST);
    let binder = play.named(ELITE_SPELLBINDER, ZoneName::Battlefield, Side::You)[0];
    let trainer = play.named(THUNDERTRAP_TRAINER, ZoneName::Battlefield, Side::You)[0];
    play.choose(|c| matches!(c, Candidate::Cast { .. }));
    play.choose(|c| *c == Candidate::Object { slot: binder });
    play.choose(|c| matches!(c, Candidate::Activate { .. }));
    play.choose(|c| *c == Candidate::Object { slot: trainer });
    pass(&mut play);
    assert!(play.decision().prompt.is(Purpose::Explore));
    play
}

/// The card an explore reveals goes to the graveyard or stays on the library: two places for
/// one card, so neither button is put forward.
#[test]
fn an_explored_card_goes_to_the_graveyard_or_stays() {
    let mut play = exploring();
    let revealed = about(&play);
    assert_eq!(play.object(revealed).name.as_deref(), Some(COUNTERSPELL));
    assert_eq!(
        play.prompt(),
        format!(
            "yesNo explore: Explore\n\
             about #{revealed}\n\
             0[Graveyard] 1[Library]\n"
        )
    );
    let mut picked = exploring();
    say(&mut picked, true);
    assert_eq!(play.press(option(0)), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);
    assert_eq!(play.object(revealed).zone, ZoneName::Graveyard);
}

/// The human's main phase with Jace, the Mind Sculptor, and a Counterspell on the opponent's
/// library.
const JACE: Scenario = Scenario::new("fateseal")
    .at(3, P0, TurnStep::PrecombatMain)
    .permanents(P0, &[Permanent::new(JACE_THE_MIND_SCULPTOR)])
    .library(P0, &[PLAINS; 6])
    .library(P1, &[COUNTERSPELL, PLAINS, PLAINS]);

/// Activates Jace's +2 at the opponent, to the look at their top card.
fn fatesealing() -> Play {
    let mut play = Play::scenario(&JACE);
    play.choose(|c| matches!(c, Candidate::Activate { ability: 0, .. }));
    let opponent = Candidate::Player {
        player: Side::Opponent,
    };
    play.choose(|c| *c == opponent);
    pass(&mut play);
    assert!(play.decision().prompt.is(Purpose::Fateseal));
    play
}

/// A fateseal of one shows the card looked at over a scry's two places, neither put forward.
#[test]
fn a_fateseal_sends_the_top_card_to_the_bottom_or_leaves_it() {
    let mut play = fatesealing();
    let top = about(&play);
    assert_eq!(play.object(top).name.as_deref(), Some(COUNTERSPELL));
    assert_eq!(
        play.prompt(),
        format!(
            "yesNo fateseal: Top of Library\n\
             about #{top}\n\
             0[BOTTOM] 1[TOP]\n"
        )
    );
    let mut picked = fatesealing();
    say(&mut picked, true);
    assert_eq!(play.press(option(0)), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);
    assert_eq!(
        play.view.players.opponent.library.first(),
        Some(&top),
        "it went to the bottom"
    );
}

/// The human's main phase with an Elite Spellbinder to cast and a Subtlety to answer it with.
const TUCK: Scenario = Scenario::new("top or bottom")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[SUBTLETY, ELITE_SPELLBINDER])
    .permanents(
        P0,
        &[
            Permanent::new(ISLAND),
            Permanent::new(ISLAND),
            Permanent::new(ISLAND),
            Permanent::new(ISLAND),
            Permanent::new(PLAINS),
            Permanent::new(PLAINS),
            Permanent::new(PLAINS),
        ],
    )
    .library(P0, &[PLAINS; 6])
    .library(P1, &[PLAINS; 6]);

/// Under full control, casts the Elite Spellbinder, casts Subtlety with it on the stack and
/// aims Subtlety's trigger at it. Returns the game at the trigger's question and the
/// Spellbinder's slot.
fn tucking() -> (Play, u16) {
    let mut play = Play::with(&TUCK, None, StopsSetting::EXACT);
    let binder = play.named(ELITE_SPELLBINDER, ZoneName::Hand, Side::You)[0];
    let subtlety = play.named(SUBTLETY, ZoneName::Hand, Side::You)[0];
    let cast = |slot: u16| move |c: &Candidate| matches!(c, Candidate::Cast { slot: s, costs, .. } if *s == slot && costs.is_empty());
    play.choose(cast(binder));
    play.choose(cast(subtlety));
    pass(&mut play);
    play.choose(|c| *c == Candidate::Number { n: 1 });
    pass(&mut play);
    assert!(play.decision().prompt.is(Purpose::TopOrBottom));
    (play, binder)
}

/// A spell sent to the top or the bottom of its owner's library is asked under the name of
/// what sends it, over the spell, with neither place put forward.
#[test]
fn a_spell_goes_on_top_of_its_library_or_under_it() {
    let (mut play, binder) = tucking();
    assert_eq!(
        play.prompt(),
        format!(
            "yesNo top-or-bottom: Subtlety\n\
             about #{binder}\n\
             0[TOP] 1[BOTTOM]\n"
        )
    );
    let (mut picked, _) = tucking();
    say(&mut picked, false);
    assert_eq!(play.press(option(1)), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);
    assert_eq!(
        play.view.players.you.library.first(),
        Some(&binder),
        "it went to the bottom"
    );
}

/// The human's main phase with Thirst for Discovery to cast beside a Currency Converter, and
/// an Island among the three cards it draws.
const CONVERT: Scenario = Scenario::new("move")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[THIRST_FOR_DISCOVERY])
    .permanents(
        P0,
        &[
            Permanent::new(CURRENCY_CONVERTER),
            Permanent::new(ISLAND),
            Permanent::new(ISLAND),
            Permanent::new(ISLAND),
        ],
    )
    .library(P0, &[COUNTERSPELL, ISLAND, COUNTERSPELL, PLAINS, PLAINS])
    .library(P1, &[PLAINS; 6]);

/// Casts Thirst for Discovery and discards the Island it drew, to the Currency Converter's
/// question about it. Returns the game and the Island's slot.
fn discarding() -> (Play, u16) {
    let mut play = Play::scenario(&CONVERT);
    play.choose(|c| matches!(c, Candidate::Cast { .. }));
    pass(&mut play);
    let island = play.named(ISLAND, ZoneName::Hand, Side::You)[0];
    assert_eq!(
        play.on(island).map(|_| ()),
        Some(()),
        "the Island is picked"
    );
    assert_eq!(play.press(ButtonId::Confirm), None, "and discarded alone");
    pass(&mut play);
    assert!(play.decision().prompt.is(Purpose::Move));
    (play, island)
}

/// Whether to move one card is asked about it, in words that say where from and where to,
/// with the move put forward.
#[test]
fn a_card_that_may_move_is_asked_about() {
    let (mut play, island) = discarding();
    assert_eq!(
        play.prompt(),
        format!(
            "yesNo move: Exile that card from your graveyard?\n\
             about #{island}\n\
             0[Take Action]* 1[Decline]\n"
        )
    );
    let (mut picked, _) = discarding();
    say(&mut picked, true);
    assert_eq!(play.press(option(0)), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);
    assert_eq!(play.object(island).zone, ZoneName::Exile);
}

/// The human's main phase with Trinket Mage to cast and a Mana Crypt in their library.
const MAGE: Scenario = Scenario::new("may search")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[TRINKET_MAGE])
    .permanents(
        P0,
        &[
            Permanent::new(ISLAND),
            Permanent::new(ISLAND),
            Permanent::new(ISLAND),
        ],
    )
    .library(P0, &[PLAINS, MANA_CRYPT, PLAINS, PLAINS])
    .library(P1, &[PLAINS; 6]);

/// Under full control, casts Trinket Mage and passes to its trigger's question.
fn searching() -> Play {
    let mut play = Play::with(&MAGE, None, StopsSetting::EXACT);
    play.choose(|c| matches!(c, Candidate::Cast { .. }));
    pass(&mut play);
    assert!(play.decision().prompt.is(Purpose::MaySearch));
    play
}

/// Whether to search at all is asked under full control alone, since declining keeps the
/// library unshuffled: a yes opens the search, a no leaves the library as it lies.
#[test]
fn whether_to_search_is_asked_under_full_control() {
    let mut play = searching();
    let open = "yesNo may-search: Search your library?\n\
                0[Take Action]* 1[Decline]\n";
    assert_eq!(play.prompt(), open);
    let before = play.view.players.you.library.clone();
    let mut picked = searching();
    say(&mut picked, false);
    assert_eq!(play.press(option(1)), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);
    assert_eq!(
        play.view.players.you.library, before,
        "nothing was shuffled"
    );

    let mut play = searching();
    assert_eq!(play.press(option(0)), None);
    assert!(
        play.decision().prompt.is(Purpose::Search),
        "the search opens"
    );
}

/// A plain "may", which no card's scenario reaches, from a decision built here: the question
/// is the asker's name, or "Take Action?" with none, and its buttons are a may's.
#[test]
fn a_plain_may_is_two_buttons() {
    let (table, state) = position(&SPRINGS);
    let mut answers = <Small as Layout>::Queue::<Opt, 64>::default();
    answers.push(Opt::Yes).unwrap();
    answers.push(Opt::No).unwrap();
    let d = Decision::options(
        Seat::P0,
        DecisionKind::YesNo,
        answers,
        Prompt::new(Purpose::May),
    );
    let view = asked(&table, &state, &d);
    let mut held = Held::opening(&view, &state, &table, Some(&d)).expect("a held prompt");
    assert_eq!(
        held.view().to_string(),
        "yesNo may: Take Action?\n\
         0[Take Action]* 1[Decline]\n"
    );
    let confirm = Click::Button {
        button: ButtonId::Confirm,
    };
    assert!(
        matches!(held.click(&state, &table, Some(&d), confirm), Clicked::Same),
        "it has none"
    );
    let Clicked::Answers(mut run) = held.click(
        &state,
        &table,
        Some(&d),
        Click::Button { button: option(1) },
    ) else {
        panic!("declining answers");
    };
    assert_eq!(run.answer(&state, &d), Some(Choice(1)));
    assert_eq!(run.rest(), None, "one pick");
}
