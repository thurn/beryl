//! Mutants: a card's definition with one node of its effect data, or one of its targets,
//! replaced by a neighbouring value.
//!
//! The neighbours of a node are the values a card writer could plausibly have written in its
//! place: another variant of an enum field (`Regeneration`, a `Players` value, a destination
//! zone, an owner for a controller), an amount one higher or one lower, an optional target
//! made required and the reverse, a "may" or an "unless" removed, an `If` condition negated,
//! one element dropped from a `Sequence` and a filter widened to `Filter::Any`.

use beryl_card::{Filter, Regeneration};
use beryl_engine::Zone;
use beryl_engine::card::{
    Ability, AbilityKind, Amount, Behavior, CardDef, Condition, Effect, Face, Objects, Players,
    Recipients, TargetSpec,
};

/// A card's definition with one node replaced.
pub(super) struct Mutant {
    /// Where the node is: "ability 0, step 1", "ability 2, target 0".
    pub node: String,
    /// What replaced it: "drop step 1", "`Regeneration::Allowed`".
    pub neighbour: String,
    pub def: &'static CardDef,
}

fn leak<T>(value: T) -> &'static T {
    Box::leak(Box::new(value))
}

fn leak_slice<T>(values: Vec<T>) -> &'static [T] {
    Vec::leak(values)
}

/// A node's neighbours, each with its name.
type Neighbours<T> = Vec<(String, T)>;

/// Every mutant of `def`: each neighbour of each node of the effect data of the abilities and
/// granted abilities of its faces that resolve (spells, activated and triggered abilities),
/// each target of those abilities made optional or required, and each such mutant of a token
/// or emblem the card makes ([`CardDef::tokens`]).
pub(super) fn mutants(def: &'static CardDef) -> Vec<Mutant> {
    let mut out = own_mutants(def);
    for (k, &token) in def.tokens.iter().enumerate() {
        for mutant in own_mutants(token) {
            out.push(Mutant {
                node: format!("{}: {}", token.name, mutant.node),
                neighbour: mutant.neighbour,
                def: making(def, k, mutant.def),
            });
        }
    }
    out
}

/// `def` with its `k`th token or emblem replaced by `made`: in [`CardDef::tokens`], so a game's
/// table holds it, and in every instruction that creates the old one, which names it by
/// pointer ([`Effect::CreateTokens`], [`Effect::CreateAttackingTokens`],
/// [`Effect::GetEmblem`]).
fn making(def: &'static CardDef, k: usize, made: &'static CardDef) -> &'static CardDef {
    let old = def.tokens[k];
    let mut tokens = def.tokens.to_vec();
    tokens[k] = made;
    let faces: Vec<Face> = def
        .faces
        .iter()
        .map(|face| {
            with(
                face,
                swapped(face.abilities, old, made),
                swapped(face.grants, old, made),
            )
        })
        .collect();
    leak(CardDef {
        name: def.name,
        faces: leak_slice(faces),
        tokens: leak_slice(tokens),
    })
}

/// `list` with each ability that creates `old` creating `made` instead.
fn swapped(
    list: &'static [Ability],
    old: &'static CardDef,
    made: &'static CardDef,
) -> &'static [Ability] {
    let mut changed = false;
    let abilities: Vec<Ability> = list
        .iter()
        .map(|&ability| match ability.body() {
            Behavior::Data(effect) => match replaced(effect, old, made) {
                Some(effect) => {
                    changed = true;
                    ability.effect(leak(effect))
                }
                None => ability,
            },
            Behavior::Custom(_) => ability,
        })
        .collect();
    if changed { leak_slice(abilities) } else { list }
}

