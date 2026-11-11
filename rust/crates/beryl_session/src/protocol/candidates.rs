//! Building each candidate answer of a decision, with its label.

use beryl_engine::card::{
    AbilityKind, CastingIx, CastingOption, Castings, Colors, Cost, FaceAbility, FaceIx, ManaColor,
    ManaCost, ObjectAbility, Types,
};
use beryl_engine::layout::Queue;
use beryl_engine::state::CounterKind;
use beryl_engine::two_player::TwoPlayerSeat as _;
use beryl_engine::{Candidates, Decision, GameTable, Layout, Observation, Opt, Seat, SlotId};

use super::{Candidate, CandidateView, Mana, Side, ability_order, face_name, name, side};

/// What each candidate of `d` is called, to the seat `obs` sees for: as the UI names the
/// human's, for the agent's decisions in a report ([`crate::ai`]).
pub(crate) fn candidate_labels<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    d: &Decision<L>,
) -> Vec<String> {
    match &d.candidates {
        Candidates::Objects(_) => (d.candidates.objects())
            .map(|slot| name(t, obs, slot).unwrap_or("a hidden card").to_owned())
            .collect(),
        Candidates::Options(opts) => opts.iter().map(|&o| candidate(t, obs, o).label).collect(),
    }
}

pub(super) fn candidate<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    o: Opt<L>,
) -> CandidateView {
    let named = |slot: L::Slot| name(t, obs, slot).unwrap_or("a hidden card");
    let player = |p: Seat| match side(obs, p) {
        Side::You => "you",
        Side::Opponent => "your opponent",
    };
    let mana = |color: ManaColor| (Mana::from(color), MANA_SYMBOLS[color.index()]);
    let (label, option) = match o {
        Opt::Pass => ("Pass".to_owned(), Candidate::Pass),
        Opt::PlayLand(slot) => (
            format!(
                "Play {}",
                land_face_name(t, obs, slot).unwrap_or("a hidden card")
            ),
            Candidate::PlayLand { slot: slot.num() },
        ),
        Opt::Play => ("Play first".to_owned(), Candidate::Play),
        Opt::Draw => ("Draw first".to_owned(), Candidate::Draw),
        Opt::Keep => ("Keep".to_owned(), Candidate::Keep),
        Opt::Mulligan => ("Mulligan".to_owned(), Candidate::Mulligan),
        Opt::Trigger(index) => trigger(t, obs, index),
        Opt::Yes => ("Yes".to_owned(), Candidate::Yes),
        Opt::No => ("No".to_owned(), Candidate::No),
        Opt::Done => ("Done".to_owned(), Candidate::Done),
        Opt::Attack(slot) => (
            format!("Attack with {}", named(slot)),
            Candidate::Attack { slot: slot.num() },
        ),
        Opt::Defender(None) => (
            format!("Attack {}", player(obs.turn().active().opponent())),
            Candidate::Defender { planeswalker: None },
        ),
        Opt::Defender(Some(slot)) => (
            format!("Attack {}", named(slot)),
            Candidate::Defender {
                planeswalker: Some(slot.num()),
            },
        ),
        Opt::Block(slot) => (
            format!("Block with {}", named(slot)),
            Candidate::Block { slot: slot.num() },
        ),
        Opt::Lethal(slot) => (
            format!("Assign lethal damage to {}", named(slot)),
            Candidate::Lethal { slot: slot.num() },
        ),
        Opt::Cast { slot, face, costs } => cast_candidate(t, obs, slot, face, costs),
        Opt::Activate { slot, ability } => (
            match activated_cost(t, obs, slot, ability) {
                Some(cost) => format!("{}: {cost}", named(slot)),
                None => format!("Activate {}", named(slot)),
            },
            activate(slot, ability),
        ),
        Opt::Mana { slot, color } => mana_candidate(t, obs, slot, color, named(slot)),
        Opt::Pool(color) => {
            let (color, symbol) = mana(color);
            (format!("Spend {symbol}"), Candidate::Pool { color })
        }
        Opt::Number(n) => (n.to_string(), Candidate::Number { n }),
        Opt::Plan(index) => {
            let index = index.into();
            (format!("Payment {}", index + 1), Candidate::Plan { index })
        }
        Opt::Player(p) => (
            match side(obs, p) {
                Side::You => "You",
                Side::Opponent => "Your opponent",
            }
            .to_owned(),
            Candidate::Player {
                player: side(obs, p),
            },
        ),
        Opt::Object(slot) => (
            named(slot).to_owned(),
            Candidate::Object { slot: slot.num() },
        ),
        Opt::TurnFaceUp(slot) => (
            format!("Turn {} face up", named(slot)),
            Candidate::TurnFaceUp { slot: slot.num() },
        ),
    };
    CandidateView {
        label,
        option,
        payments: Vec::new(),
        blocks: Vec::new(),
        nth: ability_nth(t, obs, o),
        targets: Vec::new(),
    }
}

