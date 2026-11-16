//! Neighbours of static abilities, replacements and rules.

use beryl_card::{
    Ability, Condition, Draws, Filter, Governed, LayerSpec, Modification, Objects, PlayerRules,
    Replaced, Replacement, Rewrite, RuleChange, SpellCost, Types, When, Zone,
};

use super::{Neighbours, costs, leak, leak_slice, operands};

pub(super) fn abilities(ability: &Ability) -> Vec<(&'static str, String, Ability)> {
    let mut out = Vec::new();
    if let Some(layer) = ability.layer() {
        for (field, name, layer) in layers(layer) {
            if !layer.needs_a_resolution() {
                out.push((field, name, Ability::static_ability(leak(layer))));
            }
        }
    }
    if let Some(replacement) = ability.replacement() {
        for (field, name, replacement) in replacements(replacement) {
            out.push((field, name, Ability::replacing(leak(replacement))));
        }
    }
    if let Some(rule) = ability.rule() {
        for (name, rule) in rules(rule) {
            out.push(("rule", name, Ability::rule_change(leak(rule))));
        }
    }
    out
}

fn filters(filter: Filter) -> Neighbours<Filter> {
    let mut out = controller_swaps(filter);
    if filter == Filter::Any {
        out.push((
            "filter creatures".to_owned(),
            Filter::Types(Types::CREATURE),
        ));
    } else {
        out.push(("filter any".to_owned(), Filter::Any));
    }
    out
}

fn controller_swaps(filter: Filter) -> Neighbours<Filter> {
    match filter {
        Filter::Whose(who) => operands::players(*who)
            .into_iter()
            .map(|(name, who)| (name, Filter::Whose(leak(who))))
            .collect(),
        Filter::Owned(who) => operands::players(*who)
            .into_iter()
            .map(|(name, who)| (name, Filter::Owned(leak(who))))
            .collect(),
        Filter::Not(inner) => controller_swaps(*inner)
            .into_iter()
            .map(|(name, inner)| (name, Filter::Not(leak(inner))))
            .collect(),
        Filter::All(parts) | Filter::AnyOf(parts) => {
            let mut out = Vec::new();
            for (index, &part) in parts.iter().enumerate() {
                for (name, changed) in controller_swaps(part) {
                    let mut parts = parts.to_vec();
                    parts[index] = changed;
                    let parts = leak_slice(parts);
                    let changed = if matches!(filter, Filter::All(_)) {
                        Filter::All(parts)
                    } else {
                        Filter::AnyOf(parts)
                    };
                    out.push((format!("filter {index} {name}"), changed));
                }
            }
            out
        }
        _ => Vec::new(),
    }
}

fn affected(objects: Objects) -> Neighbours<Objects> {
    match objects {
        Objects::This | Objects::AttachedTo => vec![(
            "all creatures".to_owned(),
            Objects::All {
                zone: Zone::Battlefield,
                filter: Filter::Types(Types::CREATURE),
            },
        )],
        Objects::All { zone, filter } => filters(filter)
            .into_iter()
            .map(|(name, filter)| (name, Objects::All { zone, filter }))
            .collect(),
        Objects::Everywhere(filter) => filters(filter)
            .into_iter()
            .map(|(name, filter)| (name, Objects::Everywhere(filter)))
            .collect(),
        Objects::Other(inner) => affected(*inner)
            .into_iter()
            .map(|(name, o)| (name, Objects::Other(leak(o))))
            .collect(),
        _ => operands::objects(objects),
    }
}

fn layers(layer: &LayerSpec) -> Vec<(&'static str, String, LayerSpec)> {
    let mut out = Vec::new();
    for (name, affected) in affected(layer.affected) {
        out.push(("affected", name, LayerSpec { affected, ..*layer }));
    }
    if let Some(condition) = layer.condition {
        out.push((
            "condition",
            "negated".to_owned(),
            LayerSpec {
                condition: Some(Condition::Not(leak(condition))),
                ..*layer
            },
        ));
    }
    for (index, &modification) in layer.modifications.iter().enumerate() {
        let mut modifications = layer.modifications.to_vec();
        modifications.remove(index);
        out.push((
            "modification",
            format!("{index} drop"),
            LayerSpec {
                modifications: leak_slice(modifications),
                ..*layer
            },
        ));
        for (name, changed) in modifications_of(modification) {
            let mut modifications = layer.modifications.to_vec();
            modifications[index] = changed;
            out.push((
                "modification",
                format!("{index} {name}"),
                LayerSpec {
                    modifications: leak_slice(modifications),
                    ..*layer
                },
            ));
        }
    }
    out
}