/// `effect` with each instruction that creates `old` creating `made` instead, or `None` if
/// none does. It walks the same children as [`Effect::any`].
fn replaced(
    effect: &'static Effect,
    old: &'static CardDef,
    made: &'static CardDef,
) -> Option<Effect> {
    let inner = |child: &'static Effect| replaced(child, old, made).map(leak);
    match *effect {
        Effect::CreateTokens {
            player,
            token,
            count,
        } if std::ptr::eq(token, old) => Some(Effect::CreateTokens {
            player,
            token: made,
            count,
        }),
        Effect::CreateAttackingTokens {
            player,
            token,
            count,
            attacking,
        } if std::ptr::eq(token, old) => Some(Effect::CreateAttackingTokens {
            player,
            token: made,
            count,
            attacking,
        }),
        Effect::GetEmblem { player, emblem } if std::ptr::eq(emblem, old) => {
            Some(Effect::GetEmblem {
                player,
                emblem: made,
            })
        }
        Effect::Sequence(effects) => {
            let steps: Vec<Option<&'static Effect>> = effects.iter().map(inner).collect();
            steps.iter().any(Option::is_some).then(|| {
                Effect::Sequence(leak_slice(
                    steps
                        .iter()
                        .zip(effects)
                        .map(|(changed, step)| changed.map_or(*step, |c| *c))
                        .collect(),
                ))
            })
        }
        Effect::If {
            condition,
            then,
            otherwise,
        } => {
            let (new_then, new_otherwise) = (inner(then), otherwise.and_then(inner));
            (new_then.is_some() || new_otherwise.is_some()).then(|| Effect::If {
                condition,
                then: new_then.unwrap_or(then),
                otherwise: new_otherwise.or(otherwise),
            })
        }
        Effect::May { player, effect } => {
            inner(effect).map(|effect| Effect::May { player, effect })
        }
        Effect::Unless {
            effect,
            player,
            cost,
        } => inner(effect).map(|effect| Effect::Unless {
            effect,
            player,
            cost,
        }),
        Effect::ForEachPlayer { players, effect } => {
            inner(effect).map(|effect| Effect::ForEachPlayer { players, effect })
        }
        _ => None,
    }
}

/// The mutants of the abilities on `def`'s own faces.
fn own_mutants(def: &'static CardDef) -> Vec<Mutant> {
    let mut out = Vec::new();
    for (f, face) in def.faces.iter().enumerate() {
        let face_name = if f == 0 {
            String::new()
        } else {
            format!("{}: ", face.name)
        };
        for (granted, list) in [(false, face.abilities), (true, face.grants)] {
            for (i, ability) in list.iter().enumerate() {
                let what = if granted {
                    "granted ability"
                } else {
                    "ability"
                };
                let at = format!("{face_name}{what} {i}");
                for (node, neighbour, changed) in ability_neighbours(ability, &at) {
                    let mut abilities = list.to_vec();
                    abilities[i] = changed;
                    let abilities = leak_slice(abilities);
                    let face = if granted {
                        with(face, face.abilities, abilities)
                    } else {
                        with(face, abilities, face.grants)
                    };
                    let mut faces: Vec<Face> = def
                        .faces
                        .iter()
                        .map(|f| with(f, f.abilities, f.grants))
                        .collect();
                    faces[f] = face;
                    let def = leak(CardDef {
                        name: def.name,
                        faces: leak_slice(faces),
                        tokens: def.tokens,
                    });
                    out.push(Mutant {
                        node,
                        neighbour,
                        def,
                    });
                }
            }
        }
    }
    out
}

/// `face` with these abilities and granted abilities.
fn with(face: &Face, abilities: &'static [Ability], grants: &'static [Ability]) -> Face {
    Face {
        name: face.name,
        cost: face.cost,
        supertypes: face.supertypes,
        types: face.types,
        subtypes: face.subtypes,
        colors: face.colors,
        power: face.power,
        toughness: face.toughness,
        loyalty: face.loyalty,
        keywords: face.keywords,
        abilities,
        casting: face.casting,
        transformed: face.transformed,
        additional_cost: face.additional_cost,
        grants,
    }
}

