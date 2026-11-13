//! A face's rules text: its paragraphs in Oracle's order, each on its own line.

use super::Ctx;
use super::abilities::{part_of_keyword, sentences};
use super::effects::says_nothing;
use super::words::{capitalized, keywords, list, mana_set, mana_text};
use crate::ability::{Ability, AbilityKind, Behavior};
use crate::cost::{CastingOption, Cost};
use crate::definition::{CardDef, FaceIx};
use crate::mana::ManaSet;
use crate::trigger::Subject;

/// A card's rules text in Oracle's style: each face's text ([`render_face`]), a blank line
/// between faces.
pub fn render(def: &CardDef) -> String {
    def.face_ixs()
        .map(|face| render_face(def, face))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// One face's rules text, a paragraph to a line: its additional cost, the casting options
/// printed first (kicker, offspring, impending), its keyword abilities, its abilities in the
/// order the definition writes them, and the casting options printed last (overload,
/// evoke). A Rust body renders as "[custom ability]".
pub fn render_face(def: &CardDef, face: FaceIx) -> String {
    let name = def.face_name(face);
    let face = def.face(face);
    let mut ctx = Ctx::of_face(face, name);
    let mut lines = Vec::new();
    if !face.additional_cost.is_empty() {
        let actions = ctx.cost_actions(Cost::of_parts(face.additional_cost));
        lines.push(format!(
            "As an additional cost to cast this spell, {}.",
            list(&actions, "and")
        ));
    }
    for &option in face.casting {
        if let Some(line) = casting_line(&mut ctx, option, true) {
            lines.push(line);
        }
    }
    let mut keywords = keywords(face.keywords);
    if keywords.first().is_some_and(|k| k == "flash") {
        keywords.remove(0);
        lines.push("Flash".to_owned());
    }
    if !keywords.is_empty() {
        lines.push(capitalized(&keywords.join(", ")));
    }
    let overloaded: Vec<usize> = face
        .casting
        .iter()
        .filter_map(|option| match option {
            CastingOption::Overload { ability, .. } => Some(ability.index()),
            _ => None,
        })
        .collect();
    let mut previous: Option<&Ability> = None;
    // The mana the mana abilities on the last line add between them.
    let mut mana: Option<ManaSet> = None;
    for (index, ability) in face.abilities.iter().enumerate() {
        if !printed(ability) || overloaded.contains(&index) {
            continue;
        }
        if let (Some(run), Some(before), Some(last)) = (mana, previous, lines.last_mut())
            && let Some(both) = one_mana_line(run, before, ability)
        {
            *last = format!("{}: Add {}.", ctx.cost_text(ability.cost()), mana_set(both));
            mana = Some(both);
            previous = Some(ability);
            continue;
        }
        mana = (ability.kind() == AbilityKind::Mana).then(|| ability.produces());
        let shared = match previous {
            Some(before) if ability.shares_paragraph() => Some(before),
            _ => None,
        };
        match (
            shared.and_then(|before| merged(&mut ctx, before, ability)),
            lines.last_mut(),
        ) {
            (Some(text), Some(last)) => *last = text,
            (None, Some(last)) if shared.is_some() => {
                last.push(' ');
                last.push_str(&ctx.paragraph(ability));
            }
            _ => lines.push(ctx.paragraph(ability)),
        }
        previous = Some(ability);
    }
    for &option in face.casting {
        if let Some(line) = casting_line(&mut ctx, option, false) {
            lines.push(line);
        }
    }
    lines.join("\n")
}

/// One ability's rules text, for an ability with no card to read: it refers to itself as
/// "this spell" or "this permanent", and what it names by its place on a face (a delayed
/// or granted ability) renders as a marked placeholder.
pub fn render_ability(ability: &Ability) -> String {
    Ctx::bare(ability.kind()).paragraph(ability)
}

/// One ability of a face of `def` as its rules text, the way [`render_face`] writes it.
pub fn render_ability_of(def: &CardDef, face: FaceIx, ability: &Ability) -> String {
    Ctx::of_face(def.face(face), def.face_name(face)).paragraph(ability)
}

/// The mana two mana abilities add together, when Oracle text prints them as one: the same
/// cost, the same amount and no restriction, as "{T}: Add {W} or {U}." says either
/// ([CR 106.3](https://yawgatog.com/resources/magic-rules/#R1063)).
fn one_mana_line(run: ManaSet, before: &Ability, ability: &Ability) -> Option<ManaSet> {
    let plain = |a: &Ability| a.kind() == AbilityKind::Mana && a.only_if_you_control().is_none();
    let produces = ability.produces();
    (plain(before)
        && plain(ability)
        && before.cost() == ability.cost()
        && run.amount == produces.amount)
        .then_some(ManaSet {
            colors: run.colors | produces.colors,
            amount: produces.amount,
        })
}

/// Whether an ability is printed as a paragraph of its own: not one a keyword line says
/// ([`part_of_keyword`]), a delayed ability an effect makes
/// ([CR 603.7](https://yawgatog.com/resources/magic-rules/#R6037)), nor the spell ability
/// of a permanent spell, which says nothing.
fn printed(ability: &Ability) -> bool {
    let delayed = ability.trigger().is_some_and(|trigger| trigger.delayed);
    let silent = ability.kind() == AbilityKind::Spell
        && says_nothing(ability.body())
        && ability.targets().is_empty();
    !(part_of_keyword(ability) || delayed || silent)
}

/// Two triggered abilities of the object with one effect, printed as one paragraph:
/// "Whenever this permanent enters or attacks, create two ..."
/// ([CR 603.2](https://yawgatog.com/resources/magic-rules/#R6032)).
fn merged(ctx: &mut Ctx, before: &Ability, ability: &Ability) -> Option<String> {
    let (Some(first), Some(second)) = (before.trigger(), ability.trigger()) else {
        return None;
    };
    let same_effect = match (before.body(), ability.body()) {
        (Behavior::Data(a), Behavior::Data(b)) => std::ptr::eq(a, b),
        _ => false,
    };
    let of_this = matches!(first.subject, Subject::This) && matches!(second.subject, Subject::This);
    if !same_effect || !of_this || before.condition().is_some() || ability.condition().is_some() {
        return None;
    }
    ctx.begin(ability.targets(), ability.trigger());
    let (one, other) = (ctx.event_verb(first), ctx.event_verb(second));
    let lead = format!("whenever {} {one} or {other}", ctx.this);
    let mut clauses = ctx.body(ability.body());
    match clauses.first_mut() {
        Some(clause) => *clause = format!("{lead}, {clause}"),
        None => clauses.push(lead),
    }
    Some(sentences(&clauses))
}

/// A casting option's keyword line, if it is printed `first` (kicker, offspring,
/// impending) or last (overload, evoke): "Kicker {X}. X can't be 0.", "Impending
/// 4—{2}{W}{W}", "Evoke—Exile a blue card from your hand."
/// ([CR 702.33a](https://yawgatog.com/resources/magic-rules/#R70233a),
/// [CR 702.176a](https://yawgatog.com/resources/magic-rules/#R702176a),
/// [CR 702.96a](https://yawgatog.com/resources/magic-rules/#R70296a),
/// [CR 702.74a](https://yawgatog.com/resources/magic-rules/#R70274a)).
fn casting_line(ctx: &mut Ctx, option: CastingOption, first: bool) -> Option<String> {
    let line = match option {
        CastingOption::Kicker { cost, min_x } => {
            let mut line = format!("Kicker {}", mana_text(cost));
            if min_x > 0 && cost.x > 0 {
                line.push_str(". X can't be ");
                line.push_str(&(min_x - 1).to_string());
                line.push('.');
            }
            (true, line)
        }
        CastingOption::Offspring(cost) => (true, format!("Offspring {}", mana_text(cost))),
        CastingOption::Impending { cost, counters } => (
            true,
            format!("Impending {counters}\u{2014}{}", mana_text(cost)),
        ),
        CastingOption::Overload { cost, .. } => (false, format!("Overload {}", mana_text(cost))),
        CastingOption::Evoke { cost, parts } => {
            let line = if parts.is_empty() {
                format!("Evoke {}", mana_text(cost))
            } else {
                let mut actions = ctx.cost_actions(Cost::of_parts(parts));
                if !cost.is_zero() {
                    actions.insert(0, mana_text(cost));
                }
                format!("Evoke\u{2014}{}.", capitalized(&list(&actions, "and")))
            };
            (false, line)
        }
    };
    (line.0 == first).then_some(line.1)
}
