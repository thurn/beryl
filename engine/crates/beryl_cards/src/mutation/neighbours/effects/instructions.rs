//! The neighbouring values of a single effect instruction, with its children left alone.

use beryl_card::{Condition, Duration, Effect, Filter, Players, PutOnto, Regeneration};

use super::super::continuous::rules;
use super::super::operands::{
    amounts, chooser, objects, objects_of, player_count, players, preparation, zones,
};
use super::super::{Kind, LINKED, Neighbours, leak, leak_slice};

mod damage;
mod faces;
mod phases;
mod turn;

/// The neighbours of the node `effect` itself, its children unchanged.
#[expect(
    clippy::too_many_lines,
    reason = "one arm per entry of the vocabulary that has fields to change"
)]
pub(super) fn local(effect: &'static Effect) -> Vec<(Kind, String, Effect)> {
    let mut out: Vec<(Kind, String, Effect)> = Vec::new();
    let mut add = |kind, name: String, changed: Effect| out.push((kind, name, changed));
    match *effect {
        Effect::Subgame => add(Kind::SequenceStep, "skip".to_owned(), Effect::NOTHING),
        Effect::Sequence(effects) => {
            for i in 0..effects.len() {
                let mut steps = effects.to_vec();
                steps.remove(i);
                add(
                    Kind::SequenceStep,
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
                Kind::EffectCondition,
                "condition negated".to_owned(),
                Effect::If {
                    condition: negated,
                    then,
                    otherwise,
                },
            );
        }
        Effect::May { player, effect } => {
            add(Kind::May, "without the may".to_owned(), *effect);
            for (name, player) in chooser(player) {
                add(Kind::EffectPlayers, name, Effect::May { player, effect });
            }
        }
        Effect::Unless { effect, .. } => {
            add(Kind::Unless, "without the unless".to_owned(), *effect);
        }
        Effect::ForEachPlayer {
            players: who,
            effect,
        } => {
            for (name, who) in players(who) {
                add(
                    Kind::EffectPlayers,
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
            hidden,
        } => {
            // A hidden pick is optional (`Malformed::Chosen`), so it is not made required.
            if !hidden {
                let name = if optional { "required" } else { "optional" };
                add(
                    Kind::ChoiceOptional,
                    name.to_owned(),
                    Effect::Choose {
                        player,
                        among,
                        prompt,
                        optional: !optional,
                        hidden,
                    },
                );
            }
            if optional {
                let name = if hidden { "revealed" } else { "hidden" };
                add(
                    Kind::ChoiceVisibility,
                    name.to_owned(),
                    Effect::Choose {
                        player,
                        among,
                        prompt,
                        optional,
                        hidden: !hidden,
                    },
                );
            }
            for (name, among) in objects(among) {
                add(
                    Kind::EffectObjects,
                    name,
                    Effect::Choose {
                        player,
                        among,
                        prompt,
                        optional,
                        hidden,
                    },
                );
            }
        }
        Effect::LookAt { player, cards } => {
            for (name, player) in chooser(player) {
                add(Kind::EffectPlayers, name, Effect::LookAt { player, cards });
            }
        }
        Effect::CounterInto { spells, zone } => {
            for (name, zone) in zones(zone) {
                add(
                    Kind::Destination,
                    name,
                    Effect::CounterInto { spells, zone },
                );
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
                Kind::Regeneration,
                format!("Regeneration::{other:?}"),
                Effect::Destroy {
                    objects: which,
                    regeneration: other,
                },
            );
            for (name, which) in objects(which) {
                add(
                    Kind::EffectObjects,
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
        Effect::Tap(o) => objects_of(o, Effect::Tap, &mut add),
        Effect::Untap(o) => objects_of(o, Effect::Untap, &mut add),
        Effect::PhaseOut(o) => objects_of(o, Effect::PhaseOut, &mut add),
        Effect::SetPrepared { objects, prepared } => preparation(objects, prepared, &mut add),
        Effect::Exile(o) => objects_of(o, Effect::Exile, &mut add),
        Effect::ExileUntilThisLeaves(o) => objects_of(o, Effect::ExileUntilThisLeaves, &mut add),
        Effect::Sacrifice(o) => objects_of(o, Effect::Sacrifice, &mut add),
        Effect::ReturnToHand(o) => objects_of(o, Effect::ReturnToHand, &mut add),
        Effect::PutIntoGraveyard(o) => objects_of(o, Effect::PutIntoGraveyard, &mut add),
        Effect::ReturnToBattlefield(o) => objects_of(o, Effect::ReturnToBattlefield, &mut add),
        Effect::PutOntoBattlefield(onto) => {
            let mut put = |kind, name: String, changed: PutOnto| {
                add(kind, name, Effect::PutOntoBattlefield(leak(changed)));
            };
            for (name, objects) in objects(onto.objects) {
                put(Kind::EffectObjects, name, PutOnto { objects, ..*onto });
            }
            match onto.under {
                Some(player) => {
                    put(
                        Kind::PutController,
                        "under owners".to_owned(),
                        PutOnto {
                            under: None,
                            ..*onto
                        },
                    );
                    for (name, player) in players(player) {
                        put(
                            Kind::PutController,
                            name,
                            PutOnto {
                                under: Some(player),
                                ..*onto
                            },
                        );
                    }
                }
                None => put(
                    Kind::PutController,
                    "under Players::You".to_owned(),
                    PutOnto {
                        under: Some(Players::You),
                        ..*onto
                    },
                ),
            }
            put(
                Kind::PutTapped,
                format!("tapped {}", !onto.tapped),
                PutOnto {
                    tapped: !onto.tapped,
                    ..*onto
                },
            );
            put(
                Kind::PutLink,
                format!("{LINKED}{}", !onto.linked),
                PutOnto {
                    linked: !onto.linked,
                    ..*onto
                },
            );
        }
        Effect::GainFlashback(o) => objects_of(o, Effect::GainFlashback, &mut add),
        Effect::PutIntoLibrary {
            objects: which,
            position,
        } => {
            for (name, which) in objects(which) {
                add(
                    Kind::EffectObjects,
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
        Effect::PutAnyOnBottom { player } => {
            for (name, player) in players(player) {
                add(Kind::EffectPlayers, name, Effect::PutAnyOnBottom { player });
            }
        }
        Effect::ShuffleInto { player, cards } => {
            for (name, player) in players(player) {
                add(
                    Kind::EffectPlayers,
                    name,
                    Effect::ShuffleInto { player, cards },
                );
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
        Effect::Wish { filter } => {
            if filter != Filter::Any {
                add(
                    Kind::SearchFilter,
                    "filter Filter::Any".to_owned(),
                    beryl_card::prelude::wish(Filter::Any),
                );
            }
        }
        Effect::Search {
            player,
            filter,
            count,
            found,
        } => {
            if filter != Filter::Any {
                add(
                    Kind::SearchFilter,
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
                    Kind::Amount,
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
                add(Kind::EffectPlayers, name, Effect::ManifestDread(player));
            }
        }
        Effect::DealDamage { to, amount } => damage::dealing(to, amount, &mut add),
        Effect::PreventDamage {
            to,
            prevention,
            from,
        } => damage::preventing(to, prevention, from, &mut add),
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
                    Kind::Amount,
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
        Effect::CopySpell {
            spells,
            count,
            except,
            new_targets,
        } => {
            let copy = |count, except, new_targets| Effect::CopySpell {
                spells,
                count,
                except,
                new_targets,
            };
            for (name, count) in amounts(count) {
                add(Kind::Amount, name, copy(count, except, new_targets));
            }
            add(
                Kind::CopyTargets,
                format!("new_targets {}", !new_targets),
                copy(count, except, !new_targets),
            );
            if except.is_some() {
                add(
                    Kind::CopyException,
                    "except None".to_owned(),
                    copy(count, None, new_targets),
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
                    Kind::EffectObjects,
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
                    Kind::Amount,
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
                    Kind::EffectObjects,
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
                    Kind::Amount,
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
                    Kind::Amount,
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
                    Kind::EffectObjects,
                    name,
                    Effect::Modify {
                        objects: which,
                        modifications,
                        duration,
                    },
                );
            }
        }
        Effect::ChangeText {
            objects: which,
            duration,
        } => {
            for (name, which) in objects(which) {
                add(
                    Kind::EffectObjects,
                    name,
                    Effect::ChangeText {
                        objects: which,
                        duration,
                    },
                );
            }
            for other in [Duration::EndOfTurn, Duration::Indefinite] {
                if other != duration {
                    add(
                        Kind::TextChangeDuration,
                        format!("lasting {other:?}"),
                        Effect::ChangeText {
                            objects: which,
                            duration: other,
                        },
                    );
                }
            }
        }
        Effect::ExchangeControl(first, second) => {
            for (name, first) in objects(first) {
                add(
                    Kind::EffectObjects,
                    name,
                    Effect::ExchangeControl(first, second),
                );
            }
            for (name, second) in objects(second) {
                add(
                    Kind::EffectObjects,
                    format!("second: {name}"),
                    Effect::ExchangeControl(first, second),
                );
            }
        }
        Effect::BecomeCopy {
            objects: which,
            of,
            except,
        } => {
            let copy = |objects, of| Effect::BecomeCopy {
                objects,
                of,
                except,
            };
            for (name, which) in objects(which) {
                add(Kind::EffectObjects, name, copy(which, of));
            }
            for (name, of) in objects(of) {
                add(Kind::EffectObjects, format!("of: {name}"), copy(which, of));
            }
        }
        Effect::Attach { objects: which, to } => {
            for (name, which) in objects(which) {
                add(
                    Kind::EffectObjects,
                    name,
                    Effect::Attach { objects: which, to },
                );
            }
            for (name, to) in objects(to) {
                add(
                    Kind::EffectObjects,
                    format!("to: {name}"),
                    Effect::Attach { objects: which, to },
                );
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
                    Kind::EffectObjects,
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
        Effect::MayCast {
            player,
            card,
            free,
            copy,
            ..
        } => add(
            Kind::FreeCast,
            format!("free: {}", !free),
            Effect::MayCast {
                player,
                card,
                free: !free,
                free_below: None,
                copy,
            },
        ),
        Effect::UntapUpTo {
            player,
            count,
            permanents,
        } => {
            for (name, count) in amounts(count) {
                add(
                    Kind::Amount,
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
                    Kind::EffectObjects,
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
            // A delay that carries no number has none to change.
            for (name, carrying) in carrying.map(amounts).unwrap_or_default() {
                add(
                    Kind::Amount,
                    name,
                    Effect::Delay {
                        ability,
                        carrying: Some(carrying),
                        object,
                    },
                );
            }
            for (name, which) in object.map(objects).unwrap_or_default() {
                let object = Some(which);
                add(
                    Kind::EffectObjects,
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
                add(Kind::EffectPlayers, name, Effect::WinGame(player));
            }
        }
        Effect::LoseGame(player) => {
            for (name, player) in players(player) {
                add(Kind::EffectPlayers, name, Effect::LoseGame(player));
            }
        }
        Effect::ControlPlayer(player) => {
            for (name, player) in players(player) {
                add(Kind::EffectPlayers, name, Effect::ControlPlayer(player));
            }
        }
        Effect::AdditionalStep(_) | Effect::RestartGame { .. } | Effect::EndTurn => {
            add(Kind::SequenceStep, turn::omission(effect), Effect::NOTHING);
        }
        Effect::AdditionalPhases { after, phases } => phases::neighbours(after, phases, &mut add),
        Effect::ExtraTurn(player, ability) => turn::extra_turn(player, ability, &mut add),
        Effect::ChangeRule { rule, duration } => {
            for (kind, name, rule) in rules(rule) {
                add(
                    kind,
                    name,
                    Effect::ChangeRule {
                        rule: leak(rule),
                        duration,
                    },
                );
            }
        }
        Effect::Transform(_) | Effect::Flip(_) | Effect::Meld(_) => faces::turned(effect, &mut add),
        Effect::ExileLinked(_)
        | Effect::ExileUntil { .. }
        | Effect::ReturnTransformed(_)
        | Effect::Reveal(_)
        | Effect::Explore(_)
        | Effect::Mutate(_)
        | Effect::RemoveAllCounters(_)
        | Effect::FlipCoin(_)
        | Effect::AddMana { .. }
        | Effect::GetEmblem { .. } => {}
    }
    out
}