/// Each mutant of one ability: its node, its neighbour and the changed ability.
fn ability_neighbours(ability: &Ability, at: &str) -> Vec<(String, String, Ability)> {
    let resolves = matches!(
        ability.kind(),
        AbilityKind::Spell | AbilityKind::Activated | AbilityKind::Triggered
    );
    let Behavior::Data(effect) = ability.body() else {
        return Vec::new();
    };
    if !resolves {
        return Vec::new();
    }
    let mut out = Vec::new();
    let targets = ability.targets();
    for (j, spec) in targets.iter().enumerate() {
        let mut specs: Vec<TargetSpec> = targets.to_vec();
        specs[j] = TargetSpec {
            optional: !spec.optional,
            ..*spec
        };
        let neighbour = if spec.optional {
            "required"
        } else {
            "optional"
        };
        out.push((
            format!("{at}, target {j}"),
            neighbour.to_owned(),
            ability.targeting(leak_slice(specs)),
        ));
    }
    let mut effects = Vec::new();
    effect_neighbours(effect, at, &mut effects);
    for (node, neighbour, changed) in effects {
        out.push((node, neighbour, ability.effect(leak(changed))));
    }
    out
}

/// Each neighbour of each node of `effect`, at `at`: the node, the neighbour and the whole
/// effect with that node replaced.
fn effect_neighbours(effect: &'static Effect, at: &str, out: &mut Vec<(String, String, Effect)>) {
    for (neighbour, changed) in local(effect) {
        out.push((at.to_owned(), neighbour, changed));
    }
    // A neighbour of a child, with the child replaced in this node.
    let mut child =
        |child: &'static Effect, at: String, rebuild: &dyn Fn(&'static Effect) -> Effect| {
            let mut inner = Vec::new();
            effect_neighbours(child, &at, &mut inner);
            for (node, neighbour, changed) in inner {
                out.push((node, neighbour, rebuild(leak(changed))));
            }
        };
    match *effect {
        Effect::Sequence(effects) => {
            for (i, step) in effects.iter().enumerate() {
                child(step, format!("{at}, step {i}"), &|changed| {
                    let mut steps = effects.to_vec();
                    steps[i] = *changed;
                    Effect::Sequence(leak_slice(steps))
                });
            }
        }
        Effect::If {
            condition,
            then,
            otherwise,
        } => {
            child(then, format!("{at}, then"), &|then| Effect::If {
                condition,
                then,
                otherwise,
            });
            if let Some(otherwise) = otherwise {
                child(otherwise, format!("{at}, otherwise"), &|otherwise| {
                    Effect::If {
                        condition,
                        then,
                        otherwise: Some(otherwise),
                    }
                });
            }
        }
        Effect::May { player, effect } => {
            child(effect, format!("{at}, inside"), &|effect| Effect::May {
                player,
                effect,
            });
        }
        Effect::Unless {
            effect,
            player,
            cost,
        } => {
            child(effect, format!("{at}, inside"), &|effect| Effect::Unless {
                effect,
                player,
                cost,
            });
        }
        Effect::ForEachPlayer { players, effect } => {
            child(effect, format!("{at}, inside"), &|effect| {
                Effect::ForEachPlayer { players, effect }
            });
        }
        _ => {}
    }
}

