//! The session plays each keystone of the casting and combat exceptions family: cascade
//! (Bloodbraid Elf), casting while searching a library (Panglacial Wurm), hand-only cast
//! triggers with casts inside a resolution (Knowledge Pool, Possibility Storm), multi-blocking
//! (Palace Guard), double strike (Fencing Ace), banding (Benalish Hero, Benalish Infantry)
//! and "bands with other" (Master of the Hunt).
//!
//! Each game is played through [`Game`] with both seats' views and scripts drawn on every
//! update, holding every script to the replay and invariant checks (`script/build.rs`). At
//! the first decision of each new state — including mid-resolution casts, search casts, band
//! declarations and exact combat damage divisions — the save is cut off, replayed to the same
//! view, replayed under every perturbation (`beryl_session::perturb`), and resumed for one
//! more decision.

use std::collections::BTreeSet;

use beryl_cards::{CARDS, names};
use beryl_engine::card::Purpose;
use beryl_engine::{Layout, Seat, State};
use beryl_session::perturb::{self, Perturbation};
use beryl_session::protocol::{Candidate, PlayerView, Status};
use beryl_session::{CandidateIndex, Game, ReplayFile, SessionId, StartGame, StopsSetting};

const SESSION: SessionId = SessionId(1);

/// A new state reached by a keystone of the casting and combat family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Moment {
    /// A cast offered during a resolution (`CastOffer`), while its cause is still resolving.
    CastInResolution,
    /// A library search offering a spell to cast (`Candidate::Cast` inside `Purpose::Search`).
    SearchCast,
    /// An attacker declaration offering a band choice.
    BandChoice,
    /// A combat damage division (`Purpose::CombatDamage`).
    DamageDivision,
    /// The keystone spell resolved.
    Resolved,
}

/// A keystone's game: both players' decks and the moments it must reach and cut saves at.
struct Keystone {
    card: &'static str,
    human_deck: &'static [(u8, &'static str)],
    opponent_deck: &'static [(u8, &'static str)],
    expect: &'static [Moment],
}

const KEYSTONES: &[Keystone] = &[
    Keystone {
        card: names::BLOODBRAID_ELF,
        human_deck: &[
            (12, names::MOUNTAIN),
            (12, names::FOREST),
            (8, names::BLOODBRAID_ELF),
            (8, names::LIGHTNING_BOLT),
        ],
        opponent_deck: &[(40, names::PLAINS)],
        expect: &[Moment::CastInResolution, Moment::Resolved],
    },
    Keystone {
        card: names::PANGLACIAL_WURM,
        human_deck: &[
            (28, names::FOREST),
            (6, names::SYLVAN_SCRYING),
            (6, names::PANGLACIAL_WURM),
        ],
        opponent_deck: &[(40, names::PLAINS)],
        expect: &[Moment::SearchCast, Moment::Resolved],
    },
    Keystone {
        card: names::KNOWLEDGE_POOL,
        human_deck: &[
            (24, names::MOUNTAIN),
            (6, names::KNOWLEDGE_POOL),
            (10, names::LIGHTNING_BOLT),
        ],
        opponent_deck: &[(40, names::BENALISH_HERO)],
        expect: &[Moment::CastInResolution, Moment::Resolved],
    },
    Keystone {
        card: names::POSSIBILITY_STORM,
        human_deck: &[
            (24, names::MOUNTAIN),
            (6, names::POSSIBILITY_STORM),
            (10, names::LIGHTNING_BOLT),
        ],
        opponent_deck: &[(40, names::PLAINS)],
        expect: &[Moment::CastInResolution, Moment::Resolved],
    },
    Keystone {
        card: names::BENALISH_HERO,
        human_deck: &[(20, names::PLAINS), (20, names::BENALISH_HERO)],
        opponent_deck: &[(20, names::PLAINS), (20, names::BENALISH_INFANTRY)],
        expect: &[Moment::BandChoice, Moment::DamageDivision],
    },
    Keystone {
        card: names::BENALISH_INFANTRY,
        human_deck: &[(20, names::PLAINS), (20, names::BENALISH_INFANTRY)],
        opponent_deck: &[(20, names::FOREST), (20, names::GRIZZLY_BEARS)],
        expect: &[Moment::BandChoice, Moment::DamageDivision],
    },
    Keystone {
        card: names::PALACE_GUARD,
        human_deck: &[(20, names::PLAINS), (20, names::PALACE_GUARD)],
        opponent_deck: &[(20, names::PLAINS), (20, names::BENALISH_HERO)],
        expect: &[Moment::Resolved],
    },
    Keystone {
        card: names::FENCING_ACE,
        human_deck: &[(20, names::PLAINS), (20, names::FENCING_ACE)],
        opponent_deck: &[(40, names::PLAINS)],
        expect: &[Moment::Resolved],
    },
    Keystone {
        card: names::MASTER_OF_THE_HUNT,
        human_deck: &[(24, names::FOREST), (16, names::MASTER_OF_THE_HUNT)],
        opponent_deck: &[(40, names::PLAINS)],
        expect: &[Moment::Resolved, Moment::BandChoice],
    },
];

