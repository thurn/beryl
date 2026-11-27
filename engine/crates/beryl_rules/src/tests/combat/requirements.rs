//! Seeded differential tests of combat requirements and restrictions against the independent
//! enumerating reference (`crate::reference::verify_all_paths`), plus banding interaction.

use super::Game;
use crate::card::{Ability, CardDef, Face, Keywords, Objects, Players, RuleChange, Types};
use crate::reference::verify_all_paths;
use crate::{
    Candidates, CardTable, Choice, DecisionKind, Decklist, GameTable, Opt, Replay, Step, apply,
    next,
};
use beryl_card::{Governed, When};
use beryl_kernel::format::Format;

macro_rules! rule_card {
    ($ident:ident, $name:literal, $keywords:expr, $abilities:expr) => {
        static $ident: CardDef = CardDef {
            name: $name,
            shape: beryl_card::definition::Shape::Single,
            faces: &[Face {
                types: Types::CREATURE,
                power: Some(2),
                toughness: Some(2),
                keywords: $keywords,
                abilities: $abilities,
                ..Face::EMPTY
            }],
            tokens: &[],
        };
    };
}

static MUST_BLOCK: RuleChange = RuleChange {
    players: Players::You,
    what: Governed::MustBlock(Objects::This),
    when: When::AsUsual,
};
static MUST_BE_BLOCKED: RuleChange = RuleChange {
    players: Players::You,
    what: Governed::MustBeBlocked(Objects::This),
    when: When::AsUsual,
};
static MUST_ATTACK: RuleChange = RuleChange {
    players: Players::You,
    what: Governed::MustAttack(Objects::This),
    when: When::AsUsual,
};
static ALONE_BOTH: RuleChange = RuleChange {
    players: Players::You,
    what: Governed::Alone {
        attack: true,
        block: true,
        creatures: Objects::This,
    },
    when: When::AsUsual,
};
static LIMIT_ONE_ATTACKER: RuleChange = RuleChange {
    players: Players::EachPlayer,
    what: Governed::CombatLimit {
        attackers: Some(1),
        blockers: None,
    },
    when: When::AsUsual,
};
static LIMIT_ONE_BLOCKER: RuleChange = RuleChange {
    players: Players::EachPlayer,
    what: Governed::CombatLimit {
        attackers: None,
        blockers: Some(1),
    },
    when: When::AsUsual,
};
static LIMIT_TWO_BLOCKERS: RuleChange = RuleChange {
    players: Players::EachPlayer,
    what: Governed::CombatLimit {
        attackers: None,
        blockers: Some(2),
    },
    when: When::AsUsual,
};
static BLOCK_ANY: RuleChange = RuleChange {
    players: Players::You,
    what: Governed::BlockAnyNumber(Objects::This),
    when: When::AsUsual,
};
static CANT_BLOCK_RULE: RuleChange = RuleChange {
    players: Players::You,
    what: Governed::Block(Objects::This),
    when: When::Never,
};
static CANT_ATTACK_RULE: RuleChange = RuleChange {
    players: Players::You,
    what: Governed::Attack {
        attackers: Objects::This,
        unless_defender_controls: None,
    },
    when: When::Never,
};

rule_card!(VANILLA, "Vanilla", Keywords::NONE, &[]);
rule_card!(FLIER, "Flier", Keywords::FLYING, &[]);
rule_card!(REACH, "Reach", Keywords::REACH, &[]);
rule_card!(MENACE, "Menace", Keywords::MENACE, &[]);
rule_card!(
    LURED,
    "Lured",
    Keywords::NONE,
    &[Ability::rule_change(&MUST_BLOCK)]
);
rule_card!(
    LURED_FLIER,
    "LuredFlier",
    Keywords::FLYING,
    &[Ability::rule_change(&MUST_BLOCK)]
);
rule_card!(
    LURED_MENACE,
    "LuredMenace",
    Keywords::MENACE,
    &[Ability::rule_change(&MUST_BLOCK)]
);
rule_card!(
    PREY,
    "Prey",
    Keywords::NONE,
    &[Ability::rule_change(&MUST_BE_BLOCKED)]
);
rule_card!(
    PREY_MENACE,
    "PreyMenace",
    Keywords::MENACE,
    &[Ability::rule_change(&MUST_BE_BLOCKED)]
);
rule_card!(
    RECKLESS,
    "Reckless",
    Keywords::NONE,
    &[Ability::rule_change(&MUST_ATTACK)]
);
rule_card!(
    SHY,
    "Shy",
    Keywords::NONE,
    &[Ability::rule_change(&ALONE_BOTH)]
);
rule_card!(
    SHY_RECKLESS,
    "ShyReckless",
    Keywords::NONE,
    &[
        Ability::rule_change(&ALONE_BOTH),
        Ability::rule_change(&MUST_ATTACK),
    ]
);
rule_card!(
    SHY_FLIER,
    "ShyFlier",
    Keywords::FLYING,
    &[Ability::rule_change(&ALONE_BOTH)]
);
rule_card!(
    GUARD,
    "Guard",
    Keywords::NONE,
    &[Ability::rule_change(&BLOCK_ANY)]
);
rule_card!(
    NO_BLOCK,
    "NoBlock",
    Keywords::NONE,
    &[Ability::rule_change(&CANT_BLOCK_RULE)]
);
rule_card!(
    NO_ATTACK,
    "NoAttack",
    Keywords::NONE,
    &[Ability::rule_change(&CANT_ATTACK_RULE)]
);
rule_card!(BANDER, "Bander", Keywords::BANDING, &[]);