/// The neighbours of the node `effect` itself, its children unchanged.
#[expect(
    clippy::too_many_lines,
    reason = "one arm per entry of the vocabulary that has fields to change"
)]
fn local(effect: &'static Effect) -> Neighbours<Effect> {
    let mut out: Neighbours<Effect> = Vec::new();
    let mut add = |name: String, changed: Effect| out.push((name, changed));
    match *effect {
        Effect::Sequence(effects) => {
            for i in 0..effects.len() {
                let mut steps = effects.to_vec();
                steps.remove(i);
                add(
                    format!("drop step {i}"),
                    Effect::Sequence(leak_slice(steps)),
                );
            }
        }
        Effect::If {
            condition,
            then,
            otherwise,
        } => {
            let negated = match condition {
                Condition::Not(inner) => *inner,
                _ => Condition::Not(leak(condition)),
            };
            add(
                "condition negated".to_owned(),
                Effect::If {
                    condition: negated,
                    then,
                    otherwise,
                },
            );
        }
        Effect::May { player, effect } => {
            add("without the may".to_owned(), *effect);
            for (name, player) in chooser(player) {
                add(name, Effect::May { player, effect });
            }
        }
        Effect::Unless { effect, .. } => add("without the unless".to_owned(), *effect),
        Effect::ForEachPlayer {
            players: who,
            effect,
        } => {
            for (name, who) in players(who) {
                add(
                    name,
                    Effect::ForEachPlayer {
                        players: who,
                        effect,
                    },
                );
            }
        }
        Effect::Choose {
            player,
            among,
            prompt,
            optional,
        } => {
            let name = if optional { "required" } else { "optional" };
            add(
                name.to_owned(),
                Effect::Choose {
                    player,
                    among,
                    prompt,
                    optional: !optional,
                },
            );
            for (name, among) in objects(among) {
                add(
                    name,
                    Effect::Choose {
                        player,
                        among,
                        prompt,
                        optional,
                    },
                );
            }
        }
        Effect::LookAt { player, cards } => {
            for (name, player) in chooser(player) {
                add(name, Effect::LookAt { player, cards });
            }
        }
        Effect::CounterInto { spells, zone } => {
            for (name, zone) in zones(zone) {
                add(name, Effect::CounterInto { spells, zone });
            }
        }
        Effect::Destroy {
            objects: which,
            regeneration,
        } => {
            let other = match regeneration {
                Regeneration::Allowed => Regeneration::Forbidden,
                Regeneration::Forbidden => Regeneration::Allowed,
            };
            add(
                format!("Regeneration::{other:?}"),
                Effect::Destroy {
                    objects: which,
                    regeneration: other,
                },
            );
            for (name, which) in objects(which) {
                add(
                    name,
                    Effect::Destroy {
                        objects: which,
                        regeneration,
                    },
                );
            }
        }
        Effect::Counter(o) => objects_of(o, Effect::Counter, &mut add),
        Effect::Regenerate(o) => objects_of(o, Effect::Regenerate, &mut add),
        Effect::Exile(o) => objects_of(o, Effect::Exile, &mut add),
        Effect::ExileUntilThisLeaves(o) => objects_of(o, Effect::ExileUntilThisLeaves, &mut add),
        Effect::Sacrifice(o) => objects_of(o, Effect::Sacrifice, &mut add),
        Effect::ReturnToHand(o) => objects_of(o, Effect::ReturnToHand, &mut add),
        Effect::PutIntoGraveyard(o) => objects_of(o, Effect::PutIntoGraveyard, &mut add),
        Effect::ReturnToBattlefield(o) => objects_of(o, Effect::ReturnToBattlefield, &mut add),
        Effect::GainFlashback(o) => objects_of(o, Effect::GainFlashback, &mut add),
        Effect::PutIntoLibrary {
            objects: which,
            position,
        } => {
            for (name, which) in objects(which) {
                add(
                    name,
                    Effect::PutIntoLibrary {
                        objects: which,
                        position,
                    },
                );
            }
        }
        Effect::PutBack { player, count } => {
            player_count(
                player,
                count,
                |player, count| Effect::PutBack { player, count },
                &mut add,
            );
        }
        Effect::ShuffleInto { player, cards } => {
            for (name, player) in players(player) {
                add(name, Effect::ShuffleInto { player, cards });
            }
        }
        Effect::Dig {
            player,
            look,
            take,
            rest,
        } => {
            player_count(
                player,
                look,
                |player, look| Effect::Dig {
                    player,
                    look,
                    take,
                    rest,
                },
                &mut add,
            );
        }
        Effect::Fateseal {
            player,
            library,
            count,
        } => {
            player_count(
                library,
                count,
                |library, count| Effect::Fateseal {
                    player,
                    library,
                    count,
                },
                &mut add,
            );
        }
        Effect::Search {
            player,
            filter,
            count,
            found,
        } => {
            if filter != Filter::Any {
                add(
                    "filter Filter::Any".to_owned(),
                    Effect::Search {
                        player,
                        filter: Filter::Any,
                        count,
                        found,
                    },
                );
            }
            for (name, count) in amounts(count) {
                add(
                    name,
                    Effect::Search {
                        player,
                        filter,
                        count,
                        found,
                    },
                );
            }
        }
        Effect::Draw { player, count } => {
            player_count(
                player,
                count,
                |player, count| Effect::Draw { player, count },
                &mut add,
            );
        }
        Effect::Discard { player, count } => {
            player_count(
                player,
                count,
                |player, count| Effect::Discard { player, count },
                &mut add,
            );
        }
        Effect::Mill { player, count } => {
            player_count(
                player,
                count,
                |player, count| Effect::Mill { player, count },
                &mut add,
            );
        }
        Effect::Scry { player, count } => {
            player_count(
                player,
                count,
                |player, count| Effect::Scry { player, count },
                &mut add,
            );
        }
        Effect::ManifestDread(player) => {
            for (name, player) in players(player) {
                add(name, Effect::ManifestDread(player));
            }
        }
        Effect::DealDamage { to, amount } => {
            let recipients: Neighbours<Recipients> = match to {
                Recipients::Players(p) => players(p)
                    .into_iter()
                    .map(|(n, p)| (n, Recipients::Players(p)))
                    .collect(),
                Recipients::Permanents(o) => objects(o)
                    .into_iter()
                    .map(|(n, o)| (n, Recipients::Permanents(o)))
                    .collect(),
                // Damage to only the players, or only the permanents, of the two.
                Recipients::Both(p, o) => vec![
                    ("only players".to_owned(), Recipients::Players(p)),
                    ("only permanents".to_owned(), Recipients::Permanents(o)),
                ],
            };
            for (name, to) in recipients {
                add(name, Effect::DealDamage { to, amount });
            }
            for (name, amount) in amounts(amount) {
                add(name, Effect::DealDamage { to, amount });
            }
        }
        Effect::GainLife { player, amount } => {
            player_count(
                player,
                amount,
                |player, amount| Effect::GainLife { player, amount },
                &mut add,
            );
        }
        Effect::LoseLife { player, amount } => {
            player_count(
                player,
                amount,
                |player, amount| Effect::LoseLife { player, amount },
                &mut add,
            );
        }
        Effect::CreateTokens {
            player,
            token,
            count,
        } => {
            player_count(
                player,
                count,
                |player, count| Effect::CreateTokens {
                    player,
                    token,
                    count,
                },
                &mut add,
            );
        }
        Effect::CreateAttackingTokens {
            player,
            token,
            count,
            attacking,
        } => {
            player_count(
                player,
                count,
                |player, count| Effect::CreateAttackingTokens {
                    player,
                    token,
                    count,
                    attacking,
                },
                &mut add,
            );
        }
        Effect::CreateCopies {
            player,
            of,
            count,
            except,
        } => {
            for (name, count) in amounts(count) {
                add(
                    name,
                    Effect::CreateCopies {
                        player,
                        of,
                        count,
                        except,
                    },
                );
            }
        }
        Effect::AddCounters {
            objects: which,
            kind,
            count,
        } => {
            for (name, which) in objects(which) {
                add(
                    name,
                    Effect::AddCounters {
                        objects: which,
                        kind,
                        count,
                    },
                );
            }
            for (name, count) in amounts(count) {
                add(
                    name,
                    Effect::AddCounters {
                        objects: which,
                        kind,
                        count,
                    },
                );
            }
        }
        Effect::RemoveCounters {
            objects: which,
            kind,
            count,
        } => {
            for (name, which) in objects(which) {
                add(
                    name,
                    Effect::RemoveCounters {
                        objects: which,
                        kind,
                        count,
                    },
                );
            }
            for (name, count) in amounts(count) {
                add(
                    name,
                    Effect::RemoveCounters {
                        objects: which,
                        kind,
                        count,
                    },
                );
            }
        }
        Effect::AddManaOfKind {
            player,
            kind,
            amount,
        } => {
            for (name, amount) in amounts(amount) {
                add(
                    name,
                    Effect::AddManaOfKind {
                        player,
                        kind,
                        amount,
                    },
                );
            }
        }
        Effect::Modify {
            objects: which,
            modifications,
            duration,
        } => {
            for (name, which) in objects(which) {
                add(
                    name,
                    Effect::Modify {
                        objects: which,
                        modifications,
                        duration,
                    },
                );
            }
        }
        Effect::ExchangeControl(first, second) => {
            for (name, first) in objects(first) {
                add(name, Effect::ExchangeControl(first, second));
            }
            for (name, second) in objects(second) {
                add(
                    format!("second: {name}"),
                    Effect::ExchangeControl(first, second),
                );
            }
        }
        Effect::Attach { objects: which, to } => {
            for (name, which) in objects(which) {
                add(name, Effect::Attach { objects: which, to });
            }
            for (name, to) in objects(to) {
                add(format!("to: {name}"), Effect::Attach { objects: which, to });
            }
        }
        Effect::PermitCast {
            player,
            cards,
            duration,
            terms,
        } => {
            for (name, cards) in objects(cards) {
                add(
                    name,
                    Effect::PermitCast {
                        player,
                        cards,
                        duration,
                        terms,
                    },
                );
            }
        }
        Effect::MayCast { player, card, free } => {
            add(
                format!("free: {}", !free),
                Effect::MayCast {
                    player,
                    card,
                    free: !free,
                },
            );
        }
        Effect::UntapUpTo {
            player,
            count,
            permanents,
        } => {
            for (name, count) in amounts(count) {
                add(
                    name,
                    Effect::UntapUpTo {
                        player,
                        count,
                        permanents,
                    },
                );
            }
            for (name, permanents) in objects(permanents) {
                add(
                    name,
                    Effect::UntapUpTo {
                        player,
                        count,
                        permanents,
                    },
                );
            }
        }
        Effect::Delay {
            ability,
            carrying,
            object,
        } => {
            for (name, carrying) in amounts(carrying) {
                add(
                    name,
                    Effect::Delay {
                        ability,
                        carrying,
                        object,
                    },
                );
            }
            for (name, which) in object.map(objects).unwrap_or_default() {
                let object = Some(which);
                add(
                    name,
                    Effect::Delay {
                        ability,
                        carrying,
                        object,
                    },
                );
            }
        }
        Effect::WinGame(player) => {
            for (name, player) in players(player) {
                add(name, Effect::WinGame(player));
            }
        }
        Effect::LoseGame(player) => {
            for (name, player) in players(player) {
                add(name, Effect::LoseGame(player));
            }
        }
        Effect::ExileLinked(_)
        | Effect::ReturnTransformed(_)
        | Effect::Explore(_)
        | Effect::FlipCoin(_)
        | Effect::AddMana { .. }
        | Effect::ChangeRule { .. }
        | Effect::GetEmblem { .. } => {}
    }
    out
}

