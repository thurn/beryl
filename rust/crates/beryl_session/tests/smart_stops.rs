//! What the session answers for the human under the smart stops: their priority while only
//! their own spells and abilities came and went on the stack, the order of their
//! simultaneous triggers, whether to search a library they know nothing of, and a waterbend
//! cost mana can pay. Exact stops ask both, and replays find every such answer again.

use beryl_cards::CARDS;
use beryl_cards::names::{
    AANGS_ICEBERG, GET_LOST, ISLAND, JACE_THE_MIND_SCULPTOR, JACE_VRYNS_PRODIGY, MANA_CRYPT,
    MANA_DRAIN, PLAINS, STOCK_UP, THUNDERTRAP_TRAINER, TRINKET_MAGE,
};
use beryl_engine::card::Purpose;
use beryl_engine::state::{CounterKind, StackEntry};
use beryl_scenario::{P0, P1, Permanent, Scenario};
use beryl_search::Budget;
use beryl_session::protocol::{Candidate, Kind, Side, StackItem, ZoneName};
use beryl_session::replay::Entry;
use beryl_session::{
    CandidateIndex, Game, PlayerView, Prepared, Seat, SessionId, StartGame, Status, StopsSetting,
};

const SESSION: SessionId = SessionId(1);
const HUMAN: Seat = Seat::P0;
/// Long enough for many casts, short enough to stay quick.
const TURNS: u16 = 20;

fn deck(cards: &[(u8, &str)]) -> Vec<(u8, String)> {
    cards
        .iter()
        .map(|&(n, name)| (n, name.to_owned()))
        .collect()
}

/// Trainers to cast, with offspring once there is mana for it, and Mana Drains to hold.
fn trainers() -> Vec<(u8, String)> {
    deck(&[(24, ISLAND), (10, THUNDERTRAP_TRAINER), (6, MANA_DRAIN)])
}

/// Removal to answer the human's spells with, at instant speed.
fn get_lost() -> Vec<(u8, String)> {
    deck(&[(17, PLAINS), (23, GET_LOST)])
}

/// A deck that never casts anything: no lands, and nothing free.
fn no_plays() -> Vec<(u8, String)> {
    deck(&[(40, "Thieving Skydiver")])
}

fn new_game(seed: u64, opponent: Vec<(u8, String)>, stops: StopsSetting) -> Game {
    let start = StartGame {
        seed,
        decks: [trainers(), opponent],
        human: HUMAN,
        opponent: "random".to_owned(),
        stops,
    };
    Game::new(&CARDS, &start, SESSION).unwrap()
}

/// Whether a decision is asked by a Thundertrap Trainer's look, as it resolves.
fn looks(view: &PlayerView) -> bool {
    matches!(&view.status, Status::Deciding { decision }
        if matches!(&decision.resolving,
            Some(StackItem::Ability { name, .. }) if name == THUNDERTRAP_TRAINER))
}

fn kind(view: &PlayerView) -> Kind {
    let Status::Deciding { decision } = &view.status else {
        unreachable!("a decision's view")
    };
    decision.kind
}

/// The scripted human: plays a land, else casts a Trainer, with offspring if they can;
/// otherwise passes or takes the first candidate. They keep their Mana Drains, which give them
/// something to do whenever a spell is on the stack. Returns what it chose.
fn answer(game: &mut Game, view: &PlayerView) -> Candidate {
    let Status::Deciding { decision } = &view.status else {
        unreachable!("the human answers only their decisions");
    };
    let candidates = &decision.candidates;
    let find = |want: &dyn Fn(&Candidate) -> bool| candidates.iter().position(|c| want(&c.option));
    let index = match decision.kind {
        Kind::Priority if view.stack.is_empty() => {
            find(&|c| matches!(c, Candidate::PlayLand { .. })).or_else(|| {
                candidates
                    .iter()
                    .enumerate()
                    .filter_map(|(i, c)| match &c.option {
                        Candidate::Cast { costs, .. } => Some((costs.len(), i)),
                        _ => None,
                    })
                    .max()
                    .map(|(_, i)| i)
            })
        }
        _ => None,
    }
    .unwrap_or(0);
    let checked = game.answer(decision.id, CandidateIndex(index as u16));
    game.choose(checked.unwrap());
    candidates[index].option.clone()
}