fn modifications_of(modification: Modification) -> Neighbours<Modification> {
    use Modification as M;
    let mut out = Vec::new();
    let mut add = |m| out.push((format!("{m:?}"), m));
    match modification {
        M::AddTypes(types) | M::RemoveTypes(types) => add(M::SetTypes(types)),
        M::SetTypes(types) => add(M::AddTypes(types)),
        M::AddSubtypeWord(word) => add(M::SetSubtypeWord(word)),
        M::SetSubtypeWord(word) => add(M::AddSubtypeWord(word)),
        M::AddColorWords(words) => add(M::SetColorWords(words)),
        M::SetColorWords(words) => add(M::AddColorWords(words)),
        M::ModifyPt(p, t) => {
            for p in costs::signed(p) {
                add(M::ModifyPt(p, t));
            }
            for t in costs::signed(t) {
                add(M::ModifyPt(p, t));
            }
        }
        M::SetPower(p) => {
            for (_, p) in operands::amounts(*p) {
                add(M::SetPower(leak(p)));
            }
        }
        M::SetPt(p, t) | M::SetBasePt(p, t) => {
            let build = if matches!(modification, M::SetPt(_, _)) {
                M::SetPt
            } else {
                M::SetBasePt
            };
            for (_, p) in operands::amounts(*p) {
                add(build(leak(p), t));
            }
            for (_, t) in operands::amounts(*t) {
                add(build(p, leak(t)));
            }
        }
        _ => {}
    }
    out
}

fn replacements(replacement: &Replacement) -> Vec<(&'static str, String, Replacement)> {
    let mut out = Vec::new();
    if replacement.replaces != Replaced::None {
        out.push((
            "replacement event",
            "disabled".to_owned(),
            Replacement {
                replaces: Replaced::None,
                ..*replacement
            },
        ));
    }
    let event = match replacement.replaces {
        Replaced::Draw(player, draws) => Some(Replaced::Draw(
            player,
            match draws {
                Draws::Each => Draws::ExceptFirstInDrawStep,
                Draws::ExceptFirstInDrawStep => Draws::Each,
            },
        )),
        Replaced::Enters { objects, not_cast } => Some(Replaced::Enters {
            objects,
            not_cast: !not_cast,
        }),
        _ => None,
    };
    if let Some(replaces) = event {
        out.push((
            "replacement event",
            format!("{replaces:?}"),
            Replacement {
                replaces,
                ..*replacement
            },
        ));
    }
    let rewrite = |rewrite| Replacement {
        rewrite,
        ..*replacement
    };
    if replacement.rewrite != Rewrite::None {
        out.push((
            "replacement rewrite",
            "disabled".to_owned(),
            rewrite(Rewrite::None),
        ));
    }
    let mut numbers = |n, build: fn(u8) -> Rewrite| {
        out.extend(
            costs::unsigned(n).map(|n| ("replacement rewrite", n.to_string(), rewrite(build(n)))),
        );
    };
    match replacement.rewrite {
        Rewrite::EntersTappedUnlessFewLands(n) => numbers(n, Rewrite::EntersTappedUnlessFewLands),
        Rewrite::EntersTappedUnlessPayLife(n) => numbers(n, Rewrite::EntersTappedUnlessPayLife),
        Rewrite::Dredge(n) => numbers(n, Rewrite::Dredge),
        Rewrite::Multiply(n) => numbers(n, Rewrite::Multiply),
        _ => {}
    }
    if let Rewrite::EntersWithCounters(kind, n) = replacement.rewrite {
        out.extend(costs::unsigned(n).map(|n| {
            (
                "replacement rewrite",
                n.to_string(),
                rewrite(Rewrite::EntersWithCounters(kind, n)),
            )
        }));
    }
    if let Rewrite::ToZone(zone) = replacement.rewrite {
        out.extend(
            operands::zones(zone)
                .into_iter()
                .map(|(name, zone)| ("replacement rewrite", name, rewrite(Rewrite::ToZone(zone)))),
        );
    }
    out
}

