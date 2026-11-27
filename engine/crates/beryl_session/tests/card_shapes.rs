//! The session plays each keystone of card shapes and face-down cards: a split card cast as
//! its second half and fused, an Adventure, a prepared creature, a modal double-faced card, a
//! prototype, a transforming and a flipping permanent, meld, a Saga, a Class, a Case, a
//! leveler, morph and Illusionary Mask, and cards brought in from outside the game: Burning
//! Wish and the companion Lurrus of the Dream-Den. Each game is a deck of the keystone, its partners and
//! the lands that cast them against a deck of lands, with the human casting each card as the
//! face the keystone names whenever they can and every update drawn for both seats, so each
//! script is held to the replay check of a debug build (`script/build.rs`).
//!
//! At the first decision of each new state the game reaches, as the human's view shows it, its
//! save is cut off there and replayed: to the same view as the live game, to the same position
//! under each perturbation a save must not depend on (`beryl_session::perturb`), and then on
//! for one more decision.
//!
//! The game also holds the session to sending the words of what a card's printed text has no
//! paragraph for: a Saga's chapter, level up and turning a morph face up.
//!
//! The wish and the companion games start with a sideboard. Their views must name the
//! sideboard card on the wish's pick, on the companion question before the game and on the
//! {3} action that puts the revealed companion into its owner's hand, and must then show the
//! card in the human's hand.

use std::collections::BTreeSet;

use beryl_cards::{CARDS, names};
use beryl_engine::card::{CardShape, FaceIx, Purpose};
use beryl_engine::{Layout, Seat, State};
use beryl_session::perturb::{self, Perturbation};
use beryl_session::protocol::{
    Candidate, DecisionView, DesignationView, Facing, ObjectView, PlayerView, Side, StackItem,
    ZoneName,
};
use beryl_session::{CandidateIndex, Game, ReplayFile, SessionId, StartGame, Status, StopsSetting};

const SESSION: SessionId = SessionId(1);

/// A new state the human's view shows at a decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Moment {
    /// The first decision once a spell the human cast has left the stack.
    Resolved,
    /// The first decision once an ability the human activated has left the stack.
    Activated,
    /// A permanent of the human's with its back face up: transformed or flipped.
    BackFaceUp,
    /// A melded permanent, with the card under it.
    Melded,
    /// A face-down spell on the stack.
    FaceDownSpell,
    /// A face-down permanent.
    FaceDownPermanent,
    /// A Saga with a second lore counter.
    SecondChapter,
    /// A Class at level 2 or higher.
    Leveled,
    /// A solved Case.
    Solved,
    /// A prepared creature.
    Prepared,
    /// The human picks a card from their sideboard for a wish.
    WishPick,
    /// The human is asked, before the game, whether to reveal a companion.
    CompanionReveal,
    /// The human may pay {3} to put their revealed companion into their hand.
    CompanionAction,
    /// A card of the human's sideboard is in their hand.
    BroughtIn,
}

/// A keystone's game: the human's deck, the face they prefer to cast each card with, and the
/// moments the game must reach.
struct Keystone {
    card: &'static str,
    deck: &'static [(u8, &'static str)],
    /// The human's sideboard: (count, card name).
    sideboard: &'static [(u8, &'static str)],
    face: Option<u8>,
    /// How many mulligans the human takes before keeping a hand.
    mulligans: u8,
    expect: &'static [Moment],
    /// Rules text the session must send for an ability or special action the card's printed
    /// text has no paragraph for, on a candidate or a stack item the human sees.
    words: &'static [&'static str],
}

const DOWN: u8 = FaceIx::DOWN.get();

/// A 2/10 creature card for {2}{W}{W}.
const INDOMITABLE_ANCIENTS: &str = "Indomitable Ancients";

/// A sorcery for {2}{R}{R}.
const RELENTLESS_ASSAULT: &str = "Relentless Assault";

