//! Mutants: a card's definition with one node
//! replaced by a neighbouring value.
//!
//! The neighbours of a node are the values a card writer could plausibly have written in its
//! place: another variant of an enum field (`Regeneration`, a `Players` value, a destination
//! zone, an owner for a controller), an amount one higher or one lower, an optional target
//! made required and the reverse, a "may" or an "unless" removed, an `If` condition negated,
//! one element dropped from a `Sequence` and a filter widened to `Filter::Any`.
//!
//! A node no game can read makes no mutant, since no scenario could kill it: a delay that
//! carries no number, the link of a card nothing of which reads it, and a granted spell
//! ability, which is never cast. A mutant that survives is then a real gap, or a pair of
//! definitions that `EQUIVALENT` argues always do the same.

use beryl_card::{Ability, AbilityKind, Behavior, CardDef, Effect, Face, Node, Objects};

mod abilities;
mod continuous;
mod costs;
mod effects;
mod faces;
mod operands;

use abilities::{ability_neighbours, preserve};

/// A card's definition with one node replaced.
pub(super) struct Mutant {
    /// Where the node is: "ability 0, step 1", "ability 2, target 0".
    pub node: String,
    /// What replaced it: "drop step 1", "`Regeneration::Allowed`".
    pub neighbour: String,
    pub kind: Kind,
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

/// Every mutant of `def`, including single changes to the tokens and emblems it makes.
pub(super) fn mutants(def: &'static CardDef) -> Vec<Mutant> {
    let mut out = own_mutants(def);
    for (k, &token) in def.tokens.iter().enumerate() {
        for mutant in own_mutants(token) {
            out.push(Mutant {
                node: format!("{}: {}", token.name, mutant.node),
                neighbour: mutant.neighbour,
                kind: mutant.kind,
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
        shape: def.shape,
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
    rewrite(effect, &|effect| match *effect {
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
        _ => None,
    })
}

/// Replaces matching instructions, copying only the branches that contain them.
fn rewrite(
    effect: &'static Effect,
    replace: &impl Fn(&Effect) -> Option<Effect>,
) -> Option<Effect> {
    if let Some(changed) = replace(effect) {
        return Some(changed);
    }
    let inner = |child: &'static Effect| rewrite(child, replace).map(leak);
    match *effect {
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

/// The mutants of `def`'s own faces and their abilities.
fn own_mutants(def: &'static CardDef) -> Vec<Mutant> {
    let mut out = Vec::new();
    let reads_link = reads_link(def);
    for (f, face) in def.faces.iter().enumerate() {
        let face_name = if f == 0 {
            String::new()
        } else {
            format!("{}: ", face.name)
        };
        for (node, kind, neighbour, changed) in faces::neighbours(face) {
            out.push(Mutant {
                node: format!("{face_name}{node}"),
                neighbour,
                kind,
                def: with_face(def, f, changed),
            });
        }
        for (index, ability) in face.abilities.iter().enumerate() {
            if let Some(changed) = every_time(face, index, ability) {
                out.push(Mutant {
                    node: format!("{face_name}ability {index}, trigger frequency"),
                    neighbour: "every time".to_owned(),
                    kind: Kind::TriggerFrequency,
                    def: with_face(def, f, changed),
                });
            }
        }
        for (granted, list) in [(false, face.abilities), (true, face.grants)] {
            for (i, ability) in list.iter().enumerate() {
                // A spell is cast from its face's own spell ability (`Face::spell_ability_with`),
                // never from one granted to it, so a granted spell ability is never cast: it
                // chooses no target and never resolves. On an Aura it is only an enchant
                // ability, and what that may enchant reads no part a mutant would change.
                if granted && ability.kind() == AbilityKind::Spell {
                    continue;
                }
                let what = if granted {
                    "granted ability"
                } else {
                    "ability"
                };
                let at = format!("{face_name}{what} {i}");
                for (node, kind, neighbour, changed) in
                    ability_neighbours(ability, list, (!granted).then_some(face), &at)
                {
                    if !reads_link && neighbour.starts_with(LINKED) {
                        continue;
                    }
                    let mut abilities = list.to_vec();
                    abilities[i] = changed;
                    let abilities = leak_slice(abilities);
                    let face = if granted {
                        with(face, face.abilities, abilities)
                    } else {
                        with(face, abilities, face.grants)
                    };
                    out.push(Mutant {
                        node,
                        neighbour,
                        kind,
                        def: with_face(def, f, face),
                    });
                }
            }
        }
    }
    out
}

/// A scheduled trigger with no saved resolution data can instead be a standing
/// listener. Remove its scheduling instructions so they still reference only
/// delayed listeners. Triggers that read a carried object or amount need their
/// scheduling effect's context and have no standing equivalent in the vocabulary.
fn every_time(face: &Face, index: usize, ability: &Ability) -> Option<Face> {
    let trigger = ability.trigger().filter(|trigger| trigger.delayed)?;
    let uses_schedule = |effect: &Effect| matches!(effect, Effect::Delay { ability, .. } if ability.index() == index);
    let bodies = face
        .abilities
        .iter()
        .chain(face.grants)
        .filter_map(|ability| match ability.body() {
            Behavior::Data(effect) => Some(effect),
            Behavior::Custom(_) => None,
        });
    if bodies.clone().any(|effect| {
        effect.any(&|e| {
            uses_schedule(e)
                && matches!(
                    e,
                    Effect::Delay {
                        carrying: Some(_),
                        ..
                    } | Effect::Delay {
                        object: Some(_),
                        ..
                    }
                )
        })
    }) || !bodies.clone().any(|effect| effect.any(&uses_schedule))
    {
        return None;
    }
    let standing = beryl_card::Trigger {
        delayed: false,
        ..*trigger
    };
    let changed = preserve(ability, Ability::triggered(leak(standing)));
    let strip = |ability: Ability| match ability.body() {
        Behavior::Data(effect) => rewrite(effect, &|e| uses_schedule(e).then_some(Effect::NOTHING))
            .map_or(ability, |effect| ability.effect(leak(effect))),
        Behavior::Custom(_) => ability,
    };
    let mut abilities = face.abilities.to_vec();
    abilities[index] = changed;
    Some(with(
        face,
        leak_slice(abilities.into_iter().map(strip).collect()),
        leak_slice(face.grants.iter().copied().map(strip).collect()),
    ))
}

/// The name a [`PutOnto::linked`] neighbour starts with.
const LINKED: &str = "linked ";

/// Whether something of `def` reads which objects it put onto the battlefield
/// ([`PutOnto::linked`]): an [`Objects::PutOntoBattlefieldWithThis`] or a target that must have
/// been put onto the battlefield with it ([`TargetSpec::put_with_this`]). The game remembers
/// the link for the abilities of the object that made it alone, so a card with neither has
/// nothing that tells `linked true` from `linked false`, and no such mutant is made.
fn reads_link(def: &CardDef) -> bool {
    let mut reads = false;
    def.walk(&mut |node| {
        reads |= matches!(node, Node::Objects(Objects::PutOntoBattlefieldWithThis));
    });
    let abilities = def
        .faces
        .iter()
        .flat_map(|f| f.abilities.iter().chain(f.grants));
    reads
        || abilities
            .flat_map(Ability::targets)
            .any(|spec| spec.put_with_this)
}

fn with_face(def: &CardDef, index: usize, face: Face) -> &'static CardDef {
    let mut faces: Vec<Face> = def
        .faces
        .iter()
        .map(|f| with(f, f.abilities, f.grants))
        .collect();
    faces[index] = face;
    leak(CardDef {
        name: def.name,
        shape: def.shape,
        faces: leak_slice(faces),
        tokens: def.tokens,
    })
}

/// `face` with these abilities and granted abilities.
fn with(face: &Face, abilities: &'static [Ability], grants: &'static [Ability]) -> Face {
    Face {
        abilities,
        grants,
        ..*face
    }
}

// Use the same variants for construction and for the witness test's whole set.
macro_rules! mutation_kinds {
    ($($kind:ident),+ $(,)?) => {
        /// The field or instruction operation that changed.
        #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
        pub(super) enum Kind {
            $($kind),+
        }

        impl Kind {
            #[cfg(test)]
            pub(super) const ALL: &'static [Self] = &[$(Self::$kind),+];
        }
    };
}

mutation_kinds! {
    AbilityCondition,
    ActivatedGeneric,
    AdditionalCost,
    Amount,
    CastingOption,
    CastingOptionPart,
    ChoiceOptional,
    ChoiceVisibility,
    CopyException,
    CopyTargets,
    CostPart,
    DamageRecipients,
    Destination,
    EffectCondition,
    EffectObjects,
    EffectPlayers,
    FreeCast,
    Keyword,
    ManaProduct,
    May,
    Prevention,
    PreventionSource,
    PutController,
    PutLink,
    PutTapped,
    Regeneration,
    ReplacementBody,
    ReplacementCondition,
    ReplacementEvent,
    ReplacementRewrite,
    RuleGoverned,
    RulePlayers,
    RuleWhen,
    SearchFilter,
    SequenceStep,
    StaticAffected,
    StaticCondition,
    StaticModification,
    TargetOptional,
    TextChangeDuration,
    TriggerCondition,
    TriggerEvent,
    TriggerFrequency,
    TriggerSubject,
    Unless,
}