/// The neighbours of an instruction over one set of objects.
fn objects_of(which: Objects, build: fn(Objects) -> Effect, add: &mut impl FnMut(String, Effect)) {
    for (name, which) in objects(which) {
        add(name, build(which));
    }
}

/// The neighbours of an instruction of a player and an amount.
fn player_count(
    player: Players,
    count: Amount,
    build: impl Fn(Players, Amount) -> Effect,
    add: &mut impl FnMut(String, Effect),
) {
    for (name, player) in players(player) {
        add(name, build(player, count));
    }
    for (name, count) in amounts(count) {
        add(name, build(player, count));
    }
}

/// The other players a card could have named: "you", "each opponent" and "each player" for
/// one another, and an object's owner for its controller and the reverse.
fn players(player: Players) -> Neighbours<Players> {
    const SIMPLE: [Players; 3] = [Players::You, Players::EachOpponent, Players::EachPlayer];
    match player {
        Players::You | Players::EachOpponent | Players::EachPlayer => SIMPLE
            .into_iter()
            .filter(|&p| p != player)
            .map(|p| (format!("Players::{p:?}"), p))
            .collect(),
        Players::ControllerOf(o) => vec![("Players::OwnerOf".to_owned(), Players::OwnerOf(o))],
        Players::OwnerOf(o) => vec![("Players::ControllerOf".to_owned(), Players::ControllerOf(o))],
        Players::Target(_) | Players::ThatPlayer | Players::EachOther(_) => Vec::new(),
    }
}

