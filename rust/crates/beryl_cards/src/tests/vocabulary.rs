//! The tiering: "Library effects carry hand-written scenarios; a card built only
//! from them gets generated scenarios." Every vocabulary entry a card's definition says is
//! carried out by at least one hand-written scenario, a card's own or a library entry's
//! ([`crate::library`]): each instruction of effect data, each event a trigger waits for, each
//! part of a cost, and each rule change, replacement and continuous modification.
//!
//! An instruction is carried out when the interpreter comes to it; any other entry when the
//! ability or casting option that says it does what it says (`beryl_engine::coverage`). An
//! entry no card says has no scenario to carry it out, and is not asked for.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use beryl_card::Effect;
use beryl_engine::card::{Ability, AbilityKind, Behavior, CardDef, CostPart};
use beryl_engine::coverage::{self, address};

use crate::CARDS;

/// The name of the variant `value`'s debug form begins with.
fn variant(value: &impl std::fmt::Debug) -> String {
    format!("{value:?}")
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

/// A cost part's variant, with the action it does where it does one to the object or to
/// chosen objects: `This(Tap)`, `Chosen(Exile)`, `Loyalty`.
fn cost_part(part: &CostPart) -> String {
    let debug = format!("{part:?}");
    let outer = variant(part);
    match debug[outer.len()..].strip_prefix('(') {
        Some(rest) if rest.starts_with(char::is_uppercase) => {
            format!("{outer}({})", variant(&format_args!("{rest}")))
        }
        _ => outer,
    }
}

/// Every card and token, once.
fn definitions() -> Vec<&'static CardDef> {
    let mut defs: Vec<&'static CardDef> = Vec::new();
    for &def in CARDS.cards() {
        for &d in std::iter::once(&def).chain(def.tokens) {
            if !defs.iter().any(|&known| std::ptr::eq(known, d)) {
                defs.push(d);
            }
        }
    }
    defs
}

/// The entries the definitions say, each as its kind and variant, with the addresses of the
/// parts of definitions that carry it out when coverage records one, and a card that says it.
#[derive(Default)]
struct Entries(BTreeMap<String, (BTreeSet<usize>, &'static str)>);

impl Entries {
    fn add(&mut self, entry: String, by: usize, card: &'static str) {
        self.0
            .entry(entry)
            .or_insert_with(|| (BTreeSet::new(), card))
            .0
            .insert(by);
    }

    /// The entries of `ability` of `card`.
    fn ability(&mut self, ability: &'static Ability, card: &'static str) {
        let by = address(ability);
        if let Behavior::Data(effect) = ability.body() {
            // `any` hands each instruction by reference into the static definition, as the
            // interpreter does, so its address is the one coverage records.
            let nodes = RefCell::new(Vec::new());
            effect.any(&|e: &Effect| {
                let node = std::ptr::from_ref(e).addr();
                let mut nodes = nodes.borrow_mut();
                nodes.push((format!("instruction {}", variant(e)), node));
                if let Effect::Modify { modifications, .. } = e {
                    for modification in *modifications {
                        nodes.push((format!("modification {}", variant(modification)), node));
                    }
                }
                false
            });
            // A mana ability makes its mana outside the interpreter: the ability doing what
            // it says carries out its instructions.
            let mana = ability.kind() == AbilityKind::Mana;
            for (entry, node) in nodes.into_inner() {
                if mana {
                    self.add(entry.clone(), by, card);
                }
                self.add(entry, node, card);
            }
        }
        if let Some(trigger) = ability.trigger() {
            self.add(
                format!("trigger event {}", variant(&trigger.event)),
                by,
                card,
            );
        }
        for part in ability.cost().parts() {
            self.add(format!("cost part {}", cost_part(part)), by, card);
        }
        if let Some(rule) = ability.rule() {
            self.add(format!("rule {}", variant(&rule.what)), by, card);
        }
        let replacement = variant(&ability.replacement());
        if replacement != "None" {
            self.add(format!("replacement {replacement}"), by, card);
        }
        if let Some(layer) = ability.layer() {
            for modification in layer.modifications {
                self.add(format!("modification {}", variant(modification)), by, card);
            }
        }
    }
}

/// Every entry the cards and their tokens say.
fn entries() -> Entries {
    let mut entries = Entries::default();
    for def in definitions() {
        for face in def.faces {
            let card = def.name;
            for ability in face.abilities.iter().chain(face.grants) {
                entries.ability(ability, card);
            }
            for option in face.casting {
                let by = address(option);
                entries.add(format!("casting option {:?}", option.kind()), by, card);
                for part in option.parts() {
                    entries.add(format!("cost part {}", cost_part(part)), by, card);
                }
            }
            // Paid as the face is cast, which any of its abilities doing what it says shows.
            for part in face.additional_cost {
                for ability in face.abilities {
                    entries.add(
                        format!("cost part {}", cost_part(part)),
                        address(ability),
                        card,
                    );
                }
            }
        }
    }
    entries
}

#[test]
fn every_entry_a_card_says_is_carried_out_by_a_hand_written_scenario() {
    coverage::start();
    for (_, scenarios) in crate::scenarios()
        .iter()
        .copied()
        .chain(crate::library::by_card())
    {
        for scenario in scenarios {
            // `every_card_has_scenarios_and_they_pass` reports failures.
            let _ = beryl_scenario::run(&CARDS, scenario);
        }
    }
    let hits = coverage::take();
    let missed: Vec<String> = entries()
        .0
        .into_iter()
        .filter(|(_, (by, _))| by.is_disjoint(&hits))
        .map(|(entry, (_, card))| format!("{entry}, as {card} says"))
        .collect();
    assert!(
        missed.is_empty(),
        "no hand-written scenario carries out:\n{}",
        missed.join("\n")
    );
}