/// One of the human's decisions: the view that asked it, and the stack as it stood then.
struct Asked {
    view: PlayerView,
    stack: Vec<StackEntry>,
    /// The stack once the human's previous answer was applied, if it was in this step.
    after_previous: Option<Vec<StackEntry>>,
    chosen: Candidate,
}

/// Plays the game to its end or into turn [`TURNS`]: every view, and every decision the
/// engine asked the human (the halts aside).
fn play(game: &mut Game) -> (Vec<PlayerView>, Vec<Asked>) {
    let mut views = Vec::new();
    let mut asked = Vec::new();
    let mut previous: Option<((u16, u8), Vec<StackEntry>)> = None;
    while let Some(view) = game.next_view() {
        views.push(view.clone());
        // A replay resumes where a view awaits someone, never as a step begins.
        if view.turn.number > TURNS && view.status != Status::Playing {
            break;
        }
        let Status::Deciding { decision } = &view.status else {
            continue;
        };
        if decision.halt {
            answer(game, &view);
            previous = None;
            continue;
        }
        let step = (view.turn.number, view.turn.step as u8);
        let stack = game.state().stack().to_vec();
        let after_previous = previous
            .take()
            .filter(|(at, _)| *at == step)
            .map(|(_, stack)| stack);
        let chosen = answer(game, &view);
        previous = Some((step, game.state().stack().to_vec()));
        asked.push(Asked {
            view,
            stack,
            after_previous,
            chosen,
        });
    }
    (views, asked)
}

fn priority_on_a_stack(a: &Asked) -> bool {
    kind(&a.view) == Kind::Priority && !a.stack.is_empty()
}

/// Whether the human's own spell or ability is on top, and nothing of the opponent's came
/// onto the stack since their previous answer in this step: an opponent's response that has
/// since resolved gives them nothing new to answer.
fn only_own_doings(a: &Asked) -> bool {
    let Some(before) = &a.after_previous else {
        return false;
    };
    let own = |e: &StackEntry| e.controller == HUMAN;
    let kept = before
        .iter()
        .zip(&a.stack)
        .take_while(|(x, y)| x == y)
        .count();
    a.stack.last().is_some_and(own) && a.stack[kept..].iter().all(own)
}

/// The replay reproduces the game: cut at the end, it shows the last view.
fn assert_replays(game: &Game, views: &[PlayerView]) {
    let (_, again) = Game::from_replay(&CARDS, game.replay(), SESSION).unwrap();
    assert_eq!(&again, views.last().unwrap());
}

#[test]
fn a_humans_own_spell_resolves_without_their_priority_under_smart_stops_only() {
    let mut resolved = 0;
    let mut own_priorities = [0, 0];
    for seed in 0..3 {
        for (i, stops) in [StopsSetting::SMART, StopsSetting::EXACT]
            .into_iter()
            .enumerate()
        {
            let mut game = new_game(seed, no_plays(), stops);
            let (views, asked) = play(&mut game);
            // The opponent never casts anything: every stack is the human's own.
            own_priorities[i] += asked.iter().filter(|a| priority_on_a_stack(a)).count();
            resolved += asked.iter().filter(|a| looks(&a.view)).count();
            assert_replays(&game, &views);
        }
    }
    assert!(resolved > 6, "only {resolved} Trainers resolved");
    assert_eq!(own_priorities[0], 0, "smart stops asked to pass to oneself");
    assert!(
        own_priorities[1] > 0,
        "exact stops never offered a response"
    );
}