const KEYSTONES: &[Keystone] = &[
    Keystone {
        card: names::FIRE_ICE,
        deck: &[
            (10, names::MOUNTAIN),
            (10, names::ISLAND),
            (20, names::FIRE_ICE),
        ],
        sideboard: &[],
        face: Some(FaceIx::BACK.get()),
        mulligans: 0,
        expect: &[Moment::Resolved],
        words: &[],
    },
    Keystone {
        card: names::WEAR_TEAR,
        deck: &[
            (10, names::MOUNTAIN),
            (10, names::PLAINS),
            (10, names::WEAR_TEAR),
            (5, names::LEONIN_SCIMITAR),
            (5, names::HOLY_STRENGTH),
        ],
        sideboard: &[],
        face: Some(FaceIx::FUSED.get()),
        mulligans: 0,
        expect: &[Moment::Resolved],
        words: &[],
    },
    Keystone {
        card: names::BONECRUSHER_GIANT,
        deck: &[(20, names::MOUNTAIN), (20, names::BONECRUSHER_GIANT)],
        sideboard: &[],
        face: Some(FaceIx::BACK.get()),
        mulligans: 0,
        expect: &[Moment::Resolved],
        words: &[],
    },
    Keystone {
        card: names::ADVENTUROUS_EATER_HAVE_A_BITE,
        deck: &[
            (20, names::SWAMP),
            (20, names::ADVENTUROUS_EATER_HAVE_A_BITE),
        ],
        sideboard: &[],
        face: None,
        mulligans: 0,
        expect: &[Moment::Resolved, Moment::Prepared],
        words: &[],
    },
    Keystone {
        card: names::VALAKUT_AWAKENING,
        deck: &[(20, names::MOUNTAIN), (20, names::VALAKUT_AWAKENING)],
        sideboard: &[],
        face: Some(FaceIx::FRONT.get()),
        mulligans: 0,
        expect: &[Moment::Resolved],
        words: &[],
    },
    Keystone {
        card: names::ARCANE_PROXY,
        deck: &[(20, names::ISLAND), (20, names::ARCANE_PROXY)],
        sideboard: &[],
        face: Some(FaceIx::PROTOTYPE.get()),
        mulligans: 0,
        expect: &[Moment::Resolved],
        words: &[],
    },
    Keystone {
        card: names::DELVER_OF_SECRETS,
        deck: &[
            (10, names::ISLAND),
            (10, names::MOUNTAIN),
            (4, names::DELVER_OF_SECRETS),
            (16, names::FIRE_ICE),
        ],
        sideboard: &[],
        face: None,
        mulligans: 0,
        expect: &[Moment::Resolved, Moment::BackFaceUp],
        words: &[],
    },
    Keystone {
        card: names::ERAYO_SORATAMI_ASCENDANT,
        deck: &[(20, names::ISLAND), (20, names::ERAYO_SORATAMI_ASCENDANT)],
        sideboard: &[],
        face: None,
        mulligans: 0,
        expect: &[Moment::Resolved],
        words: &[],
    },
    Keystone {
        card: names::GISELA_THE_BROKEN_BLADE,
        deck: &[
            (24, names::PLAINS),
            (8, names::GISELA_THE_BROKEN_BLADE),
            (8, names::BRUNA_THE_FADING_LIGHT),
        ],
        sideboard: &[],
        face: None,
        mulligans: 0,
        expect: &[Moment::Resolved, Moment::Melded],
        words: &[],
    },
    Keystone {
        card: names::THE_ELDEST_REBORN,
        deck: &[(22, names::SWAMP), (18, names::THE_ELDEST_REBORN)],
        sideboard: &[],
        face: None,
        mulligans: 0,
        expect: &[Moment::Resolved, Moment::SecondChapter],
        words: &["I — Each opponent sacrifices a creature or planeswalker of their choice."],
    },
    Keystone {
        card: names::WIZARD_CLASS,
        deck: &[(20, names::ISLAND), (20, names::WIZARD_CLASS)],
        sideboard: &[],
        face: None,
        mulligans: 0,
        expect: &[Moment::Resolved, Moment::Activated, Moment::Leveled],
        words: &[],
    },
    Keystone {
        card: names::CASE_OF_THE_CRIMSON_PULSE,
        // A small hand and Lightning Bolts at the opponent empty the hand, which solves the
        // Case.
        deck: &[
            (20, names::MOUNTAIN),
            (6, names::CASE_OF_THE_CRIMSON_PULSE),
            (14, names::LIGHTNING_BOLT),
        ],
        sideboard: &[],
        face: None,
        mulligans: 3,
        expect: &[Moment::Resolved, Moment::Solved],
        words: &[],
    },
    Keystone {
        card: names::STUDENT_OF_WARFARE,
        deck: &[(20, names::PLAINS), (20, names::STUDENT_OF_WARFARE)],
        sideboard: &[],
        face: None,
        mulligans: 0,
        expect: &[Moment::Resolved, Moment::Activated],
        words: &["Level up {W}"],
    },
    Keystone {
        card: names::EXALTED_ANGEL,
        deck: &[(20, names::PLAINS), (20, names::EXALTED_ANGEL)],
        sideboard: &[],
        face: Some(DOWN),
        mulligans: 0,
        expect: &[
            Moment::FaceDownSpell,
            Moment::Resolved,
            Moment::FaceDownPermanent,
        ],
        words: &["Morph {2}{W}{W}"],
    },
    Keystone {
        card: names::GATHAN_RAIDERS,
        deck: &[(20, names::MOUNTAIN), (20, names::GATHAN_RAIDERS)],
        sideboard: &[],
        face: Some(DOWN),
        mulligans: 0,
        expect: &[
            Moment::FaceDownSpell,
            Moment::Resolved,
            Moment::FaceDownPermanent,
        ],
        words: &["Morph—Discard a card."],
    },
    Keystone {
        card: names::ILLUSIONARY_MASK,
        // A creature with no morph ability, so a face-down one was cast through the Mask.
        deck: &[
            (24, names::PLAINS),
            (6, names::ILLUSIONARY_MASK),
            (10, INDOMITABLE_ANCIENTS),
        ],
        sideboard: &[],
        face: Some(DOWN),
        mulligans: 0,
        expect: &[
            Moment::Resolved,
            Moment::Activated,
            Moment::FaceDownPermanent,
        ],
        words: &[],
    },
    Keystone {
        card: names::BURNING_WISH,
        deck: &[(20, names::MOUNTAIN), (20, names::BURNING_WISH)],
        // An instant the wish may not bring in, and a sorcery it may.
        sideboard: &[(1, names::LIGHTNING_BOLT), (1, RELENTLESS_ASSAULT)],
        face: None,
        mulligans: 0,
        expect: &[Moment::WishPick, Moment::BroughtIn],
        words: &[],
    },
    Keystone {
        card: names::LURRUS_OF_THE_DREAM_DEN,
        // Every permanent card has mana value 2 or less, so Lurrus may be revealed.
        deck: &[(20, names::PLAINS), (20, names::STUDENT_OF_WARFARE)],
        sideboard: &[(1, names::LURRUS_OF_THE_DREAM_DEN)],
        face: None,
        mulligans: 0,
        expect: &[
            Moment::CompanionReveal,
            Moment::CompanionAction,
            Moment::BroughtIn,
        ],
        words: &[],
    },
];