fn mana_candidate<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    slot: L::Slot,
    color: ManaColor,
    name: &str,
) -> (String, Candidate) {
    let ability = obs.card(slot).and_then(|card| {
        t.def(card)
            .face(obs.object(slot).face())
            .abilities
            .iter()
            .filter(|a| a.kind == AbilityKind::Mana && a.produces.includes(color))
            .find(|a| a.cost.tap || a.cost.sacrifice_self)
    });
    let verb = if ability.is_some_and(|a| a.cost.sacrifice_self) {
        "Sacrifice"
    } else {
        "Tap"
    };
    let symbol =
        MANA_SYMBOLS[color.index()].repeat(ability.map_or(1, |a| a.produces.amount).into());
    (
        format!("{verb} {name} for {symbol}"),
        Candidate::Mana {
            slot: slot.num(),
            color: color.into(),
        },
    )
}

/// [`CandidateView::nth`].
fn ability_nth<L: Layout>(t: &GameTable, obs: Observation<'_, L>, o: Opt<L>) -> Option<u8> {
    let (face, i) = match o {
        Opt::Activate {
            slot,
            ability: ObjectAbility::Own(i),
        } => (t.def(obs.card(slot)?).face(obs.object(slot).face()), i),
        Opt::Trigger(index) => {
            let trigger = obs.triggers()[Into::<usize>::into(index)];
            (t.def(trigger.card).face(trigger.face), trigger.ability)
        }
        _ => return None,
    };
    Some(ability_order(face.abilities, i.get()).1)
}

/// The name of the face of the card in `slot` it is played with as a land, if the viewer knows
/// it: its first land face ([CR 712.12](https://yawgatog.com/resources/magic-rules/#R71212):
/// "A player playing a modal double-faced card ... as a land chooses one of its faces that's a
/// land before putting it onto the battlefield"), as Sink into Stupor plays as Soporific
/// Springs.
pub(crate) fn land_face_name<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    slot: L::Slot,
) -> Option<&'static str> {
    let def = t.def(obs.card(slot)?);
    let face = def
        .face_ixs()
        .find(|&f| def.face(f).types.intersects(Types::LAND))?;
    Some(def.face_name(face))
}

/// A candidate that casts face `face` of the card in `slot` paying the options in `costs`.
fn cast_candidate<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    slot: L::Slot,
    face: FaceIx,
    costs: Castings,
) -> (String, Candidate) {
    let spell = face_name(t, obs, slot, face).unwrap_or("a hidden card");
    (
        format!("Cast {spell}{}", casting_options(t, obs, slot, face, costs)),
        Candidate::Cast {
            slot: slot.num(),
            face: face.get(),
            costs: costs.indices().map(CastingIx::get).collect(),
        },
    )
}

/// How a cast of face `face` of the card in `slot` paying the options in `costs` reads after
/// "Cast <name>", so that the casts of one card tell apart
/// ([CR 601.2b](https://yawgatog.com/resources/magic-rules/#R6012b): "If the spell has
/// alternative or additional costs that will be paid as it's being cast ... the player
/// announces their intentions to pay any or all of those costs").
fn casting_options<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    slot: L::Slot,
    face: FaceIx,
    costs: Castings,
) -> String {
    let Some(card) = obs.card(slot) else {
        return String::new();
    };
    // CR 702.34a: a flashback cost is an alternative cost, so no other is paid with it. A
    // permission lasts until the turn ends, so one whose card has since changed zones, as
    // when Timetwister draws it again, no longer names this object.
    let incarnation = obs.object(slot).incarnation();
    if obs.permissions().iter().any(|p| {
        p.object.slot == slot
            && p.object.incarnation == incarnation
            && p.player == obs.viewer()
            && p.flashback
    }) {
        return " with flashback".to_owned();
    }
    costs
        .paid(t.def(card).face(face))
        .map(|option| match option {
            CastingOption::Kicker { cost, .. } => format!(" with kicker {}", mana_text(cost)),
            CastingOption::Offspring(cost) => format!(" with offspring {}", mana_text(cost)),
            CastingOption::Impending { cost, counters } => {
                format!(" with impending {counters}, {}", mana_text(cost))
            }
            CastingOption::Overload { cost, .. } => format!(" with overload {}", mana_text(cost)),
            CastingOption::Evoke { cost, exile } => {
                let mut parts = Vec::new();
                if cost != ManaCost::NONE {
                    parts.push(mana_text(cost));
                }
                if exile != Colors::NONE {
                    parts.push(format!("exile a {} card", color_words(exile)));
                }
                format!(" with evoke, {}", parts.join(", "))
            }
        })
        .collect()
}