#[test]
fn an_opponents_response_gives_the_human_priority_until_it_resolves() {
    let (mut responses, mut resolved_unasked) = (0, 0);
    // A response leaving the stack from above the human's spell is rare (seed 34 has one,
    // once the mulligan put its bottom picks after the keep).
    for seed in 0..64 {
        let mut game = new_game(seed, get_lost(), StopsSetting::SMART);
        let (views, asked) = play(&mut game);
        let theirs = |e: &StackEntry| e.controller != HUMAN;
        for a in asked.iter().filter(|a| priority_on_a_stack(a)) {
            assert!(
                !only_own_doings(a),
                "seed {seed}: asked to pass to their own spell at {:?}",
                a.view.turn
            );
            if a.stack.last().is_some_and(theirs) {
                responses += 1;
            }
        }
        // Passing to the opponent's response leaves the human's spell beneath it on top once
        // it resolves: that spell resolves too, without asking them again.
        for pair in asked.windows(2) {
            let [then, next] = pair else { unreachable!() };
            let [below @ .., mine, response] = then.stack.as_slice() else {
                continue;
            };
            if kind(&then.view) != Kind::Priority
                || !theirs(response)
                || theirs(mine)
                || !matches!(then.chosen, Candidate::Pass)
            {
                continue;
            }
            let mut left = below.to_vec();
            left.push(*mine);
            assert!(
                kind(&next.view) != Kind::Priority || next.stack != left,
                "seed {seed}: asked again at their own spell once the response resolved"
            );
            resolved_unasked += usize::from(!next.stack.contains(mine));
        }
        assert_replays(&game, &views);
    }
    assert!(responses > 0, "the opponent never responded");
    assert!(
        resolved_unasked > 0,
        "no spell resolved unasked once a response above it left"
    );
}

#[test]
fn smart_stops_order_the_humans_triggers_and_exact_stops_ask() {
    let mut orders_asked = [0, 0];
    let (mut offspring, mut autos) = (0, 0);
    for seed in 0..4 {
        for (i, stops) in [StopsSetting::SMART, StopsSetting::EXACT]
            .into_iter()
            .enumerate()
        {
            let mut game = new_game(seed, no_plays(), stops);
            let (views, asked) = play(&mut game);
            orders_asked[i] += asked
                .iter()
                .filter(|a| kind(&a.view) == Kind::TriggerOrder)
                .count();
            if stops == StopsSetting::SMART {
                offspring += offspring_first(&asked);
                autos += game
                    .replay()
                    .log
                    .iter()
                    .filter(|e| matches!(e, Entry::Auto(_)))
                    .count();
            }
            assert_replays(&game, &views);
        }
    }
    assert_eq!(orders_asked[0], 0, "smart stops asked a trigger order");
    assert!(
        orders_asked[1] > 0,
        "exact stops never asked a trigger order"
    );
    assert!(offspring > 0, "no Trainer was cast with offspring");
    assert!(autos > offspring, "the session answered only {autos} times");
}

/// How many Trainers cast with offspring made their token before the first look that
/// followed: the heuristic resolves offspring's plain effect before a trigger that asks.
fn offspring_first(asked: &[Asked]) -> usize {
    let tokens = |v: &PlayerView| {
        v.objects
            .iter()
            .filter(|o| o.token && o.zone == ZoneName::Battlefield && o.controller == Side::You)
            .count()
    };
    let mut checked = 0;
    let mut cast: Option<usize> = None;
    for a in asked {
        if let Some(had) = cast
            && looks(&a.view)
        {
            assert_eq!(
                tokens(&a.view),
                had + 1,
                "a look came before offspring's token"
            );
            checked += 1;
            cast = None;
        }
        if matches!(&a.chosen, Candidate::Cast { costs, .. } if !costs.is_empty()) {
            cast = Some(tokens(&a.view));
        }
    }
    checked
}

/// Aang's Iceberg, with three Plains and a creature, which may tap for one of its waterbend {3}.
const WATERBEND: Scenario = Scenario::new("waterbend")
    .battlefield(
        P0,
        &[
            Permanent::new(AANGS_ICEBERG),
            Permanent::new(JACE_VRYNS_PRODIGY),
            Permanent::new(PLAINS),
            Permanent::new(PLAINS),
            Permanent::new(PLAINS),
        ],
    )
    .library(P0, &[ISLAND, ISLAND, ISLAND])
    .library(P1, &[ISLAND, ISLAND, ISLAND]);