/// What the human prefers among a decision's candidates, the highest first: keep a hand, play
/// a land, put the revealed companion into their hand, cast with the keystone's face, activate an ability or turn a permanent face up
/// once a turn, cast otherwise, then the largest number (Illusionary Mask's X). A mulligan
/// comes first while the human has taken fewer than `mulligans`, and the opponent is the
/// target of choice. From a sideboard catalog (a wish's pick, the companion question) the
/// human takes a card over none.
fn rank(
    candidate: &Candidate,
    face: Option<u8>,
    mulligan: bool,
    activated: bool,
    catalog: bool,
) -> i32 {
    match *candidate {
        Candidate::Number { .. } if catalog => 30,
        Candidate::Mulligan if mulligan => 70,
        Candidate::Keep => 60,
        Candidate::PlayLand { .. } => 50,
        Candidate::Companion => 48,
        Candidate::Cast { face: f, .. } if Some(f) == face => 45,
        // A face-down cast is only ever the keystone's choice: the others would not show the
        // card's face-up states.
        Candidate::Cast { face: DOWN, .. } => -1,
        Candidate::Cast { .. } => 40,
        Candidate::Activate { .. } | Candidate::TurnFaceUp { .. } if !activated => 42,
        Candidate::Number { n } => i32::from(n),
        Candidate::Player {
            player: Side::Opponent,
        } => 2,
        Candidate::Object { .. } | Candidate::Player { .. } | Candidate::Yes => 1,
        _ => 0,
    }
}

