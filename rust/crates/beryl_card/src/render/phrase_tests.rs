use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use super::filters::Kind;
use super::{Ctx, render_ability_of, words::capitalized};
use crate::phrases::*;
use crate::{Ability, AbilityKind, CardDef, Effect, Face, FaceIx, Filter, TargetSpec, Types};
use crate::{card, mana};

const LAND: Face = Face::EMPTY.types(Types::LAND);
const ARTIFACT: Face = Face::EMPTY.types(Types::ARTIFACT);
const SPELL_TARGET: &[TargetSpec] = &[TargetSpec::spell()];
const UNLESS_PAID: Effect = unless_its_controller_pays(&COUNTER_TARGET_SPELL, mana("{1}{U}"));

static CREATURE: CardDef = card("Creature", &[Face::EMPTY.types(Types::CREATURE)]);

fn effect_text(effect: &'static Effect, targets: &'static [TargetSpec]) -> String {
    super::render_ability(&Ability::spell().targeting(targets).effect(effect))
}

fn filter_text(filter: Filter, kind: Kind) -> String {
    Ctx::bare(AbilityKind::Static)
        .noun(filter, kind)
        .singular(kind)
}

fn plural_filter_text(filter: Filter) -> String {
    Ctx::bare(AbilityKind::Static)
        .noun(filter, Kind::Permanent)
        .plural(Kind::Permanent)
}

/// Ignore the names of parameters when deciding whether two wordings are the same.
/// Mana symbols such as {T} and {1} are literal text.
fn wording_key(wording: &str) -> String {
    wording
        .split_inclusive('}')
        .map(|part| {
            if let Some((prefix, hole)) = part.rsplit_once('{')
                && hole.starts_with(|c: char| c.is_ascii_lowercase())
                && hole.ends_with('}')
            {
                return format!("{prefix}{{}}");
            }
            part.to_owned()
        })
        .collect()
}

/// The quoted first doc line is shared by the index and this test. Discover
/// declarations from source so a new phrase needs a rendering fixture to pass.
fn documented_wordings() -> BTreeMap<String, String> {
    let directory = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
    )
    .join("src/phrases");
    let mut phrases = BTreeMap::new();
    let mut keys = BTreeMap::new();
    for file in fs::read_dir(directory).unwrap() {
        let path = file.unwrap().path();
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        let source = fs::read_to_string(&path).unwrap();
        let mut first_line = None;
        for line in source.lines().map(str::trim) {
            if let Some(doc) = line.strip_prefix("///") {
                first_line.get_or_insert(doc.trim());
                continue;
            }
            if line.starts_with("#[") {
                continue;
            }
            let doc = first_line.take();
            let Some(declaration) = line.strip_prefix("pub const ") else {
                continue;
            };
            let name = declaration
                .strip_prefix("fn ")
                .unwrap_or(declaration)
                .split(['(', ':'])
                .next()
                .unwrap()
                .trim();
            let wording = doc
                .and_then(|line| line.strip_prefix('"'))
                .and_then(|line| line.strip_suffix('"'))
                .unwrap_or_else(|| panic!("{name} needs a quoted wording on its first doc line"));
            if let Some(previous) = keys.insert(wording_key(wording), name.to_owned()) {
                panic!("{previous} and {name} share the wording {wording:?}");
            }
            assert!(
                phrases
                    .insert(name.to_owned(), wording.to_owned())
                    .is_none(),
                "duplicate phrase {name}"
            );
        }
    }
    phrases
}