/// The kinds of decision the human is asked after activating [`WATERBEND`]'s Iceberg, up to
/// its scry, each with whether it is the waterbend payment.
fn asked_after_waterbend(stops: StopsSetting) -> Vec<(Kind, bool)> {
    let (table, state) = beryl_scenario::position(&CARDS, &WATERBEND).unwrap();
    let random = beryl_search::agent("random").unwrap();
    let mut game = Game::prepared(
        Prepared {
            opponent: (random.new)(&table, 1, Budget::DEFAULT),
            stand_in: (random.new)(&table, 2, Budget::DEFAULT),
            scripted: false,
            hand_over: None,
            table,
            state,
            human: HUMAN,
            stops,
        },
        SESSION,
    );
    let mut asked: Option<Vec<(Kind, bool)>> = None;
    while let Some(view) = game.next_view() {
        let Status::Deciding { decision } = &view.status else {
            continue;
        };
        let bending = decision.prompt.is(Purpose::Waterbend);
        if let Some(asked) = &mut asked {
            asked.push((decision.kind, bending));
            if decision.kind == Kind::ChooseUpTo {
                break;
            }
        }
        if bending {
            let suggested = decision.waterbend.as_deref().expect("a suggested payment");
            assert!(suggested.benders.is_empty());
            assert_eq!(suggested.taps.len(), 3);
            let jace = view
                .objects
                .iter()
                .find(|o| o.name.as_deref() == Some(JACE_VRYNS_PRODIGY))
                .unwrap()
                .slot;
            let request = beryl_session::ipc::WaterbendRequest {
                decision: decision.id,
                selected: vec![jace],
            };
            let mut outdated = request.clone();
            outdated.decision.seq += 1;
            assert!(game.preview_waterbend(&outdated).is_err());
            let changed = game.preview_waterbend(&request).unwrap().unwrap();
            assert_eq!(changed.benders, vec![jace]);
            assert_eq!(changed.taps.len(), 3);
            assert!(changed.taps.contains(&jace));
            assert_eq!(changed.taps.iter().filter(|s| **s != jace).count(), 2);
            let none = beryl_session::ipc::WaterbendRequest {
                selected: Vec::new(),
                ..request.clone()
            };
            let empty = game.preview_waterbend(&none).unwrap().unwrap();
            assert_eq!(&empty, suggested);
            let invalid = beryl_session::ipc::WaterbendRequest {
                selected: vec![u16::MAX],
                ..request
            };
            assert!(game.preview_waterbend(&invalid).unwrap().is_none());
            assert!(game.answer(decision.id, CandidateIndex(0)).is_ok());
        }
        let iceberg = view
            .objects
            .iter()
            .find(|o| o.name.as_deref() == Some(AANGS_ICEBERG))
            .map(|o| o.slot);
        let activate = decision.candidates.iter().position(
            |c| matches!(c.option, Candidate::Activate { slot, .. } if Some(slot) == iceberg),
        );
        let index = match (asked.is_none(), activate) {
            (true, Some(i)) => {
                asked = Some(Vec::new());
                i
            }
            _ => 0,
        };
        let checked = game.answer(decision.id, CandidateIndex(index as u16));
        game.choose(checked.unwrap());
    }
    asked.expect("the waterbend was activated")
}

/// Even with enough mana, the player reviews the proposal and may substitute a creature
/// without tapping anything until they confirm (CR 701.67a).
#[test]
fn smart_stops_offer_waterbend_even_when_mana_can_pay() {
    assert_eq!(
        asked_after_waterbend(StopsSetting::SMART).first(),
        Some(&(Kind::ManaPayment, true))
    );
    assert_eq!(
        asked_after_waterbend(StopsSetting::EXACT).first(),
        Some(&(Kind::ManaPayment, true))
    );
}

/// Trinket Mage with the mana to cast it, and an artifact in a library the human knows nothing
/// of.
const TRINKET: Scenario = Scenario::new("trinket")
    .battlefield(P0, &[Permanent::new(ISLAND); 3])
    .hand(P0, &[TRINKET_MAGE])
    .library(P0, &[ISLAND, MANA_CRYPT, ISLAND])
    .library(P1, &[ISLAND, ISLAND, ISLAND]);

/// [`TRINKET`] after Stock Up, which puts three cards the human has seen on the bottom of
/// their library.
const STOCKED: Scenario = Scenario::new("stocked")
    .battlefield(P0, &[Permanent::new(ISLAND); 6])
    .hand(P0, &[STOCK_UP, TRINKET_MAGE])
    .library(
        P0,
        &[ISLAND, ISLAND, ISLAND, ISLAND, ISLAND, MANA_CRYPT, ISLAND],
    )
    .library(P1, &[ISLAND, ISLAND, ISLAND]);