/// What the human prefers among a decision's candidates, highest first: keep, play land,
/// take casts offered during resolution or search, cast keystones/spells, activate abilities,
/// start/join bands when offered, block, and pick targets/numbers.
fn rank(
    candidate: &Candidate,
    label: &str,
    purpose: Purpose,
    card: &str,
    lands: usize,
    has_keystone: bool,
) -> i32 {
    match *candidate {
        Candidate::Keep => 80,
        Candidate::PlayLand { .. } => 70,
        Candidate::Cast { .. } if matches!(purpose, Purpose::CastOffer | Purpose::Search) => 65,
        Candidate::Cast { .. }
            if card == names::PANGLACIAL_WURM
                && ((label.contains(names::SYLVAN_SCRYING) && lands < 9)
                    || label.contains(names::PANGLACIAL_WURM)) =>
        {
            -1
        }
        Candidate::Cast { .. }
            if matches!(card, names::KNOWLEDGE_POOL | names::POSSIBILITY_STORM)
                && label.contains(names::LIGHTNING_BOLT)
                && !has_keystone =>
        {
            -1
        }
        Candidate::Activate { .. } if card == names::MASTER_OF_THE_HUNT => 55,
        Candidate::Cast { .. } => 50,
        Candidate::Activate { .. } => 45,
        Candidate::Attack { .. } if label.contains("’s band") => 42,
        Candidate::Attack { .. } if label.contains("band") => 40,
        Candidate::Attack { .. } => 35,
        Candidate::Block { .. } => 30,
        Candidate::Number { n } => 20 + i32::from(n == 0),
        Candidate::Lethal { .. } => 20,
        Candidate::Yes | Candidate::Object { .. } | Candidate::Player { .. } => 10,
        _ => 0,
    }
}

fn preferred(
    decision: &beryl_session::protocol::DecisionView,
    card: &str,
    view: &PlayerView,
) -> usize {
    use beryl_session::protocol::{CardType, Side, ZoneName};

    let purpose = decision.prompt.purpose;
    let lands = view
        .objects
        .iter()
        .filter(|o| {
            o.zone == ZoneName::Battlefield
                && o.controller == Side::You
                && o.types.contains(&CardType::Land)
        })
        .count();
    let has_keystone = view.objects.iter().any(|o| {
        o.zone == ZoneName::Battlefield
            && o.controller == Side::You
            && o.name.as_deref() == Some(card)
    });
    (0..decision.candidates.len())
        .max_by_key(|&i| {
            let c = &decision.candidates[i];
            (
                rank(&c.option, &c.label, purpose, card, lands, has_keystone),
                std::cmp::Reverse(i),
            )
        })
        .expect("a decision has a candidate")
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
}