/// The candidate the human takes: the first of those [`rank`] puts highest.
fn preferred(
    options: &[&Candidate],
    face: Option<u8>,
    mulligan: bool,
    activated: bool,
    catalog: bool,
) -> usize {
    (0..options.len())
        .max_by_key(|&i| {
            (
                rank(options[i], face, mulligan, activated, catalog),
                std::cmp::Reverse(i),
            )
        })
        .expect("a decision has a candidate")
}

/// The shape states the human's view shows, with `sideboard` the names of the human's
/// sideboard cards.
fn moments(view: &PlayerView, sideboard: &[&str]) -> Vec<Moment> {
    let mut moments = Vec::new();
    // The name of a back face that is not also its card's front, as a transformed or flipped
    // permanent shows: not a split card's or an Adventurer's.
    let back = |name: &str| {
        CARDS.cards().iter().any(|def| {
            matches!(def.shape, CardShape::Transforming | CardShape::Flip)
                && def.face_name(FaceIx::BACK) == name
        })
    };
    let battlefield = || {
        view.objects
            .iter()
            .filter(|o| o.zone == ZoneName::Battlefield && o.controller == Side::You)
    };
    if battlefield().any(|o| o.name.as_deref().is_some_and(back)) {
        moments.push(Moment::BackFaceUp);
    }
    if view.objects.iter().any(|o| o.part_of.is_some()) {
        moments.push(Moment::Melded);
    }
    let on_stack_down = view.stack.iter().any(|item| match item {
        StackItem::Spell { slot, .. } => view
            .objects
            .iter()
            .any(|o| o.slot == *slot && o.facing == Facing::Down),
        StackItem::Ability { .. } => false,
    });
    if on_stack_down {
        moments.push(Moment::FaceDownSpell);
    }
    if battlefield().any(|o| o.facing == Facing::Down) {
        moments.push(Moment::FaceDownPermanent);
    }
    if battlefield().any(|o| {
        o.counters
            .iter()
            .any(|c| c.count >= 2 && serde_json::to_string(&c.kind).is_ok_and(|k| k == "\"lore\""))
    }) {
        moments.push(Moment::SecondChapter);
    }
    let designated = |wanted: fn(&DesignationView) -> bool| {
        battlefield().any(|o| o.designations.iter().any(wanted))
    };
    if designated(|d| matches!(d, DesignationView::Level { level } if *level >= 2)) {
        moments.push(Moment::Leveled);
    }
    if designated(|d| *d == DesignationView::Solved) {
        moments.push(Moment::Solved);
    }
    if designated(|d| *d == DesignationView::Prepared) {
        moments.push(Moment::Prepared);
    }
    let in_hand = |o: &&ObjectView| o.zone == ZoneName::Hand && o.owner == Side::You;
    if (view.objects.iter().filter(in_hand)).any(|o| {
        o.name
            .as_deref()
            .is_some_and(|name| sideboard.contains(&name))
    }) {
        moments.push(Moment::BroughtIn);
    }
    if let Status::Deciding { decision } = &view.status {
        moments.extend(outside_the_game(decision, sideboard));
    }
    moments
}

/// The moments of a decision about a card outside the game, each checked to name the
/// sideboard card it is about: the wish's pick and the companion question list the cards by
/// name, and the {3} action names the revealed companion.
fn outside_the_game(decision: &DecisionView, sideboard: &[&str]) -> Option<Moment> {
    let labels = || decision.candidates.iter().map(|c| c.label.as_str());
    let moment = match decision.prompt.purpose {
        Purpose::Wish => Moment::WishPick,
        Purpose::Companion => Moment::CompanionReveal,
        _ => {
            let action = (decision.candidates.iter()).find(|c| c.option == Candidate::Companion)?;
            assert!(
                sideboard
                    .iter()
                    .any(|name| action.label == format!("Put {name} into hand")),
                "the companion action names no sideboard card: {:?}",
                action.label
            );
            return Some(Moment::CompanionAction);
        }
    };
    assert!(
        labels().any(|label| sideboard.contains(&label)),
        "{moment:?} names no sideboard card: {:?}",
        labels().collect::<Vec<_>>()
    );
    Some(moment)
}