/// [`TRINKET`] after Jace, the Mind Sculptor's 0, which puts two cards from the human's hand
/// on top of their library: they know the card they draw next.
const KNOWN_TOP: Scenario = Scenario::new("known top")
    .battlefield(
        P0,
        &[
            Permanent::new(ISLAND),
            Permanent::new(ISLAND),
            Permanent::new(ISLAND),
            Permanent::new(JACE_THE_MIND_SCULPTOR).counters(CounterKind::Loyalty, 3),
        ],
    )
    .hand(P0, &[TRINKET_MAGE, ISLAND, ISLAND])
    .library(
        P0,
        &[
            MANA_CRYPT, ISLAND, ISLAND, ISLAND, ISLAND, ISLAND, MANA_CRYPT,
        ],
    )
    .library(P1, &[ISLAND, ISLAND, ISLAND]);

/// The kinds of decision the human is asked after casting `scenario`'s Trinket Mage, up to its
/// search, having first cast anything else they hold and used any Jace, the Mind Sculptor's 0.
fn asked_after_trinket_mage(scenario: &Scenario, stops: StopsSetting) -> Vec<Kind> {
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
            human: HUMAN,
            stops,
        },
        SESSION,
    );
    let mut asked: Option<Vec<Kind>> = None;
    while let Some(view) = game.next_view() {
        let Status::Deciding { decision } = &view.status else {
            continue;
        };
        if let Some(asked) = &mut asked {
            asked.push(decision.kind);
            if decision.kind == Kind::ChooseUpTo {
                break;
            }
        }
        let name = |slot: u16| {
            view.objects
                .iter()
                .find(|o| o.slot == slot)
                .and_then(|o| o.name.clone())
        };
        let casts: Vec<_> = decision
            .candidates
            .iter()
            .enumerate()
            .filter_map(|(i, c)| match c.option {
                Candidate::Cast { slot, .. } => Some((i, name(slot))),
                _ => None,
            })
            .collect();
        let other = casts
            .iter()
            .find(|(_, n)| n.as_deref() != Some(TRINKET_MAGE));
        let mage = casts
            .iter()
            .find(|(_, n)| n.as_deref() == Some(TRINKET_MAGE));
        let jace = decision.candidates.iter().position(|c| {
            matches!(c.option, Candidate::Activate { slot, ability: 1, .. }
                if name(slot).as_deref() == Some(JACE_THE_MIND_SCULPTOR))
        });
        // Otherwise the first: a pass, a card to take, or searching.
        let index = match (asked.is_none(), other, mage) {
            (true, _, _) if jace.is_some() => jace.unwrap(),
            // Jace's put-back keeps the Mage in hand.
            (true, None, None) if decision.kind != Kind::Priority => decision
                .candidates
                .iter()
                .position(|c| !c.label.contains(TRINKET_MAGE))
                .unwrap_or(0),
            (true, Some(&(i, _)), _) => i,
            (true, None, Some(&(i, _))) => {
                asked = Some(Vec::new());
                i
            }
            _ => 0,
        };
        let checked = game.answer(decision.id, CandidateIndex(index as u16));
        game.choose(checked.unwrap());
    }
    asked.expect("the Mage was cast")
}

/// Declining Trinket Mage's search differs from finding nothing only by the shuffle: the
/// smart stops search, whatever the human knows of their library, and the search itself may
/// find nothing. Exact stops ask.
#[test]
fn smart_stops_search_unasked() {
    for scenario in [&TRINKET, &STOCKED, &KNOWN_TOP] {
        let smart = asked_after_trinket_mage(scenario, StopsSetting::SMART);
        assert!(
            !smart.contains(&Kind::YesNo),
            "{}: {smart:?}",
            scenario.name
        );
        assert_eq!(
            smart.last(),
            Some(&Kind::ChooseUpTo),
            "{}: {smart:?}",
            scenario.name
        );
    }
    let exact = asked_after_trinket_mage(&TRINKET, StopsSetting::EXACT);
    assert!(exact.contains(&Kind::YesNo), "{exact:?}");
}

/// Blocks with every legal creature, then passes.
struct BlockAll;