/// A set of colors as card text names them: "blue", "white or blue".
fn color_words(colors: Colors) -> String {
    [
        (Colors::WHITE, "white"),
        (Colors::BLUE, "blue"),
        (Colors::BLACK, "black"),
        (Colors::RED, "red"),
        (Colors::GREEN, "green"),
    ]
    .into_iter()
    .filter(|&(c, _)| colors.contains(c))
    .map(|(_, word)| word)
    .collect::<Vec<_>>()
    .join(" or ")
}

/// The cost of activating ability `ability` of the object in `slot`, as its text reads
/// ([CR 602.1a](https://yawgatog.com/resources/magic-rules/#R6021a): "Activated abilities
/// have a cost and an effect. They are written as '[Cost]: [Effect.] [Activation
/// instructions (if any).]'"), if the viewer knows the card. A loyalty ability's cost is its
/// loyalty symbol ([CR 606.4](https://yawgatog.com/resources/magic-rules/#R6064)).
fn activated_cost<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    slot: L::Slot,
    ability: ObjectAbility,
) -> Option<String> {
    let (card, face, ability) = match ability {
        ObjectAbility::Own(i) => (
            obs.card(slot)?,
            obs.object(slot).face(),
            FaceAbility::Own(i),
        ),
        ObjectAbility::Granted(k) => {
            let grant = obs.derived(slot).granted[usize::from(k)]?;
            (grant.card, FaceIx::FRONT, FaceAbility::Grant(grant.grant))
        }
    };
    Some(cost_text(&t.def(card).face(face).ability(ability).cost))
}

/// An activation cost as Oracle text writes it: "{W}, {T}, Remove a story counter".
pub(super) fn cost_text(cost: &Cost) -> String {
    if let Some(loyalty) = cost.loyalty {
        // A minus sign, as printed, rather than a hyphen.
        return match loyalty {
            0 => "0".to_owned(),
            l if l > 0 => format!("+{l}"),
            l => format!("\u{2212}{}", l.unsigned_abs()),
        };
    }
    if cost.crew > 0 {
        return format!("Crew {}", cost.crew);
    }
    let mut parts = Vec::new();
    if cost.waterbend > 0 {
        parts.push(format!("Waterbend {}", mana_text(cost.mana)));
    } else if cost.mana != ManaCost::NONE {
        parts.push(mana_text(cost.mana));
    }
    if cost.tap {
        parts.push("{T}".to_owned());
    }
    if cost.sacrifice_self {
        parts.push("Sacrifice this".to_owned());
    }
    if cost.discard_self {
        parts.push("Discard this card".to_owned());
    }
    if let Some(kind) = cost.remove {
        parts.push(format!("Remove a {} counter", counter_name(kind)));
    }
    if parts.is_empty() {
        "Activate".to_owned()
    } else {
        parts.join(", ")
    }
}

/// A mana cost in braces, X first, as Oracle text writes it: "{X}{2}{W}{W}"
/// ([CR 107.4](https://yawgatog.com/resources/magic-rules/#R1074)).
pub(crate) fn mana_text(cost: ManaCost) -> String {
    let mut text = "{X}".repeat(usize::from(cost.x));
    if cost.generic > 0 || (cost.x == 0 && cost.symbols == [0; 6]) {
        text.push('{');
        text.push_str(&cost.generic.to_string());
        text.push('}');
    }
    for (symbol, &count) in MANA_SYMBOLS.iter().zip(&cost.symbols) {
        text.push_str(&symbol.repeat(usize::from(count)));
    }
    text
}

fn counter_name(kind: CounterKind) -> &'static str {
    match kind {
        CounterKind::PlusOne => "+1/+1",
        CounterKind::MinusOne => "-1/-1",
        CounterKind::Loyalty => "loyalty",
        CounterKind::Time => "time",
        CounterKind::Story => "story",
        CounterKind::Chorus => "chorus",
    }
}

/// Activating ability `ability` of the object in `slot`.
pub(super) fn activate<S: SlotId>(slot: S, ability: ObjectAbility) -> Candidate {
    let (ability, granted) = match ability {
        ObjectAbility::Own(i) => (i.get(), false),
        ObjectAbility::Granted(k) => (k, true),
    };
    Candidate::Activate {
        slot: slot.num(),
        ability,
        granted,
    }
}

/// Putting the waiting trigger at `index` on the stack.
pub(super) fn trigger<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    index: L::Index,
) -> (String, Candidate) {
    let trigger = obs.triggers()[Into::<usize>::into(index)];
    (
        format!(
            "Put {}'s trigger on the stack",
            t.def(trigger.card).face_name(trigger.face)
        ),
        Candidate::Trigger {
            index: index.into(),
            source: trigger.source.map(|r| r.slot.num()),
            ability: trigger.ability.get(),
        },
    )
}

/// Mana symbols by [`ManaColor`] index.
const MANA_SYMBOLS: [&str; 6] = ["{W}", "{U}", "{B}", "{R}", "{G}", "{C}"];