/// The other player a card could have named for one who makes a choice: "you" for "an
/// opponent" and the reverse. "Each player" is no value of such a field, which asks one
/// player; the engine asks the first of them in turn order (`Run::one` in `beryl_rules`), and
/// a card in which each player chooses says so with `Effect::ForEachPlayer`.
fn chooser(player: Players) -> Neighbours<Players> {
    match player {
        Players::You => vec![("Players::EachOpponent".to_owned(), Players::EachOpponent)],
        Players::EachOpponent => vec![("Players::You".to_owned(), Players::You)],
        _ => players(player),
    }
}

/// An amount one higher and one lower.
fn amounts(amount: Amount) -> Neighbours<Amount> {
    match amount {
        Amount::Fixed(n) => {
            let mut out = vec![(format!("Amount::Fixed({})", n + 1), Amount::Fixed(n + 1))];
            if n > 0 {
                out.push((format!("Amount::Fixed({})", n - 1), Amount::Fixed(n - 1)));
            }
            out
        }
        Amount::Times(n, of) => {
            let mut out = vec![(format!("times {}", n + 1), Amount::Times(n + 1, of))];
            if n > 1 {
                out.push((format!("times {}", n - 1), Amount::Times(n - 1, of)));
            }
            out
        }
        _ => Vec::new(),
    }
}