impl<L: beryl_engine::Layout> beryl_search::Agent<L> for BlockAll {
    fn choose(
        &mut self,
        _: beryl_engine::Observation<'_, L>,
        decision: &beryl_engine::Decision<L>,
    ) -> beryl_engine::Choice {
        use beryl_engine::{Choice, Opt};

        (0..decision.len() as u16)
            .map(Choice)
            .find(|&c| matches!(decision.candidates.option(c), Some(Opt::Block(_))))
            .unwrap_or(Choice(0))
    }
}

#[test]
fn smart_damage_takes_the_only_lethal_outcome_and_defaults_real_choices() {
    use beryl_cards::names::{
        ABHORRENT_OCULUS, BEZA_THE_BOUNDING_SPRING, SPARAS_HEADQUARTERS, SUBTLETY,
    };
    use beryl_engine::TurnStep;
    use beryl_session::protocol::Step;

    const BEZA: &[Permanent] = &[Permanent::new(BEZA_THE_BOUNDING_SPRING)];
    const OCULUS: &[Permanent] = &[
        Permanent::new(SPARAS_HEADQUARTERS).face_down(),
        Permanent::new(ABHORRENT_OCULUS),
    ];
    const FLASH: &[Permanent] = &[Permanent::new(TRINKET_MAGE), Permanent::new(SUBTLETY)];
    const BOTH: &[Permanent] = &[
        Permanent::new(TRINKET_MAGE),
        Permanent::new(JACE_VRYNS_PRODIGY),
    ];

    for (stops, opponents, big, ask, kill) in [
        (StopsSetting::SMART, OCULUS, ABHORRENT_OCULUS, false, true),
        (StopsSetting::EXACT, OCULUS, ABHORRENT_OCULUS, true, false),
        (StopsSetting::SMART, FLASH, SUBTLETY, true, true),
        (StopsSetting::SMART, BOTH, JACE_VRYNS_PRODIGY, false, true),
    ] {
        let scenario = Scenario::new("Divide combat damage")
            .at(3, P0, TurnStep::PrecombatMain)
            .battlefield(P0, BEZA)
            .battlefield(P1, opponents)
            .library(P0, &[ISLAND, ISLAND, ISLAND])
            .library(P1, &[ISLAND, ISLAND, ISLAND]);
        let (table, state) = beryl_scenario::position(&CARDS, &scenario).unwrap();
        let mut game = Game::prepared(
            Prepared {
                table,
                state,
                human: P0,
                stops,
                opponent: Box::new(BlockAll),
                stand_in: Box::new(BlockAll),
                hand_over: None,
                scripted: false,
            },
            SESSION,
        );
        let small = (big != ABHORRENT_OCULUS).then_some(TRINKET_MAGE);
        let mut asked = false;
        loop {
            let view = game.next_view().expect("a view before postcombat main");
            if view.turn.step == Step::PostcombatMain {
                let survives = |name| {
                    view.objects.iter().any(|object| {
                        object.zone == ZoneName::Battlefield && object.name.as_deref() == name
                    })
                };
                assert_eq!(survives(small), !kill, "{big}");
                assert_eq!(survives(Some(big)), big != JACE_VRYNS_PRODIGY, "{big}");
                assert_eq!(asked, ask, "{big}");
                break;
            }
            let Status::Deciding { decision } = &view.status else {
                continue;
            };
            let index =
                match decision.kind {
                    Kind::Attacker => decision
                        .candidates
                        .iter()
                        .position(|c| matches!(c.option, Candidate::Attack { .. }))
                        .map_or(CandidateIndex(0), |i| CandidateIndex(i as u16)),
                    Kind::DamageDivision => {
                        asked = true;
                        let preferred = decision.damage_default.expect("a recommended blocker");
                        let Candidate::Lethal { slot } =
                            decision.candidates[preferred.0 as usize].option
                        else {
                            panic!("the recommendation must deal lethal damage");
                        };
                        assert!(view.objects.iter().any(|object| {
                            object.slot == slot && object.name.as_deref() == small
                        }));
                        if stops.full_control {
                            let done = &decision.candidates[0].option;
                            assert_eq!(*done, Candidate::Done);
                            CandidateIndex(0)
                        } else {
                            preferred
                        }
                    }
                    _ => CandidateIndex(0),
                };
            game.choose(game.answer(decision.id, index).unwrap());
        }
    }
}