pub(super) fn rules(rule: &RuleChange) -> Neighbours<RuleChange> {
    let mut out = Vec::new();
    for (name, players) in operands::players(rule.players) {
        out.push((
            format!("rule players {name}"),
            RuleChange { players, ..*rule },
        ));
    }
    // A cost or player rule is read independently of the timing field.
    let when = match rule.when {
        When::OnlyAsSorcery => Some(When::AsInstant),
        When::AsInstant => Some(When::OnlyAsSorcery),
        When::Never => Some(When::AsUsual),
        When::AsUsual => None,
    };
    if let Some(when) = when {
        out.push((format!("rule when {when:?}"), RuleChange { when, ..*rule }));
    }
    for (name, what) in governed(rule.what) {
        out.push((
            format!("rule governed {name}"),
            RuleChange { what, ..*rule },
        ));
    }
    out
}

fn governed(what: Governed) -> Neighbours<Governed> {
    use Governed as G;
    match what {
        G::CastSpells(filter) => filters(filter)
            .into_iter()
            .map(|(name, f)| (name, G::CastSpells(f)))
            .collect(),
        G::ActivateLoyaltyAbilities(o) => objects_of(o, G::ActivateLoyaltyAbilities),
        G::Block(o) => vec![(
            "attack".to_owned(),
            G::Attack {
                attackers: o,
                unless_defender_controls: None,
            },
        )],
        G::Counter(Objects::All {
            filter: Filter::Any,
            ..
        }) => {
            vec![("this spell".to_owned(), G::Counter(Objects::This))]
        }
        G::Counter(_) => vec![(
            "any spell".to_owned(),
            G::Counter(Objects::All {
                zone: Zone::Stack,
                filter: Filter::Any,
            }),
        )],
        G::Attack {
            attackers,
            unless_defender_controls,
        } => {
            let mut out = vec![("block".to_owned(), G::Block(attackers))];
            if unless_defender_controls.is_some() {
                out.push((
                    "no exception".to_owned(),
                    G::Attack {
                        attackers,
                        unless_defender_controls: None,
                    },
                ));
            }
            out
        }
        G::ActivateAbilities {
            sources,
            mana_abilities,
        } => vec![(
            format!("mana abilities {}", !mana_abilities),
            G::ActivateAbilities {
                sources,
                mana_abilities: !mana_abilities,
            },
        )],
        G::SpellCosts { spells, change } => {
            let (n, build): (_, fn(u8) -> SpellCost) = match change {
                SpellCost::More(n) => (n, SpellCost::More),
                SpellCost::Less(n) => (n, SpellCost::Less),
                SpellCost::AtLeast(n) => (n, SpellCost::AtLeast),
            };
            let mut out: Neighbours<_> = costs::unsigned(n)
                .map(|n| {
                    (
                        n.to_string(),
                        G::SpellCosts {
                            spells,
                            change: build(n),
                        },
                    )
                })
                .collect();
            out.extend(
                filters(spells)
                    .into_iter()
                    .map(|(name, spells)| (name, G::SpellCosts { spells, change })),
            );
            out
        }
        G::Player(changes) => {
            let bits = [
                PlayerRules::LIFE_CANT_CHANGE,
                PlayerRules::PROTECTION_FROM_EVERYTHING,
            ];
            bits.into_iter()
                .enumerate()
                .filter(|&(_, bit)| changes.contains(bit))
                .map(|(index, _)| {
                    let other = bits[1 - index];
                    let action = if changes.contains(other) {
                        "drop"
                    } else {
                        "swap"
                    };
                    (format!("{action} player rule {index}"), G::Player(other))
                })
                .collect()
        }
    }
}

fn objects_of(objects: Objects, build: fn(Objects) -> Governed) -> Neighbours<Governed> {
    affected(objects)
        .into_iter()
        .map(|(name, objects)| (name, build(objects)))
        .collect()
}