/// The objects a card could have named instead: the same zone with any filter, and the
/// cards of the other players named as [`players`] names them.
fn objects(which: Objects) -> Neighbours<Objects> {
    let any = "filter Filter::Any".to_owned();
    match which {
        Objects::All { zone, filter } if filter != Filter::Any => {
            vec![(
                any,
                Objects::All {
                    zone,
                    filter: Filter::Any,
                },
            )]
        }
        Objects::InZoneOf {
            zone,
            players: who,
            filter,
        } => {
            let mut out: Neighbours<Objects> = players(*who)
                .into_iter()
                .map(|(name, p)| {
                    (
                        name,
                        Objects::InZoneOf {
                            zone,
                            players: leak(p),
                            filter,
                        },
                    )
                })
                .collect();
            if filter != Filter::Any {
                out.push((
                    any,
                    Objects::InZoneOf {
                        zone,
                        players: who,
                        filter: Filter::Any,
                    },
                ));
            }
            out
        }
        Objects::CardsIn {
            players: who,
            zones,
        } => players(*who)
            .into_iter()
            .map(|(name, p)| {
                (
                    name,
                    Objects::CardsIn {
                        players: leak(p),
                        zones,
                    },
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The other zones a card could have put a card into.
fn zones(zone: Zone) -> Neighbours<Zone> {
    [Zone::Hand, Zone::Library, Zone::Graveyard, Zone::Exile]
        .into_iter()
        .filter(|&z| z != zone)
        .map(|z| (format!("Zone::{z:?}"), z))
        .collect()
}