fn position_of<L: Layout>(s: &State<L>) -> Position {
    Position {
        turn: s.turn().number(),
        life: Seat::BOTH.map(|p| beryl_engine::SignedNumber::to_i16_saturating(s.player(p).life())),
        library: Seat::BOTH.map(|p| s.library(p).len()),
        graveyard: Seat::BOTH.map(|p| s.graveyard(p).len()),
        battlefield: s.battlefield().len(),
        stack: s.stack().len(),
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

fn moments(decision: &beryl_session::protocol::DecisionView) -> Vec<Moment> {
    let mut out = Vec::new();
    let purpose = decision.prompt.purpose;
    if purpose == Purpose::CastOffer
        && decision
            .candidates
            .iter()
            .any(|c| matches!(c.option, Candidate::Cast { .. }))
    {
        out.push(Moment::CastInResolution);
    }
    if purpose == Purpose::Search
        && decision
            .candidates
            .iter()
            .any(|c| matches!(c.option, Candidate::Cast { .. }))
    {
        out.push(Moment::SearchCast);
    }
    if purpose == Purpose::Attack
        && decision
            .candidates
            .iter()
            .any(|c| matches!(c.option, Candidate::Attack { .. }) && c.label.contains("band"))
    {
        out.push(Moment::BandChoice);
    }
    if purpose == Purpose::CombatDamage {
        out.push(Moment::DamageDivision);
    }
    out
}

fn plays(keystone: &Keystone) {
    let card = keystone.card;
    let start = StartGame {
        format: beryl_session::Format::CONSTRUCTED,
        seed: 7,
        decks: [owned(keystone.human_deck), owned(keystone.opponent_deck)],
        human: Seat::P0,
        opponent: "greedy".to_owned(),
        stops: StopsSetting::EXACT,
        sideboards: Default::default(),
    };
    let mut game = Game::new(&CARDS, &start, SESSION).unwrap();
    let mut seen = BTreeSet::new();
    let mut casting = false;
    for _ in 0..5000 {
        if keystone.expect.iter().all(|m| seen.contains(m)) {
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
        let mut now = moments(decision);
        if view.stack.is_empty() && casting {
            casting = false;
            now.push(Moment::Resolved);
        }
        for moment in now {
            if keystone.expect.contains(&moment) && seen.insert(moment) {
                cut(&game, &view, card, moment);
            }
        }
        let Some(open) = game.open() else {
            continue;
        };
        let index = preferred(decision, card, &view);
        if matches!(decision.candidates[index].option, Candidate::Cast { .. }) {
            casting = true;
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
}

#[test]
fn saves_cut_in_casting_and_combat_keystones_replay_under_each_perturbation() {
    for keystone in KEYSTONES {
        plays(keystone);
    }
}

/// Cascade under Trinisphere previews the 3 tapped lands on `CastOffer` and warns before
/// targeting your own creature on `Target` (UI Conventions exception 7).
#[test]
fn cascade_under_trinisphere_previews_payment_and_warns_on_own_target() {
    use beryl_engine::DecisionKind;
    use beryl_scenario::prelude::{ME, Resolve};
    use beryl_scenario::{P0, P1, Scenario};
    use beryl_session::preview::add_previews;
    use beryl_session::prompt::{Click, Clicked, Held};
    use beryl_session::protocol::{DecisionId, Moment, Side, player_view};
    use beryl_session::script::Subject;

    const CASCADE: Scenario = Scenario::new("Cascade into Get Lost under Trinisphere")
        .battlefield(
            P0,
            &[
                names::TRINISPHERE,
                names::MOUNTAIN,
                names::FOREST,
                names::FOREST,
                names::FOREST,
                names::FOREST,
                names::FOREST,
                names::FOREST,
                names::BENALISH_HERO,
            ],
        )
        .battlefield(P1, &[names::GRIZZLY_BEARS])
        .hand(P0, &[names::BLOODBRAID_ELF])
        .library(P0, &[names::GET_LOST])
        .actions(&[
            ME.casts(names::BLOODBRAID_ELF),
            Resolve,
            ME.chooses(DecisionKind::CastOffer, names::GET_LOST),
            ME.targets(names::BENALISH_HERO),
        ]);

    let id = DecisionId {
        session: SESSION,
        seq: 1,
    };
    let mut saw_preview = false;
    let mut saw_warning = false;
    beryl_scenario::visit(&CARDS, &CASCADE, |t, s, d| {
        let mut scratch = beryl_engine::Scratch::new();
        let obs = beryl_engine::observe(s, t, &mut scratch, d.decider);
        let mut view_out = player_view(
            t,
            obs,
            Moment::Deciding(id, d, None),
            StopsSetting::EXACT,
            &[],
        );
        add_previews(&mut view_out, s, t, d);
        if d.kind == DecisionKind::CastOffer {
            let Status::Deciding { decision } = &view_out.status else {
                panic!("expected deciding view");
            };
            let cast_candidate = decision
                .candidates
                .iter()
                .find(|c| matches!(c.option, Candidate::Cast { .. }))
                .expect("a cast candidate");
            assert_eq!(cast_candidate.payments.len(), 1);
            assert_eq!(
                cast_candidate.payments[0].taps.len(),
                3,
                "Trinisphere requires 3 tapped lands for the cascaded spell"
            );
            saw_preview = true;
        }
        if d.kind == DecisionKind::Target {
            let mut held = Held::opening(&view_out, s, t, Some(d)).expect("a held target prompt");
            let own_hero = view_out
                .objects
                .iter()
                .find(|o| {
                    o.name.as_deref() == Some(names::BENALISH_HERO) && o.controller == Side::You
                })
                .expect("own Benalish Hero")
                .slot;
            let reply = held.click(
                s,
                t,
                Some(d),
                Click::On {
                    on: Subject::Object { slot: own_hero },
                },
            );
            assert!(
                matches!(reply, Clicked::Changed),
                "targeting own creature with cascaded removal should ask for confirmation first"
            );
            saw_warning = true;
        }
    })
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(saw_preview, "never reached CastOffer");
    assert!(saw_warning, "never reached Target");
}

/// Panglacial Wurm cast during a library search previews the 7 tapped Forests.
#[test]
fn search_previews_panglacial_wurm_payment() {
    use beryl_engine::DecisionKind;
    use beryl_scenario::prelude::{ME, Resolve};
    use beryl_scenario::{P0, Scenario};
    use beryl_session::preview::add_previews;
    use beryl_session::protocol::{DecisionId, Moment, player_view};

    const WURM: Scenario = Scenario::new("Search previews Panglacial Wurm payment")
        .battlefield(
            P0,
            &[
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
                names::FOREST,
                names::FOREST,
                names::FOREST,
                names::FOREST,
                names::FOREST,
                names::FOREST,
                names::FOREST,
            ],
        )
        .hand(P0, &[names::TRINKET_MAGE])
        .library(P0, &[names::PANGLACIAL_WURM, names::MANA_CRYPT])
        .actions(&[
            ME.casts(names::TRINKET_MAGE),
            Resolve,
            Resolve,
            ME.yes(),
            ME.picks_up_to(names::MANA_CRYPT),
        ]);

    let id = DecisionId {
        session: SESSION,
        seq: 1,
    };
    let mut saw_wurm_preview = false;
    beryl_scenario::visit(&CARDS, &WURM, |t, s, d| {
        if d.kind != DecisionKind::ChooseUpTo {
            return;
        }
        let mut scratch = beryl_engine::Scratch::new();
        let obs = beryl_engine::observe(s, t, &mut scratch, d.decider);
        let mut view_out = player_view(
            t,
            obs,
            Moment::Deciding(id, d, None),
            StopsSetting::EXACT,
            &[],
        );
        add_previews(&mut view_out, s, t, d);
        let Status::Deciding { decision } = &view_out.status else {
            panic!("expected deciding view");
        };
        let cast_candidate = decision
            .candidates
            .iter()
            .find(|c| matches!(c.option, Candidate::Cast { .. }))
            .expect("a Wurm cast candidate");
        assert_eq!(cast_candidate.payments.len(), 1);
        assert_eq!(
            cast_candidate.payments[0].taps.len(),
            7,
            "Panglacial Wurm cast during search should preview 7 tapped Forests"
        );
        saw_wurm_preview = true;
    })
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(saw_wurm_preview, "never reached search decision");
}