#[test]
fn phrases_render_their_documented_wording() {
    let mut wordings = documented_wordings();
    macro_rules! check {
        ($name:ident, $rendered:expr $(, $hole:literal => $value:expr)* $(,)?) => {{
            let name = stringify!($name);
            let expected = wordings.remove(name).unwrap_or_else(|| panic!("{name} has no wording"));
            $(
                assert!(expected.contains($hole), "{name} has no {} hole", $hole);
                let expected = expected.replace($hole, $value);
            )*
            assert_eq!($rendered, expected, "{name}");
        }};
    }

    check!(NONLAND, filter_text(NONLAND, Kind::Permanent));
    check!(NONCREATURE, filter_text(NONCREATURE, Kind::Spell));
    check!(NONBASIC_LANDS, plural_filter_text(NONBASIC_LANDS));
    check!(
        UNTAPPED,
        Ctx::bare(AbilityKind::Static)
            .noun(UNTAPPED, Kind::Permanent)
            .adjectives_only()
            .unwrap()
    );
    check!(YOU_CONTROL, filter_text(YOU_CONTROL, Kind::Permanent));
    check!(
        OPPONENT_CONTROLS,
        filter_text(OPPONENT_CONTROLS, Kind::Permanent)
    );
    check!(
        CREATURES_YOU_CONTROL,
        plural_filter_text(CREATURES_YOU_CONTROL)
    );
    check!(LANDS_YOU_CONTROL, plural_filter_text(LANDS_YOU_CONTROL));
    check!(
        CREATURE_YOU_DONT_CONTROL,
        filter_text(CREATURE_YOU_DONT_CONTROL, Kind::Permanent)
    );
    check!(
        INSTANT_OR_SORCERY,
        filter_text(INSTANT_OR_SORCERY, Kind::Spell)
    );

    let target = |spec| Ctx::bare(AbilityKind::Spell).target_phrase(spec);
    check!(TARGET_PERMANENT, target(TARGET_PERMANENT));
    check!(ANOTHER_TARGET_PERMANENT, target(ANOTHER_TARGET_PERMANENT));
    check!(NONLAND_PERMANENT, target(NONLAND_PERMANENT));
    check!(TARGET_LAND, target(TARGET_LAND));
    check!(UP_TO_ONE_TARGET_CREATURE, target(UP_TO_ONE_TARGET_CREATURE));
    check!(TARGET_TAPPED_CREATURE, target(TARGET_TAPPED_CREATURE));
    check!(
        TARGET_CREATURE_YOU_CONTROL,
        target(TARGET_CREATURE_YOU_CONTROL)
    );
    check!(
        TARGET_YOUR_INSTANT_OR_SORCERY_CARD,
        target(TARGET_YOUR_INSTANT_OR_SORCERY_CARD)
    );
    check!(
        TARGET_OPPONENT_SPELL_OR_NONLAND_PERMANENT,
        target(TARGET_OPPONENT_SPELL_OR_NONLAND_PERMANENT)
    );

    let trigger = |trigger| capitalized(&Ctx::bare(AbilityKind::Static).trigger_text(trigger));
    check!(EACH_END_STEP, trigger(&EACH_END_STEP));
    check!(NEXT_END_STEP, trigger(&NEXT_END_STEP));

    let ability = |ability| render_ability_of(&CREATURE, FaceIx::FRONT, &ability);
    check!(at_your_upkeep, ability(at_your_upkeep(&DRAW_A_CARD)), "{effect}" => "draw a card.");
    check!(at_each_opponents_upkeep, ability(at_each_opponents_upkeep(&DISCARD_A_CARD)), "{effect}" => "discard a card.");
    check!(when_enters, ability(when_enters(&DRAW_A_CARD)), "{effect}" => "draw a card.");
    check!(whenever_attacks, ability(whenever_attacks(&DRAW_A_CARD)), "{effect}" => "draw a card.");

    check!(DRAW_A_CARD, effect_text(&DRAW_A_CARD, &[]));
    check!(DRAW_THREE_CARDS, effect_text(&DRAW_THREE_CARDS, &[]));
    check!(DRAW_SEVEN_CARDS, effect_text(&DRAW_SEVEN_CARDS, &[]));
    check!(DISCARD_A_CARD, effect_text(&DISCARD_A_CARD, &[]));
    check!(GAIN_TWO_LIFE, effect_text(&GAIN_TWO_LIFE, &[]));
    check!(
        COUNTER_TARGET_SPELL,
        effect_text(&COUNTER_TARGET_SPELL, SPELL_TARGET)
    );
    check!(
        unless_its_controller_pays,
        effect_text(&UNLESS_PAID, SPELL_TARGET),
        "{effect}" => "Counter target spell",
        "{mana}" => "{1}{U}",
    );

    check!(
        TAP_AND_SACRIFICE,
        Ctx::of_face(&LAND, "").cost_text(&TAP_AND_SACRIFICE)
    );
    check!(
        DISCARD_HAND_AND_SACRIFICE,
        Ctx::of_face(&ARTIFACT, "").cost_text(&DISCARD_HAND_AND_SACRIFICE)
    );

    assert!(
        wordings.is_empty(),
        "phrases without rendering fixtures: {:?}",
        wordings.keys()
    );
}