/// Where a game stands, as far as what a save must not depend on lets two replays agree.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Position {
    turn: u16,
    life: [i16; 2],
    library: [usize; 2],
    graveyard: [usize; 2],
    battlefield: usize,
    stack: usize,
    faces: Vec<u8>,
}

fn position_of<L: Layout>(s: &State<L>) -> Position {
    let mut faces: Vec<u8> = s
        .battlefield()
        .iter()
        .map(|&slot| s.object(slot).face().get())
        .collect();
    faces.sort_unstable();
    Position {
        turn: s.turn().number(),
        life: Seat::BOTH.map(|p| beryl_engine::SignedNumber::to_i16_saturating(s.player(p).life())),
        library: Seat::BOTH.map(|p| s.library(p).len()),
        graveyard: Seat::BOTH.map(|p| s.graveyard(p).len()),
        battlefield: s.battlefield().len(),
        stack: s.stack().len(),
        faces,
    }
}

fn position(game: &Game) -> Position {
    match game.large() {
        Some(s) => position_of(s),
        None => position_of(game.state()),
    }
}

/// Cuts `game`'s save off at the decision `view` shows and replays it: to `view` itself, to
/// the same position under each perturbation, and on through one more decision.
fn cut(game: &Game, view: &PlayerView, card: &str, moment: Moment) {
    let file: ReplayFile =
        serde_json::from_str(&serde_json::to_string(game.replay()).unwrap()).unwrap();
    let (mut replayed, again) = Game::replay_exactly(&CARDS, &file, SESSION)
        .unwrap_or_else(|e| panic!("{card}, {moment:?}: the save does not replay: {e}"));
    assert_eq!(&again, view, "{card}, {moment:?}: the replayed view");
    let at = position(game);
    assert_eq!(position(&replayed), at, "{card}, {moment:?}");
    for (name, _, perturbation) in Perturbation::EACH {
        let (perturbed, _) = perturb::with(perturbation, || {
            Game::replay_exactly(&CARDS, &file, SESSION)
        })
        .unwrap_or_else(|e| panic!("{card}, {moment:?}, {name}: {e}"));
        assert_eq!(position(&perturbed), at, "{card}, {moment:?}, {name}");
    }
    let open = replayed.open().expect("the replayed game is at a decision");
    let answer = open.answer(open.id(), CandidateIndex(0)).unwrap();
    replayed.choose(answer);
    replayed.opponent_view();
    assert!(
        replayed.next_view().is_some(),
        "{card}, {moment:?}: the replayed game plays on"
    );
}

fn owned(list: &[(u8, &str)]) -> Vec<(u8, String)> {
    list.iter().map(|&(n, name)| (n, name.to_owned())).collect()
}