static ARBITER_ATK: CardDef = CardDef {
    name: "ArbiterAtk",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::rule_change(&LIMIT_ONE_ATTACKER)],
        ..Face::EMPTY
    }],
    tokens: &[],
};
static ARBITER_BLK1: CardDef = CardDef {
    name: "ArbiterBlk1",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::rule_change(&LIMIT_ONE_BLOCKER)],
        ..Face::EMPTY
    }],
    tokens: &[],
};
static ARBITER_BLK2: CardDef = CardDef {
    name: "ArbiterBlk2",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::rule_change(&LIMIT_TWO_BLOCKERS)],
        ..Face::EMPTY
    }],
    tokens: &[],
};

const POOL: &[&CardDef] = &[
    &VANILLA,
    &FLIER,
    &REACH,
    &MENACE,
    &LURED,
    &LURED_FLIER,
    &LURED_MENACE,
    &PREY,
    &PREY_MENACE,
    &RECKLESS,
    &SHY,
    &SHY_RECKLESS,
    &SHY_FLIER,
    &GUARD,
    &NO_BLOCK,
    &NO_ATTACK,
    &BANDER,
    &ARBITER_ATK,
    &ARBITER_BLK1,
    &ARBITER_BLK2,
];

const CREATURES: &[&str] = &[
    "Vanilla",
    "Flier",
    "Reach",
    "Menace",
    "Lured",
    "LuredFlier",
    "LuredMenace",
    "Prey",
    "PreyMenace",
    "Reckless",
    "Shy",
    "ShyReckless",
    "ShyFlier",
    "Guard",
    "NoBlock",
    "NoAttack",
];

fn next_rng(rng: &mut u64) -> u64 {
    *rng ^= *rng << 13;
    *rng ^= *rng >> 7;
    *rng ^= *rng << 17;
    *rng
}

static CARDS: CardTable = CardTable::new(POOL);

#[test]
fn requirements_match_enumerating_reference_on_300_seeded_boards() {
    let mut deck: Vec<(u8, &str)> = CREATURES.iter().map(|&name| (3, name)).collect();
    deck.push((1, "ArbiterAtk"));
    deck.push((1, "ArbiterBlk1"));
    deck.push((1, "ArbiterBlk2"));
    let t = GameTable::new(
        Format::CONSTRUCTED,
        &CARDS,
        [deck.as_slice(), deck.as_slice()],
    )
    .expect("requirements test table");

    for seed in 0..300u64 {
        let mut rng = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
        let mut g = Game::with_table(t.clone(), seed);
        let active = g.attacker;
        let defender = g.defender();

        let n_atk = 1 + (next_rng(&mut rng) % 4) as usize;
        let n_def = 1 + (next_rng(&mut rng) % 4) as usize;

        let mut atk_counts = [0u8; 16];
        for _ in 0..n_atk {
            let mut idx = (next_rng(&mut rng) as usize) % CREATURES.len();
            while atk_counts[idx] >= 3 {
                idx = (idx + 1) % CREATURES.len();
            }
            atk_counts[idx] += 1;
            g.put(active, CREATURES[idx]);
        }
        let mut guards = 0usize;
        let mut def_counts = [0u8; 16];
        for _ in 0..n_def {
            let mut idx = (next_rng(&mut rng) as usize) % CREATURES.len();
            // Keep at most 2 `Guard`s so brute-force enumeration stays fast.
            if CREATURES[idx] == "Guard" {
                if guards >= 2 {
                    idx = 0;
                } else {
                    guards += 1;
                }
            }
            while def_counts[idx] >= 3 {
                idx = (idx + 1) % CREATURES.len();
            }
            def_counts[idx] += 1;
            g.put(defender, CREATURES[idx]);
        }
        match next_rng(&mut rng) % 8 {
            0 => {
                g.put(active, "ArbiterAtk");
            }
            1 => {
                g.put(defender, "ArbiterBlk1");
            }
            2 => {
                g.put(defender, "ArbiterBlk2");
            }
            _ => {}
        }

        // Advance to DeclareAttackers.
        loop {
            match next(&mut g.s, &g.t, &mut g.x, &mut Replay) {
                Step::Decide(d) if d.kind == DecisionKind::Priority => {
                    apply(&mut g.s, &g.t, &mut g.x, &mut Replay, Choice(0));
                }
                Step::Decide(d) if d.kind == DecisionKind::Attacker => {
                    verify_all_paths(&g.s, &g.t);
                    break;
                }
                Step::Decide(_) | Step::Over(_) => break,
            }
        }

        // Pick a random legal attack declaration so we can test DeclareBlockers too.
        loop {
            match next(&mut g.s, &g.t, &mut g.x, &mut Replay) {
                Step::Decide(d) if d.kind == DecisionKind::Attacker => {
                    let Candidates::Options(ref opts) = d.candidates else {
                        break;
                    };
                    let slice = opts.as_slice();
                    // Prefer attacking if possible so DeclareBlockers is reached.
                    let non_done: Vec<usize> = slice
                        .iter()
                        .enumerate()
                        .filter(|&(_, &o)| o != Opt::Done)
                        .map(|(i, _)| i)
                        .collect();
                    let pick = if !non_done.is_empty() && !next_rng(&mut rng).is_multiple_of(3) {
                        non_done[(next_rng(&mut rng) as usize) % non_done.len()]
                    } else {
                        (next_rng(&mut rng) as usize) % slice.len()
                    };
                    apply(&mut g.s, &g.t, &mut g.x, &mut Replay, Choice(pick as u16));
                }
                Step::Decide(d) if d.kind == DecisionKind::Priority => {
                    apply(&mut g.s, &g.t, &mut g.x, &mut Replay, Choice(0));
                }
                Step::Decide(d) if d.kind == DecisionKind::Blocker => {
                    verify_all_paths(&g.s, &g.t);
                    break;
                }
                Step::Decide(_) | Step::Over(_) => break,
            }
        }
    }
}