/// Plays `keystone`'s game until it has reached and cut each moment it expects.
fn plays(keystone: &Keystone) {
    let card = keystone.card;
    let lands = keystone.deck[0].1;
    let start = StartGame {
        format: beryl_session::Format::CONSTRUCTED,
        seed: 7,
        decks: [owned(keystone.deck), vec![(40, lands.to_owned())]],
        human: Seat::P0,
        opponent: "random".to_owned(),
        stops: StopsSetting::EXACT,
        sideboards: [owned(keystone.sideboard), Vec::new()],
    };
    let sideboard: Vec<&str> = keystone.sideboard.iter().map(|&(_, name)| name).collect();
    let mut game = Game::new(&CARDS, &start, SESSION).unwrap();
    let mut seen = BTreeSet::new();
    // What the stack emptying next means: a spell the human cast or an ability they activated
    // resolved.
    let mut using: Option<Moment> = None;
    // The turn the human last activated an ability or turned a permanent face up in: an
    // ability with {X} could otherwise be activated for X = 0 again and again.
    let mut activated_in: Option<u16> = None;
    // The rules text the session sent for abilities and special actions with no printed
    // paragraph.
    let mut sent: BTreeSet<String> = BTreeSet::new();
    for _ in 0..4000 {
        if keystone.expect.iter().all(|m| seen.contains(m))
            && keystone.words.iter().all(|w| sent.contains(*w))
        {
            return;
        }
        let Some(view) = game.next_view() else {
            break;
        };
        game.opponent_view();
        if matches!(view.status, Status::Over { .. }) {
            break;
        }
        let Status::Deciding { decision } = &view.status else {
            continue;
        };
        sent.extend(
            decision
                .candidates
                .iter()
                .filter_map(|c| c.text.clone())
                .chain(view.stack.iter().filter_map(|item| match item {
                    StackItem::Ability { text, .. } => text.clone(),
                    StackItem::Spell { .. } => None,
                })),
        );
        let mut now = moments(&view, &sideboard);
        if view.stack.is_empty()
            && let Some(used) = using.take()
        {
            now.push(used);
        }
        for moment in now {
            if keystone.expect.contains(&moment) && seen.insert(moment) {
                cut(&game, &view, card, moment);
            }
        }
        let Some(open) = game.open() else {
            continue;
        };
        let options: Vec<&Candidate> = decision.candidates.iter().map(|c| &c.option).collect();
        let mulligan = view.players.you.mulligans < keystone.mulligans;
        let catalog = matches!(decision.prompt.purpose, Purpose::Wish | Purpose::Companion);
        let index = preferred(
            &options,
            keystone.face,
            mulligan,
            activated_in == Some(view.turn.number),
            catalog,
        );
        match options[index] {
            Candidate::Cast { .. } => using = Some(Moment::Resolved),
            Candidate::Activate { .. } | Candidate::TurnFaceUp { .. } => {
                using = Some(Moment::Activated);
                activated_in = Some(view.turn.number);
            }
            _ => {}
        }
        let answer = open
            .answer(open.id(), CandidateIndex(u16::try_from(index).unwrap()))
            .unwrap();
        game.choose(answer);
    }
    let missing: Vec<_> = keystone
        .expect
        .iter()
        .filter(|m| !seen.contains(m))
        .collect();
    assert!(missing.is_empty(), "{card}: never reached {missing:?}");
    let unsent: Vec<_> = keystone
        .words
        .iter()
        .filter(|w| !sent.contains(**w))
        .collect();
    assert!(
        unsent.is_empty(),
        "{card}: never sent {unsent:?}; sent {sent:?}"
    );
}

#[test]
fn saves_cut_in_each_new_card_shape_state_replay_under_each_perturbation() {
    for keystone in KEYSTONES {
        plays(keystone);
    }
}

/// A save names the designations of a permanent, so that answers naming two copies of a card
/// alike but for them differ: here the opponent targets one of two Wizard Classes with Tear,
/// one at level 2 and one at level 1, and the two targets are not the same answer.
#[test]
fn a_save_tells_two_classes_apart_by_level() {
    use beryl_engine::{DecisionKind, Opt};
    use beryl_scenario::prelude::{ME, Resolve, THEM};
    use beryl_scenario::{P0, P1, Scenario};
    use beryl_session::save::Act;

    const SCENARIO: Scenario = Scenario::new("Tear targets one of two Wizard Classes")
        .battlefield(
            P0,
            &[
                names::WIZARD_CLASS,
                names::WIZARD_CLASS,
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
            ],
        )
        .library(P0, &[names::ISLAND, names::ISLAND, names::ISLAND])
        .hand(P1, &[names::WEAR_TEAR])
        .battlefield(P1, &[names::PLAINS])
        .actions(&[
            ME.activates(names::WIZARD_CLASS, 1),
            Resolve,
            Resolve,
            ME.passes(),
            THEM.casts_face(names::WEAR_TEAR, FaceIx::BACK),
            THEM.targets(names::WIZARD_CLASS),
        ]);
    let mut compared = false;
    beryl_scenario::visit(&CARDS, &SCENARIO, |t, s, d| {
        if d.kind != DecisionKind::Target || d.player != Seat::P1 {
            return;
        }
        let classes: Vec<Act> = (0..u16::try_from(d.len()).unwrap())
            .map(beryl_engine::Choice)
            .filter(|&c| {
                let slot = match d.candidates.option(c) {
                    Some(Opt::Object(slot)) => Some(slot),
                    _ => d.candidates.object(c),
                };
                slot.is_some_and(|slot| s.definition_name(t, slot) == names::WIZARD_CLASS)
            })
            .map(|c| Act::of(t, s, d, c))
            .collect();
        assert_eq!(classes.len(), 2);
        assert_ne!(classes[0], classes[1]);
        compared = true;
    })
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(compared, "the opponent targeted a Class");
}