#[test]
fn banding_spread_block_does_not_satisfy_lure() {
    let deck: &Decklist = &[(4, "Bander"), (4, "Lured"), (12, "Vanilla")];
    let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [deck, deck]).expect("banding+lure table");
    let mut g = Game::with_table(t, 1);
    let active = g.attacker;
    let defender = g.defender();

    let bander = g.put(active, "Bander");
    let lured = g.put(active, "Lured");
    let other_lured = g.put(active, "Lured");
    let blocker = g.put(defender, "Vanilla");

    // Advance to DeclareAttackers and attack as one band [bander, lured] plus other_lured alone.
    loop {
        match next(&mut g.s, &g.t, &mut g.x, &mut Replay) {
            Step::Decide(d) if d.kind == DecisionKind::Priority => {
                apply(&mut g.s, &g.t, &mut g.x, &mut Replay, Choice(0));
            }
            Step::Decide(d) if d.kind == DecisionKind::Attacker => {
                let Candidates::Options(opts) = d.candidates else {
                    panic!("expected options");
                };
                let slice = opts.as_slice();
                let start_band = Opt::Attack {
                    slot: bander,
                    band: Some(bander),
                };
                let join_band = Opt::Attack {
                    slot: lured,
                    band: Some(bander),
                };
                let pick = slice
                    .iter()
                    .position(|&o| o == start_band)
                    .or_else(|| slice.iter().position(|&o| o == join_band))
                    .or_else(|| slice.iter().position(|&o| o == Opt::attack(other_lured)))
                    .or_else(|| slice.iter().position(|&o| o == Opt::Done))
                    .expect("pick");
                let done = slice[pick] == Opt::Done;
                apply(&mut g.s, &g.t, &mut g.x, &mut Replay, Choice(pick as u16));
                if done {
                    break;
                }
            }
            Step::Decide(_) | Step::Over(_) => break,
        }
    }

    // Advance to DeclareBlockers: blocker is forced to block (only 1 blocker, no Done), so
    // the engine advances silently to BlockTarget where ONLY the two Lured creatures are offered,
    // NOT Bander (even though blocking Bander would later spread to Lured under CR 702.22h).
    loop {
        match next(&mut g.s, &g.t, &mut g.x, &mut Replay) {
            Step::Decide(d) if d.kind == DecisionKind::Priority => {
                apply(&mut g.s, &g.t, &mut g.x, &mut Replay, Choice(0));
            }
            Step::Decide(d) if d.kind == DecisionKind::BlockTarget => {
                let Candidates::Objects(targets) = d.candidates else {
                    panic!("expected objects");
                };
                assert_eq!(
                    targets.count(),
                    2,
                    "only the two Lured creatures should be offered, not Bander"
                );
                assert!(targets.contains(lured));
                assert!(targets.contains(other_lured));
                assert!(!targets.contains(bander));
                let _ = blocker;
                break;
            }
            Step::Decide(d) => panic!("unexpected decision before BlockTarget: {:?}", d.kind),
            Step::Over(o) => panic!("unexpected game over before BlockTarget: {o:?}"),
        }
    }
}
